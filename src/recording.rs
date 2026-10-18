//! Session capture decisions. Terminal and future device input share this clock domain.
use std::{collections::BTreeMap, time::Instant};

use crate::{
    history::Revision,
    musical_time::{GridMap, Tick},
    state::{App, Mode, PlaybackMode, PlaybackState, View},
    tracker::{Note, PatternEvent},
    transport::{ClockSnapshot, TransportPhase},
};

#[derive(Clone, Debug, Default)]
pub struct Recording {
    pub armed: bool,
    pub count_in: bool,
    pub phase: TransportPhase,
    pub target: Option<(usize, usize)>,
    pub(crate) clock: Option<ClockSnapshot>,
    pub(crate) input_time: Option<Instant>,
    last_input: Option<Instant>,
    take: Option<(u64, Revision)>,
    grid: Option<(u64, GridMap)>,
    monitored: BTreeMap<(usize, u64), u64>,
}

impl Recording {
    pub(crate) fn owns_revision(&self, revision: Revision) -> bool {
        self.take.is_some_and(|(_, take_revision)| take_revision == revision)
    }

    pub(crate) const fn close_take(&mut self) {
        self.take = None;
    }

    pub(crate) fn stop(&mut self) {
        self.target = None;
        self.clock = None;
        self.phase = TransportPhase::Stopped;
        self.take = None;
        self.grid = None;
        self.last_input = None;
        self.monitored.clear();
    }

    pub(crate) fn audible(&mut self, track: usize, local: Tick, absolute: u64) -> bool {
        self.monitored.retain(|_, at| *at >= absolute);
        self.monitored.get(&(track, local.0)) != Some(&absolute)
    }
}

impl App {
    pub(crate) fn recording_command(&mut self, command: &str, value: &str) -> anyhow::Result<()> {
        anyhow::ensure!(self.mode == Mode::Normal, "Recording controls require Normal mode");
        if command == "countin" {
            self.recording.count_in = match value {
                "0" => false,
                "1" => true,
                _ => anyhow::bail!("Expected :countin 0|1 (bars)"),
            };
            self.status = format!("Count-in: {} bar(s)", u8::from(self.recording.count_in));
            return Ok(());
        }
        let armed = match value {
            "on" => true,
            "off" => false,
            "toggle" => !self.recording.armed,
            _ => anyhow::bail!("Expected :record on|off|toggle"),
        };
        if armed != self.recording.armed {
            self.recording.close_take();
            self.recording.grid = None;
            self.recording.last_input = None;
            self.recording.armed = armed;
            if !armed {
                if matches!(self.recording.phase, TransportPhase::Counting { .. }) {
                    self.stop_playback();
                }
                self.recording.target = None;
            } else if matches!(self.playback_state, PlaybackState::Playing)
                && self.playback_mode == PlaybackMode::Pattern
            {
                self.recording.target = Some((self.playing_pattern, self.cursor_track));
            }
        }
        self.status = self.recording_label();
        Ok(())
    }

    #[must_use]
    pub fn capture_eligible(&self) -> bool {
        self.recording.armed
            && self.recording.clock.is_some()
            && self.recording.phase == TransportPhase::Playing
            && matches!(self.playback_state, PlaybackState::Playing)
            && self.playback_mode == PlaybackMode::Pattern
            && self.view == View::Pattern
            && self.mode == Mode::Insert
            && !self.help_visible
            && self.prompt.is_none()
            && !self.shutdown
            && self.recording.target.is_some_and(|(pattern, track)| {
                pattern == self.playing_pattern
                    && pattern == self.selected_pattern
                    && self
                        .project
                        .patterns
                        .get(pattern)
                        .is_some_and(|p| track < p.tracks.len())
            })
    }

    pub(crate) fn reconcile_recording(&mut self) {
        if !self.capture_eligible() {
            self.recording.close_take();
        }
    }

    pub(crate) fn sync_recording(&mut self, phase: TransportPhase, snapshot: Option<ClockSnapshot>, now: Instant) {
        if self
            .recording
            .clock
            .zip(snapshot)
            .is_some_and(|(old, new)| old.generation() != new.generation())
        {
            self.recording.close_take();
            self.recording.grid = None;
            self.recording.last_input = None;
            self.recording.monitored.clear();
        }
        self.recording.phase = phase;
        self.recording.clock = snapshot;
        if let (Some(clock), Some((pattern, _)), Some((pass, _))) =
            (snapshot, self.recording.target, self.recording.take)
        {
            let length = self.project.patterns[pattern].length_ticks().0;
            if clock
                .tick_at(now)
                .is_none_or(|tick| tick.max(clock.previous_tick()) / length != pass)
            {
                self.recording.close_take();
            }
        }
        if let (Some(clock), Some((pattern, _))) = (snapshot, self.recording.target) {
            let current = self.project.patterns[pattern].grid();
            if let Some(absolute) = clock.tick_at(now) {
                let absolute = absolute.max(clock.previous_tick());
                let length = current.end().0;
                let bar = absolute / length * length + current.meter().bar_start(Tick(absolute % length)).0;
                if self.recording.grid.as_ref().is_none_or(|(active, _)| *active != bar) {
                    self.recording.grid = Some((bar, current.clone()));
                }
            }
        }
        self.reconcile_recording();
    }

