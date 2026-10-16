use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{
    bindings::handle_key,
    command::Command,
    state::{App, Mode},
    tracker::{Instrument, Note, NotePitch},
};

fn key(app: &mut App, code: KeyCode) {
    handle_key(KeyEvent::new(code, KeyModifiers::NONE), app);
}
fn keys(app: &mut App, text: &str) {
    for ch in text.chars() {
        key(app, KeyCode::Char(ch));
    }
}
fn note(app: &App, track: usize, row: usize) -> Option<Note> {
    app.project.patterns[app.selected_pattern]
        .get_step(track, row)
        .unwrap()
        .note
}
fn seed() -> App {
    let mut app = App {
        project: crate::project::Project::new(8, 4),
        ..App::default()
    };
    for (track, row, pitch) in [(0, 0, NotePitch::C), (1, 1, NotePitch::D), (2, 2, NotePitch::E)] {
        app.project.patterns[app.selected_pattern]
            .set_step(
                track,
                row,
                Some(Note {
                    length_ticks: crate::tracker::NoteLength::default(),
                    pitch,
                    octave: 4,
                }),
            )
            .unwrap();
    }
    app.project.set_instrument(0, Instrument::Kick);
    app.project.set_instrument(1, Instrument::Snare);
    app
}

#[test]
fn reversed_snapshot_overlaps_overwrites_empties_and_undoes_atomically() {
    let mut app = seed();
    keys(&mut app, "jlvkhy"); // reversed 2x2, cursor ends at origin
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.selection_anchor, None);
    let revision = app.revision();
    keys(&mut app, "jlp"); // overlaps source
    assert_eq!(note(&app, 1, 1).unwrap().pitch, NotePitch::C);
    assert_eq!(note(&app, 2, 2).unwrap().pitch, NotePitch::D);
    assert_eq!(note(&app, 1, 2), None);
    assert_eq!(note(&app, 2, 1), None);
    assert_eq!(app.project.instrument(1), Instrument::Snare);
    assert_eq!((app.cursor_row, app.cursor_track), (1, 1));
    keys(&mut app, "u");
    assert_eq!(app.revision(), revision);
    assert_eq!(note(&app, 1, 1).unwrap().pitch, NotePitch::D);
    assert_eq!(note(&app, 2, 2).unwrap().pitch, NotePitch::E);
    handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL), &mut app);
    keys(&mut app, "u6j2lp"); // bottom right: clip both axes
    assert_eq!(note(&app, 3, 7).unwrap().pitch, NotePitch::C);
    assert_eq!(app.project.patterns[app.selected_pattern].num_rows, 8);
    assert_eq!(app.project.patterns[app.selected_pattern].tracks.len(), 4);
    keys(&mut app, "u");
    assert_eq!(note(&app, 3, 7), None);
}

#[test]
fn cut_clear_cancel_and_clipboard_survive_history() {
    let mut app = seed();
    let revision = app.revision();
    keys(&mut app, "p");
    assert_eq!(app.revision(), revision);
    keys(&mut app, "vjld");
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(note(&app, 0, 0), None);
    assert_eq!(note(&app, 1, 1), None);
    assert_eq!(note(&app, 2, 2).unwrap().pitch, NotePitch::E);
    keys(&mut app, "u");
    assert_eq!(app.revision(), revision);
    let clipboard = app.clipboard.clone();
    keys(&mut app, "vkhv");
    keys(&mut app, "vjl");
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.revision(), revision);
    assert_eq!(app.clipboard, clipboard);
    for clear in [KeyCode::Char('x'), KeyCode::Delete] {
        keys(&mut app, "vkh");
        key(&mut app, clear);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.selection_anchor, None);
        assert_eq!(app.clipboard, clipboard);
        keys(&mut app, "u");
    }
    keys(&mut app, "d");
    assert_eq!(app.clipboard.len(), 1);
    assert_eq!(app.clipboard[0].len(), 1);
    keys(&mut app, "u");
    let clipboard = app.clipboard.clone();
    handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL), &mut app);
    assert_eq!(app.clipboard, clipboard);
}

#[test]
fn steps_advance_valid_entry_including_noop_but_not_other_edits() {
    let mut app = seed();
    keys(&mut app, "i");
    for step in [0, 1, 2, 4] {
        assert_eq!(app.edit_step, step);
        app.cursor_row = 0;
        let revision = app.revision();
        keys(&mut app, "z"); // same C4 is a no-op but successful entry
        assert_eq!(app.revision(), revision);
        assert_eq!(app.cursor_row, step);
        keys(&mut app, "rf");
        key(&mut app, KeyCode::Delete);
        assert_eq!(app.cursor_row, step);
        app.cursor_row = 0;
        app.handle_command(Command::InsertNote {
            pitch: NotePitch::C,
            octave_offset: 0,
        });
        key(&mut app, KeyCode::Tab);
    }
    assert_eq!(app.edit_step, 0);
    key(&mut app, KeyCode::Tab);
    app.cursor_row = 7;
    keys(&mut app, "z");
    assert_eq!(app.cursor_row, 7);
    app.cursor_track = 99;
    app.cursor_row = 0;
    keys(&mut app, "z");
    assert_eq!(app.cursor_row, 0);
}

#[test]
fn operators_obey_counts_help_modifiers_and_press_only() {
    let mut app = seed();
    keys(&mut app, "y");
    let clipboard = app.clipboard.clone();
    let revision = app.revision();
    for ch in ['x', 'd', 'y', 'p'] {
        keys(&mut app, "2");
        key(&mut app, KeyCode::Char(ch));
        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            handle_key(
                KeyEvent::new_with_kind(KeyCode::Char(ch), KeyModifiers::NONE, kind),
                &mut app,
            );
        }
        handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::ALT), &mut app);
    }
    keys(&mut app, "vjl?dxyp");
    assert_eq!(app.revision(), revision);
    assert_eq!(app.clipboard, clipboard);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Visual);
    assert!(app.selection_anchor.is_some());
    key(&mut app, KeyCode::Esc);
    keys(&mut app, "i");
    handle_key(
        KeyEvent::new_with_kind(KeyCode::Tab, KeyModifiers::NONE, KeyEventKind::Repeat),
        &mut app,
    );
    assert_eq!(app.edit_step, 0);
    keys(&mut app, "dyp");
    assert_eq!(app.clipboard, clipboard);
    assert_eq!(app.revision(), revision);
}
