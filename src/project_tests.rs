#![allow(clippy::float_cmp)]

use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde_json::{json, Value};

use crate::{
    bindings::{handle_key, resolve},
    command::Command,
    history::MAX_TRANSACTIONS,
    project::Project,
    project_file::{self, MAX_CELLS, MAX_FILE_BYTES, MAX_ORDER, MAX_PATTERNS, MAX_ROWS, MAX_TRACKS},
    state::{App, Mode, PlaybackState},
    tracker::{Instrument, Note, NotePitch, Pattern},
    transport::{collect_row, Transport},
};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project-v1.json")
}

fn press(app: &mut App, code: KeyCode) {
    handle_key(KeyEvent::new(code, KeyModifiers::NONE), app);
}

fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        press(app, KeyCode::Char(ch));
    }
}

fn command(app: &mut App, text: &str) {
    type_text(app, ":");
    assert!(app.prompt.is_some());
    type_text(app, text);
    press(app, KeyCode::Enter);
    assert!(app.prompt.is_none());
}

fn path_text(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn version_one_fixture_and_multi_pattern_round_trip_preserve_shared_voices() {
    let project = project_file::load(&fixture()).unwrap();
    assert_eq!(project.bpm, 137.5);
    assert_eq!(project.order, [0, 0, 1]);
    assert_eq!(project.patterns.len(), 2);
    assert_eq!(project.patterns[1].num_rows, 3);
    assert_eq!(project.instrument(2), Instrument::Sine);
    assert_eq!(project.patterns[0].get_step(2, 0).unwrap().note.unwrap().octave, 9);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("round trip.json");
    project_file::save(&project, &path).unwrap();
    assert_eq!(project_file::load(&path).unwrap(), project);
    let encoded = fs::read_to_string(path).unwrap();
    assert!(encoded.contains("\"F#\""));
    assert!(encoded.contains("\"HiHat\""));
    assert!(!encoded.contains("playback"));
    let mut transport = Transport::default();
    let row = transport.update(Instant::now(), true, 2, project.bpm, 4).unwrap();
    let notes = collect_row(&project.patterns[0], project.instruments(), row);
    assert_eq!(notes[0].instrument, Instrument::Kick);
    assert_eq!(notes[1].instrument, Instrument::Sine);
}

#[test]
fn malformed_values_and_dimension_corruption_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bad.json");
    let original: Value = serde_json::from_slice(&fs::read(fixture()).unwrap()).unwrap();
    let mut corruptions = Vec::new();
    for bpm in [json!(19.9), json!(300.1), json!(null), json!("NaN")] {
        let mut value = original.clone();
        value["bpm"] = bpm;
        corruptions.push(value);
    }
    for (pointer, replacement) in [
        ("/version", json!(2)),
        ("/version", json!(0)),
        ("/instruments/0", json!("Unknown")),
        ("/instruments", json!([])),
        ("/patterns", json!([])),
        ("/patterns/0/rows", json!(0)),
        ("/patterns/0/rows", json!(MAX_ROWS + 1)),
        ("/patterns/0/rows", json!(usize::MAX)),
        ("/patterns/0/tracks", json!(2)),
        ("/patterns/0/tracks", json!(0)),
        ("/patterns/0/cells/0", json!([null])),
        ("/patterns/0/cells/0/0/pitch", json!("H")),
        ("/patterns/0/cells/0/0/pitch", json!(12)),
        ("/patterns/0/cells/0/0/octave", json!(10)),
        ("/patterns/0/cells/0/0/octave", json!(-1)),
        ("/order", json!([])),
        ("/order/0", json!(2)),
        ("/order/0", json!(-1)),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        corruptions.push(value);
    }
    let mut extra_track = original;
    extra_track["patterns"][0]["cells"]
        .as_array_mut()
        .unwrap()
        .push(json!([null, null]));
    corruptions.push(extra_track);
    for value in corruptions {
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(project_file::load(&path).is_err(), "{value}");
    }
    for bytes in [
        "{",
        "null",
        "{}",
        "{\"version\":1,\"bpm\":NaN}",
        "{\"version\":1,\"bpm\":1e999}",
    ] {
        fs::write(&path, bytes).unwrap();
        assert!(project_file::load(&path).is_err());
    }
    fs::write(&path, "{\"version\":999}").unwrap();
    assert!(project_file::load(&path)
        .unwrap_err()
        .to_string()
        .contains("Unsupported project version 999"));
}

