//! Device-free MIDI input. Packets contain one complete backend message, not a
//! byte stream: unsupported messages are ignored, without running-status buffering.
//!
//! Callback receipt times use the transport's `Instant` domain; driver timestamps
//! are deliberately ignored. No application, audio, or device work happens here.
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc,
    },
    time::Instant,
};

use crate::tracker::{Note, NoteLength, NotePitch};

pub const CAPACITY: usize = 128;
pub const MAX_DRAIN: usize = 64;

/// Channels are zero-based internally; add one for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteMessage {
    pub channel: u8,
    pub pitch: u8,
    /// Includes release velocity on explicit Note Off messages.
    pub velocity: u8,
    pub on: bool,
}

/// Decode exactly one three-byte note packet. Reject malformed data bytes and
/// pitches below tracker C0 (MIDI 12); MIDI 60 maps to tracker C4.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<NoteMessage> {
    let [status, pitch, velocity] = *bytes else { return None };
    if !(12..=127).contains(&pitch) || velocity > 127 || !matches!(status & 0xf0, 0x80 | 0x90) {
        return None;
    }
    Some(NoteMessage {
        channel: status & 15,
        pitch,
        velocity,
        on: status & 0xf0 == 0x90 && velocity != 0,
    })
}

/// Convert MIDI's octave numbering to the tracker's C0–B9 numbering without
/// clamping. MIDI only reaches G9; the tracker's G#9–B9 have no MIDI pitches.
#[must_use]
pub fn note(pitch: u8, length_ticks: NoteLength) -> Option<Note> {
    let index = pitch.checked_sub(12).filter(|_| pitch <= 127)?;
    let pitches = [
        NotePitch::C,
        NotePitch::Cs,
        NotePitch::D,
        NotePitch::Ds,
        NotePitch::E,
        NotePitch::F,
        NotePitch::Fs,
        NotePitch::G,
        NotePitch::Gs,
        NotePitch::A,
        NotePitch::As,
        NotePitch::B,
    ];
    Some(Note {
        length_ticks,
        pitch: pitches[usize::from(index % 12)],
        octave: index / 12,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub message: NoteMessage,
    pub received_at: Instant,
}

/// One callback producer per connection. A fresh queue is the source generation:
/// dropping the old consumer invalidates all old callbacks and pending packets.
pub struct InputSender {
    sender: SyncSender<InputEvent>,
    overflow: Arc<AtomicBool>,
}

impl InputSender {
    /// Call at callback entry. Backend-relative timestamps are not accepted.
    pub fn receive(&self, bytes: &[u8]) {
        self.receive_at(bytes, Instant::now());
    }

    /// Explicit receipt time supports deterministic adapters/tests. Keep callback
    /// arrival order; timestamps are metadata, not a sorting or scheduling clock.
    pub fn receive_at(&self, bytes: &[u8], received_at: Instant) {
        if self.overflow.load(Ordering::Acquire) {
            return;
        }
        if let Some(message) = decode(bytes) {
            if let Err(TrySendError::Full(_)) = self.sender.try_send(InputEvent { message, received_at }) {
                self.overflow.store(true, Ordering::Release);
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum InputBatch {
    Notes(Vec<InputEvent>),
    /// Discard this source's held notes/previews and close its take. Capture is
    /// suspended until `reselect`; never interpret a lost release as still held.
    ResetRequired,
}

pub struct InputQueue {
    receiver: Receiver<InputEvent>,
    overflow: Arc<AtomicBool>,
    channel: Option<u8>,
    since: Instant,
}

impl InputQueue {
    #[must_use]
    pub fn new(since: Instant) -> (InputSender, Self) {
        let (sender, receiver) = mpsc::sync_channel(CAPACITY);
        let overflow = Arc::new(AtomicBool::new(false));
        (
            InputSender {
                sender,
                overflow: Arc::clone(&overflow),
            },
            Self {
                receiver,
                overflow,
                channel: None,
                since,
            },
        )
    }

    /// Replace the source generation, preserving the channel filter. The caller
    /// must also clear MIDI-owned state on reselection/disconnection.
    #[must_use]
    pub fn reselect(&mut self, since: Instant) -> InputSender {
        let (sender, mut queue) = Self::new(since);
        queue.channel = self.channel;
        *self = queue;
        sender
    }

    /// `None` accepts all channels. Invalid zero-based channels leave the filter
    /// unchanged. Changing filters requires caller-owned held-note cleanup.
    pub fn set_channel(&mut self, channel: Option<u8>) -> bool {
        if channel.is_some_and(|channel| channel > 15) {
            return false;
        }
        self.channel = channel;
        true
    }

    /// Consume at most `MAX_DRAIN` packets, including filtered/stale packets.
    /// Overflow remains latched and ambiguous pending packets are never delivered.
    pub fn drain(&mut self) -> InputBatch {
        if self.overflow.load(Ordering::Acquire) {
            return InputBatch::ResetRequired;
        }
        let mut notes = Vec::new();
        for _ in 0..MAX_DRAIN {
            let Ok(event) = self.receiver.try_recv() else { break };
            if event.received_at >= self.since && self.channel.is_none_or(|channel| channel == event.message.channel) {
                notes.push(event);
            }
        }
        if self.overflow.load(Ordering::Acquire) {
            InputBatch::ResetRequired
        } else {
            InputBatch::Notes(notes)
        }
    }
}

// Compare backend identities as well as names: indices belong to the saved list,
// never to a fresh enumeration which may have been reordered.
fn select_port<'a, P: PartialEq>(
    listed: &'a [(P, String)],
    available: &[(P, String)],
    index: usize,
) -> anyhow::Result<&'a (P, String)> {
    let selected = listed
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("Run :midi list, then use a listed index"))?;
    anyhow::ensure!(
        listed.iter().filter(|(_, name)| name == &selected.1).count() == 1
            && available.iter().filter(|(_, name)| name == &selected.1).count() == 1,
        "Missing or duplicate port name; run :midi list again"
    );
    anyhow::ensure!(available.contains(selected), "Port changed; run :midi list again");
    Ok(selected)
}

/// Device ownership stays on the control thread. No input is opened at startup.
#[derive(Default)]
pub struct Session {
    listed: Vec<(midir::MidiInputPort, String)>,
    connection: Option<midir::MidiInputConnection<InputSender>>,
    queue: Option<InputQueue>,
    channel: Option<u8>,
    held: Vec<(u8, u8)>,
    current: Option<(u8, u8)>,
    since: Option<Instant>,
}

impl Session {
    pub fn command(&mut self, text: &str, app: &mut crate::state::App, audio: &mut crate::audio::AudioEngine) {
        let result = self.control(text, app, audio);
        app.status = result.unwrap_or_else(|error| format!("MIDI: {error:#}"));
    }

    fn control(
        &mut self,
        text: &str,
        app: &mut crate::state::App,
        audio: &mut crate::audio::AudioEngine,
    ) -> anyhow::Result<String> {
        let args: Vec<_> = text.split_whitespace().collect();
        match args.as_slice() {
            ["list"] => {
                let input = midir::MidiInput::new("track-rs discovery")?;
                let listed = input
                    .ports()
                    .into_iter()
                    .map(|port| {
                        let name = input.port_name(&port)?;
                        Ok((port, name))
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;
                let status = if listed.is_empty() {
                    "MIDI: no input ports".into()
                } else {
                    listed
                        .iter()
                        .enumerate()
                        .map(|(i, (_, name))| format!("{i}: {name}"))
                        .collect::<Vec<_>>()
                        .join(" | ")
                };
                self.listed = listed;
                Ok(status)
            }
            ["connect", index] => {
                let index: usize = index.parse()?;
                let mut input = midir::MidiInput::new("track-rs input")?;
                input.ignore(midir::Ignore::None);
                let available = input
                    .ports()
                    .into_iter()
                    .map(|port| {
                        let name = input.port_name(&port)?;
                        Ok((port, name))
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;
                let (port, name) = select_port(&self.listed, &available, index)?;
                let now = Instant::now();
                let (sender, mut queue) = InputQueue::new(now);
                queue.set_channel(self.channel);
                let connection = input
                    .connect(port, "track-rs notes", |_, bytes, sender| sender.receive(bytes), sender)
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                let status = format!("MIDI connected: {name}");
                self.reset(app, audio, now);
                self.connection = Some(connection);
                self.queue = Some(queue);
                Ok(status)
            }
            ["disconnect"] => {
                self.disconnect(app, audio);
                Ok("MIDI disconnected".into())
            }
            ["channel", value] => {
                let channel = if matches!(*value, "omni" | "all") {
                    None
                } else {
                    let number: u8 = value.parse()?;
                    anyhow::ensure!((1..=16).contains(&number), "Channel must be omni or 1..16");
                    Some(number - 1)
                };
                self.channel = channel;
                self.reset(app, audio, Instant::now());
                if let Some(queue) = &mut self.queue {
                    queue.set_channel(channel);
                }
                Ok(format!("MIDI channel: {value}"))
            }
            _ => anyhow::bail!("Expected :midi list|connect N|disconnect|channel omni|1..16"),
        }
    }

    fn reset(&mut self, app: &mut crate::state::App, audio: &mut crate::audio::AudioEngine, now: Instant) {
        self.held.clear();
        self.current = None;
        self.since = Some(now);
        app.recording.close_take();
        audio.stop_midi_preview();
    }

    pub fn disconnect(&mut self, app: &mut crate::state::App, audio: &mut crate::audio::AudioEngine) {
        self.connection = None;
        self.queue = None;
        self.reset(app, audio, Instant::now());
        app.recording.armed = false;
    }

    /// Invalidate pending receipt times after a terminal action changes focus or target.
    pub fn invalidate(&mut self, app: &mut crate::state::App, audio: &mut crate::audio::AudioEngine) {
        self.reset(app, audio, Instant::now());
    }

    pub fn poll(
        &mut self,
        app: &mut crate::state::App,
        audio: &mut crate::audio::AudioEngine,
        transport: &mut crate::transport::Transport,
    ) {
        let Some(queue) = &mut self.queue else { return };
        let batch = queue.drain();
        let InputBatch::Notes(events) = batch else {
            self.disconnect(app, audio);
            app.status = "MIDI queue overflow: disconnected and disarmed; reconnect explicitly".into();
            return;
        };
        for event in events {
            if self.since.is_some_and(|since| event.received_at < since) {
                continue;
            }
            for note in transport.update_app(event.received_at, app, || audio.stop()) {
                audio.play_event(note);
            }
            if let Some(click) = transport.take_metronome_event() {
                audio.metronome_event(click);
            }
            if let Some(preview) = self.deliver(event, app) {
                audio.midi_preview_event(preview);
            } else if !event.message.on && self.current == Some((event.message.channel, event.message.pitch)) {
                self.current = None;
                audio.stop_midi_preview();
            }
        }
    }

    fn deliver(&mut self, event: InputEvent, app: &mut crate::state::App) -> Option<crate::transport::NoteEvent> {
        use crate::state::{Mode, PlaybackState, View};
        let key = (event.message.channel, event.message.pitch);
        self.held.retain(|held| *held != key);
        if !event.message.on {
            return None;
        }
        if app.view != View::Pattern
            || !matches!(app.mode, Mode::Insert | Mode::Audition)
            || app.help_visible
            || app.prompt.is_some()
            || app.shutdown
        {
            return None;
        }
        self.held.push(key);
        self.current = Some(key);
        let note = note(event.message.pitch, app.entry_length)?;
        // Preserve any pending terminal preview; only transfer this input's request.
        let terminal = app.audition.take();
        app.recording.input_time = Some(event.received_at);
        if app.mode == Mode::Audition {
            app.preview_note_on(note, app.cursor_track);
        } else if matches!(app.playback_state, PlaybackState::Playing) {
            app.live_note(note);
        } else {
            app.enter_note(note);
        }
        app.recording.input_time = None;
        let preview = app.audition.take();
        app.audition = terminal;
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn decode_channels_velocity_and_exact_packets() {
        for channel in 0..16 {
            let on = decode(&[0x90 | channel, 60, 127]).unwrap();
            assert_eq!(
                on,
                NoteMessage {
                    channel,
                    pitch: 60,
                    velocity: 127,
                    on: true
                }
            );
            assert!(!decode(&[0x90 | channel, 60, 0]).unwrap().on);
            let off = decode(&[0x80 | channel, 60, 42]).unwrap();
            assert!(!off.on);
            assert_eq!(off.velocity, 42);
        }
        for packet in [
            vec![],
            vec![0x90, 60],
            vec![0x90, 60, 1, 0],
            vec![60, 60, 1],
            vec![0x90, 128, 1],
            vec![0x90, 60, 128],
            vec![0xb0, 60, 1],
            vec![0xf0, 60, 1],
            vec![0xf8],
            vec![0xc0, 1],
        ] {
            assert_eq!(decode(&packet), None);
        }
    }

    #[test]
    fn mapping_matches_tracker_frequency_and_bounds() {
        let length = NoteLength::default();
        for pitch in 0..=255 {
            assert_eq!(note(pitch, length).is_some(), (12..=127).contains(&pitch));
            assert_eq!(decode(&[0x90, pitch, 1]).is_some(), (12..=127).contains(&pitch));
        }
        for (pitch, expected_pitch, octave) in [
            (12, NotePitch::C, 0),
            (60, NotePitch::C, 4),
            (69, NotePitch::A, 4),
            (119, NotePitch::B, 8),
            (120, NotePitch::C, 9),
            (127, NotePitch::G, 9),
        ] {
            let mapped = note(pitch, length).unwrap();
            assert_eq!(
                mapped,
                Note {
                    pitch: expected_pitch,
                    octave,
                    length_ticks: length
                }
            );
        }
        assert!((note(69, length).unwrap().freq() - 440.0).abs() < 0.001);
    }

    fn events(batch: InputBatch) -> Vec<InputEvent> {
        let InputBatch::Notes(events) = batch else {
            panic!("unexpected overflow")
        };
        events
    }

    #[test]
    fn arrival_order_repeats_releases_filter_and_stale_times() {
        let now = Instant::now();
        let (sender, mut queue) = InputQueue::new(now);
        assert!(queue.set_channel(Some(15)));
        assert!(!queue.set_channel(Some(16)));
        sender.receive_at(&[0x9f, 60, 1], now.checked_sub(Duration::from_secs(1)).unwrap());
        sender.receive_at(&[0x90, 60, 1], now);
        for pitch in [64, 60, 60] {
            sender.receive_at(&[0x9f, pitch, 80], now);
        }
        sender.receive_at(&[0x9f, 60, 0], now);
        let batch = events(queue.drain());
        assert_eq!(
            batch.iter().map(|event| event.message.pitch).collect::<Vec<_>>(),
            [64, 60, 60, 60]
        );
        assert!(batch.iter().all(|event| event.received_at == now));
        assert!(!batch[3].message.on);
        assert!(queue.set_channel(None));
        sender.receive_at(&[0x90, 60, 1], now);
        assert_eq!(events(queue.drain()).len(), 1);
    }

    #[test]
    fn frame_budget_counts_filtered_packets_too() {
        let now = Instant::now();
        let (sender, mut queue) = InputQueue::new(now);
        queue.set_channel(Some(1));
        for _ in 0..MAX_DRAIN {
            sender.receive_at(&[0x90, 60, 1], now);
        }
        sender.receive_at(&[0x91, 61, 1], now);
        assert_eq!(events(queue.drain()), []);
        assert_eq!(events(queue.drain()).len(), 1);
    }

    #[test]
    fn overflow_latches_until_reselection_and_invalidates_old_callbacks() {
        let now = Instant::now();
        let (old, mut queue) = InputQueue::new(now);
        for _ in 0..CAPACITY {
            old.receive_at(&[0x90, 60, 1], now);
        }
        old.receive_at(&[0x80, 60, 0], now);
        assert_eq!(queue.drain(), InputBatch::ResetRequired);
        assert_eq!(queue.drain(), InputBatch::ResetRequired);
        let new = queue.reselect(now);
        old.receive_at(&[0x90, 61, 1], now);
        new.receive_at(&[0x90, 62, 1], now);
        let batch = events(queue.drain());
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].message.pitch, 62);
    }

    #[test]
    fn full_queue_is_valid_and_each_drain_is_bounded() {
        let now = Instant::now();
        let (sender, mut queue) = InputQueue::new(now);
        for _ in 0..CAPACITY {
            sender.receive_at(&[0x90, 60, 1], now);
        }
        assert_eq!(events(queue.drain()).len(), MAX_DRAIN);
        assert_eq!(events(queue.drain()).len(), MAX_DRAIN);
        assert_eq!(events(queue.drain()), []);
        let _new = queue.reselect(now);
        sender.receive(&[0x90, 60, 1]);
        assert_eq!(events(queue.drain()), []);
    }
    fn hit(
        session: &mut Session,
        app: &mut crate::state::App,
        at: Instant,
        bytes: &[u8],
    ) -> Option<crate::transport::NoteEvent> {
        session.deliver(
            InputEvent {
                message: decode(bytes).unwrap(),
                received_at: at,
            },
            app,
        )
    }

    #[test]
    fn focus_guards_and_stopped_step_entry_use_real_app_policy() {
        use crate::state::{App, Mode, View};
        let now = Instant::now();
        let mut session = Session::default();
        let mut app = App::default();
        let revision = app.revision();
        for mode in [Mode::Normal, Mode::Visual] {
            app.mode = mode;
            assert!(hit(&mut session, &mut app, now, &[0x90, 60, 100]).is_none());
        }
        app.mode = Mode::Insert;
        for view in [View::Arrangement, View::Mixer, View::Instrument] {
            app.view = view;
            assert!(hit(&mut session, &mut app, now, &[0x90, 60, 100]).is_none());
        }
        app.view = View::Pattern;
        app.help_visible = true;
        assert!(hit(&mut session, &mut app, now, &[0x90, 60, 100]).is_none());
        app.help_visible = false;
        app.prompt = Some(String::new());
        assert!(hit(&mut session, &mut app, now, &[0x90, 60, 100]).is_none());
        app.prompt = None;
        assert_eq!(app.revision(), revision);
        let preview = hit(&mut session, &mut app, now, &[0x90, 60, 1]).unwrap();
        assert_eq!(preview.note.octave, 4);
        assert_eq!(preview.duration, crate::audio::DEFAULT_NOTE_DURATION);
        assert_ne!(app.revision(), revision);
        assert_eq!(app.cursor_row, app.edit_step);
        assert!(app.recording.input_time.is_none());
    }

    #[test]
    fn audition_never_writes_and_terminal_request_survives() {
        use crate::state::{App, Mode};
        let now = Instant::now();
        let mut app = App {
            mode: Mode::Audition,
            ..App::default()
        };
        app.recording.armed = true;
        let mut session = Session::default();
        app.preview_note_on(note(69, app.entry_length).unwrap(), 0);
        let revision = app.revision();
        for pitch in [60, 64, 67, 67] {
            assert!(hit(&mut session, &mut app, now, &[0x90, pitch, 127]).is_some());
        }
        hit(&mut session, &mut app, now, &[0x80, 60, 0]);
        assert_eq!(session.current, Some((0, 67)));
        assert_eq!(session.held, [(0, 64), (0, 67)]);
        assert_eq!(app.revision(), revision);
        assert_eq!(app.take_audition().unwrap().note.pitch, crate::tracker::NotePitch::A);
    }

    #[test]
    fn midi_live_capture_uses_timestamp_pinned_target_and_grouped_undo() {
        use crate::{
            command::Command,
            musical_time::Tick,
            state::{App, Mode},
            transport::Transport,
        };
        let now = Instant::now();
        let mut app = App::default();
        app.recording.armed = true;
        app.toggle_playback();
        app.mode = Mode::Insert;
        let mut transport = Transport::default();
        transport.update_app(now, &mut app, || {});
        let before = app.project.clone();
        let mut session = Session::default();
        for (millis, pitch) in [(0, 60), (125, 64)] {
            let at = now + Duration::from_millis(millis);
            transport.update_app(at, &mut app, || {});
            hit(&mut session, &mut app, at, &[0x90, pitch, 80]).unwrap();
        }
        assert!(app.project.patterns[0].event_at(0, Tick(0)).is_some());
        assert!(app.project.patterns[0].event_at(0, Tick(240)).is_some());
        app.mode = Mode::Normal;
        app.handle_command(Command::Undo);
        assert_eq!(app.project, before);
    }

    #[test]
    fn normal_colon_command_reaches_control_thread_without_opening_device() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = crate::state::App::default();
        for code in std::iter::once(KeyCode::Char(':'))
            .chain("midi channel omni".chars().map(KeyCode::Char))
            .chain(std::iter::once(KeyCode::Enter))
        {
            crate::bindings::handle_key_at(KeyEvent::new(code, KeyModifiers::NONE), &mut app, Instant::now());
        }
        assert_eq!(app.midi_command.as_deref(), Some("channel omni"));
        assert!(Session::default().connection.is_none());
    }
    #[test]
    fn fake_ports_keep_list_identity_and_reject_replacement_or_duplicates() {
        let listed = vec![(10, "Keys".to_owned()), (20, "Pads".to_owned())];
        let reordered = vec![listed[1].clone(), listed[0].clone()];
        assert_eq!(select_port(&listed, &reordered, 0).unwrap().0, 10);
        assert!(select_port(&listed, &[(30, "Keys".into())], 0).is_err());
        assert!(select_port(&listed, &[(10, "Renamed".into())], 0).is_err());
        let duplicate = vec![(10, "Keys".into()), (20, "Keys".into())];
        assert!(select_port(&duplicate, &duplicate, 0).is_err());
        assert!(select_port(&listed, &duplicate, 0).is_err());
        assert!(select_port(&listed, &listed, 2).is_err());
    }
}
