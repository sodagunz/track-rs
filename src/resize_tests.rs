use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    bindings::handle_key,
    command::Command,
    project::Project,
    project_file::{MAX_CELLS, MAX_ROWS},
    state::{App, Mode, PlaybackState},
    tracker::Pattern,
};

fn key(app: &mut App, code: KeyCode) {
    handle_key(KeyEvent::new(code, KeyModifiers::NONE), app);
}

fn prompt(app: &mut App, text: &str) {
    key(app, KeyCode::Char(':'));
    assert!(app.prompt.is_some());
    for ch in text.chars() {
        key(app, KeyCode::Char(ch));
    }
    key(app, KeyCode::Enter);
    assert!(app.prompt.is_none());
}

#[test]
fn exact_end_validates_noops_and_partial_projection_without_stopping() {
    use crate::musical_time::Tick;
    let mut app = App::default();
    app.handle_command(Command::TogglePlayback);
    let revision = app.revision();
    for input in [
        "end",
        "end 0t",
        "end -1t",
        "end 1",
        "end 1.5t",
        "end 983041t",
        "end 18446744073709551616t",
    ] {
        prompt(&mut app, input);
        assert_eq!(app.revision(), revision, "{input}");
        assert!(matches!(app.playback_state, PlaybackState::Playing));
    }
    let end = app.project.patterns[0].length_ticks();
    prompt(&mut app, &format!("end {}t", end.0));
    assert_eq!(app.revision(), revision);
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    prompt(&mut app, "end 241t");
    assert!(matches!(app.playback_state, PlaybackState::Stopped));
    assert!(app.take_stop_request());
    let pattern = &app.project.patterns[0];
    assert_eq!(pattern.length_ticks(), Tick(241));
    assert_eq!(pattern.view_rows(), 2);
    assert_eq!(pattern.row_tick(1), Some(Tick(240)));
    assert_eq!(pattern.row_tick(2), Some(Tick(241)));
    assert!(app.grid_context().contains("Partial"));
    app.handle_command(Command::LastRow);
    assert_eq!(app.cursor_tick(), Tick(240));
    prompt(&mut app, "end 1t");
    assert_eq!(app.cursor_row, 0);
    assert_eq!(app.project.patterns[0].view_rows(), 1);
    prompt(&mut app, "resize 1");
    assert_eq!(app.project.patterns[0].length_ticks(), Tick(240));
}

#[test]
fn exact_end_force_restores_ids_overrides_and_gates_in_one_transaction() {
    use crate::{
        musical_time::{DurationTicks, Tick},
        tracker::{NotePitch, PatternEvent},
    };
    let mut app = App::default();
    prompt(&mut app, "meter 7/8");
    app.cursor_row = 14;
    prompt(&mut app, "grid 64 bar");
    for (track, start) in [(0, 0), (1, 240), (2, 241), (3, 3360)] {
        app.edit(|edit| {
            edit.set_event(
                0,
                track,
                Tick(start),
                Some(PatternEvent {
                    start_tick: Tick(start),
                    duration_ticks: DurationTicks::new(61439).unwrap(),
                    pitch: NotePitch::C,
                    octave: 4,
                }),
            );
        });
    }
    let before = app.project.clone();
    let ids: Vec<_> = before.patterns[0]
        .tracks
        .iter()
        .flat_map(|track| track.events().iter().map(|event| track.event_id(event.start_tick)))
        .collect();
    app.handle_command(Command::TogglePlayback);
    let revision = app.revision();
    prompt(&mut app, "end 241t");
    assert_eq!(app.revision(), revision);
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    assert!(app.status.contains("2 note onsets and 1 bar overrides"));
    prompt(&mut app, "end! 241t");
    assert_eq!(app.project.patterns[0].grid().overrides().count(), 0);
    assert_eq!(
        app.project.patterns[0].tracks[1].events()[0].duration_ticks.ticks(),
        61439
    );
    assert_eq!(app.project.patterns[0].tracks[2].events(), []);
    app.handle_command(Command::Undo);
    assert_eq!(app.project, before);
    let restored: Vec<_> = app.project.patterns[0]
        .tracks
        .iter()
        .flat_map(|track| track.events().iter().map(|event| track.event_id(event.start_tick)))
        .collect();
    assert_eq!(restored, ids);
    app.handle_command(Command::Redo);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("partial.json");
    crate::project_file::save(&app.project, &path).unwrap();
    assert_eq!(crate::project_file::load(&path).unwrap(), app.project);
    prompt(&mut app, "end 3361t");
    assert_eq!(app.project.patterns[0].length_ticks(), Tick(3361));
    assert_eq!(app.project.patterns[0].grid().overrides().count(), 0);
}