#[test]
fn file_and_allocation_limits_are_enforced_with_inclusive_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("limits.json");
    let file = fs::File::create(&path).unwrap();
    file.set_len(MAX_FILE_BYTES + 1).unwrap();
    assert!(project_file::load(&path).unwrap_err().to_string().contains("16 MiB"));
    for project in [
        Project::new(1, MAX_TRACKS + 1),
        Project::new(MAX_ROWS + 1, 1),
        Project {
            patterns: vec![Pattern::new(1, 1); MAX_PATTERNS + 1],
            ..Project::new(1, 1)
        },
        Project {
            order: vec![0; MAX_ORDER + 1],
            ..Project::new(1, 1)
        },
        Project {
            patterns: vec![Pattern::new(MAX_ROWS, MAX_TRACKS); 2],
            ..Project::new(1, MAX_TRACKS)
        },
    ] {
        assert!(project_file::save(&project, &path).is_err());
    }
    // A compact null-cell document hits the aggregate limit exactly, below the byte limit.
    assert_eq!(MAX_CELLS % 125, 0);
    let project = Project {
        patterns: vec![Pattern::new(4000, 125); 2],
        ..Project::new(1, 125)
    };
    project_file::save(&project, &path).unwrap();
    assert_eq!(project_file::load(&path).unwrap(), project);
    for project in [
        Project::new(MAX_ROWS, 1),
        Project::new(1, MAX_TRACKS),
        Project {
            patterns: vec![Pattern::new(1, 1); MAX_PATTERNS],
            order: vec![0; MAX_ORDER],
            ..Project::new(1, 1)
        },
    ] {
        project_file::save(&project, &path).unwrap();
        assert_eq!(project_file::load(&path).unwrap(), project);
    }
    // Decoder limits also apply to compact malicious input, not just save validation.
    let value = json!({"version":1,"bpm":120,"instruments":["Pluck"],"patterns":vec![json!({"rows":1,"tracks":1,"cells":[[null]]});MAX_PATTERNS+1],"order":[0]});
    fs::write(&path, value.to_string()).unwrap();
    assert!(project_file::load(&path).is_err());
}

#[test]
fn decoder_rejects_each_allocation_limit_before_installation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("limits");
    let original: Value = serde_json::from_slice(&fs::read(fixture()).unwrap()).unwrap();
    for (field, value) in [
        ("instruments", json!(vec!["Pluck"; MAX_TRACKS + 1])),
        (
            "patterns",
            json!(vec![
                json!({"rows":1,"tracks":3,"cells":[[null],[null],[null]]});
                MAX_PATTERNS + 1
            ]),
        ),
        ("order", json!(vec![0; MAX_ORDER + 1])),
        (
            "patterns",
            json!(vec![
                json!({"rows":MAX_ROWS,"tracks":128,"cells":vec![vec![Value::Null;MAX_ROWS];128]});
                2
            ]),
        ),
    ] {
        let mut document = original.clone();
        document[field] = value;
        if field == "patterns" && document[field][0]["tracks"] == 128 {
            document["instruments"] = json!(vec!["Pluck"; 128]);
        }
        fs::write(&path, document.to_string()).unwrap();
        assert!(project_file::load(&path).is_err(), "{field}");
    }
}

#[test]
fn ordinary_clean_load_and_prompt_mode_event_rules() {
    let mut app = App::default();
    for mode in [Mode::Insert, Mode::Visual] {
        app.mode = mode;
        type_text(&mut app, ":");
        assert_eq!(app.prompt, None);
    }
    app.mode = Mode::Normal;
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        handle_key(
            KeyEvent::new_with_kind(KeyCode::Char(':'), KeyModifiers::SHIFT, kind),
            &mut app,
        );
        assert_eq!(app.prompt, None);
    }
    handle_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::SHIFT), &mut app);
    type_text(&mut app, &format!("e {}", path_text(&fixture())));
    handle_key(
        KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Repeat),
        &mut app,
    );
    assert!(app.prompt.is_some());
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.project, project_file::load(&fixture()).unwrap());
    assert_eq!(app.path, Some(fixture()));
    assert!(!app.is_dirty());
    assert!(app.take_stop_request());
    let revision = app.revision();
    command(&mut app, "e! missing/no-file.json");
    assert_eq!(app.revision(), revision);
    assert!(!app.is_dirty());
    assert!(!app.take_stop_request());
}

