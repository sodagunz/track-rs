use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{
    bindings::handle_key,
    command::Command,
    state::{App, PlaybackState, View},
    tracker::{Instrument, NotePitch},
};

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
fn starting_transport_discards_pending_audition_and_cancels_audio() {
    let mut app = App::default();
    app.handle_command(Command::ShowInstrument);
    enter(&mut app);
    app.handle_command(Command::TogglePlayback);
    assert!(app.take_audition().is_none());
    assert!(app.take_stop_request());
    assert!(!app.take_stop_request());
}
