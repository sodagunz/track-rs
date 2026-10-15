//! Device-free playback clock. Deadlines, not UI redraws, own row advancement.
use std::time::{Duration, Instant};

use crate::tracker::{Note, Pattern};

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
    pub note: Note,
    pub beats: f64,
    pub duration: Duration,
}

/// Collect notes without constructing voices or opening an output device.
#[must_use]
pub fn collect_row(pattern: &Pattern, event: RowEvent) -> Vec<NoteEvent> {
    if event.row >= pattern.num_rows {
        return Vec::new();
    }
    pattern
        .tracks
        .iter()
        .enumerate()
        .filter_map(|(track, steps)| {
            Some(NoteEvent {
                track,
                note: steps.get(event.row)?.note?,
                beats: event.beats,
                duration: event.duration,
            })
        })
        .collect()
}

#[derive(Default)]
pub struct Transport {
    deadline: Option<Instant>,
    row: usize,
}

impl Transport {
    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub const fn stop(&mut self) {
        self.deadline = None;
    }

    /// Invalid rates or empty patterns stop the clock. A new start emits row zero.
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
            self.row = 0;
            return Some(RowEvent {
                row: 0,
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
            pitch: crate::tracker::NotePitch::C,
            octave: 4,
        };
        pattern.set_step(2, 0, Some(note)).unwrap();
        let event = RowEvent {
            row: 0,
            beats: 0.25,
            duration: Duration::from_millis(125),
        };
        let notes = collect_row(&pattern, event);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].track, 2);
        assert_eq!(notes[0].note, note);
        assert_eq!(notes[0].beats, 0.25);
        assert_eq!(notes[0].duration, event.duration);

        let second_note = Note { octave: 5, ..note };
        pattern.set_step(0, 0, Some(second_note)).unwrap();
        let notes = collect_row(&pattern, event);
        assert_eq!(notes.len(), 2);
        assert_eq!((notes[0].track, notes[0].note), (0, second_note));
        assert_eq!((notes[1].track, notes[1].note), (2, note));
        for note in notes {
            assert_eq!(note.beats, event.beats);
            assert_eq!(note.duration, event.duration);
        }
        assert!(collect_row(&pattern, RowEvent { row: 1, ..event }).is_empty());
        assert!(collect_row(&pattern, RowEvent { row: 2, ..event }).is_empty());
        assert!(collect_row(&Pattern::new(0, 3), event).is_empty());
        assert!(collect_row(&Pattern::new(2, 0), event).is_empty());
    }
}