    pub(crate) fn live_note(&mut self, note: Note) {
        // Preview remains available even when capture is suspended or a hit is rejected.
        let eligible = self.capture_eligible();
        let preview_track = if eligible {
            self.recording.target.unwrap().1
        } else {
            self.cursor_track
        };
        self.preview_note_on(note, preview_track);
        if !eligible {
            return;
        }
        if let Err(error) = self.capture_note(note) {
            self.status = format!("Recording: {error}");
        }
    }

    fn capture_note(&mut self, note: Note) -> anyhow::Result<()> {
        let clock = self.recording.clock.expect("eligible clock");
        let time = self
            .recording
            .input_time
            .ok_or_else(|| anyhow::anyhow!("missing input timestamp"))?;
        anyhow::ensure!(
            self.recording.last_input.is_none_or(|last| time >= last),
            "out-of-order input dropped"
        );
        let absolute = clock
            .tick_at(time)
            .ok_or_else(|| anyhow::anyhow!("input predates this run; dropped"))?;
        anyhow::ensure!(absolute >= clock.previous_tick(), "late input dropped");
        self.recording.last_input = Some(time);
        let (pattern, track) = self.recording.target.expect("eligible target");
        let current = self.project.patterns[pattern].grid();
        let length = current.end().0;
        let pass = absolute / length;
        let local = Tick(absolute % length);
        let bar = pass * length + current.meter().bar_start(local).0;
        if self.recording.grid.as_ref().is_none_or(|(active, _)| *active != bar) {
            self.recording.grid = Some((bar, current.clone()));
        }
        let grid = &self.recording.grid.as_ref().unwrap().1;
        let (lower, upper) = grid
            .neighbors(local)
            .map_err(|error| anyhow::anyhow!("invalid capture grid: {error:?}"))?;
        let snapped = if local.0 - lower.0 < upper.0 - local.0 {
            lower
        } else {
            upper
        };
        let destination = pass
            .checked_mul(length)
            .and_then(|prefix| prefix.checked_add(snapped.0))
            .ok_or_else(|| anyhow::anyhow!("timeline exhausted"))?;
        let start = Tick(destination % length);
        if self.recording.take.is_some_and(|(previous, _)| previous != pass) {
            self.recording.close_take();
        }
        let before = self.revision();
        let mut event = PatternEvent::from_note(start, note);
        if let Some(duration) = self.entry_duration {
            event.duration_ticks = duration;
        }
        let mut result = Ok(());
        self.history.edit(&mut self.project, |edit| {
            result = edit.try_set_event(pattern, track, start, Some(event));
        });
        result?;
        if self.revision() != before {
            let retained =
                self.recording.take != Some((pass, before)) || self.history.join_record_edits(before, pattern, track);
            self.recording.take = retained.then_some((pass, self.revision()));
        }
        // Monitor now; don't also dispatch this newly stored occurrence as a song voice.
        if destination > clock.previous_tick() {
            self.recording.monitored.insert((track, start.0), destination);
        }
        Ok(())
    }

    #[must_use]
    pub fn recording_label(&self) -> String {
        if let TransportPhase::Counting { beat, total } = self.recording.phase {
            let meter = self.project.patterns[self.playing_pattern].grid().meter();
            return format!(
                "COUNT {}/{total} {}/{}",
                beat.max(1),
                meter.numerator(),
                meter.denominator()
            );
        }
        if let Some((pattern, track)) = self.recording.target.filter(|_| self.capture_eligible()) {
            return format!("REC P{pattern} T{}", track + 1);
        }
        if self.recording.armed {
            if self.playback_mode == PlaybackMode::Song {
                "REC ARMED: Song unavailable".into()
            } else if matches!(self.playback_state, PlaybackState::Stopped) {
                "REC ARMED".into()
            } else if let Some((pattern, track)) = self.recording.target {
                format!("REC ARMED PREVIEW P{pattern} T{}", track + 1)
            } else {
                "REC ARMED PREVIEW".into()
            }
        } else {
            "PREVIEW".into()
        }
    }
}
