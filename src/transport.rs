//! Device-free playback clock. Anchored tick deadlines, not redraws, own advancement.
#[path = "transport_clock.rs"]
mod clock;
use std::time::{Duration, Instant};

use crate::tracker::{Instrument, Note, Pattern};

pub const DEFAULT_ROWS_PER_BEAT: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct RowEvent {
    pub row: usize,
    pub beats: f64,
    pub duration: Duration,
}

#[derive(Clone, Copy, Debug)]
pub struct NoteEvent {
    pub track: usize,
    pub instrument: Instrument,
    /// Pitch/octave and legacy entry metadata; `duration` owns the scheduled gate.
    pub note: Note,
    pub beats: f64,
    pub duration: Duration,
}

/// Collect notes without constructing voices or opening an output device.
#[must_use]
pub fn collect_row(pattern: &Pattern, instruments: &[Instrument], event: RowEvent) -> Vec<NoteEvent> {
    if event.row >= pattern.num_rows() {
        return Vec::new();
    }
    pattern
        .tracks
        .iter()
        .enumerate()
        .filter_map(|(track, _)| {
            let note = pattern.get_step(track, event.row)?.note?;
            let rows = f64::from(note.length_ticks.ticks()) / f64::from(crate::tracker::NoteLength::TICKS_PER_ROW);
            Some(NoteEvent {
                track,
                instrument: instruments.get(track).copied().unwrap_or_default(),
                note,
                beats: event.beats * rows,
                duration: event.duration.mul_f64(rows),
            })
        })
        .collect()
}

