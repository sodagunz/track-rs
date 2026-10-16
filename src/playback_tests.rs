use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{
    bindings::handle_key,
    command::Command,
    state::{App, Mode, PlaybackState},
    tracker::Pattern,
    transport::Transport,
};

fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    handle_key(KeyEvent::new(code, modifiers), app);
}
fn space(app: &mut App) {
    key(app, KeyCode::Char(' '), KeyModifiers::NONE);
}
fn restart(app: &mut App) {
    key(app, KeyCode::Null, KeyModifiers::CONTROL);
}

#[test]
fn cursor_start_pins_route_stop_move_start_and_last_restart_wins() {
    let mut app = App::default();
    app.project.patterns.push(Pattern::new(5, 4));
    app.cursor_row = 9;
    let revision = app.revision();
    let now = Instant::now();
    let mut transport = Transport::default();
    space(&mut app);
    transport.update_app(now, &mut app, || panic!("no old run"));
    assert_eq!(app.playback_row, 9);
    let boundary = transport.deadline().unwrap();
    app.handle_command(Command::NextPattern);
    app.cursor_row = 3;
    transport.update_app(boundary, &mut app, || panic!("navigation cannot stop"));
    assert_eq!((app.playing_pattern, app.playback_row), (0, 10));
    space(&mut app);
    space(&mut app);
    let mut cancelled = 0;
    transport.update_app(boundary, &mut app, || cancelled += 1);
    assert_eq!((app.playing_pattern, app.playback_row), (1, 3));
    assert_eq!(cancelled, 1);
    restart(&mut app);
    app.handle_command(Command::PreviousPattern);
    restart(&mut app);
    transport.update_app(boundary, &mut app, || cancelled += 1);
    assert_eq!((app.playing_pattern, app.playback_row), (0, 0));
    assert_eq!(cancelled, 2);
    assert_eq!(transport.deadline(), Some(boundary + Duration::from_millis(125)));
    assert_eq!(app.revision(), revision);
    assert!(!app.is_dirty());
    app.handle_command(Command::Undo);
    assert_eq!(app.revision(), revision);
}

#[test]
fn song_cursor_uses_selected_entry_clamps_and_offsets_variable_repeated_patterns() {
    let mut app = App::default();
    app.project.patterns = vec![Pattern::new(3, 4), Pattern::new(5, 4)];
    app.project.order = vec![0, 1, 0, 1];
    app.handle_command(Command::ToggleSong);
    app.take_stop_request();
    app.selected_order = 3;
    app.cursor_row = 99;
    let now = Instant::now();
    let mut transport = Transport::default();
    space(&mut app);
    transport.update_app(now, &mut app, || panic!("already cancelled"));
    assert_eq!((app.playing_order, app.playing_pattern, app.playback_row), (3, 1, 4));
    assert_eq!(app.selected_pattern, 0);
    app.selected_order = 0;
    app.cursor_row = 0;
    transport.update_app(now + Duration::from_millis(125), &mut app, || panic!("pinned"));
    assert_eq!((app.playing_order, app.playback_row), (0, 0));
    transport.update_app(now + Duration::from_millis(2_000_625), &mut app, || panic!("pinned"));
    assert_eq!((app.playing_order, app.playback_row), (1, 1));
    assert_eq!(transport.deadline(), Some(now + Duration::from_millis(2_000_750)));
    restart(&mut app);
    transport.update_app(now, &mut app, || {});
    assert_eq!((app.playing_order, app.playing_pattern, app.playback_row), (0, 0, 0));
    space(&mut app);
    app.selected_order = usize::MAX;
    space(&mut app);
    transport.update_app(now, &mut app, || {});
    assert_eq!((app.playing_order, app.playback_row), (3, 0));
}

#[test]
fn song_nonzero_row_and_tempo_changes_keep_the_next_boundary() {
    let mut app = App::default();
    app.project.patterns = vec![Pattern::new(3, 4), Pattern::new(5, 4)];
    app.project.order = vec![0, 1, 0, 1];
    app.handle_command(Command::ToggleSong);
    app.selected_order = 2;
    app.cursor_row = 1;
    space(&mut app);
    let now = Instant::now();
    let mut transport = Transport::default();
    transport.update_app(now, &mut app, || {});
    assert_eq!((app.playing_order, app.playback_row), (2, 1));
    app.handle_command(Command::IncreaseTempo);
    let boundary = now + Duration::from_millis(125);
    assert!(transport
        .update_app(
            boundary.checked_sub(Duration::from_nanos(1)).unwrap(),
            &mut app,
            || panic!("tempo is not structural")
        )
        .is_empty());
    assert_eq!(transport.deadline(), Some(boundary));
    transport.update_app(boundary, &mut app, || panic!("tempo is not structural"));
    assert_eq!((app.playing_order, app.playback_row), (2, 2));
    let period = Duration::from_secs_f64(60.0 / 121.0 / 4.0);
    assert_eq!(transport.deadline(), Some(boundary + period));
    transport.update_app(boundary + period, &mut app, || {});
    assert_eq!((app.playing_order, app.playback_row), (3, 0));
}

#[test]
fn shortcuts_obey_modes_modifiers_counts_help_prompt_and_event_kind() {
    for mode in [Mode::Normal, Mode::Insert, Mode::Visual] {
        for code in [KeyCode::Char(' '), KeyCode::Null] {
            let mut app = App {
                mode,
                cursor_row: 8,
                ..App::default()
            };
            key(&mut app, code, KeyModifiers::CONTROL);
            assert!(matches!(app.playback_state, PlaybackState::Playing));
            assert_eq!(app.playback_row, 0);
            for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
                handle_key(KeyEvent::new_with_kind(code, KeyModifiers::CONTROL, kind), &mut app);
                handle_key(
                    KeyEvent::new_with_kind(KeyCode::Char(' '), KeyModifiers::NONE, kind),
                    &mut app,
                );
                assert!(matches!(app.playback_state, PlaybackState::Playing));
            }
            space(&mut app);
            for modifiers in [
                KeyModifiers::ALT,
                KeyModifiers::SHIFT,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ] {
                key(&mut app, code, modifiers);
                assert!(matches!(app.playback_state, PlaybackState::Stopped));
            }
            key(&mut app, KeyCode::Null, KeyModifiers::NONE);
            assert!(matches!(app.playback_state, PlaybackState::Stopped));
            app.help_visible = true;
            space(&mut app);
            restart(&mut app);
            assert!(matches!(app.playback_state, PlaybackState::Stopped));
            app.help_visible = false;
            app.prompt = Some(String::new());
            space(&mut app);
            restart(&mut app);
            assert!(matches!(app.playback_state, PlaybackState::Stopped));
            app.prompt = None;
            app.pending_count = Some(2);
            restart(&mut app);
            assert_eq!(app.pending_count, None);
            assert!(matches!(app.playback_state, PlaybackState::Stopped));
        }
    }
    let mut app = App {
        cursor_row: 8,
        ..App::default()
    };
    for ch in ":restart".chars() {
        key(&mut app, KeyCode::Char(ch), KeyModifiers::NONE);
    }
    assert!(matches!(app.playback_state, PlaybackState::Stopped));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    assert_eq!(app.playback_row, 0);
    app.cursor_row = 10;
    for ch in ":restart".chars() {
        key(&mut app, KeyCode::Char(ch), KeyModifiers::NONE);
    }
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.take_stop_request());
    assert_eq!(app.playback_row, 0);
}
