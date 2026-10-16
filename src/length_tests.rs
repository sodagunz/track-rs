use std::{fs, path::Path, time::Duration};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde_json::{json, Value};

use crate::{
    bindings::handle_key,
    command::Command,
    project::Project,
    project_file,
    state::App,
    tracker::{Instrument, Note, NoteLength, NotePitch},
    transport::{collect_row, RowEvent},
};

fn text(app: &mut App, text: &str) {
    for ch in text.chars() {
        handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), app);
    }
}

fn note(app: &App, track: usize, row: usize) -> Option<Note> {
    app.project.patterns[app.selected_pattern]
        .get_step(track, row)
        .unwrap()
        .note
}

fn ticks(app: &App) -> u16 {
    note(app, app.cursor_track, app.cursor_row)
        .unwrap()
        .length_ticks
        .ticks()
}

#[test]
fn real_insert_keys_clamp_preserve_pitch_replacement_and_do_not_move_cursor() {
    let mut app = App::default();
    let clean = app.revision();
    text(&mut app, "{}"); // Normal mode has no length bindings.
    text(&mut app, "i{}"); // Empty cell is not an edit.
    assert_eq!(app.revision(), clean);
    text(&mut app, "z");
    assert_eq!(ticks(&app), 16);
    app.edit_step = 4;
    text(&mut app, "}}");
    assert_eq!(ticks(&app), 64);
    assert_eq!(app.cursor_row, 0);
    text(&mut app, "x");
    assert_eq!(app.cursor_row, 4);
    assert_eq!(note(&app, 0, 0).unwrap().length_ticks.ticks(), 64);
    assert_eq!(note(&app, 0, 0).unwrap().pitch, NotePitch::D);
    text(&mut app, "z");
    assert_eq!(note(&app, 0, 4).unwrap().length_ticks.ticks(), 16);
    app.cursor_row = 0;
    text(&mut app, "rf");
    assert_eq!(ticks(&app), 64);
    for _ in 0..20 {
        text(&mut app, "{");
    }
    assert_eq!(ticks(&app), 1);
    let shortest = app.revision();
    text(&mut app, "{");
    assert_eq!(app.revision(), shortest);
    app.handle_command(Command::EnterNormal);
    app.handle_command(Command::Undo);
    assert_eq!(ticks(&app), 2);
    text(&mut app, "i{"); // A real edit replaces redo.
    for _ in 0..20 {
        text(&mut app, "}");
    }
    assert_eq!(ticks(&app), 4096);
    let longest = app.revision();
    text(&mut app, "}");
    assert_eq!(app.revision(), longest);
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        handle_key(
            KeyEvent::new_with_kind(KeyCode::Char('{'), KeyModifiers::NONE, kind),
            &mut app,
        );
    }
    assert_eq!(app.revision(), longest);
    app.handle_command(Command::Undo);
    assert_eq!(ticks(&app), 2048);
    app.handle_command(Command::Redo);
    assert_eq!(ticks(&app), 4096);
    assert_eq!(app.edit_step, 4);
    assert!(NoteLength::try_from(0).is_err());
    assert!(NoteLength::try_from(4097).is_err());
    assert_eq!(NoteLength::try_from(3).unwrap().adjusted(false).ticks(), 1);
}

#[test]
fn whole_notes_survive_rectangle_cut_clip_paste_duplicate_and_atomic_history() {
    let mut app = App {
        project: Project::new(8, 2),
        ..App::default()
    };
    text(&mut app, "iz}}jz{{");
    app.handle_command(Command::EnterNormal);
    app.cursor_row = 0;
    let source = app.project.patterns[0].clone();
    text(&mut app, "vjld"); // 2x2 including empty track.
    assert!(note(&app, 0, 0).is_none());
    assert!(note(&app, 0, 1).is_none());
    app.handle_command(Command::Undo);
    assert_eq!(app.project.patterns[0], source);
    app.cursor_track = 0;
    app.cursor_row = 6;
    let before = app.project.clone();
    text(&mut app, "p");
    assert_eq!(note(&app, 0, 6).unwrap().length_ticks.ticks(), 64);
    assert_eq!(note(&app, 0, 7).unwrap().length_ticks.ticks(), 4);
    assert_eq!(note(&app, 1, 6), None);
    let pasted = app.project.clone();
    app.handle_command(Command::Undo);
    assert_eq!(app.project, before);
    app.handle_command(Command::Redo);
    assert_eq!(app.project, pasted);
    app.cursor_track = 1;
    app.cursor_row = 7;
    text(&mut app, "p"); // Both dimensions clipped.
    assert_eq!(note(&app, 1, 7).unwrap().length_ticks.ticks(), 64);
    app.handle_command(Command::DuplicatePattern);
    assert_eq!(app.project.patterns[0], app.project.patterns[1]);
    app.handle_command(Command::Undo);
    assert_eq!(app.project.patterns.len(), 1);
    app.handle_command(Command::Redo);
    assert_eq!(app.project.patterns[0], app.project.patterns[1]);
}

