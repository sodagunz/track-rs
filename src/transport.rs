//! Device-free playback clock. Deadlines, not UI redraws, own row advancement.
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
    pub note: Note,
    pub beats: f64,
    pub duration: Duration,
}

/// Collect notes without constructing voices or opening an output device.
#[must_use]
pub fn collect_row(pattern: &Pattern, instruments: &[Instrument], event: RowEvent) -> Vec<NoteEvent> {
    if event.row >= pattern.num_rows {
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
}

#[derive(Default)]
pub struct Transport {
    deadline: Option<Instant>,
    row: usize,
    route: Option<PlaybackRoute>,
    start_offset: usize,
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
        let Some(event) = self.update_project(now, playing, &app.project, route, DEFAULT_ROWS_PER_BEAT) else {
            return Vec::new();
        };
        app.playback_row = event.row.row;
        app.playing_pattern = event.pattern;
        app.playing_order = event.order.unwrap_or(0);
        collect_row(
            &app.project.patterns[event.pattern],
            app.project.instruments(),
            event.row,
        )
    }

    /// Flatten one cycle, skip elapsed cycles arithmetically, then locate one row.
    /// Work is bounded by order length, never by the number of missed deadlines.
    pub fn update_project(
        &mut self,
        now: Instant,
        playing: bool,
        project: &crate::project::Project,
        route: PlaybackRoute,
        rows_per_beat: u32,
    ) -> Option<PlaybackEvent> {
        let route = if self.deadline.is_some() {
            self.route.unwrap_or(route)
        } else {
            route
        };
        self.route = Some(route);
        match route {
            PlaybackRoute::Pattern(pattern) => {
                let rows = project.patterns.get(pattern).map_or(0, |p| p.num_rows);
                let row = self.update(now, playing, rows, project.bpm, rows_per_beat)?;
                Some(PlaybackEvent {
                    pattern,
                    order: None,
                    row,
                })
            }
            PlaybackRoute::Song => {
                let rows = project.order.iter().try_fold(0_usize, |total, &index| {
                    total.checked_add(project.patterns.get(index)?.num_rows)
                });
                let Some(rows) = rows.filter(|&rows| rows > 0) else {
                    self.stop();
                    return None;
                };
                let mut row = self.update(now, playing, rows, project.bpm, rows_per_beat)?;
                for (order, &pattern) in project.order.iter().enumerate() {
                    let length = project.patterns[pattern].num_rows;
                    if row.row < length {
                        return Some(PlaybackEvent {
                            pattern,
                            order: Some(order),
                            row,
                        });
                    }
                    row.row -= length;
                }
                None
            }
        }
    }

    /// Set the initial flattened row only before a run; active routes stay pinned.
    pub fn start_at(&mut self, project: &crate::project::Project, route: PlaybackRoute, order: usize, row: usize) {
        if self.deadline.is_some() {
            return;
        }
        self.start_offset = match route {
            PlaybackRoute::Pattern(index) => project
                .patterns
                .get(index)
                .map_or(0, |pattern| row.min(pattern.num_rows.saturating_sub(1))),
            PlaybackRoute::Song => {
                let order = order.min(project.order.len().saturating_sub(1));
                let prefix: usize = project
                    .order
                    .iter()
                    .take(order)
                    .filter_map(|&index| project.patterns.get(index))
                    .map(|pattern| pattern.num_rows)
                    .sum();
                prefix
                    + project
                        .order
                        .get(order)
                        .and_then(|&index| project.patterns.get(index))
                        .map_or(0, |pattern| row.min(pattern.num_rows.saturating_sub(1)))
            }
        };
    }

    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub const fn stop(&mut self) {
        self.deadline = None;
        self.route = None;
        self.start_offset = 0;
    }

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