#[test]
fn failed_saves_preserve_target_bytes_and_saved_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("song.json");
    let mut app = App::default();
    command(&mut app, &format!("w {}", path_text(&path)));
    let previous = fs::read(&path).unwrap();
    app.handle_command(Command::IncreaseTempo);
    let revision = app.revision();
    let old_path = app.path.clone();
    let target_directory = directory.path().join("target directory");
    fs::create_dir(&target_directory).unwrap();
    fs::write(target_directory.join("sentinel"), b"keep").unwrap();
    command(&mut app, &format!("wq {}", path_text(&target_directory)));
    assert!(!app.shutdown);
    assert!(app.is_dirty());
    assert_eq!(app.revision(), revision);
    assert_eq!(app.path, old_path);
    assert_eq!(fs::read(&path).unwrap(), previous);
    assert_eq!(fs::read(target_directory.join("sentinel")).unwrap(), b"keep");
    command(
        &mut app,
        &format!("w {}", path_text(&directory.path().join("missing/child"))),
    );
    assert!(app.is_dirty());
    assert_eq!(app.path, old_path);
    app.project.bpm = f64::NAN;
    assert!(project_file::save(&app.project, &path).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    // Failed persist cleans up its same-directory temporary file.
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn saved_revision_survives_undo_redo_divergence_and_eviction() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("song");
    let mut app = App::default();
    assert!(!app.is_dirty());
    app.handle_command(Command::IncreaseTempo);
    command(&mut app, &format!("w {}", path_text(&path)));
    let saved = app.revision();
    assert!(!app.is_dirty());
    app.handle_command(Command::IncreaseTempo);
    assert!(app.is_dirty());
    app.handle_command(Command::Undo);
    assert_eq!(app.revision(), saved);
    assert!(!app.is_dirty());
    app.handle_command(Command::Undo);
    assert!(app.is_dirty());
    app.handle_command(Command::Redo);
    assert!(!app.is_dirty());
    app.handle_command(Command::Undo);
    app.handle_command(Command::IncreaseTempo);
    // Same values as saved, but a different revision on a divergent branch.
    assert_eq!(app.project.bpm, 121.0);
    assert_ne!(app.revision(), saved);
    assert!(app.is_dirty());
    for index in 0..MAX_TRANSACTIONS + 10 {
        app.handle_command(if index % 2 == 0 {
            Command::IncreaseTempo
        } else {
            Command::DecreaseTempo
        });
    }
    for _ in 0..=MAX_TRANSACTIONS {
        app.handle_command(Command::Undo);
    }
    assert!(app.is_dirty());
    assert_ne!(app.revision(), saved);
    command(&mut app, "w");
    assert!(!app.is_dirty());
    app.handle_command(Command::Undo);
    assert!(!app.is_dirty()); // Evicted oldest boundary.
}

#[test]
fn pattern_indexed_history_does_not_follow_selected_pattern() {
    let mut app = App::default();
    app.project.patterns.push(Pattern::new(3, 4));
    let note = Some(Note {
        pitch: NotePitch::Cs,
        octave: 4,
    });
    app.edit(|edit| {
        edit.set_cell(0, 0, 0, note);
        edit.set_cell(1, 0, 0, note);
    });
    app.selected_pattern = 1;
    app.insert_note_under_cursor(None);
    app.selected_pattern = 0;
    app.handle_command(Command::Undo);
    assert_eq!(app.project.patterns[1].get_step(0, 0).unwrap().note, note);
    app.handle_command(Command::Undo);
    for pattern in &app.project.patterns {
        assert_eq!(pattern.get_step(0, 0).unwrap().note, None);
    }
    app.handle_command(Command::Redo);
    for pattern in &app.project.patterns {
        assert_eq!(pattern.get_step(0, 0).unwrap().note, note);
    }
}

#[test]
fn successful_load_resets_editor_and_requests_a_reliable_runtime_stop() {
    let mut app = App::default();
    app.handle_command(Command::IncreaseTempo);
    app.handle_command(Command::Copy);
    app.handle_command(Command::ToggleVisual);
    app.cursor_row = 5;
    app.cursor_track = 2;
    app.playback_row = 7;
    app.handle_command(Command::TogglePlayback);
    app.mode = Mode::Normal; // Commands are Normal-only; retain selection to exercise reset.
    let mut transport = Transport::default();
    transport.update(Instant::now(), true, 64, app.project.bpm, 4).unwrap();
    command(&mut app, &format!("e! {}", path_text(&fixture())));
    assert_eq!(app.project, project_file::load(&fixture()).unwrap());
    assert!(!app.is_dirty());
    assert_eq!(app.clipboard, Vec::<Vec<Option<Note>>>::new());
    assert_eq!(app.selection_anchor, None);
    assert_eq!(
        (app.cursor_row, app.cursor_track, app.selected_pattern, app.playback_row),
        (0, 0, 0, 0)
    );
    assert_eq!(app.mode, Mode::Normal);
    assert!(matches!(app.playback_state, PlaybackState::Stopped));
    let loaded = app.revision();
    app.handle_command(Command::Undo);
    app.handle_command(Command::Redo);
    assert_eq!(app.revision(), loaded);
    // A restored/restarted Playing mirror must not swallow the load's stop transition.
    app.playback_state = PlaybackState::Playing;
    assert!(app.take_stop_request());
    transport.stop();
    assert!(!app.take_stop_request());
    assert_eq!(transport.deadline(), None);
    assert_eq!(
        transport
            .update(Instant::now(), true, 2, app.project.bpm, 4)
            .unwrap()
            .row,
        0
    );
    app.handle_command(Command::IncreaseTempo);
    assert!(app.is_dirty());
}