#[test]
fn net_noop_length_transactions_and_empty_pastes_preserve_redo() {
    let mut app = App::default();
    text(&mut app, "iz}");
    app.handle_command(Command::Undo);
    let original = note(&app, 0, 0).unwrap();
    let revision = app.revision();
    app.edit(|edit| {
        edit.set_cell(
            0,
            0,
            0,
            Some(Note {
                length_ticks: NoteLength::try_from(128).unwrap(),
                ..original
            }),
        );
        edit.set_cell(0, 0, 0, Some(original));
    });
    assert_eq!(app.revision(), revision);
    app.handle_command(Command::EnterNormal);
    app.cursor_row = 1;
    text(&mut app, "yp"); // Empty -> empty is also a no-op.
    assert_eq!(app.revision(), revision);
    app.handle_command(Command::Redo);
    assert_eq!(note(&app, 0, 0).unwrap().length_ticks.ticks(), 32);
}

#[test]
fn strict_v2_lengths_and_legacy_fixture_import() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.json");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project-v1.json");
    let mut project = project_file::load(&fixture).unwrap();
    for note in project
        .patterns
        .iter()
        .flat_map(|p| &p.tracks)
        .flatten()
        .filter_map(|step| step.note)
    {
        assert_eq!(note.length_ticks.ticks(), 16);
    }

    for (row, ticks) in [(0, 1), (1, 4096)] {
        let mut note = project.patterns[0].get_step(0, 0).unwrap().note.unwrap();
        note.length_ticks = NoteLength::try_from(ticks).unwrap();
        project.patterns[0].set_step(0, row, Some(note)).unwrap();
    }
    project_file::save(&project, &path).unwrap();
    assert_eq!(project_file::load(&path).unwrap(), project);
    let encoded: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(encoded["version"], 2);
    assert_eq!(encoded["patterns"][0]["cells"][0][0]["length_ticks"], 1);
    for value in [json!(0), json!(4097), json!(-1), json!(1.5), json!(null), json!("16")] {
        let mut invalid = encoded.clone();
        invalid["patterns"][0]["cells"][0][0]["length_ticks"] = value;
        fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(project_file::load(&path).is_err());
    }
    let mut missing = encoded.clone();
    missing["patterns"][0]["cells"][0][0]
        .as_object_mut()
        .unwrap()
        .remove("length_ticks");
    fs::write(&path, serde_json::to_vec(&missing).unwrap()).unwrap();
    assert!(project_file::load(&path).is_err());
    let mut unknown = encoded;
    unknown["version"] = json!(3);
    fs::write(&path, serde_json::to_vec(&unknown).unwrap()).unwrap();
    assert!(project_file::load(&path)
        .unwrap_err()
        .to_string()
        .contains("Unsupported project version 3"));
    let v2 = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project-v2.json");
    assert_eq!(
        project_file::load(&v2).unwrap().patterns[0]
            .get_step(0, 0)
            .unwrap()
            .note
            .unwrap()
            .length_ticks
            .ticks(),
        4096
    );
}

#[test]
fn event_durations_resolve_at_trigger_bpm_for_short_and_long_gates() {
    let mut pattern = crate::tracker::Pattern::new(1, 1);
    for milliseconds in [50, 125, 750] {
        let row = RowEvent {
            row: 0,
            beats: 0.25,
            duration: Duration::from_millis(milliseconds),
        };
        for ticks in [1, 16, 4096] {
            pattern
                .set_step(
                    0,
                    0,
                    Some(Note {
                        pitch: NotePitch::A,
                        octave: 4,
                        length_ticks: NoteLength::try_from(ticks).unwrap(),
                    }),
                )
                .unwrap();
            let event = collect_row(&pattern, &[Instrument::Sine], row)[0];
            assert_eq!(event.duration, row.duration.mul_f64(f64::from(ticks) / 16.0));
            let future = collect_row(
                &pattern,
                &[Instrument::Sine],
                RowEvent {
                    duration: row.duration * 2,
                    ..row
                },
            )[0];
            assert_eq!(future.duration, event.duration * 2);
            assert_eq!(event.note.length_ticks.ticks(), ticks);
        }
    }
}

#[test]
fn invalid_v2_forced_load_keeps_live_lengths_history_and_playback() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid.json");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project-v2.json");
    let original: Value = serde_json::from_slice(&fs::read(source).unwrap()).unwrap();
    let mut app = App::default();
    text(&mut app, "iz}}");
    app.handle_command(Command::EnterNormal);
    app.handle_command(Command::TogglePlayback);
    let project = app.project.clone();
    let revision = app.revision();
    for invalid in [json!(0), json!(4097), json!(null)] {
        let mut value = original.clone();
        value["patterns"][0]["cells"][0][0]["length_ticks"] = invalid;
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        text(&mut app, &format!(":e! {}", path.display()));
        handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
        assert_eq!(app.project, project);
        assert_eq!(app.revision(), revision);
        assert!(matches!(app.playback_state, crate::state::PlaybackState::Playing));
        assert!(!app.take_stop_request());
    }
    app.handle_command(Command::Undo);
    assert_eq!(ticks(&app), 32);
    app.handle_command(Command::Redo);
    assert_eq!(ticks(&app), 64);
}
