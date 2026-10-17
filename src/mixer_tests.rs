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
#[test]
fn mute_solo_flags_persist_default_and_reject_non_booleans() {
    let old = r#"{"volume_db":0.0,"pan":0.0}"#;
    let neutral: ChannelSettings = serde_json::from_str(old).unwrap();
    assert_eq!(neutral, ChannelSettings::default());
    for muted in [false, true] {
        for solo in [false, true] {
            let setting = ChannelSettings { muted, solo, ..neutral };
            let saved = serde_json::to_string(&setting).unwrap();
            assert_eq!(serde_json::from_str::<ChannelSettings>(&saved).unwrap(), setting);
            for any_solo in [false, true] {
                assert_eq!(setting.audible(any_solo), !muted && (!any_solo || solo));
            }
        }
    }
    assert!(serde_json::from_str::<ChannelSettings>(r#"{"volume_db":0,"pan":0,"muted":1}"#).is_err());
    assert!(serde_json::from_str::<ChannelSettings>(r#"{"volume_db":0,"pan":0,"solo":null}"#).is_err());
}

#[test]
fn mixer_mute_solo_are_normal_only_undoable_and_not_parameter_resets() {
    let mut app = App::default();
    key(&mut app, KeyCode::F(3));
    text(&mut app, "mM");
    assert!(app.project.channels()[0].muted);
    assert!(app.project.channels()[0].solo);
    assert!(app.is_dirty());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flags.json");
    project_file::save(&app.project, &path).unwrap();
    assert_eq!(project_file::load(&path).unwrap(), app.project);
    text(&mut app, "d");
    assert!(app.project.channels()[0].muted);
    assert!(app.project.channels()[0].solo);
    text(&mut app, "u");
    assert!(!app.project.channels()[0].solo);
    assert!(app.project.channels()[0].muted);
    text(&mut app, "u");
    assert!(!app.is_dirty());
    text(&mut app, "2m3M");
    assert!(!app.is_dirty());
    handle_key(
        KeyEvent::new_with_kind(KeyCode::Char('m'), KeyModifiers::NONE, KeyEventKind::Repeat),
        &mut app,
    );
    assert!(!app.is_dirty());
    text(&mut app, "imM");
    assert_eq!(app.project.channels()[0], ChannelSettings::default());
    key(&mut app, KeyCode::Esc);
    text(&mut app, "vmM");
    assert_eq!(app.project.channels()[0], ChannelSettings::default());
    key(&mut app, KeyCode::Esc);
    text(&mut app, "lm");
    assert!(app.project.channels()[1].muted);
    assert!(!app.project.channels()[0].muted);
}

#[test]
fn visual_mixer_rectangles_counts_off_reset_and_saved_pattern() {
    let mut app = App::default();
    text(&mut app, "3jv2l");
    let saved = (app.mode, app.cursor_row, app.cursor_track, app.selection_anchor);
    let notes = app.project.patterns.clone();
    key(&mut app, KeyCode::F(3));
    // Mixer height is independent of even a one-row pattern.
    app.project.patterns[0] = crate::tracker::Pattern::new(1, 4);
    text(&mut app, "v2lgj2s");
    assert_eq!(app.mode, Mode::Visual);
    for setting in &app.project.channels()[..3] {
        assert_eq!(setting.volume_db, Some(2.0));
        assert_eq!(setting.pan, 2.0);
        assert_eq!(setting.low_shelf_db, 2.0);
        assert_eq!(setting.high_shelf_db, 2.0);
        assert!((setting.hpf_hz.unwrap() - 21.189_262).abs() < 0.001);
        assert_eq!(setting.lpf_hz, None);
    }
    assert_eq!(app.project.channels()[3], ChannelSettings::default());
    let adjusted = app.project.channels().to_vec();
    text(&mut app, "d");
    assert_eq!(app.mode, Mode::Visual);
    assert!(app.project.channels().iter().all(|s| *s == ChannelSettings::default()));
    key(&mut app, KeyCode::Esc);
    text(&mut app, "u");
    assert_eq!(app.project.channels(), adjusted);
    text(&mut app, "u");
    assert!(!app.is_dirty());
    handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL), &mut app);
    assert_eq!(app.project.channels(), adjusted);
    // Reversed geometry and huge counts clamp without wrapping.
    text(&mut app, "vgkgh9999a");
    assert_eq!((app.cursor_row, app.cursor_track), (0, 0));
    for setting in &app.project.channels()[..3] {
        assert_eq!(setting.volume_db, None);
        assert_eq!(setting.pan, -100.0);
        assert_eq!(setting.low_shelf_db, -12.0);
        assert_eq!(setting.high_shelf_db, -12.0);
        assert_eq!(setting.hpf_hz, None);
        assert_eq!(setting.lpf_hz, Some(20.0));
    }
    text(&mut app, "ypi{A");
    assert_eq!(app.project.patterns[0].num_rows, 1);
    key(&mut app, KeyCode::Esc);
    assert_eq!((app.mode, app.selection_anchor), (Mode::Normal, None));
    app.project.patterns = notes;
    key(&mut app, KeyCode::F(1));
    assert_eq!(
        (app.mode, app.cursor_row, app.cursor_track, app.selection_anchor),
        saved
    );
}

#[test]
fn mixer_gestures_bound_repeats_and_close_at_input_boundaries() {
    use crate::bindings::handle_key_at;
    use std::time::{Duration, Instant};
    let start = Instant::now();
    let event = |ch, kind| KeyEvent::new_with_kind(KeyCode::Char(ch), KeyModifiers::NONE, kind);
    for kind in [KeyEventKind::Press, KeyEventKind::Repeat] {
        let mut app = App::default();
        key(&mut app, KeyCode::F(3));
        text(&mut app, "i");
        handle_key_at(event('h', KeyEventKind::Press), &mut app, start);
        for millis in [50, 100, 200] {
            handle_key_at(event('h', kind), &mut app, start + Duration::from_millis(millis));
        }
        assert_eq!(app.project.channels()[0].volume_db, Some(-4.0));
        app.handle_command(Command::Undo);
        assert_eq!(app.project.channels()[0].volume_db, Some(0.0));
        assert!(!app.is_dirty());
        app.handle_command(Command::Redo);
        assert_eq!(app.project.channels()[0].volume_db, Some(-4.0));
    }
    let mut app = App::default();
    key(&mut app, KeyCode::F(3));
    text(&mut app, "i");
    for millis in [0, 300, 600, 900, 1200, 1500, 1800, 2100] {
        handle_key_at(
            event('h', KeyEventKind::Press),
            &mut app,
            start + Duration::from_millis(millis),
        );
    }
    app.handle_command(Command::Undo);
    assert_eq!(app.project.channels()[0].volume_db, Some(-7.0));
    app.handle_command(Command::Undo);
    assert!(!app.is_dirty());
    for boundary in ["jkh", "?", "2h", "l", "g", ":"] {
        let mut app = App::default();
        key(&mut app, KeyCode::F(3));
        text(&mut app, "i");
        handle_key_at(event('h', KeyEventKind::Press), &mut app, start);
        text(&mut app, boundary);
        assert!(app
            .mixer_gesture
            .as_ref()
            .is_none_or(|_| boundary == "jkh" || boundary == "l"));
    }
    for (delay, release) in [(100, true), (401, false), (2000, false)] {
        let mut app = App::default();
        key(&mut app, KeyCode::F(3));
        text(&mut app, "i");
        handle_key_at(event('h', KeyEventKind::Press), &mut app, start);
        if release {
            handle_key_at(event('h', KeyEventKind::Release), &mut app, start);
        }
        handle_key_at(
            event('h', KeyEventKind::Press),
            &mut app,
            start + Duration::from_millis(delay),
        );
        app.handle_command(Command::Undo);
        assert_eq!(app.project.channels()[0].volume_db, Some(-1.0));
    }
}

#[test]
fn visual_gestures_counts_noops_and_mode_prompt_boundaries() {
    let mut app = App::default();
    let notes = app.project.patterns.clone();
    key(&mut app, KeyCode::F(3));
    text(&mut app, "vljss");
    assert_eq!(app.project.channels()[0].pan, 2.0);
    assert_eq!(app.project.channels()[1].volume_db, Some(2.0));
    key(&mut app, KeyCode::Esc);
    text(&mut app, "u");
    assert!(!app.is_dirty());
    handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL), &mut app);
    text(&mut app, "gkghi2h3h");
    assert_eq!(app.project.channels()[0].volume_db, Some(-3.0));
    key(&mut app, KeyCode::Esc);
    text(&mut app, "u");
    assert_eq!(app.project.channels()[0].volume_db, Some(0.0));
    text(&mut app, "u");
    assert_eq!(app.project.channels()[0].volume_db, Some(2.0));
    text(&mut app, "i9999l");
    let revision = app.revision();
    text(&mut app, "lll");
    assert_eq!(app.revision(), revision);
    numeric(&mut app, "6");
    assert_eq!(app.revision(), revision);
    numeric(&mut app, "oops");
    assert_eq!(app.revision(), revision);
    key(&mut app, KeyCode::Esc);
    text(&mut app, "u");
    assert_eq!(app.project.channels()[0].volume_db, Some(2.0));
    assert_eq!(app.project.patterns, notes);
}

