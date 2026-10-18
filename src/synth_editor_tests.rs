#![allow(clippy::float_cmp)]

use crate::{
    bindings::{handle_key, handle_key_at},
    state::{App, Mode, View},
    synth::{SynthPatch, Waveform},
    synth_controls::CONTROLS,
    tracker::Instrument,
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::time::{Duration, Instant};

fn key(app: &mut App, code: KeyCode) {
    handle_key(KeyEvent::new(code, KeyModifiers::NONE), app);
}
fn text(app: &mut App, value: &str) {
    for ch in value.chars() {
        key(app, KeyCode::Char(ch));
    }
}
fn setup() -> App {
    let mut app = App::default();
    key(&mut app, KeyCode::F(4));
    text(&mut app, "[");
    app.saved_revision = app.revision();
    app
}
fn row(app: &mut App, index: usize) {
    key(app, KeyCode::Esc);
    text(app, "gk");
    if index > 0 {
        text(app, &format!("{index}j"));
    }
    assert_eq!(app.cursor_row, index);
}
fn enter(app: &mut App, value: &str) {
    text(app, "i");
    key(app, KeyCode::Enter);
    assert!(app.prompt.is_some());
    text(app, value);
    key(app, KeyCode::Enter);
    assert!(app.prompt.is_none());
}

#[test]
fn every_control_accepts_exact_values_units_and_resets_independently() {
    let mut app = setup();
    let notes = app.project.patterns.clone();
    let values = [
        "Square", "-12 st", "-8 ct", "0.25", "sine", "12", "7", "0.5", "triangle", "-24", "100", "0.3", "8 kHz", "2",
        "25 ms", "1.25 s", "0.8", "0 s",
    ];
    for (index, value) in values.iter().enumerate() {
        row(&mut app, index);
        enter(&mut app, value);
        assert!(app.status.is_empty(), "{index}: {}", app.status);
    }
    let patch = app.project.synth_patch(0);
    assert_eq!(patch.oscillators[0].waveform, Waveform::Square);
    assert_eq!(patch.oscillators[0].coarse, -12.0);
    assert_eq!(patch.oscillators[0].fine, -8.0);
    assert_eq!(patch.oscillators[0].gain, 0.25);
    assert_eq!(patch.oscillators[1].waveform, Waveform::Sine);
    assert_eq!(patch.oscillators[2].waveform, Waveform::Triangle);
    assert_eq!(patch.cutoff, 8000.0);
    assert_eq!(patch.q, 2.0);
    assert_eq!(patch.envelope.attack, 0.025);
    assert_eq!(patch.envelope.decay, 1.25);
    assert_eq!(patch.envelope.sustain, 0.8);
    assert_eq!(patch.envelope.release, 0.0);
    assert_eq!(app.project.synth_patch(1), SynthPatch::default());
    assert_eq!(app.project.patterns, notes);
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("edited.json");
    crate::project_file::save(&app.project, &destination).unwrap();
    assert_eq!(crate::project_file::load(&destination).unwrap(), app.project);
    for (index, control) in CONTROLS.iter().enumerate() {
        row(&mut app, index);
        let before = app.project.synth_patch(0);
        text(&mut app, if index % 2 == 0 { "d" } else { "x" });
        assert_eq!(app.project.synth_patch(0), control.reset(before));
        let revision = app.revision();
        text(&mut app, "d");
        assert_eq!(app.revision(), revision);
    }
    assert_eq!(app.project.synth_patch(0), SynthPatch::default());
    for _ in 0..CONTROLS.len() {
        text(&mut app, "u");
    }
    assert_eq!(app.project.synth_patch(0), patch);
    for _ in 0..CONTROLS.len() {
        text(&mut app, "u");
    }
    assert!(!app.is_dirty());
}

#[test]
fn invalid_values_are_atomic_and_cannot_escape_into_commands() {
    let mut app = setup();
    for (index, value) in [
        (0, "noise"),
        (1, "25"),
        (2, "101 ct"),
        (3, "1 dB"),
        (12, "off"),
        (12, "20001 Hz"),
        (13, "4.01"),
        (14, "1msms"),
        (14, "-1ms"),
        (14, "1e309"),
        (15, "NaN"),
        (16, "0.7ms"),
        (17, "inf"),
        (17, "q!"),
        (17, "midi connect 0"),
    ] {
        row(&mut app, index);
        let before = app.project.clone();
        let revision = app.revision();
        enter(&mut app, value);
        assert!(!app.status.is_empty(), "accepted {index}: {value}");
        assert_eq!(app.project, before);
        assert_eq!(app.revision(), revision);
        assert!(!app.shutdown && app.midi_command.is_none());
    }
    assert!(!app.is_dirty());
}

#[test]
fn counted_adjustments_clamp_cycle_and_reset_with_single_undo() {
    let mut app = setup();
    row(&mut app, 1);
    text(&mut app, "i9999l");
    assert_eq!(app.project.synth_patch(0).oscillators[0].coarse, 24.0);
    let revision = app.revision();
    text(&mut app, "l");
    assert_eq!(app.revision(), revision);
    key(&mut app, KeyCode::Backspace);
    assert_eq!(app.project.synth_patch(0).oscillators[0].coarse, 0.0);
    key(&mut app, KeyCode::Esc);
    text(&mut app, "u");
    assert_eq!(app.project.synth_patch(0).oscillators[0].coarse, 24.0);
    text(&mut app, "u");
    assert!(!app.is_dirty());
    row(&mut app, 0);
    text(&mut app, "i4l");
    assert!(!app.is_dirty()); // Complete waveform cycle is a no-op.
    text(&mut app, "h");
    assert_eq!(app.project.synth_patch(0).oscillators[0].waveform, Waveform::Sine);
    key(&mut app, KeyCode::Esc);
    text(&mut app, "u");
    assert!(!app.is_dirty());
    text(&mut app, "3j");
    enter(&mut app, "0.25");
    text(&mut app, "3l");
    assert_eq!(app.project.synth_patch(0).oscillators[0].gain, 0.28);
}

#[test]
fn repeated_adjustments_join_gestures_but_navigation_breaks_them() {
    let mut app = setup();
    row(&mut app, 2);
    text(&mut app, "i");
    let now = Instant::now();
    for (i, kind) in [KeyEventKind::Press, KeyEventKind::Repeat, KeyEventKind::Repeat]
        .into_iter()
        .enumerate()
    {
        handle_key_at(
            KeyEvent::new_with_kind(KeyCode::Char('l'), KeyModifiers::NONE, kind),
            &mut app,
            now + Duration::from_millis(u64::try_from(i).unwrap() * 100),
        );
    }
    assert_eq!(app.project.synth_patch(0).oscillators[0].fine, 3.0);
    text(&mut app, "j");
    key(&mut app, KeyCode::Char('h'));
    key(&mut app, KeyCode::Esc);
    text(&mut app, "u");
    assert_eq!(app.project.synth_patch(0).oscillators[0].fine, 3.0);
    assert_eq!(app.project.synth_patch(0).oscillators[0].gain, 1.0);
    text(&mut app, "u");
    assert!(!app.is_dirty());
}

#[test]
fn parameter_navigation_is_view_local_and_builtin_controls_are_not_exposed() {
    let mut app = App::default();
    text(&mut app, "3jv");
    let saved = (app.mode, app.cursor_row, app.selection_anchor);
    let notes = app.project.patterns.clone();
    key(&mut app, KeyCode::F(4));
    text(&mut app, "i");
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.status.contains("Select Synth"));
    text(&mut app, "dxj");
    assert!(!app.is_dirty());
    text(&mut app, "[");
    app.project.patterns[0] = crate::tracker::Pattern::new(1, 4);
    text(&mut app, "gj");
    assert_eq!(app.cursor_row, 17);
    text(&mut app, "i");
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::F(4));
    assert_eq!((app.cursor_row, app.mode), (17, Mode::Insert));
    text(&mut app, "gl"); // Jump to a built-in track closes Synth Insert.
    assert_eq!(app.cursor_track, 3);
    assert_eq!(app.mode, Mode::Normal);
    text(&mut app, "gh");
    assert_eq!(app.project.instrument(0), Instrument::Synth);
    app.project.patterns = notes;
    key(&mut app, KeyCode::F(1));
    assert_eq!((app.mode, app.cursor_row, app.selection_anchor), saved);
}

