use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{
    bindings::handle_key,
    command::Command,
    history::MAX_MUTATIONS,
    project::Project,
    state::{App, Mode, PlaybackState},
    tracker::{Instrument, Note, NotePitch, Pattern},
};

fn add(app: &mut App) {
    handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL), app);
}
fn note() -> Note {
    Note {
        length_ticks: crate::tracker::NoteLength::default(),
        pitch: NotePitch::C,
        octave: 4,
    }
}

#[test]
fn track_keys_are_normal_press_only_and_obey_help_prompt_counts_and_modifiers() {
    let mut app = App::default();
    for mode in [Mode::Insert, Mode::Visual] {
        app.mode = mode;
        add(&mut app);
    }
    app.mode = Mode::Normal;
    for modifiers in [
        KeyModifiers::NONE,
        KeyModifiers::ALT,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ] {
        handle_key(KeyEvent::new(KeyCode::Char('t'), modifiers), &mut app);
    }
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        handle_key(
            KeyEvent::new_with_kind(KeyCode::Char('t'), KeyModifiers::CONTROL, kind),
            &mut app,
        );
    }
    app.help_visible = true;
    add(&mut app);
    app.help_visible = false;
    app.prompt = Some(String::new());
    add(&mut app);
    assert_eq!(app.prompt.as_deref(), Some(""));
    app.prompt = None;
    app.pending_count = Some(3);
    add(&mut app);
    assert_eq!(app.pending_count, None);
    assert_eq!(app.project.instruments().len(), 4);
    assert!(!app.is_dirty());
    add(&mut app);
    assert_eq!(app.project.instruments().len(), 5);
    assert_eq!(app.cursor_track, 4);
    assert!(crate::bindings::DEFAULT_BINDINGS
        .iter()
        .any(|b| b.command == Command::AddTrack && b.description.contains("track")));
}

#[test]
fn variable_columns_history_selection_clipboard_and_roundtrip() {
    let mut app = App::default();
    app.project.patterns.push(Pattern::new(7, 4));
    app.project.patterns[0].set_step(2, 9, Some(note())).unwrap();
    let before = app.project.clone();
    let mut source = Pattern::new(2, 2);
    source.set_step(0, 0, Some(note())).unwrap();
    source.set_step(1, 0, Some(note())).unwrap();
    app.clipboard = crate::clipboard::EventClipboard::capture(
        &source,
        0..2,
        crate::musical_time::Tick(0)..crate::musical_time::Tick(480),
    );
    let clipboard = app.clipboard.clone();
    app.handle_command(Command::TogglePlayback);
    add(&mut app);
    assert!(matches!(app.playback_state, PlaybackState::Stopped));
    assert!(app.take_stop_request());
    for (old, new) in before.patterns.iter().zip(&app.project.patterns) {
        assert_eq!(&new.tracks[..4], old.tracks);
        assert_eq!(new.tracks[4].events(), []);
        assert_eq!(new.get_step(4, old.num_rows() - 1).unwrap().note, None);
    }
    assert_eq!(app.project.instrument(4), Instrument::Pluck);
    app.insert_note_under_cursor(Some(note()));
    app.handle_command(Command::SetInstrument(Instrument::Saw));
    let after = app.project.clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("five.json");
    crate::project_file::save(&app.project, &path).unwrap();
    assert_eq!(crate::project_file::load(&path).unwrap(), after);
    app.handle_command(Command::Undo);
    app.handle_command(Command::Undo);
    app.mode = Mode::Visual;
    app.selection_anchor = Some((63, 4));
    app.handle_command(Command::TogglePlayback);
    app.handle_command(Command::Undo);
    assert_eq!(app.project, before);
    assert!(!app.is_dirty());
    assert_eq!(app.cursor_track, 3);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.selection_anchor, None);
    assert_eq!(app.clipboard, clipboard);
    assert!(app.take_stop_request());
    for _ in 0..3 {
        app.handle_command(Command::Redo);
    }
    assert_eq!(app.project, after);
    app.cursor_track = 4;
    app.cursor_row = 63;
    app.handle_command(Command::Paste);
    assert_eq!(app.project.patterns[0].get_step(4, 63).unwrap().note, Some(note()));
    assert_eq!(app.project.patterns[0].tracks.len(), 5);
    assert_eq!(app.clipboard, clipboard);
}

