#![allow(clippy::float_cmp)]

use crate::{
    bindings::handle_key,
    command::Command,
    project::{ChannelSettings, Project},
    project_file,
    state::{App, Mode, PlaybackState, View},
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

fn key(app: &mut App, code: KeyCode) {
    handle_key(KeyEvent::new(code, KeyModifiers::NONE), app);
}
fn text(app: &mut App, value: &str) {
    for ch in value.chars() {
        key(app, KeyCode::Char(ch));
    }
}
fn numeric(app: &mut App, value: &str) {
    key(app, KeyCode::Enter);
    assert!(app.prompt.is_some());
    text(app, value);
    key(app, KeyCode::Enter);
}

#[test]
fn real_mixer_keys_counts_modes_numeric_reset_history_and_playback() {
    let mut app = App::default();
    let notes = app.project.patterns.clone();
    key(&mut app, KeyCode::F(3));
    text(&mut app, "3l2j");
    assert_eq!((app.cursor_track, app.cursor_row), (3, 1));
    text(&mut app, "ghgjivazs{");
    assert_eq!(app.mode, Mode::Insert);
    assert_eq!(app.project.patterns, notes);
    text(&mut app, "10h");
    assert_eq!(app.project.channels()[0].volume_db, Some(-10.0));
    app.handle_command(Command::TogglePlayback);
    let revision = app.revision();
    numeric(&mut app, "-10");
    assert_eq!(app.revision(), revision);
    for invalid in ["NaN", "inf", "-infinity", "7", "-61", "", "no"] {
        numeric(&mut app, invalid);
        assert_ne!(app.status, "");
        assert_eq!(app.revision(), revision);
    }
    numeric(&mut app, "-inf");
    assert_eq!(app.project.channels()[0].volume_db, None);
    text(&mut app, "l");
    assert_eq!(app.project.channels()[0].volume_db, Some(-60.0));
    text(&mut app, "j25l");
    assert_eq!(app.project.channels()[0].pan, 25.0);
    for invalid in ["NaN", "101", "-101", "-inf"] {
        let revision = app.revision();
        numeric(&mut app, invalid);
        assert_eq!(app.revision(), revision);
    }
    numeric(&mut app, "-100");
    assert_eq!(app.project.channels()[0].pan, -100.0);
    key(&mut app, KeyCode::Backspace);
    assert_eq!(app.project.channels()[0].pan, 0.0);
    key(&mut app, KeyCode::Esc);
    text(&mut app, "u");
    assert_eq!(app.project.channels()[0].pan, -100.0);
    handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL), &mut app);
    assert_eq!(app.project.channels()[0].pan, 0.0);
    let revision = app.revision();
    text(&mut app, "dxv");
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.revision(), revision);
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    assert_eq!(app.project.channels()[1], ChannelSettings::default());
    assert_eq!(app.project.patterns, notes);
}

#[test]
fn mixer_arrows_repeat_prompt_priority_and_saved_modes() {
    let mut app = App::default();
    text(&mut app, "3jvi");
    let saved = (app.mode, app.cursor_row, app.cursor_track);
    key(&mut app, KeyCode::F(3));
    text(&mut app, "i");
    key(&mut app, KeyCode::Left);
    handle_key(
        KeyEvent::new_with_kind(KeyCode::Left, KeyModifiers::NONE, KeyEventKind::Repeat),
        &mut app,
    );
    assert_eq!(app.project.channels()[0].volume_db, Some(-2.0));
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Right);
    assert_eq!(app.project.channels()[0].pan, 1.0);
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::F(1));
    assert_eq!(app.view, View::Mixer);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Insert);
    key(&mut app, KeyCode::F(1));
    assert_eq!((app.mode, app.cursor_row, app.cursor_track), saved);
    key(&mut app, KeyCode::F(3));
    assert_eq!((app.mode, app.cursor_row), (Mode::Insert, 1));
    key(&mut app, KeyCode::Esc);
    text(&mut app, "?");
    let revision = app.revision();
    text(&mut app, "il");
    assert_eq!(app.revision(), revision);
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn channel_transactions_append_undo_redo_and_invalid_noops() {
    let mut app = App::default();
    let clean = app.revision();
    for invalid in [
        ChannelSettings {
            volume_db: Some(f32::NAN),
            pan: 0.0,
        },
        ChannelSettings {
            volume_db: Some(7.0),
            pan: 0.0,
        },
        ChannelSettings {
            volume_db: None,
            pan: f32::INFINITY,
        },
    ] {
        app.edit(|edit| edit.set_channel(0, invalid));
    }
    app.edit(|edit| edit.set_channel(0, ChannelSettings::default()));
    assert_eq!(app.revision(), clean);
    app.handle_command(Command::AddTrack);
    let track = app.project.channels().len() - 1;
    assert_eq!(app.project.channels()[track], ChannelSettings::default());
    let settings = ChannelSettings {
        volume_db: None,
        pan: 33.5,
    };
    app.edit(|edit| edit.set_channel(track, settings));
    app.handle_command(Command::Undo);
    assert_eq!(app.project.channels()[track], ChannelSettings::default());
    app.handle_command(Command::Undo);
    assert_eq!(app.project.channels().len(), track);
    assert!(!app.is_dirty());
    app.handle_command(Command::Redo);
    app.handle_command(Command::Redo);
    assert_eq!(app.project.channels()[track], settings);
}

#[test]
fn v3_roundtrip_strict_channels_and_explicit_v2_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mixer.json");
    let mut project = Project::new(2, 2);
    project.channels[0] = ChannelSettings {
        volume_db: None,
        pan: -100.0,
    };
    project.channels[1] = ChannelSettings {
        volume_db: Some(6.0),
        pan: 100.0,
    };
    project_file::save(&project, &path).unwrap();
    assert_eq!(project_file::load(&path).unwrap(), project);
    let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(original["version"], 3);
    for invalid in [
        serde_json::json!([]),
        serde_json::json!([{"volume_db":7,"pan":0},{"volume_db":0,"pan":0}]),
        serde_json::json!([{"volume_db":0,"pan":101},{"volume_db":0,"pan":0}]),
        serde_json::Value::Null,
    ] {
        let mut value = original.clone();
        value["channels"] = invalid;
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(project_file::load(&path).is_err());
    }
    let mut legacy = original;
    legacy["version"] = 2.into();
    legacy.as_object_mut().unwrap().remove("channels");
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let migrated = project_file::load(&path).unwrap();
    assert_eq!(migrated.channels(), &[ChannelSettings::default(); 2]);
    assert_eq!(migrated.patterns, project.patterns);
    legacy["version"] = 3.into();
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert!(project_file::load(&path).is_err());
}