#[test]
fn resize_prompt_validates_before_editing_or_stopping() {
    let mut app = App::default();
    app.handle_command(Command::TogglePlayback);
    let revision = app.revision();
    for input in [
        "resize",
        "resize 0",
        "resize -1",
        "resize +1",
        "resize 4097",
        "resize 18446744073709551616",
        "resize 999999999999999999999999999999999999999999999999",
        "resize 2rows",
        "resize 2 3",
        "resize 2.0",
    ] {
        prompt(&mut app, input);
        assert_eq!(app.revision(), revision, "{input}");
        assert!(matches!(app.playback_state, PlaybackState::Playing));
        assert_ne!(app.status, "");
    }
    let rows = app.project.patterns[0].num_rows();
    prompt(&mut app, &format!("resize! {rows}"));
    assert_eq!(app.revision(), revision);
    assert!(!app.is_dirty());
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    prompt(&mut app, "  resize   001 \t");
    assert_eq!(app.project.patterns[0].num_rows(), 1);
    assert!(app.is_dirty());
    assert!(matches!(app.playback_state, PlaybackState::Stopped));
    assert!(app.take_stop_request());
    app.handle_command(Command::Undo);
    assert_eq!(app.project.patterns[0].num_rows(), rows);
    assert!(!app.is_dirty());
}

#[test]
fn resize_force_restores_all_tracks_notes_and_saved_cursor_atomically() {
    let mut app = App {
        cursor_row: 20,
        cursor_track: 3,
        ..App::default()
    };
    app.handle_command(Command::EnterInsert);
    key(&mut app, KeyCode::Char('z'));
    key(&mut app, KeyCode::Esc);
    let before = app.project.clone();
    let revision = app.revision();
    app.handle_command(Command::ToggleVisual);
    app.handle_command(Command::ShowArrangement);
    app.handle_command(Command::TogglePlayback);
    prompt(&mut app, "resize 4");
    assert_eq!(app.revision(), revision);
    assert!(app.status.contains(":resize! 4"));
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    prompt(&mut app, "resize! 4");
    assert!(app.take_stop_request());
    assert_eq!(app.project.patterns[0].num_rows(), 4);
    assert!(app.project.patterns[0]
        .tracks
        .iter()
        .all(|track| track.events().is_empty()));
    app.handle_command(Command::ShowPattern);
    assert_eq!(app.cursor_row, 3);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.selection_anchor, None);
    app.handle_command(Command::Undo);
    assert_eq!(app.project.patterns, before.patterns);
    app.handle_command(Command::Redo);
    assert_eq!(app.project.patterns[0].num_rows(), 4);
}

#[test]
fn resize_selected_pattern_roundtrips_repeated_order_and_neutral_channels() {
    let mut app = App::default();
    app.handle_command(Command::NewPattern);
    app.project.order = vec![0, 1, 1, 0];
    let channels = app.project.channels.clone();
    prompt(&mut app, "resize 128");
    assert_eq!(app.project.patterns[0].num_rows(), 64);
    assert_eq!(app.project.patterns[1].num_rows(), 128);
    assert_eq!(app.project.order, [0, 1, 1, 0]);
    assert_eq!(app.project.channels, channels);
    assert!(app.project.patterns[1]
        .tracks
        .iter()
        .all(|track| track.events().is_empty()));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resized.json");
    prompt(&mut app, &format!("w {}", path.display()));
    assert!(!app.is_dirty());
    prompt(&mut app, "resize 1");
    prompt(&mut app, &format!("e! {}", path.display()));
    assert!(!app.is_dirty());
    assert_eq!(app.project.patterns[1].num_rows(), 128);
    assert_eq!(app.project.order, [0, 1, 1, 0]);
    assert_eq!(app.project.channels, channels);
}