/// Collect exactly one canonical start per track, never a projected row bucket.
#[must_use]
pub fn collect_tick(pattern: &Pattern, instruments: &[Instrument], event: PlaybackEvent) -> Vec<NoteEvent> {
    let Some(clock) = clock::TickClock::new(event.scheduled_at, event.absolute_tick, event.bpm) else {
        return Vec::new();
    };
    pattern
        .tracks
        .iter()
        .enumerate()
        .filter_map(|(track, values)| {
            let index = values
                .events()
                .binary_search_by_key(&event.tick, |value| value.start_tick)
                .ok()?;
            let value = values.events()[index];
            Some(NoteEvent {
                track,
                instrument: instruments.get(track).copied().unwrap_or_default(),
                note: Note {
                    pitch: value.pitch,
                    octave: value.octave,
                    length_ticks: u16::try_from(value.duration_ticks.ticks() / 15)
                        .ok()
                        .filter(|_| value.duration_ticks.ticks().is_multiple_of(15))
                        .and_then(|ticks| crate::tracker::NoteLength::try_from(ticks).ok())
                        .unwrap_or_default(),
                },
                beats: f64::from(value.duration_ticks.ticks()) / 960.0,
                duration: clock.duration(u64::from(value.duration_ticks.ticks()))?,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub enum PlaybackRoute {
    Pattern(usize),
    Song,
}

#[derive(Clone, Copy, Debug)]
pub struct PlaybackEvent {
    pub pattern: usize,
    pub order: Option<usize>,
    pub row: RowEvent,
    pub tick: crate::musical_time::Tick,
    pub absolute_tick: u64,
    pub generation: u64,
    pub bpm: f64,
    pub scheduled_at: Instant,
}

/// Immutable tempo segment for timestamp mapping. A restarted run has a new generation.
#[derive(Clone, Copy, Debug)]
pub struct ClockSnapshot {
    generation: u64,
    segment: clock::TickClock,
}

impl ClockSnapshot {
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn anchor(self) -> (Instant, u64) {
        (self.segment.time, self.segment.tick)
    }

    /// Map a timestamp through this segment, never the current UI tempo or cursor.
    #[must_use]
    pub fn tick_at(self, time: Instant) -> Option<u64> {
        self.segment.position(time)
    }
}

#[derive(Default)]
pub struct Transport {
    deadline: Option<Instant>,
    row: usize,
    route: Option<PlaybackRoute>,
    start_offset: usize,
    tick_start: u64,
    next_tick: u64,
    clock: Option<clock::TickClock>,
    generation: u64,
}

impl Transport {
    /// Cancel the old graph before collecting any notes for a new run.
    pub fn update_app(
        &mut self,
        now: Instant,
        app: &mut crate::state::App,
        stop_audio: impl FnOnce(),
    ) -> Vec<NoteEvent> {
        let playing = !app.shutdown && matches!(app.playback_state, crate::state::PlaybackState::Playing);
        if app.take_stop_request() || (!playing && self.deadline.is_some()) || app.shutdown {
            self.stop();
            stop_audio();
        }
        let route = if app.playback_mode == crate::state::PlaybackMode::Song {
            PlaybackRoute::Song
        } else {
            PlaybackRoute::Pattern(app.playing_pattern)
        };
        self.start_at(&app.project, route, app.playing_order, app.playback_row);
        if self.deadline.is_none() {
            if let Some(start) = app.playback_start_tick {
                let pattern = &app.project.patterns[app.playing_pattern];
                let projected = pattern.row_tick(app.playback_row).map_or(0, |tick| tick.0);
                self.tick_start = self
                    .tick_start
                    .saturating_sub(projected)
                    .saturating_add(start.0.min(pattern.length_ticks().0.saturating_sub(1)));
            }
        }
        let mut notes = Vec::new();
        // Small wake jitter catches up in order. Long stalls skip stale music arithmetically.
        for _ in 0..256 {
            let at = self
                .deadline
                .filter(|deadline| now.saturating_duration_since(*deadline) <= Duration::from_millis(50))
                .map_or(now, |deadline| deadline.min(now));
            let Some(event) = self.update_project(at, playing, &app.project, route, DEFAULT_ROWS_PER_BEAT) else {
                break;
            };
            app.playback_tick = event.tick;
            app.playback_row = app.project.patterns[event.pattern].row_at(event.tick);
            app.playing_pattern = event.pattern;
            app.playing_order = event.order.unwrap_or(0);
            notes.extend(collect_tick(
                &app.project.patterns[event.pattern],
                app.project.instruments(),
                event,
            ));
            if self.deadline.is_none_or(|deadline| deadline > now) {
                break;
            }
        }
        notes
    }

    /// Locate the latest elapsed musical boundary; skip whole cycles arithmetically.
    /// Tempo changes keep the next committed deadline and reanchor future ticks there.
    pub fn update_project(
        &mut self,
        now: Instant,
        playing: bool,
        project: &crate::project::Project,
        route: PlaybackRoute,
        rows_per_beat: u32,
    ) -> Option<PlaybackEvent> {
        let Some(spacing) = crate::musical_time::PPQ
            .checked_div(u64::from(rows_per_beat))
            .filter(|spacing| *spacing > 0)
        else {
            self.stop();
            return None;
        };
        if !playing {
            self.stop();
            return None;
        }
        let route = if self.clock.is_some() {
            self.route.unwrap_or(route)
        } else {
            route
        };
        self.route = Some(route);
        let absolute = if let Some(mut clock) = self.clock {
            let boundary = self.deadline?;
            if now < boundary {
                return None;
            }
            if !clock.same_tempo(project.bpm) {
                let Some(new) = clock::TickClock::new(boundary, self.next_tick, project.bpm) else {
                    self.stop();
                    return None;
                };
                clock = new;
                self.clock = Some(clock);
            }
            let Some(target) = clock.position(now) else {
                self.stop();
                return None;
            };
            let Some(position) = clock::locate(project, route, target) else {
                self.stop();
                return None;
            };
            position.previous_boundary(project, spacing).max(self.next_tick)
        } else {
            let Some(clock) = clock::TickClock::new(now, self.tick_start, project.bpm) else {
                self.stop();
                return None;
            };
            self.clock = Some(clock);
            self.generation = self.generation.checked_add(1)?;
            self.tick_start
        };
        let Some(position) = clock::locate(project, route, absolute) else {
            self.stop();
            return None;
        };
        let Some(next) = position.next_boundary(project, spacing) else {
            self.stop();
            return None;
        };
        let clock = self.clock?;
        let Some(deadline) = clock.deadline(next) else {
            self.stop();
            return None;
        };
        self.next_tick = next;
        self.deadline = Some(deadline);
        Some(PlaybackEvent {
            pattern: position.pattern,
            order: position.order,
            tick: crate::musical_time::Tick(position.local),
            absolute_tick: absolute,
            generation: self.generation,
            bpm: project.bpm,
            scheduled_at: clock.deadline(absolute)?,
            row: RowEvent {
                row: usize::try_from(position.local / spacing).ok()?,
                beats: 1.0 / f64::from(rows_per_beat),
                duration: clock.duration(spacing)?,
            },
        })
    }

    /// Set the initial flattened row only before a run; active routes stay pinned.
    pub fn start_at(&mut self, project: &crate::project::Project, route: PlaybackRoute, order: usize, row: usize) {
        if self.deadline.is_some() {
            return;
        }
        self.tick_start = match route {
            PlaybackRoute::Pattern(index) => project.patterns.get(index).map_or(0, |pattern| {
                pattern
                    .row_tick(row.min(pattern.view_rows().saturating_sub(1)))
                    .map_or(0, |tick| tick.0)
            }),
            PlaybackRoute::Song => {
                let order = order.min(project.order.len().saturating_sub(1));
                let prefix = project
                    .order
                    .iter()
                    .take(order)
                    .filter_map(|&index| project.patterns.get(index))
                    .fold(0u64, |sum, pattern| sum.saturating_add(pattern.length_ticks().0));
                prefix.saturating_add(
                    project
                        .order
                        .get(order)
                        .and_then(|&index| project.patterns.get(index))
                        .map_or(0, |pattern| {
                            pattern
                                .row_tick(row.min(pattern.view_rows().saturating_sub(1)))
                                .map_or(0, |tick| tick.0)
                        }),
                )
            }
        };
        self.start_offset = match route {
            PlaybackRoute::Pattern(index) => project
                .patterns
                .get(index)
                .map_or(0, |pattern| row.min(pattern.num_rows().saturating_sub(1))),
            PlaybackRoute::Song => {
                let order = order.min(project.order.len().saturating_sub(1));
                let prefix: usize = project
                    .order
                    .iter()
                    .take(order)
                    .filter_map(|&index| project.patterns.get(index))
                    .map(crate::tracker::Pattern::num_rows)
                    .sum();
                prefix
                    + project
                        .order
                        .get(order)
                        .and_then(|&index| project.patterns.get(index))
                        .map_or(0, |pattern| row.min(pattern.num_rows().saturating_sub(1)))
            }
        };
    }

    #[must_use]
    pub fn clock_snapshot(&self) -> Option<ClockSnapshot> {
        Some(ClockSnapshot {
            generation: self.generation,
            segment: self.clock?,
        })
    }

    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub const fn stop(&mut self) {
        self.clock = None;
        self.tick_start = 0;
        self.next_tick = 0;
        self.deadline = None;
        self.route = None;
        self.start_offset = 0;
    }

    /// Legacy standalone row-clock compatibility. Project/app playback uses tick segments.
    /// Invalid rates or empty patterns stop the clock. A new start emits its chosen row.
    /// Tempo changes apply at the already scheduled boundary. Late updates emit
    /// only the current row, skipping stale rows in constant time. Unrepresentable
    /// deadlines stop the clock without emitting a row.
    pub fn update(
        &mut self,
        now: Instant,
        playing: bool,
        rows: usize,
        bpm: f64,
        rows_per_beat: u32,
    ) -> Option<RowEvent> {
        let seconds = 60.0 / bpm / f64::from(rows_per_beat);
        let period = Duration::try_from_secs_f64(seconds).ok().filter(|d| !d.is_zero());
        let Some(period) = period.filter(|_| playing && rows > 0 && bpm > 0.0 && rows_per_beat > 0) else {
            self.stop();
            return None;
        };
        let Some(boundary) = self.deadline else {
            self.deadline = now.checked_add(period);
            self.deadline?;
            self.row = self.start_offset.min(rows - 1);
            return Some(RowEvent {
                row: self.row,
                beats: 1.0 / f64::from(rows_per_beat),
                duration: period,
            });
        };
        if now < boundary {
            return None;
        }
        let elapsed = now.duration_since(boundary).as_nanos();
        let skipped = elapsed / period.as_nanos();
        let advance = (skipped + 1) % rows as u128;
        let row = usize::try_from((self.row as u128 + advance) % rows as u128).ok()?;
        let remainder = elapsed % period.as_nanos();
        let until_next = period.checked_sub(Duration::new(
            u64::try_from(remainder / 1_000_000_000).ok()?,
            u32::try_from(remainder % 1_000_000_000).ok()?,
        ))?;
        self.deadline = now.checked_add(until_next);
        self.deadline?;
        self.row = row;
        Some(RowEvent {
            row: self.row,
            beats: 1.0 / f64::from(rows_per_beat),
            duration: period,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshots_pin_tempo_mapping_and_restart_generation() {
        let mut project = crate::project::Project::new(4, 1);
        let now = Instant::now();
        let mut transport = Transport::default();
        let first = transport
            .update_project(now, true, &project, PlaybackRoute::Pattern(0), 4)
            .unwrap();
        let old = transport.clock_snapshot().unwrap();
        assert_eq!(old.anchor(), (now, 0));
        assert_eq!(old.tick_at(now + Duration::from_millis(125)), Some(240));
        project.bpm = 60.0;
        let next = transport
            .update_project(
                now + Duration::from_millis(125),
                true,
                &project,
                PlaybackRoute::Pattern(0),
                4,
            )
            .unwrap();
        assert_eq!(first.generation, next.generation);
        let new = transport.clock_snapshot().unwrap();
        assert_eq!(new.anchor(), (now + Duration::from_millis(125), 240));
        assert_eq!(new.tick_at(now + Duration::from_millis(375)), Some(480));
        assert_eq!(old.tick_at(now + Duration::from_millis(375)), Some(720));
        transport.stop();
        assert!(transport.clock_snapshot().is_none());
        transport
            .update_project(now, true, &project, PlaybackRoute::Pattern(0), 4)
            .unwrap();
        assert_ne!(old.generation(), transport.clock_snapshot().unwrap().generation());
    }

    #[test]
    fn tempo_commands_keep_boundary_and_resolve_next_note_duration() {
        use crate::{
            command::Command,
            state::{App, PlaybackState},
            tracker::NotePitch,
        };
        for command in [Command::DecreaseTempo, Command::IncreaseTempo] {
            let mut app = App::default();
            let mut transport = Transport::default();
            let start = Instant::now();
            app.handle_command(command);
            assert!(transport
                .update(
                    start,
                    matches!(app.playback_state, PlaybackState::Playing),
                    64,
                    app.project.bpm,
                    4
                )
                .is_none());
            assert_eq!(transport.deadline(), None);
            app.project.bpm = 120.0;
            app.handle_command(Command::TogglePlayback);
            assert_eq!(
                transport.update(start, true, 64, app.project.bpm, 4).unwrap().duration,
                Duration::from_millis(125)
            );
            let boundary = transport.deadline().unwrap();
            app.handle_command(command);
            assert!(transport
                .update(
                    boundary.checked_sub(Duration::from_nanos(1)).unwrap(),
                    true,
                    64,
                    app.project.bpm,
                    4
                )
                .is_none());
            assert_eq!(transport.deadline(), Some(boundary));
            let event = transport.update(boundary, true, 64, app.project.bpm, 4).unwrap();
            let period = Duration::from_secs_f64(60.0 / app.project.bpm / 4.0);
            assert_eq!(event.row, 1);
            assert_eq!(event.duration, period);
            assert_eq!(transport.deadline(), Some(boundary + period));
            app.project.patterns[app.selected_pattern]
                .set_step(
                    0,
                    1,
                    Some(crate::tracker::Note {
                        length_ticks: crate::tracker::NoteLength::default(),
                        pitch: NotePitch::Cs,
                        octave: 4,
                    }),
                )
                .unwrap();
            assert_eq!(
                collect_row(
                    &app.project.patterns[app.selected_pattern],
                    &app.project.instruments,
                    event
                )[0]
                .duration,
                period
            );
            assert!(transport.update(boundary, true, 64, app.project.bpm, 4).is_none());
        }
    }

    #[test]
    fn exact_boundaries_wrap_without_drift() {
        let start = Instant::now();
        let mut t = Transport::default();
        assert_eq!(t.update(start, true, 4, 120.0, 4).unwrap().row, 0);
        assert_eq!(t.deadline(), Some(start + Duration::from_millis(125)));
        for tick in 1..100_u32 {
            let boundary = start + Duration::from_millis(u64::from(tick) * 125);
            let previous_deadline = t.deadline();
            for lead in [17, 1] {
                assert!(t
                    .update(
                        boundary.checked_sub(Duration::from_millis(lead)).unwrap(),
                        true,
                        4,
                        120.0,
                        4
                    )
                    .is_none());
                assert_eq!(t.deadline(), previous_deadline);
            }
            let delay = u64::from((tick * 37) % 101);
            let now = boundary + Duration::from_millis(delay);
            assert_eq!(
                t.update(now, true, 4, 120.0, 4).unwrap().row,
                usize::try_from(tick % 4).unwrap()
            );
            assert_eq!(
                t.deadline(),
                Some(start + Duration::from_millis(u64::from(tick + 1) * 125))
            );
            assert!(t.update(now, true, 4, 120.0, 4).is_none());
        }
    }

    #[test]
    fn unrepresentable_deadlines_emit_no_rows_and_allow_recovery() {
        let now = Instant::now();
        // This fits Duration but exceeds Instant's range on common platforms.
        // Platforms with a wider Instant range have no overflow to exercise.
        let seconds = 2.0_f64.powi(63);
        let period = Duration::try_from_secs_f64(seconds).unwrap();
        if now.checked_add(period).is_some() {
            return;
        }
        let bpm = 60.0 / seconds;
        let mut t = Transport::default();
        for _ in 0..3 {
            assert!(t.update(now, true, 4, bpm, 1).is_none());
            assert_eq!(t.deadline(), None);
        }
        assert_eq!(t.update(now, true, 4, 120.0, 4).unwrap().row, 0);
        let boundary = t.deadline().unwrap();
        assert!(boundary.checked_add(period).is_none());
        assert!(t.update(boundary, true, 4, bpm, 1).is_none());
        assert_eq!(t.deadline(), None);
        assert!(t.update(boundary, true, 4, bpm, 1).is_none());
        assert_eq!(t.update(boundary, true, 4, 120.0, 4).unwrap().row, 0);
        assert_eq!(t.deadline(), Some(boundary + Duration::from_millis(125)));
    }

    #[test]
    fn start_wrap_skip_stop_restart_and_tempo_boundary() {
        let now = Instant::now();
        let mut t = Transport::default();
        assert_eq!(t.update(now, true, 8, 120.0, 4).unwrap().row, 0);
        assert!(t.update(now + Duration::from_millis(100), true, 8, 60.0, 4).is_none());
        assert_eq!(
            t.update(now + Duration::from_millis(125), true, 8, 60.0, 4)
                .unwrap()
                .row,
            1
        );
        assert_eq!(t.deadline(), Some(now + Duration::from_millis(375)));
        assert_eq!(
            t.update(now + Duration::from_millis(2_500_400), true, 8, 60.0, 4)
                .unwrap()
                .row,
            2
        );
        assert_eq!(t.deadline(), Some(now + Duration::from_millis(2_500_625)));
        assert!(t
            .update(now + Duration::from_millis(2_500_400), true, 8, 60.0, 4)
            .is_none());
        t.stop();
        assert_eq!(t.update(now, true, 8, 120.0, 4).unwrap().row, 0);
        assert!(t.update(now, false, 8, 120.0, 4).is_none());
        for bpm in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE] {
            assert!(t.update(now, true, 8, bpm, 4).is_none());
        }
        assert!(t.update(now, true, 0, 120.0, 4).is_none());
        assert!(t.update(now, true, 8, 120.0, 0).is_none());
    }

    #[test]
    fn collection_preserves_track_and_musical_duration() {
        let mut pattern = Pattern::new(2, 3);
        let note = Note {
            length_ticks: crate::tracker::NoteLength::default(),
            pitch: crate::tracker::NotePitch::C,
            octave: 4,
        };
        pattern.set_step(2, 0, Some(note)).unwrap();
        let instruments = [Instrument::Pluck, Instrument::Pluck, Instrument::Kick];
        let event = RowEvent {
            row: 0,
            beats: 0.25,
            duration: Duration::from_millis(125),
        };
        let notes = collect_row(&pattern, &instruments, event);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].track, 2);
        assert_eq!(notes[0].instrument, Instrument::Kick);
        assert_eq!(notes[0].note, note);
        assert_eq!(notes[0].beats, 0.25);
        assert_eq!(notes[0].duration, event.duration);

        let second_note = Note { octave: 5, ..note };
        pattern.set_step(0, 0, Some(second_note)).unwrap();
        let notes = collect_row(&pattern, &instruments, event);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].instrument, Instrument::Pluck);
        assert_eq!(notes[1].instrument, Instrument::Kick);
        assert_eq!((notes[0].track, notes[0].note), (0, second_note));
        assert_eq!((notes[1].track, notes[1].note), (2, note));
        for note in notes {
            assert_eq!(note.beats, event.beats);
            assert_eq!(note.duration, event.duration);
        }
        assert!(collect_row(&pattern, &instruments, RowEvent { row: 1, ..event }).is_empty());
        assert!(collect_row(&pattern, &instruments, RowEvent { row: 2, ..event }).is_empty());
        assert!(collect_row(&Pattern::new(0, 3), &instruments, event).is_empty());
        assert!(collect_row(&Pattern::new(2, 0), &instruments, event).is_empty());
    }
}
