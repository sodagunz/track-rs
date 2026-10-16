use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{
    bindings::handle_key,
    command::Command,
    state::{App, PlaybackState, View},
    tracker::{Instrument, NotePitch},
};

fn key(app: &mut App, c: char) {
    handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), app);
}

#[test]
fn insert_keys_preview_committed_note_even_on_noop_without_extra_history() {
    for instrument in [Instrument::Pad, Instrument::Kick] {
        let mut app = App {
            cursor_track: 2,
            octave: 5,
            ..App::default()
        };
        app.handle_command(Command::SetInstrument(instrument));
        key(&mut app, 'i');
        key(&mut app, ',');
        let event = app.take_audition().unwrap();
        assert_eq!(event.track, 2);
        assert_eq!(event.instrument, instrument);
        assert_eq!(event.note.pitch, NotePitch::C);
        assert_eq!(event.note.octave, 6);
        assert_eq!(event.duration, crate::audio::DEFAULT_NOTE_DURATION);
        let before = app.project.clone();
        key(&mut app, ',');
        assert_eq!(app.take_audition().unwrap().note, event.note);
        assert_eq!(app.project, before);
        app.handle_command(Command::Undo);
        assert!(app.project.patterns[0].get_step(2, 0).unwrap().note.is_none());
    }
}

#[test]
fn entry_preview_preserves_playback_and_obeys_real_key_guards() {
    let mut app = App::default();
    key(&mut app, 'z');
    assert!(app.take_audition().is_none());
    key(&mut app, 'i');
    app.handle_command(Command::TogglePlayback);
    let _ = app.take_stop_request();
    let position = (app.playing_order, app.playing_pattern);
    key(&mut app, 'x');
    assert_eq!(app.take_audition().unwrap().note.pitch, NotePitch::D);
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    assert_eq!((app.playing_order, app.playing_pattern), position);
    assert!(!app.take_stop_request());
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        handle_key(
            KeyEvent::new_with_kind(KeyCode::Char('z'), KeyModifiers::NONE, kind),
            &mut app,
        );
        assert!(app.take_audition().is_none());
    }
    app.help_visible = true;
    key(&mut app, 'z');
    assert!(app.take_audition().is_none());
    app.help_visible = false;
    app.prompt = Some(String::new());
    key(&mut app, 'z');
    assert!(app.take_audition().is_none());
}

fn enter(app: &mut App) {
    handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), app);
}

#[test]
fn audition_captures_voice_octave_and_track_without_editing() {
    let mut app = App::default();
    app.handle_command(Command::ShowInstrument);
    app.cursor_track = 2;
    app.cursor_row = 17;
    app.octave = 6;
    app.handle_command(Command::SetInstrument(Instrument::Pad));
    let before = app.project.clone();
    let dirty = app.is_dirty();
    enter(&mut app);
    let event = app.take_audition().unwrap();
    assert_eq!(event.track, 2);
    assert_eq!(event.instrument, Instrument::Pad);
    assert_eq!(event.note.pitch, NotePitch::C);
    assert_eq!(event.note.octave, 6);
    assert_eq!(event.duration, crate::audio::DEFAULT_NOTE_DURATION);
    assert!(app.take_audition().is_none());
    assert_eq!(app.project, before);
    assert_eq!(app.is_dirty(), dirty);
    assert_eq!(app.cursor_row, 17);
    app.handle_command(Command::Undo);
    assert_eq!(app.project.instrument(2), Instrument::Pluck);
    assert!(!app.is_dirty());
}

#[test]
fn audition_obeys_panel_playback_help_prompt_and_key_guards() {
    let mut app = App::default();
    for view in [View::Pattern, View::Arrangement, View::Mixer] {
        app.view = view;
        enter(&mut app);
        assert!(app.take_audition().is_none());
    }
    app.handle_command(Command::ShowInstrument);
    app.help_visible = true;
    enter(&mut app);
    assert!(app.take_audition().is_none());
    app.help_visible = false;
    app.prompt = Some(String::new());
    enter(&mut app);
    assert!(app.take_audition().is_none());
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        handle_key(
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, kind),
            &mut app,
        );
        assert!(app.take_audition().is_none());
    }
    app.playback_state = PlaybackState::Playing;
    enter(&mut app);
    assert!(app.take_audition().is_none());
    app.playback_state = PlaybackState::Stopped;
    enter(&mut app);
    assert!(app.take_audition().is_some());
    assert!(!app.is_dirty());
}

#[test]
fn pattern_audition_load_and_transport_lifecycle_discard_stale_requests() {
    let mut app = App::default();
    enter(&mut app);
    assert_eq!(app.mode, crate::state::Mode::Audition);
    key(&mut app, 'z');
    app.handle_command(Command::TogglePlayback);
    assert!(app.take_audition().is_none());
    assert!(app.take_stop_request());
    key(&mut app, 'x');
    app.handle_command(Command::RestartPlayback);
    assert!(app.take_audition().is_none());
    assert!(app.take_stop_request());
    key(&mut app, 'c');
    app.handle_command(Command::TogglePlayback);
    assert!(app.take_audition().is_none());
    assert!(app.take_stop_request());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project.json");
    app.execute_prompt(&format!("w {}", path.display()));
    key(&mut app, 'q');
    let revision = app.revision();
    app.execute_prompt(&format!("e {}/missing.json", directory.path().display()));
    assert!(app.take_audition().is_some());
    assert_eq!(app.mode, crate::state::Mode::Audition);
    assert_eq!(app.revision(), revision);
    key(&mut app, 'q');
    app.execute_prompt(&format!("e {}", path.display()));
    assert!(app.take_audition().is_none());
    assert!(app.take_stop_request());
    assert_eq!(app.mode, crate::state::Mode::Normal);
    enter(&mut app);
    key(&mut app, 'z');
    app.execute_prompt("q");
    assert!(app.shutdown);
    assert!(app.take_audition().is_none());
}

#[test]
fn starting_transport_discards_pending_audition_and_cancels_audio() {
    let mut app = App::default();
    app.handle_command(Command::ShowInstrument);
    enter(&mut app);
    app.handle_command(Command::TogglePlayback);
    assert!(app.take_audition().is_none());
    assert!(app.take_stop_request());
    assert!(!app.take_stop_request());
}
