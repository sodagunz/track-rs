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
    pending: Option<InputEvent>,
    budget_exhausted: bool,
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
                pending: None,
                budget_exhausted: false,
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
        self.drain_before(None)
    }

    /// Drain in arrival order up to an inclusive receipt cutoff, retaining the first future packet.
    pub fn drain_before(&mut self, cutoff: Option<Instant>) -> InputBatch {
        self.budget_exhausted = false;
        if self.overflow.load(Ordering::Acquire) {
            return InputBatch::ResetRequired;
        }
        let mut notes = Vec::new();
        for packet in 0..MAX_DRAIN {
            let Some(event) = self.pending.take().or_else(|| self.receiver.try_recv().ok()) else {
                break;
            };
            if cutoff.is_some_and(|cutoff| event.received_at > cutoff) {
                self.pending = Some(event);
                break;
            }
            self.budget_exhausted = packet + 1 == MAX_DRAIN;
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

/// Audio side effects of the device-free input loop.
///
/// Control-thread output seam. Preview tokens identify one gate and must not be
/// reused across focus, target or connection generations. Cancellation is MIDI-only.
pub trait Output {
    fn stop(&mut self);
    fn play_event(&mut self, event: crate::transport::NoteEvent);
    fn metronome_event(&mut self, event: crate::transport::MetronomeEvent);
    fn midi_preview_event(&mut self, event: crate::transport::NoteEvent, token: u64);
    fn release_midi_preview(&mut self, token: u64);
    fn stop_midi_preview(&mut self);
}

impl Output for crate::audio::AudioEngine {
    fn stop(&mut self) {
        Self::stop(self);
    }
    fn play_event(&mut self, event: crate::transport::NoteEvent) {
        Self::play_event(self, event);
    }
    fn metronome_event(&mut self, event: crate::transport::MetronomeEvent) {
        Self::metronome_event(self, event);
    }
    fn midi_preview_event(&mut self, event: crate::transport::NoteEvent, token: u64) {
        Self::midi_preview_event(self, event, token);
    }
    fn release_midi_preview(&mut self, token: u64) {
        Self::release_midi_preview(self, token);
    }
    fn stop_midi_preview(&mut self) {
        Self::stop_midi_preview(self);
    }
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
    // Never reset on focus/load/reconnect: queued releases cannot own a later voice.
    preview_token: u64,
    since: Option<Instant>,
}

impl Session {
    #[cfg(test)]
    pub(crate) fn test_input(now: Instant) -> (Self, InputSender) {
        let (sender, queue) = InputQueue::new(now);
        (
            Self {
                queue: Some(queue),
                ..Self::default()
            },
            sender,
        )
    }

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

    fn reset(&mut self, app: &mut crate::state::App, audio: &mut impl Output, now: Instant) {
        // Resets discard ambiguous ownership; already committed onsets remain provisional.
        app.recording.discard_held();
        app.recording.close_take();
        self.invalidate_at(audio, now);
    }

    fn invalidate_at(&mut self, audio: &mut impl Output, now: Instant) {
        self.held.clear();
        self.current = None;
        self.since = Some(now);
        audio.stop_midi_preview();
    }

    pub fn disconnect(&mut self, app: &mut crate::state::App, audio: &mut impl Output) {
        self.connection = None;
        self.queue = None;
        self.reset(app, audio, Instant::now());
        app.recording.armed = false;
    }

    /// Invalidate pending receipt times after a terminal action changes focus or target.
    pub fn invalidate(&mut self, app: &mut crate::state::App, audio: &mut impl Output) {
        self.reset(app, audio, Instant::now());
    }

    /// Drain through `now`; false means defer clock advancement until the backlog is drained.
    pub fn poll(
        &mut self,
        app: &mut crate::state::App,
        audio: &mut impl Output,
        transport: &mut crate::transport::Transport,
        now: Instant,
    ) -> bool {
        self.poll_before(app, audio, transport, Some(now));
        !self.queue.as_ref().is_some_and(|queue| queue.budget_exhausted)
    }

    fn poll_before(
        &mut self,
        app: &mut crate::state::App,
        audio: &mut impl Output,
        transport: &mut crate::transport::Transport,
        cutoff: Option<Instant>,
    ) {
        let Some(queue) = &mut self.queue else { return };
        let batch = queue.drain_before(cutoff);
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
                self.preview_token = self.preview_token.checked_add(1).expect("MIDI preview token exhausted");
                audio.midi_preview_event(preview, self.preview_token);
            } else if !event.message.on && self.current == Some((event.message.channel, event.message.pitch)) {
                self.current = None;
                audio.release_midi_preview(self.preview_token);
            }
        }
    }

    /// Process MIDI through the inclusive key cutoff, then the terminal receipt.
    /// Equal receipt times use MIDI arrival order before the terminal key.
    /// Returns false when the packet budget requires deferring the key.
    pub fn terminal_key(
        &mut self,
        app: &mut crate::state::App,
        audio: &mut impl Output,
        transport: &mut crate::transport::Transport,
        key: crossterm::event::KeyEvent,
        at: Instant,
        processed_at: Instant,
    ) -> bool {
        self.poll_before(app, audio, transport, Some(at));
        if self.queue.as_ref().is_some_and(|queue| queue.budget_exhausted) {
            return false;
        }
        let focus = midi_focus(app);
        let revision = app.revision();
        for event in transport.update_app(at, app, || audio.stop()) {
            audio.play_event(event);
        }
        if let Some(event) = transport.take_metronome_event() {
            audio.metronome_event(event);
        }
        crate::bindings::handle_key_at(key, app, at);
        if focus != midi_focus(app) {
            app.finish_midi_holds();
            app.recording.clear_monitor();
            app.recording.close_take();
            self.invalidate_at(audio, processed_at);
        } else if revision != app.revision() {
            app.recording.clear_monitor();
            if app.recording.owns_revision(app.revision()) {
                self.invalidate_at(audio, at);
            } else {
                app.recording.close_take();
                self.invalidate_at(audio, processed_at);
            }
        }
        true
    }

    fn deliver(&mut self, event: InputEvent, app: &mut crate::state::App) -> Option<crate::transport::NoteEvent> {
        use crate::state::{Mode, PlaybackState, View};
        let key = (event.message.channel, event.message.pitch);
        self.held.retain(|held| *held != key);
        app.finish_midi_note(key, event.received_at);
        if !event.message.on {
            app.recording.release_monitor(key);
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
        app.recording.clear_monitor();
        self.held.push(key);
        self.current = Some(key);
        let note = note(event.message.pitch, app.entry_length)?;
        // Preserve any pending terminal preview; only transfer this input's request.
        let terminal = app.audition.take();
        app.recording.input_time = Some(event.received_at);
        app.recording.midi_key = Some(key);
        if app.mode == Mode::Audition {
            app.preview_note_on(note, app.cursor_track);
        } else if matches!(app.playback_state, PlaybackState::Playing) {
            app.live_note(note);
        } else {
            app.enter_note(note);
        }
        app.recording.input_time = None;
        app.recording.midi_key = None;
        let preview = app.audition.take();
        app.audition = terminal;
        preview
    }
}

fn midi_focus(app: &crate::state::App) -> impl PartialEq {
    (
        app.view,
        app.mode,
        app.help_visible,
        app.prompt.is_some(),
        matches!(app.playback_state, crate::state::PlaybackState::Playing),
        app.selected_pattern,
        app.cursor_track,
        app.recording.target,
        app.shutdown,
        (app.playback_mode, app.midi_reset_requested()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[derive(Default)]
    struct Silent;
    impl Output for Silent {
        fn stop(&mut self) {}
        fn play_event(&mut self, _: crate::transport::NoteEvent) {}
        fn metronome_event(&mut self, _: crate::transport::MetronomeEvent) {}
        fn midi_preview_event(&mut self, _: crate::transport::NoteEvent, _: u64) {}
        fn release_midi_preview(&mut self, _: u64) {}
        fn stop_midi_preview(&mut self) {}
    }
    #[derive(Default)]
    struct Monitor {
        starts: Vec<(crate::transport::NoteEvent, u64)>,
        songs: Vec<crate::transport::NoteEvent>,
        releases: Vec<u64>,
        resets: usize,
    }
    impl Output for Monitor {
        fn stop(&mut self) {}
        fn play_event(&mut self, event: crate::transport::NoteEvent) {
            self.songs.push(event);
        }
        fn metronome_event(&mut self, _: crate::transport::MetronomeEvent) {}
        fn midi_preview_event(&mut self, event: crate::transport::NoteEvent, token: u64) {
            self.starts.push((event, token));
        }
        fn release_midi_preview(&mut self, token: u64) {
            self.releases.push(token);
        }
        fn stop_midi_preview(&mut self) {
            self.resets += 1;
        }
    }

    #[test]
    fn loop_routes_matching_releases_and_records_held_length() {
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        sender.receive_at(&[0x90, 60, 80], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        let recorded = app.project.clone();
        let later = now + Duration::from_secs(1);
        sender.receive_at(&[0x90, 64, 80], later);
        sender.receive_at(&[0x80, 60, 0], later);
        session.poll(&mut app, &mut audio, &mut transport, later);
        assert_eq!(audio.releases, [] as [u64; 0]);
        assert_eq!(audio.starts.len(), 2);
        // Even without focus, releases are delivered, and velocity zero means off.
        app.help_visible = true;
        sender.receive_at(&[0x90, 64, 0], later);
        session.poll(&mut app, &mut audio, &mut transport, later);
        assert_eq!(audio.releases, [audio.starts[1].1]);
        assert_eq!(audio.starts[0].0.duration, crate::audio::DEFAULT_NOTE_DURATION);
        assert_ne!(
            app.project.patterns[0].event_at(0, crate::musical_time::Tick(0)),
            recorded.patterns[0].event_at(0, crate::musical_time::Tick(0))
        );
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, crate::musical_time::Tick(0))
                .unwrap()
                .duration_ticks
                .ticks(),
            1920
        );
    }

    #[test]
    fn loop_focus_reset_reconnect_and_repeated_pitch_do_not_reuse_tokens() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        for _ in 0..2 {
            sender.receive_at(&[0x90, 60, 80], now);
        }
        sender.receive_at(&[0x80, 60, 0], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        assert_eq!(audio.releases, [audio.starts[1].1]);
        assert!(audio.starts[1].1 > audio.starts[0].1);
        session.terminal_key(
            &mut app,
            &mut audio,
            &mut transport,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            now,
            now,
        );
        assert!(audio.resets > 0);
        assert!(session.current.is_none());
        app.mode = crate::state::Mode::Audition;
        let (new_sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        sender.receive_at(&[0x80, 60, 0], now);
        new_sender.receive_at(&[0x90, 64, 80], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        assert!(audio.starts[2].1 > audio.starts[1].1);
        assert_eq!(audio.releases.len(), 1);
        session.disconnect(&mut app, &mut audio);
        assert!(session.current.is_none());
        assert_eq!(session.held, [] as [(u8, u8); 0]);
    }

    #[test]
    fn held_monitor_suppresses_owned_song_on_later_loops() {
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        sender.receive_at(&[0x90, 60, 80], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        let length = app.project.patterns[0].length_ticks().0;
        for cycle in 1..=2 {
            let at = now + Duration::from_nanos(length * cycle * 1_000_000_000 / 1920);
            for event in transport.update_app(at, &mut app, || {}) {
                audio.play_event(event);
            }
            assert!(audio.songs.is_empty(), "owned note replayed on loop {cycle}");
        }
        let off = now + Duration::from_nanos((length * 2 + 240) * 1_000_000_000 / 1920);
        sender.receive_at(&[0x80, 60, 0], off);
        session.poll(&mut app, &mut audio, &mut transport, off);
        assert!(audio.songs.is_empty());
        let at = now + Duration::from_nanos(length * 3 * 1_000_000_000 / 1920);
        assert_eq!(transport.update_app(at, &mut app, || {}).len(), 1);
    }

    #[test]
    fn held_monitor_disarm_keeps_ownership_until_matching_release() {
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        sender.receive_at(&[0x90, 60, 80], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        transport.update_app(now + Duration::from_millis(250), &mut app, || {});
        app.mode = crate::state::Mode::Normal;
        app.execute_prompt("record off");
        app.mode = crate::state::Mode::Insert;
        let at = now + Duration::from_secs(8);
        assert!(transport.update_app(at, &mut app, || {}).is_empty());
        sender.receive_at(&[0x81, 60, 0], at);
        session.poll(&mut app, &mut audio, &mut transport, at);
        assert!(transport
            .update_app(now + Duration::from_secs(16), &mut app, || {})
            .is_empty());
        sender.receive_at(&[0x80, 60, 0], now + Duration::from_millis(16250));
        session.poll(&mut app, &mut audio, &mut transport, now + Duration::from_millis(16250));
        assert_eq!(
            transport
                .update_app(now + Duration::from_secs(24), &mut app, || {})
                .len(),
            1
        );
    }

    #[test]
    fn held_monitor_never_suppresses_unrelated_or_preexisting_notes() {
        use crate::{musical_time::Tick, tracker::PatternEvent};
        for existing in [false, true] {
            let (mut session, mut app, mut transport, now) = recording_loop();
            let event = PatternEvent::from_note(Tick(0), note(60, app.entry_length).unwrap());
            app.edit(|edit| {
                edit.set_event(0, 1, Tick(0), Some(event));
                if existing {
                    edit.set_event(0, 0, Tick(0), Some(event));
                }
            });
            let (sender, queue) = InputQueue::new(now);
            session.queue = Some(queue);
            sender.receive_at(&[0x90, 60, 80], now);
            session.poll(&mut app, &mut Monitor::default(), &mut transport, now);
            let events = transport.update_app(now + Duration::from_secs(8), &mut app, || {});
            assert_eq!(events.len(), if existing { 2 } else { 1 });
            assert!(events.iter().any(|event| event.track == 1));
        }
    }

    #[test]
    fn held_monitor_old_release_cannot_clear_replacement_or_cross_patterns() {
        use crate::musical_time::Tick;
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        for (bytes, milliseconds) in [([0x90, 60, 80], 0), ([0x90, 64, 80], 125), ([0x80, 60, 0], 250)] {
            let at = now + Duration::from_millis(milliseconds);
            sender.receive_at(&bytes, at);
            session.poll(&mut app, &mut audio, &mut transport, at);
        }
        let mut cloned = app.project.clone();
        cloned.patterns.push(cloned.patterns[0].clone());
        assert!(app.recording.audible(&cloned, 1, 0, Tick(240), 240));
        let events = transport.update_app(now + Duration::from_secs(8), &mut app, || {});
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].note.pitch, crate::tracker::NotePitch::C);
        assert!(transport
            .update_app(now + Duration::from_millis(8125), &mut app, || {})
            .is_empty());
        assert_eq!(audio.releases, [] as [u64; 0]);
    }

    #[test]
    fn held_monitor_edits_and_resets_revoke_suppression() {
        use crate::musical_time::{DurationTicks, Tick};
        for action in ["edit", "undo", "disconnect", "overflow", "replace"] {
            let (mut session, mut app, mut transport, now) = recording_loop();
            let (sender, queue) = InputQueue::new(now);
            session.queue = Some(queue);
            let mut audio = Monitor::default();
            sender.receive_at(&[0x90, 60, 80], now);
            session.poll(&mut app, &mut audio, &mut transport, now);
            match action {
                "edit" => {
                    let mut event = app.project.patterns[0].event_at(0, Tick(0)).unwrap();
                    event.duration_ticks = DurationTicks::new(17).unwrap();
                    app.edit(|edit| edit.set_event(0, 0, Tick(0), Some(event)));
                }
                "undo" => {
                    app.recording.armed = false;
                    app.mode = crate::state::Mode::Normal;
                    app.handle_command(crate::command::Command::Undo);
                    app.handle_command(crate::command::Command::Redo);
                }
                "disconnect" => session.disconnect(&mut app, &mut audio),
                "overflow" => {
                    for _ in 0..=CAPACITY {
                        sender.receive_at(&[0x90, 64, 80], now);
                    }
                    session.poll(&mut app, &mut audio, &mut transport, now);
                }
                "replace" => {
                    app.recording.armed = false;
                    sender.receive_at(&[0x90, 64, 80], now + Duration::from_millis(250));
                    session.poll(&mut app, &mut audio, &mut transport, now + Duration::from_millis(250));
                    sender.receive_at(&[0x80, 60, 0], now + Duration::from_millis(500));
                    session.poll(&mut app, &mut audio, &mut transport, now + Duration::from_millis(500));
                    assert_eq!(audio.releases, [] as [u64; 0]);
                }
                _ => unreachable!(),
            }
            assert_eq!(
                transport
                    .update_app(now + Duration::from_secs(8), &mut app, || {})
                    .len(),
                1,
                "{action}"
            );
        }
    }

    fn recording_loop() -> (Session, crate::state::App, crate::transport::Transport, Instant) {
        let now = Instant::now();
        let mut app = crate::state::App::default();
        app.recording.armed = true;
        app.toggle_playback();
        app.mode = crate::state::Mode::Insert;
        let mut transport = crate::transport::Transport::default();
        transport.update_app(now, &mut app, || {});
        (Session::default(), app, transport, now)
    }
    fn terminal(
        session: &mut Session,
        app: &mut crate::state::App,
        transport: &mut crate::transport::Transport,
        at: Instant,
        ch: char,
    ) {
        session.terminal_key(
            app,
            &mut Silent,
            transport,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            ),
            at,
            at,
        );
    }
    #[test]
    fn loop_disconnected_terminal_pass_has_single_undo() {
        let (mut session, mut app, mut transport, now) = recording_loop();
        let before = app.project.clone();
        terminal(&mut session, &mut app, &mut transport, now, 'z');
        terminal(
            &mut session,
            &mut app,
            &mut transport,
            now + Duration::from_millis(125),
            'x',
        );
        app.mode = crate::state::Mode::Normal;
        app.handle_command(crate::command::Command::Undo);
        assert_eq!(app.project, before);
    }
    #[test]
    fn loop_future_midi_does_not_drop_earlier_terminal() {
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        sender.receive_at(&[0x90, 60, 80], now);
        sender.receive_at(&[0x90, 64, 80], now + Duration::from_millis(250));
        terminal(
            &mut session,
            &mut app,
            &mut transport,
            now + Duration::from_millis(125),
            'x',
        );
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, crate::musical_time::Tick(240))
                .unwrap()
                .pitch,
            crate::tracker::NotePitch::D
        );
        session.poll_before(
            &mut app,
            &mut Silent,
            &mut transport,
            Some(now + Duration::from_millis(250)),
        );
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, crate::musical_time::Tick(480))
                .unwrap()
                .pitch,
            crate::tracker::NotePitch::E
        );
    }

    #[test]
    fn cutoff_retains_future_and_reselection_discards_it() {
        let now = Instant::now();
        let future = now + Duration::from_secs(1);
        let (old, mut queue) = InputQueue::new(now);
        old.receive_at(&[0x90, 60, 1], future);
        assert_eq!(events(queue.drain_before(Some(now))), []);
        assert_eq!(events(queue.drain_before(Some(future)))[0].message.pitch, 60);
        old.receive_at(&[0x90, 61, 1], future);
        assert_eq!(events(queue.drain_before(Some(now))), []);
        let new = queue.reselect(now);
        old.receive_at(&[0x90, 62, 1], future);
        new.receive_at(&[0x90, 63, 1], now);
        assert_eq!(events(queue.drain())[0].message.pitch, 63);
        assert_eq!(events(queue.drain()), []);
    }
    #[test]
    fn loop_budget_defers_key_and_ties_use_midi_before_terminal() {
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, mut queue) = InputQueue::new(now);
        queue.set_channel(Some(1));
        session.queue = Some(queue);
        for _ in 0..MAX_DRAIN {
            sender.receive_at(&[0x90, 60, 1], now);
        }
        sender.receive_at(&[0x91, 64, 1], now);
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(!session.terminal_key(&mut app, &mut Silent, &mut transport, key, now, now));
        assert!(app.project.patterns[0]
            .event_at(0, crate::musical_time::Tick(0))
            .is_none());
        assert!(session.terminal_key(&mut app, &mut Silent, &mut transport, key, now, now));
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, crate::musical_time::Tick(0))
                .unwrap()
                .pitch,
            crate::tracker::NotePitch::D
        );
    }
    #[test]
    fn retained_future_overflow_still_requires_reset() {
        let now = Instant::now();
        let (sender, mut queue) = InputQueue::new(now);
        sender.receive_at(&[0x90, 60, 1], now + Duration::from_secs(1));
        assert_eq!(events(queue.drain_before(Some(now))), []);
        for _ in 0..=CAPACITY {
            sender.receive_at(&[0x90, 60, 1], now);
        }
        assert_eq!(queue.drain_before(Some(now)), InputBatch::ResetRequired);
    }

    #[test]
    fn loop_same_cell_later_midi_wins_after_terminal() {
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        sender.receive_at(&[0x90, 64, 80], now + Duration::from_millis(110));
        terminal(
            &mut session,
            &mut app,
            &mut transport,
            now + Duration::from_millis(100),
            'x',
        );
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, crate::musical_time::Tick(240))
                .unwrap()
                .pitch,
            crate::tracker::NotePitch::D
        );
        session.poll_before(
            &mut app,
            &mut Silent,
            &mut transport,
            Some(now + Duration::from_millis(110)),
        );
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, crate::musical_time::Tick(240))
                .unwrap()
                .pitch,
            crate::tracker::NotePitch::E
        );
    }

    #[test]
    fn loop_undo_invalidates_retained_packet() {
        let (mut session, mut app, mut transport, now) = recording_loop();
        terminal(&mut session, &mut app, &mut transport, now, 'z');
        app.mode = crate::state::Mode::Normal;
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        sender.receive_at(&[0x90, 64, 80], now + Duration::from_millis(100));
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('u'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(session.terminal_key(
            &mut app,
            &mut Silent,
            &mut transport,
            key,
            now + Duration::from_millis(50),
            now + Duration::from_millis(150)
        ));
        app.mode = crate::state::Mode::Insert;
        session.poll_before(
            &mut app,
            &mut Silent,
            &mut transport,
            Some(now + Duration::from_millis(200)),
        );
        assert!(app.project.patterns[0]
            .event_at(0, crate::musical_time::Tick(240))
            .is_none());
        assert!(app.project.patterns[0]
            .event_at(0, crate::musical_time::Tick(0))
            .is_none());
    }

    #[test]
    fn stale_cutoff_packets_count_against_budget_and_retained_filter_is_current() {
        let now = Instant::now();
        let (sender, mut queue) = InputQueue::new(now);
        for _ in 0..MAX_DRAIN {
            sender.receive_at(&[0x90, 60, 1], now.checked_sub(Duration::from_secs(1)).unwrap());
        }
        sender.receive_at(&[0x90, 64, 1], now + Duration::from_secs(1));
        assert_eq!(events(queue.drain_before(Some(now))), []);
        assert!(queue.budget_exhausted);
        assert_eq!(events(queue.drain_before(Some(now))), []);
        queue.set_channel(Some(1));
        assert_eq!(events(queue.drain()), []);
    }

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
    fn held_capture_quantizes_both_boundaries_ties_forward_and_minimum_one_tick() {
        use crate::musical_time::Tick;
        for (on, off, start, duration) in [(0, 0, 0, 1), (0, 120, 0, 240), (120, 360, 240, 240), (119, 359, 0, 240)] {
            let (mut session, mut app, mut transport, now) = recording_loop();
            let (sender, queue) = InputQueue::new(now);
            session.queue = Some(queue);
            let mut audio = Monitor::default();
            for (bytes, tick) in [([0x90, 60, 80], on), ([0x80, 60, 0], off)] {
                let at = now + Duration::from_nanos(tick * 1_000_000_000 / 1920);
                sender.receive_at(&bytes, at);
                session.poll(&mut app, &mut audio, &mut transport, at);
            }
            assert_eq!(
                app.project.patterns[0]
                    .event_at(0, Tick(start))
                    .unwrap()
                    .duration_ticks
                    .ticks(),
                duration
            );
            assert_eq!(audio.starts.len(), 1);
        }
    }

    #[test]
    fn held_capture_uses_transport_tempo_segments_not_release_bpm() {
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        sender.receive_at(&[0x90, 60, 80], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        app.project.bpm = 60.0;
        let at = now + Duration::from_millis(375);
        sender.receive_at(&[0x80, 60, 0], at);
        session.poll(&mut app, &mut audio, &mut transport, at);
        // Old tempo through committed tick 240 (125ms), then 240 ticks at 60 BPM.
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, crate::musical_time::Tick(0))
                .unwrap()
                .duration_ticks
                .ticks(),
            480
        );
    }

    #[test]
    fn held_capture_crosses_loop_without_joining_the_next_pass() {
        use crate::{command::Command, musical_time::Tick, state::Mode};
        let (mut session, mut app, mut transport, now) = recording_loop();
        let length = app.project.patterns[0].length_ticks().0;
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        for (bytes, tick) in [
            ([0x90, 60, 80], length - 240),
            ([0x90, 64, 80], length),
            ([0x80, 60, 0], length + 240),
        ] {
            let at = now + Duration::from_nanos(tick * 1_000_000_000 / 1920);
            sender.receive_at(&bytes, at);
            session.poll(&mut app, &mut audio, &mut transport, at);
        }
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, Tick(length - 240))
                .unwrap()
                .duration_ticks
                .ticks(),
            480
        );
        app.mode = Mode::Normal;
        app.handle_command(Command::Undo);
        assert!(app.project.patterns[0].event_at(0, Tick(length - 240)).is_some());
        // Undo before Note Off never restores an absent onset.
        app.handle_command(Command::Undo);
        let before = app.project.clone();
        let at = now + Duration::from_secs(20);
        sender.receive_at(&[0x80, 64, 0], at);
        session.poll(&mut app, &mut audio, &mut transport, at);
        assert_eq!(app.project, before);
    }

    #[test]
    fn held_capture_stop_and_disarm_finalize_at_last_transport_position() {
        use crate::{command::Command, musical_time::Tick, state::Mode};
        for stop in [true, false] {
            let (mut session, mut app, mut transport, now) = recording_loop();
            let (sender, queue) = InputQueue::new(now);
            session.queue = Some(queue);
            let mut audio = Monitor::default();
            sender.receive_at(&[0x90, 60, 80], now);
            session.poll(&mut app, &mut audio, &mut transport, now);
            transport.update_app(now + Duration::from_millis(500), &mut app, || {});
            if stop {
                app.handle_command(Command::TogglePlayback);
            } else {
                app.mode = Mode::Normal;
                app.execute_prompt("record off");
            }
            assert_eq!(
                app.project.patterns[0]
                    .event_at(0, Tick(0))
                    .unwrap()
                    .duration_ticks
                    .ticks(),
                960
            );
        }
    }

    #[test]
    fn held_capture_does_not_overwrite_an_identity_preserving_edit() {
        use crate::musical_time::{DurationTicks, Tick};
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        sender.receive_at(&[0x90, 60, 80], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        let id = app.project.patterns[0].tracks[0].event_id(Tick(0)).unwrap();
        let mut event = app.project.patterns[0].event_at(0, Tick(0)).unwrap();
        event.duration_ticks = DurationTicks::new(17).unwrap();
        app.edit(|edit| edit.set_event(0, 0, Tick(0), Some(event)));
        app.project.patterns[0].tracks[0].restore_id(Tick(0), id);
        let at = now + Duration::from_secs(1);
        sender.receive_at(&[0x80, 60, 0], at);
        session.poll(&mut app, &mut audio, &mut transport, at);
        assert_eq!(app.project.patterns[0].event_at(0, Tick(0)), Some(event));
    }

    #[test]
    fn held_capture_retrigger_and_old_channel_release_have_distinct_ownership() {
        use crate::musical_time::Tick;
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        for (bytes, millis) in [
            ([0x90, 60, 80], 0),
            ([0x90, 60, 80], 250),
            ([0x81, 60, 0], 375),
            ([0x90, 60, 0], 750),
        ] {
            let at = now + Duration::from_millis(millis);
            sender.receive_at(&bytes, at);
            session.poll(&mut app, &mut audio, &mut transport, at);
        }
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, Tick(0))
                .unwrap()
                .duration_ticks
                .ticks(),
            480
        );
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, Tick(480))
                .unwrap()
                .duration_ticks
                .ticks(),
            960
        );
        assert_eq!(audio.releases, [audio.starts[1].1]);
    }

    #[test]
    fn held_capture_load_and_source_reset_discard_pending_finalization() {
        use crate::musical_time::Tick;
        for action in ["load", "disconnect", "overflow"] {
            let (mut session, mut app, mut transport, now) = recording_loop();
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("held.json");
            crate::project_file::save(&app.project, &path).unwrap();
            let (sender, queue) = InputQueue::new(now);
            session.queue = Some(queue);
            let mut audio = Monitor::default();
            sender.receive_at(&[0x90, 60, 80], now);
            session.poll(&mut app, &mut audio, &mut transport, now);
            match action {
                "load" => app.execute_prompt(&format!("e! {}", path.display())),
                "disconnect" => session.disconnect(&mut app, &mut audio),
                _ => {
                    for _ in 0..=CAPACITY {
                        sender.receive_at(&[0x80, 60, 0], now);
                    }
                    session.poll(&mut app, &mut audio, &mut transport, now);
                }
            }
            let before = app.project.clone();
            let at = now + Duration::from_secs(1);
            sender.receive_at(&[0x80, 60, 0], at);
            session.poll(&mut app, &mut audio, &mut transport, at);
            assert_eq!(app.project, before);
            if action == "load" {
                assert!(app.project.patterns[0].event_at(0, Tick(0)).is_none());
            }
        }
    }

    #[test]
    fn held_capture_clamps_long_gate_visibly_without_adding_events() {
        use crate::musical_time::{Tick, MAX_DURATION_TICKS};
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        sender.receive_at(&[0x90, 60, 80], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        let at = now + Duration::from_secs(40);
        sender.receive_at(&[0x80, 60, 0], at);
        session.poll(&mut app, &mut audio, &mut transport, at);
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, Tick(0))
                .unwrap()
                .duration_ticks
                .ticks(),
            MAX_DURATION_TICKS
        );
        assert_eq!(app.project.patterns[0].tracks[0].events().len(), 1);
        assert!(app.status.contains("clamped"));
    }

    #[test]
    fn held_capture_uses_active_triplet_grid_and_pins_midbar_grid_edits() {
        use crate::{musical_time::Tick, state::Mode};
        let (mut session, mut app, mut transport, now) = recording_loop();
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        sender.receive_at(&[0x90, 60, 80], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        app.mode = Mode::Normal;
        app.execute_prompt("grid 3");
        app.mode = Mode::Insert;
        // Current bar retains sixteenths despite the document grid edit.
        let at = now + Duration::from_millis(250);
        sender.receive_at(&[0x80, 60, 0], at);
        session.poll(&mut app, &mut audio, &mut transport, at);
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, Tick(0))
                .unwrap()
                .duration_ticks
                .ticks(),
            480
        );
        // Next bar uses triplets; 400 ticks rounds down to 320, not 480.
        let next = now + Duration::from_secs(2);
        sender.receive_at(&[0x90, 64, 80], next);
        session.poll(&mut app, &mut audio, &mut transport, next);
        let at = next + Duration::from_nanos(208_333_333);
        sender.receive_at(&[0x80, 64, 0], at);
        session.poll(&mut app, &mut audio, &mut transport, at);
        assert_eq!(
            app.project.patterns[0]
                .event_at(0, Tick(3840))
                .unwrap()
                .duration_ticks
                .ticks(),
            320
        );
    }

    #[test]
    fn held_capture_32nd_grid_and_precount_hold_do_not_create_zero_or_phantom_notes() {
        use crate::{
            musical_time::Tick,
            state::{App, Mode},
            transport::Transport,
        };
        for count_in in [false, true] {
            let now = Instant::now();
            let mut app = App::default();
            app.execute_prompt("grid 8"); // eight divisions per quarter: 32nd notes
            app.recording.armed = true;
            app.recording.count_in = count_in;
            app.toggle_playback();
            app.mode = Mode::Insert;
            let mut transport = Transport::default();
            transport.update_app(now, &mut app, || {});
            let mut session = Session::default();
            let (sender, queue) = InputQueue::new(now);
            session.queue = Some(queue);
            let mut audio = Monitor::default();
            sender.receive_at(&[0x90, 60, 80], now);
            session.poll(&mut app, &mut audio, &mut transport, now);
            let at = now
                + if count_in {
                    Duration::from_secs(3)
                } else {
                    Duration::from_micros(31_250)
                };
            sender.receive_at(&[0x80, 60, 0], at);
            session.poll(&mut app, &mut audio, &mut transport, at);
            if count_in {
                assert_eq!(app.project.patterns[0].tracks[0].events(), []);
            } else {
                assert_eq!(
                    app.project.patterns[0]
                        .event_at(0, Tick(0))
                        .unwrap()
                        .duration_ticks
                        .ticks(),
                    120
                );
            }
        }
    }

    #[test]
    fn held_capture_noop_onset_does_not_join_an_unrelated_existing_edit() {
        use crate::{command::Command, musical_time::Tick, state::Mode, tracker::PatternEvent};
        let (mut session, mut app, mut transport, now) = recording_loop();
        let event = PatternEvent::from_note(Tick(0), note(60, app.entry_length).unwrap());
        app.edit(|edit| edit.set_event(0, 0, Tick(0), Some(event)));
        let (sender, queue) = InputQueue::new(now);
        session.queue = Some(queue);
        let mut audio = Monitor::default();
        sender.receive_at(&[0x90, 60, 80], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        let at = now + Duration::from_millis(500);
        sender.receive_at(&[0x80, 60, 0], at);
        session.poll(&mut app, &mut audio, &mut transport, at);
        app.mode = Mode::Normal;
        app.handle_command(Command::Undo);
        assert_eq!(app.project.patterns[0].event_at(0, Tick(0)), Some(event));
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