#[test]
fn live_patch_edits_keep_arm_and_unarmed_history_keeps_playback() {
    let mut app = setup();
    app.recording.armed = true;
    app.handle_command(crate::command::Command::TogglePlayback);
    let _ = app.take_stop_request();
    row(&mut app, 12);
    enter(&mut app, "500 Hz");
    assert_eq!(app.project.synth_patch(0).cutoff, 500.0);
    assert!(matches!(app.playback_state, crate::state::PlaybackState::Playing));
    assert!(app.recording.armed);
    assert!(!app.take_stop_request());
    key(&mut app, KeyCode::Esc);
    // Armed undo intentionally stops transport under the existing recording policy.
    // Exercise live patch undo/redo separately, without an armed recording pass.
    app.recording.armed = false;
    text(&mut app, "u");
    assert_eq!(app.project.synth_patch(0).cutoff, 4000.0);
    handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL), &mut app);
    assert_eq!(app.project.synth_patch(0).cutoff, 500.0);
    assert!(matches!(app.playback_state, crate::state::PlaybackState::Playing));
    assert!(!app.recording.armed);
    assert!(!app.take_stop_request());
    key(&mut app, KeyCode::Enter);
    assert!(app.take_audition().is_none()); // Existing F4 audition is stopped-only.
}

#[test]
fn overlays_and_mode_guards_preserve_patch_and_audition_is_no_write() {
    let mut app = setup();
    let before = app.project.clone();
    text(&mut app, "?");
    text(&mut app, "ijldx");
    assert_eq!(app.project, before);
    key(&mut app, KeyCode::Esc);
    text(&mut app, "i");
    key(&mut app, KeyCode::Enter);
    text(&mut app, "square");
    key(&mut app, KeyCode::F(1));
    assert_eq!(app.view, View::Instrument);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.project, before);
    assert_eq!(app.mode, Mode::Insert);
    text(&mut app, "[]vzs"); // No voice changes, Visual mode, or hidden piano writes.
    assert_eq!(app.project, before);
    app.midi_browser = Some(crate::midi::DeviceBrowser::default());
    text(&mut app, "ldxi");
    key(&mut app, KeyCode::Enter);
    assert!(app.prompt.is_none());
    assert_eq!(app.project, before);
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Esc);
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        handle_key(
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, kind),
            &mut app,
        );
        assert!(app.take_audition().is_none());
    }
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.take_audition().unwrap().instrument, Instrument::Synth);
    assert_eq!(app.project, before);
    assert!(!app.is_dirty());
}
