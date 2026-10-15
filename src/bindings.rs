use std::{
    sync::mpsc::{channel, Receiver},
    thread::spawn,
    time::Duration,
};

use crossterm::event::{self, Event, KeyCode, KeyEvent};

use crate::{
    state::{App, CursorOffset},
    tracker::{Note, NotePitch},
};

pub fn listen() -> Receiver<KeyEvent> {
    let (tx, rx) = channel();
    spawn(move || loop {
        if event::poll(Duration::from_millis(100)).expect("Polling should be fine") {
            if let Event::Key(key) = event::read().expect("Reading an event as well") {
                if tx.send(key).is_err() {
                    break;
                }
            }
        }
    });

    rx
}

pub fn handle(rx: &Receiver<KeyEvent>, state: &mut App) {
    if let Ok(key) = rx.try_recv() {
        match key.code {
            KeyCode::Char('q' | 'Q') => state.shutdown(),
            KeyCode::Char(' ') => state.toggle_playback(),
            KeyCode::Char('k') => state.offset_cursor(&CursorOffset::Neg(1), &CursorOffset::None),
            KeyCode::Char('j') => state.offset_cursor(&CursorOffset::Pos(1), &CursorOffset::None),
            KeyCode::Char('h') => state.offset_cursor(&CursorOffset::None, &CursorOffset::Neg(1)),
            KeyCode::Char('l') => state.offset_cursor(&CursorOffset::None, &CursorOffset::Pos(1)),

            // TODO: inputs for notes
            KeyCode::Char('z') => state.insert_note_under_cursor(Some(Note {
                pitch: NotePitch::C,
                octave: state.octave,
            })),
            KeyCode::Char('x') => state.insert_note_under_cursor(Some(Note {
                pitch: NotePitch::D,
                octave: state.octave,
            })),
            KeyCode::Char('c') => state.insert_note_under_cursor(Some(Note {
                pitch: NotePitch::E,
                octave: state.octave,
            })),
            KeyCode::Char('v') => state.insert_note_under_cursor(Some(Note {
                pitch: NotePitch::F,
                octave: state.octave,
            })),
            KeyCode::Char('b') => state.insert_note_under_cursor(Some(Note {
                pitch: NotePitch::G,
                octave: state.octave,
            })),
            KeyCode::Char('n') => state.insert_note_under_cursor(Some(Note {
                pitch: NotePitch::A,
                octave: state.octave,
            })),
            KeyCode::Char('m') => state.insert_note_under_cursor(Some(Note {
                pitch: NotePitch::B,
                octave: state.octave,
            })),
            KeyCode::Char(',') => state.insert_note_under_cursor(Some(Note {
                pitch: NotePitch::C,
                octave: state.octave + 1,
            })),
            KeyCode::Delete => state.insert_note_under_cursor(None),
            KeyCode::Char('a') => state.octave = state.octave.saturating_sub(1).clamp(0, 9),
            KeyCode::Char('s') => state.octave = state.octave.saturating_add(1).clamp(0, 9),
            // KeyCode::Char('?') => open_help_floating_pane
            _ => {}
        }
    }
}