#[test]
fn structural_barriers_preserve_mixed_transaction_order_and_multiple_appends() {
    for track_first in [false, true] {
        let mut app = App::default();
        let before = app.project.clone();
        app.edit(|edit| {
            if track_first {
                edit.append_track();
            }
            edit.append_pattern(0, true);
            if !track_first {
                edit.append_track();
            }
            edit.set_cell(1, 4, 3, Some(note()));
            edit.set_instrument(4, Instrument::Kick);
            edit.append_track();
            edit.set_cell(0, 5, 2, Some(note()));
            edit.set_cell(1, 4, 3, None);
            edit.set_cell(1, 4, 3, Some(note()));
        });
        let after = app.project.clone();
        app.handle_command(Command::Undo);
        assert_eq!(app.project, before);
        app.handle_command(Command::Redo);
        assert_eq!(app.project, after);
    }
}

#[test]
fn limits_are_atomic_and_total_all_patterns_at_boundary() {
    let mut app = App {
        project: Project::new(1, 127),
        ..App::default()
    };
    add(&mut app);
    assert_eq!(app.project.instruments().len(), 128);
    app.handle_command(Command::Undo);
    app.handle_command(Command::Redo);
    app.handle_command(Command::TogglePlayback);
    app.selection_anchor = Some((0, 3));
    let revision = app.revision();
    let project = app.project.clone();
    add(&mut app);
    assert_eq!(app.project, project);
    assert_eq!(app.revision(), revision);
    assert_eq!(app.selection_anchor, Some((0, 3)));
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    assert!(app.status.contains("limit"));

    // 200,000 rows * five resulting tracks is exactly one million cells.
    let mut project = Project::new(4096, 4);
    project.patterns = vec![Pattern::new(4096, 4); 48];
    project.patterns.push(Pattern::new(3392, 4));
    assert!(project.can_append_track());
    project.patterns.push(Pattern::new(1, 4));
    assert!(project.can_append_track());
    let mut app = App {
        project,
        ..App::default()
    };
    app.edit(|edit| edit.set_tempo(121.0));
    app.handle_command(Command::Undo);
    let revision = app.revision();
    add(&mut app);
    assert_ne!(app.revision(), revision);
    assert!(app.is_dirty());
    app.handle_command(Command::Redo);
    assert_eq!(app.project.bpm.to_bits(), 120.0_f64.to_bits());
}

#[test]
fn empty_column_snapshots_do_not_charge_projected_cells() {
    let mut app = App {
        project: Project::new(4096, 1),
        ..App::default()
    };
    app.project.patterns = vec![Pattern::new(4096, 1); MAX_MUTATIONS / 4096];
    let revision = app.revision();
    add(&mut app); // Only the sparse column metadata consumes retention budget.
    assert!(app.is_dirty());
    assert_ne!(app.revision(), revision);
    app.handle_command(Command::Undo);
    assert_eq!(app.project.instruments().len(), 1);
    assert!(!app.is_dirty());
    assert_eq!(app.revision(), revision);
}

#[test]
fn track_structures_cancel_deadlines_and_fifth_track_emits_in_song() {
    use crate::transport::{collect_row, PlaybackRoute, Transport};
    use std::time::{Duration, Instant};
    let mut app = App::default();
    let mut clock = Transport::default();
    let start = Instant::now();
    app.handle_command(Command::TogglePlayback);
    assert!(clock
        .update_project(start, true, &app.project, PlaybackRoute::Song, 4)
        .is_some());
    assert!(clock.deadline().is_some());
    add(&mut app);
    assert!(app.take_stop_request());
    clock.stop();
    assert!(clock.deadline().is_none());
    app.insert_note_under_cursor(Some(note()));
    app.handle_command(Command::TogglePlayback);
    let event = clock
        .update_project(
            start + Duration::from_secs(10),
            true,
            &app.project,
            PlaybackRoute::Song,
            4,
        )
        .unwrap();
    assert_eq!(event.row.row, 0);
    let notes = collect_row(
        &app.project.patterns[event.pattern],
        app.project.instruments(),
        event.row,
    );
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].track, 4);
    assert_eq!(notes[0].instrument, Instrument::Pluck);
    app.handle_command(Command::Undo); // note edit is not structural
    assert!(!app.take_stop_request());
    app.handle_command(Command::Undo); // track removal
    assert!(app.take_stop_request());
    clock.stop();
    assert!(clock.deadline().is_none());
    app.handle_command(Command::TogglePlayback);
    app.handle_command(Command::Redo);
    assert!(app.take_stop_request());
    assert!(matches!(app.playback_state, PlaybackState::Stopped));
}
