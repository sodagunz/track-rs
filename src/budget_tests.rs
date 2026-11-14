use std::fmt::Write as _;

use crate::{
    clipboard::EventClipboard,
    command::Command,
    history::History,
    musical_time::{DurationTicks, Tick},
    project::Project,
    project_file::{self, MAX_EVENTS, MAX_PATTERN_TICKS, MAX_TRACK_EVENTS},
    state::App,
    tracker::{NotePitch, Pattern, PatternEvent},
};

fn event(start: u64) -> PatternEvent {
    PatternEvent {
        start_tick: Tick(start),
        duration_ticks: DurationTicks::new(1).unwrap(),
        pitch: NotePitch::C,
        octave: 4,
    }
}

fn full_pattern() -> Pattern {
    let mut pattern = Pattern::new(4096, 2);
    for start in 0..MAX_TRACK_EVENTS as u64 {
        pattern.replace_event(1, Tick(start), Some(event(start))).unwrap();
    }
    pattern
}

#[test]
fn track_capacity_allows_replacement_and_rejects_paste_atomically() {
    let mut project = Project::new(4096, 2);
    project.patterns[0] = full_pattern();
    let mut source = Pattern::new(1, 2);
    for track in 0..2 {
        source.replace_event(track, Tick(0), Some(event(0))).unwrap();
    }
    let clipboard = EventClipboard::capture(&source, 0..2, Tick(0)..Tick(1));
    let before = project.clone();
    let mut history = History::default();
    let revision = history.revision();
    history.edit(&mut project, |edit| {
        assert!(clipboard.paste(edit, 0, 0, Tick(70000)).is_err());
    });
    assert_eq!(project, before);
    assert_eq!(history.revision(), revision);
    history.edit(&mut project, |edit| {
        edit.set_event(0, 1, Tick(70000), Some(event(70000)));
    });
    assert_eq!(history.revision(), revision);
    let id = project.patterns[0].tracks[1].event_id(Tick(0));
    history.edit(&mut project, |edit| {
        edit.set_event(
            0,
            1,
            Tick(0),
            Some(PatternEvent {
                pitch: NotePitch::D,
                ..event(0)
            }),
        );
    });
    assert_eq!(project.patterns[0].tracks[1].event_id(Tick(0)), id);
    history.undo(&mut project);
    assert_eq!(project, before);
    history.edit(&mut project, |edit| clipboard.paste(edit, 0, 0, Tick(0)).unwrap());
    assert_eq!(project.event_count(), MAX_TRACK_EVENTS + 1);
    assert_eq!(project.patterns[0].tracks[0].events().len(), 1);
    let mut app = App {
        project,
        edit_step: 1,
        ..App::default()
    };
    app.cursor_track = 1;
    app.cursor_row = 300;
    let revision = app.revision();
    app.handle_command(Command::EnterInsert);
    app.handle_command(Command::InsertNote {
        pitch: NotePitch::C,
        octave_offset: 0,
    });
    assert!(app.status.contains("event limit"));
    assert_eq!(app.cursor_row, 300);
    assert_eq!(app.revision(), revision);
    assert!(app.take_audition().is_none());
}

#[test]
fn global_capacity_blocks_growth_not_replacement_deletion_or_empty_structures() {
    let full = full_pattern();
    let mut project = Project::new(4096, 2);
    project.patterns = (0..MAX_EVENTS / MAX_TRACK_EVENTS).map(|_| full.duplicate()).collect();
    let remaining = MAX_EVENTS % MAX_TRACK_EVENTS;
    let mut tail = full.duplicate();
    tail.resize_ticks(Tick(remaining as u64));
    project.patterns.push(tail);
    assert_eq!(project.event_count(), MAX_EVENTS);
    let mut history = History::default();
    let revision = history.revision();
    history.edit(&mut project, |edit| {
        edit.set_event(0, 0, Tick(0), Some(event(0)));
        edit.append_pattern(0, true);
    });
    assert_eq!(history.revision(), revision);
    assert_eq!(project.event_count(), MAX_EVENTS);
    history.edit(&mut project, |edit| {
        edit.set_event(0, 1, Tick(0), None);
        edit.set_event(0, 0, Tick(0), Some(event(0)));
    });
    assert_eq!(project.event_count(), MAX_EVENTS);
    history.undo(&mut project);
    assert_eq!(project.patterns[0].tracks[0].events(), []);
    history.edit(&mut project, |edit| {
        edit.append_track();
        edit.append_pattern(15, false);
    });
    assert_eq!(project.event_count(), MAX_EVENTS);
    assert_eq!(project.instruments().len(), 3);
    assert_eq!(project.patterns.last().unwrap().length_ticks(), Tick(remaining as u64));
}

#[test]
fn sparse_extent_is_independent_of_grid_density_and_legacy_cell_limits() {
    let mut app = App {
        project: Project::new(4096, 127),
        ..App::default()
    };
    app.project.patterns.push(Pattern::new(4096, 127));
    app.execute_prompt("grid 64");
    assert_eq!(
        app.project.patterns[0].view_rows(),
        usize::try_from(MAX_PATTERN_TICKS).unwrap() / 15
    );
    app.handle_command(Command::AddTrack);
    assert_eq!(app.project.instruments().len(), 128);
    app.handle_command(Command::DuplicatePattern);
    assert_eq!(app.project.patterns.len(), 3);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sparse.json");
    project_file::save(&app.project, &path).unwrap();
    assert_eq!(project_file::load(&path).unwrap(), app.project);
    assert!(std::fs::metadata(path).unwrap().len() < 100_000);
}

#[test]
fn v4_track_event_limit_is_checked_before_installation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("dense-events.json");
    project_file::save(&Project::new(4096, 1), &path).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut encoded = String::new();
    for start in 0..=MAX_TRACK_EVENTS {
        if start > 0 {
            encoded.push(',');
        }
        write!(
            encoded,
            "{{\"start_tick\":{start},\"duration_ticks\":1,\"pitch\":\"C\",\"octave\":4}}"
        )
        .unwrap();
    }
    let input = value
        .to_string()
        .replace("\"tracks\":[[]]", &format!("\"tracks\":[[{encoded}]]"));
    std::fs::write(&path, input).unwrap();
    assert!(project_file::load(&path)
        .unwrap_err()
        .to_string()
        .contains("Track exceeds"));
    let mut app = App::default();
    app.handle_command(Command::IncreaseTempo);
    let before = app.project.clone();
    let revision = app.revision();
    app.execute_prompt(&format!("e! {}", path.display()));
    assert_eq!(app.project, before);
    assert_eq!(app.revision(), revision);
}
