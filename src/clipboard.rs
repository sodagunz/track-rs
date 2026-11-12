//! Owned sparse snapshots. The tick span preserves empty space without storing empty cells.
use std::ops::Range;

use crate::{
    history::Edit,
    musical_time::Tick,
    tracker::{Pattern, PatternEvent},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventClipboard {
    pub(crate) span_ticks: u64,
    pub(crate) tracks: Vec<Vec<PatternEvent>>,
}

impl EventClipboard {
    pub(crate) fn capture(pattern: &Pattern, tracks: Range<usize>, range: Range<Tick>) -> Self {
        let Ok(end) = pattern.end_tick() else {
            return Self::default();
        };
        if range.start >= range.end
            || range.end > end
            || tracks.start >= tracks.end
            || tracks.end > pattern.tracks.len()
        {
            return Self::default();
        }
        Self {
            span_ticks: range.end.0 - range.start.0,
            tracks: pattern.tracks[tracks]
                .iter()
                .map(|track| {
                    track
                        .events_in(range.clone())
                        .iter()
                        .map(|event| PatternEvent {
                            start_tick: Tick(event.start_tick.0 - range.start.0),
                            ..*event
                        })
                        .collect()
                })
                .collect(),
        }
    }

    pub(crate) fn paste(&self, edit: &mut Edit<'_>, pattern: usize, first_track: usize, start: Tick) {
        let Some(target) = edit.pattern(pattern) else {
            return;
        };
        let Ok(end) = target.end_tick() else {
            return;
        };
        if self.span_ticks == 0 || first_track >= target.tracks.len() || target.validate_event(start, None).is_err() {
            return;
        }
        let span = self.span_ticks.min(end.0 - start.0);
        // Preflight every translated value before clearing anything, including empty space.
        let mut translated = Vec::new();
        for events in self.tracks.iter().take(target.tracks.len() - first_track) {
            let mut track = Vec::new();
            for event in events.iter().filter(|event| event.start_tick.0 < span) {
                let Ok(position) = start.checked_add(event.start_tick.0) else {
                    return;
                };
                let event = PatternEvent {
                    start_tick: position,
                    ..*event
                };
                if target.validate_event(position, Some(event)).is_err() {
                    return;
                }
                track.push(event);
            }
            translated.push(track);
        }
        let range = start..Tick(start.0 + span);
        for (offset, events) in translated.into_iter().enumerate() {
            let track = first_track + offset;
            edit.clear_range(pattern, track, range.clone());
            for event in events {
                edit.set_event(pattern, track, event.start_tick, Some(event));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        history::History,
        project::Project,
        tracker::{Note, NoteLength, NotePitch},
    };

    fn note(ticks: u16) -> Note {
        Note {
            pitch: NotePitch::C,
            octave: 4,
            length_ticks: NoteLength::try_from(ticks).unwrap(),
        }
    }

    #[test]
    fn sparse_snapshot_keeps_empty_extent_and_exact_relative_positions() {
        let mut source = Pattern::new(4096, 128);
        source.set_step(127, 1, Some(note(4096))).unwrap();
        source.set_step(127, 2, Some(note(1))).unwrap();
        let empty = EventClipboard::capture(&source, 0..127, Tick(0)..Tick(4096 * 240));
        assert_eq!(empty.span_ticks, 4096 * 240);
        assert_eq!(empty.tracks.len(), 127);
        assert!(empty.tracks.iter().all(Vec::is_empty));
        let copy = EventClipboard::capture(&source, 127..128, Tick(1)..Tick(480));
        assert_eq!(copy.span_ticks, 479);
        assert_eq!(copy.tracks[0].len(), 1);
        assert_eq!(copy.tracks[0][0].start_tick, Tick(239));
        assert_eq!(copy.tracks[0][0].duration_ticks.ticks(), 61_440);
        assert_eq!(source.tracks[127].events().len(), 2);
    }

    #[test]
    fn empty_span_overwrites_notes_and_undo_restores_them() {
        let source = Pattern::new(3, 1);
        let clipboard = EventClipboard::capture(&source, 0..1, Tick(0)..Tick(720));
        let mut project = Project::new(8, 1);
        for row in 0..8 {
            project.patterns[0].set_step(0, row, Some(note(4096))).unwrap();
        }
        let before = project.clone();
        let mut history = History::default();
        history.edit(&mut project, |edit| clipboard.paste(edit, 0, 0, Tick(240)));
        for row in 0..8 {
            assert_eq!(
                project.patterns[0].get_step(0, row).unwrap().note,
                if (1..4).contains(&row) { None } else { Some(note(4096)) }
            );
        }
        history.undo(&mut project);
        assert_eq!(project, before);
        history.redo(&mut project);
        assert_eq!(project.patterns[0].tracks[0].events().len(), 5);
    }

    #[test]
    fn paste_clips_starts_not_gates_and_rejects_unsupported_translation_atomically() {
        let mut source = Pattern::new(2, 2);
        for track in 0..2 {
            for row in 0..2 {
                source.set_step(track, row, Some(note(4096))).unwrap();
            }
        }
        let clipboard = EventClipboard::capture(&source, 0..2, Tick(0)..Tick(480));
        let mut project = Project::new(4, 2);
        let mut history = History::default();
        history.edit(&mut project, |edit| clipboard.paste(edit, 0, 1, Tick(720)));
        assert_eq!(project.patterns[0].tracks[0].events(), []);
        assert_eq!(project.patterns[0].tracks[1].events().len(), 1);
        assert_eq!(project.patterns[0].tracks[1].events()[0].duration_ticks.ticks(), 61_440);
        let before = project.clone();
        let revision = history.revision();
        let unsupported = EventClipboard::capture(&source, 0..2, Tick(1)..Tick(480));
        history.edit(&mut project, |edit| unsupported.paste(edit, 0, 0, Tick(720)));
        assert_eq!(project, before);
        assert_eq!(history.revision(), revision);
        history.edit(&mut project, |edit| clipboard.paste(edit, 0, 0, Tick(u64::MAX)));
        assert_eq!(project, before);
        assert_eq!(history.revision(), revision);
    }
}