#[test]
fn mixer_selection_load_append_and_structural_history_cleanup() {
    let mut app = App::default();
    key(&mut app, KeyCode::F(3));
    text(&mut app, "vglgj");
    key(&mut app, KeyCode::F(1));
    app.handle_command(Command::AddTrack);
    key(&mut app, KeyCode::F(3));
    assert_eq!(app.selection_anchor, None);
    assert_eq!(app.mode, Mode::Normal);
    text(&mut app, "glvghgks");
    key(&mut app, KeyCode::Esc);
    text(&mut app, "uu");
    assert_eq!(app.project.channels().len(), 4);
    key(&mut app, KeyCode::F(1));
    key(&mut app, KeyCode::F(3));
    assert!(app.cursor_track < 4);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.json");
    project_file::save(&Project::new(1, 1), &path).unwrap();
    text(&mut app, "v");
    key(&mut app, KeyCode::Esc);
    text(&mut app, &format!(":e! {}", path.display()));
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        (app.mode, app.cursor_row, app.cursor_track, app.selection_anchor),
        (Mode::Normal, 0, 0, None)
    );
    assert!(!app.is_dirty());
    text(&mut app, "v9999j9999ls");
    assert_eq!((app.cursor_row, app.cursor_track), (5, 0));
    assert_eq!(app.project.channels()[0].low_shelf_db, 1.0);
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
    assert_eq!((app.cursor_track, app.cursor_row), (3, 2));
    text(&mut app, "ghgkivazs{");
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
    assert_eq!(app.mode, Mode::Visual);
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
            ..ChannelSettings::default()
        },
        ChannelSettings {
            volume_db: Some(7.0),
            pan: 0.0,
            ..ChannelSettings::default()
        },
        ChannelSettings {
            volume_db: None,
            pan: f32::INFINITY,
            ..ChannelSettings::default()
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
        ..ChannelSettings::default()
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
        ..ChannelSettings::default()
    };
    project.channels[1] = ChannelSettings {
        volume_db: Some(6.0),
        pan: 100.0,
        ..ChannelSettings::default()
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

#[test]
fn tone_keys_numeric_off_boundaries_reset_and_history() {
    let mut app = App::default();
    let notes = app.project.patterns.clone();
    key(&mut app, KeyCode::F(3));
    text(&mut app, "2ji12l");
    assert_eq!(app.project.channels()[0].low_shelf_db, 12.0);
    numeric(&mut app, "-6.5");
    assert_eq!(app.project.channels()[0].low_shelf_db, -6.5);
    for invalid in ["NaN", "inf", "-inf", "off", "12.1", "-12.1"] {
        let revision = app.revision();
        numeric(&mut app, invalid);
        assert_eq!(app.revision(), revision);
    }
    text(&mut app, "j3h");
    assert_eq!(app.project.channels()[0].high_shelf_db, -3.0);
    text(&mut app, "jhl");
    assert_eq!(app.project.channels()[0].hpf_hz, Some(20.0));
    text(&mut app, "12l");
    assert!((app.project.channels()[0].hpf_hz.unwrap() - 40.0).abs() < 0.001);
    numeric(&mut app, "20.1");
    text(&mut app, "h");
    assert_eq!(app.project.channels()[0].hpf_hz, None);
    text(&mut app, "jlh");
    assert_eq!(app.project.channels()[0].lpf_hz, Some(20_000.0));
    text(&mut app, "12h");
    assert!((app.project.channels()[0].lpf_hz.unwrap() - 10_000.0).abs() < 0.01);
    numeric(&mut app, "19999");
    text(&mut app, "l");
    assert_eq!(app.project.channels()[0].lpf_hz, None);
    for invalid in ["0", "19.99", "20001", "NaN", "inf", "-inf"] {
        let revision = app.revision();
        numeric(&mut app, invalid);
        assert_eq!(app.revision(), revision);
    }
    numeric(&mut app, "1234.5");
    numeric(&mut app, "off");
    key(&mut app, KeyCode::Esc);
    text(&mut app, "u");
    assert_eq!(app.project.channels()[0].lpf_hz, Some(1234.5));
    handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL), &mut app);
    assert_eq!(app.project.channels()[0].lpf_hz, None);
    text(&mut app, "gj");
    assert_eq!(app.cursor_row, 5);
    key(&mut app, KeyCode::F(1));
    key(&mut app, KeyCode::F(3));
    assert_eq!(app.cursor_row, 5);
    text(&mut app, "2kdx");
    assert_eq!(app.project.channels()[0].high_shelf_db, 0.0);
    assert_eq!(app.project.patterns, notes);
}

#[test]
fn tone_v3_roundtrip_old_v3_defaults_and_validation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.json");
    let mut project = Project::new(2, 1);
    project.channels[0] = ChannelSettings {
        low_shelf_db: -12.0,
        high_shelf_db: 12.0,
        hpf_hz: Some(20_000.0),
        lpf_hz: Some(20.0),
        ..ChannelSettings::default()
    };
    project_file::save(&project, &path).unwrap();
    assert_eq!(project_file::load(&path).unwrap(), project);
    let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for (field, invalid) in [
        ("low_shelf_db", -12.1),
        ("high_shelf_db", 12.1),
        ("hpf_hz", 0.0),
        ("lpf_hz", 20001.0),
    ] {
        let mut value = original.clone();
        value["channels"][0][field] = invalid.into();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(project_file::load(&path).is_err());
    }
    let mut old = original;
    for field in ["low_shelf_db", "high_shelf_db", "hpf_hz", "lpf_hz"] {
        old["channels"][0].as_object_mut().unwrap().remove(field);
    }
    std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    assert_eq!(
        project_file::load(&path).unwrap().channels()[0],
        ChannelSettings::default()
    );
}