#[test]
fn failed_load_leaves_document_history_and_runtime_unchanged_even_when_forced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bad");
    fs::write(&path, b"{\"version\":42}").unwrap();
    let mut app = App::default();
    app.handle_command(Command::IncreaseTempo);
    app.handle_command(Command::Copy);
    app.cursor_row = 12;
    app.cursor_track = 2;
    app.selection_anchor = Some((4, 1));
    app.selected_pattern = 0;
    app.handle_command(Command::TogglePlayback);
    app.playback_row = 7;
    let before = app.clone();
    let mut transport = Transport::default();
    transport.update(Instant::now(), true, 64, 120.0, 4).unwrap();
    let deadline = transport.deadline();
    command(&mut app, &format!("e {}", path_text(&path)));
    assert!(app.status.contains("Unsaved changes"));
    command(&mut app, &format!("e! {}", path_text(&path)));
    assert!(app.status.contains("Unsupported project version 42"));
    assert_eq!(app.project, before.project);
    assert_eq!(app.path, before.path);
    assert_eq!(app.revision(), before.revision());
    assert_eq!(app.saved_revision, before.saved_revision);
    assert_eq!(app.clipboard, before.clipboard);
    assert_eq!(app.selection_anchor, before.selection_anchor);
    assert_eq!(
        (app.cursor_row, app.cursor_track, app.playback_row, app.selected_pattern),
        (12, 2, 7, 0)
    );
    assert_eq!(app.mode, before.mode);
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    assert!(!app.take_stop_request());
    assert_eq!(transport.deadline(), deadline);
    app.handle_command(Command::Undo);
    assert!(!app.is_dirty());
    app.handle_command(Command::Redo);
    assert_eq!(app.revision(), before.revision());
}

#[test]
fn prompt_keys_are_literal_and_never_dispatch_editor_shortcuts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("q notes 123; $(touch nope) ~.json");
    let mut app = App::default();
    type_text(&mut app, ":");
    type_text(&mut app, "w ");
    type_text(&mut app, path_text(&path));
    assert!(!app.shutdown);
    assert_eq!(app.pending_count, None);
    assert_eq!(app.project, Project::default());
    assert_eq!(
        resolve(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE), &mut app),
        None
    );
    handle_key(
        KeyEvent::new_with_kind(KeyCode::Char('x'), KeyModifiers::NONE, KeyEventKind::Release),
        &mut app,
    );
    handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL), &mut app);
    press(&mut app, KeyCode::Enter);
    assert!(path.exists());
    assert_eq!(app.path.as_deref(), Some(path.as_path()));
    type_text(&mut app, ":q!");
    press(&mut app, KeyCode::Esc);
    assert!(!app.shutdown);
    type_text(&mut app, ":qu");
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.prompt.as_deref(), Some("q"));
    press(&mut app, KeyCode::Esc);
    app.handle_command(Command::IncreaseTempo);
    type_text(&mut app, "?");
    type_text(&mut app, ":wq123");
    assert!(app.help_visible);
    assert_eq!(app.prompt, None);
    assert!(!app.shutdown);
}

#[test]
fn quit_load_guards_and_write_quit_failures_use_real_prompt_keys() {
    let mut app = App::default();
    command(&mut app, "w");
    assert!(app.status.contains("No file path"));
    assert!(!app.is_dirty());
    command(&mut app, "wq");
    assert!(!app.shutdown);
    assert!(app.status.contains("No file path"));
    app.handle_command(Command::IncreaseTempo);
    type_text(&mut app, "qQ");
    assert!(!app.shutdown);
    command(&mut app, "q");
    assert!(!app.shutdown);
    command(&mut app, "e!");
    assert!(!app.shutdown);
    assert!(app.status.contains("Expected"));
    command(&mut app, "q! extra");
    assert!(!app.shutdown);
    command(&mut app, "unknown");
    assert!(app.status.contains("Unknown command"));
    command(&mut app, "q!");
    assert!(app.shutdown);
    let mut clean = App::default();
    command(&mut clean, "q");
    assert!(clean.shutdown);
}

#[cfg(unix)]
#[test]
fn saving_remembered_non_unicode_path_does_not_round_trip_through_display() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(OsString::from_vec(b"song\xff.json".to_vec()));
    let mut app = App {
        path: Some(path.clone()),
        ..App::default()
    };
    command(&mut app, "w");
    assert_eq!(app.path, Some(path.clone()));
    assert_eq!(project_file::load(&path).unwrap(), app.project);
    app.handle_command(Command::IncreaseTempo);
    command(&mut app, "wq");
    assert!(app.shutdown);
    assert!(!app.is_dirty());
    assert_eq!(project_file::load(&path).unwrap(), app.project);
}