#[test]
fn resize_cancels_transport_and_preserves_gates_crossing_pattern_end() {
    use crate::transport::{collect_row, RowEvent, Transport};
    use std::time::{Duration, Instant};
    let mut app = App::default();
    app.handle_command(Command::EnterInsert);
    for ch in ['z', '}', '}', '}'] {
        key(&mut app, KeyCode::Char(ch));
    }
    key(&mut app, KeyCode::Esc);
    app.handle_command(Command::TogglePlayback);
    let now = Instant::now();
    let mut transport = Transport::default();
    assert_eq!(transport.update_app(now, &mut app, || {}).len(), 1);
    prompt(&mut app, "resize 1");
    let mut cancelled = false;
    assert!(transport.update_app(now, &mut app, || cancelled = true).is_empty());
    assert!(cancelled);
    let events = collect_row(
        &app.project.patterns[0],
        app.project.instruments(),
        RowEvent {
            row: 0,
            beats: 0.25,
            duration: Duration::from_millis(125),
        },
    );
    assert_eq!(events[0].note.length_ticks.ticks(), 128);
    assert_eq!(events[0].duration, Duration::from_secs(1));
    app.handle_command(Command::TogglePlayback);
    assert_eq!(transport.update_app(now, &mut app, || {}).len(), 1);
    app.handle_command(Command::Undo);
    let mut cancelled = false;
    transport.update_app(now, &mut app, || cancelled = true);
    assert!(cancelled);
}

#[test]
fn resize_history_barriers_noops_and_snapshot_budget() {
    use crate::{
        history::History,
        tracker::{Note, NotePitch},
    };
    let mut project = Project::new(4, 1);
    let mut history = History::default();
    let note = Some(Note {
        pitch: NotePitch::C,
        octave: 4,
        length_ticks: crate::tracker::NoteLength::default(),
    });
    history.edit(&mut project, |edit| {
        edit.set_cell(0, 0, 3, note);
        edit.resize_pattern(0, 2, true).unwrap();
        edit.set_cell(0, 0, 0, note);
        edit.resize_pattern(0, 6, false).unwrap();
    });
    let edited = project.patterns.clone();
    history.undo(&mut project);
    assert_eq!(project.patterns, [Pattern::new(4, 1)]);
    let revision = history.revision();
    history.edit(&mut project, |edit| {
        edit.resize_pattern(0, 4, false).unwrap();
        assert!(edit.resize_pattern(0, 0, true).is_err());
        assert!(edit.resize_pattern(0, usize::MAX, true).is_err());
    });
    assert_eq!(history.revision(), revision);
    history.redo(&mut project);
    assert_eq!(project.patterns, edited);
    // Actual events in both snapshots count; empty grid cells do not.
    // A later populated snapshot pushes the first resize out.
    let mut project = Project::new(MAX_ROWS, 8);
    for row in 0..MAX_ROWS - 2 {
        for track in 0..8 {
            project.patterns[0].set_step(track, row, note).unwrap();
        }
    }
    let mut history = History::default();
    history.edit(&mut project, |edit| {
        edit.resize_pattern(0, MAX_ROWS - 1, false).unwrap();
    });
    history.undo(&mut project);
    assert_eq!(project.patterns[0].num_rows(), MAX_ROWS);
    history.redo(&mut project);
    history.edit(&mut project, |edit| edit.set_cell(0, 0, MAX_ROWS - 2, note));
    history.edit(&mut project, |edit| edit.resize_pattern(0, MAX_ROWS, false).unwrap());
    history.undo(&mut project);
    history.undo(&mut project);
    let retained = history.revision();
    history.undo(&mut project);
    assert_eq!(history.revision(), retained);
    assert_eq!(project.patterns[0].num_rows(), MAX_ROWS - 1);
}

#[test]
fn resize_limits_are_inclusive_and_rejections_preserve_revision() {
    let mut app = App {
        project: Project::new(4000, 125),
        ..App::default()
    };
    app.project.patterns.push(Pattern::new(3999, 125));
    app.selected_pattern = 1;
    prompt(&mut app, "resize 4000");
    assert_eq!(
        app.project
            .patterns
            .iter()
            .map(|p| p.num_rows() * p.tracks.len())
            .sum::<usize>(),
        MAX_CELLS
    );
    let revision = app.revision();
    prompt(&mut app, "resize! 4001");
    assert_ne!(app.revision(), revision);
    assert_eq!(app.project.patterns[1].num_rows(), 4001);
    assert_eq!(app.project.event_count(), 0);
    let mut app = App::default();
    prompt(&mut app, &format!("resize {MAX_ROWS}"));
    assert_eq!(app.project.patterns[0].num_rows(), MAX_ROWS);
    for view in [Command::ShowMixer, Command::ShowInstrument] {
        app.handle_command(view);
        let revision = app.revision();
        prompt(&mut app, "resize 1");
        assert_eq!(app.revision(), revision);
        assert!(app.status.contains("Normal mode"));
    }
}
