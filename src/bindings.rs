use std::{
    sync::mpsc::{channel, Receiver},
    thread::spawn,
    time::Duration,
};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{
    command::Command,
    state::{App, Mode},
    tracker::NotePitch,
};

/// A default keybinding and the metadata used to present it in help.
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    pub key: KeyCode,
    pub modifiers: KeyModifiers,
    pub modes: &'static [Mode],
    pub command: Command,
    pub description: &'static str,
    pub category: &'static str,
}

impl Binding {
    const fn new(key: KeyCode, command: Command, description: &'static str, category: &'static str) -> Self {
        Self {
            key,
            modifiers: KeyModifiers::NONE,
            modes: match command {
                Command::InsertNote { .. }
                | Command::DeleteNote
                | Command::LowerNote
                | Command::RaiseNote
                | Command::DecreaseOctave
                | Command::IncreaseOctave => &[Mode::Insert],
                Command::Undo | Command::Redo => &[Mode::Normal],
                Command::EnterInsert | Command::ToggleVisual | Command::CountDigit(_) => &[Mode::Normal, Mode::Visual],
                _ => &[Mode::Normal, Mode::Insert, Mode::Visual],
            },
            command,
            description,
            category,
        }
    }
}

/// The single source of truth for default controls and their descriptions.
pub const DEFAULT_BINDINGS: &[Binding] = &[
    Binding::new(KeyCode::Char('u'), Command::Undo, "Undo edit", "History"),
    Binding {
        modifiers: KeyModifiers::CONTROL,
        ..Binding::new(KeyCode::Char('r'), Command::Redo, "Redo edit", "History")
    },
    Binding::new(KeyCode::Char('i'), Command::EnterInsert, "Enter Insert", "Modes"),
    Binding::new(
        KeyCode::Char('v'),
        Command::ToggleVisual,
        "Toggle rectangle selection",
        "Modes",
    ),
    Binding::new(KeyCode::Esc, Command::EnterNormal, "Normal / cancel selection", "Modes"),
    Binding::new(
        KeyCode::Char('0'),
        Command::CountDigit(0),
        "Movement count (1–9999; 0 only trailing)",
        "Counts",
    ),
    Binding::new(
        KeyCode::Char('1'),
        Command::CountDigit(1),
        "Movement count (1–9999; 0 only trailing)",
        "Counts",
    ),
    Binding::new(
        KeyCode::Char('2'),
        Command::CountDigit(2),
        "Movement count (1–9999; 0 only trailing)",
        "Counts",
    ),
    Binding::new(
        KeyCode::Char('3'),
        Command::CountDigit(3),
        "Movement count (1–9999; 0 only trailing)",
        "Counts",
    ),
    Binding::new(
        KeyCode::Char('4'),
        Command::CountDigit(4),
        "Movement count (1–9999; 0 only trailing)",
        "Counts",
    ),
    Binding::new(
        KeyCode::Char('5'),
        Command::CountDigit(5),
        "Movement count (1–9999; 0 only trailing)",
        "Counts",
    ),
    Binding::new(
        KeyCode::Char('6'),
        Command::CountDigit(6),
        "Movement count (1–9999; 0 only trailing)",
        "Counts",
    ),
    Binding::new(
        KeyCode::Char('7'),
        Command::CountDigit(7),
        "Movement count (1–9999; 0 only trailing)",
        "Counts",
    ),
    Binding::new(
        KeyCode::Char('8'),
        Command::CountDigit(8),
        "Movement count (1–9999; 0 only trailing)",
        "Counts",
    ),
    Binding::new(
        KeyCode::Char('9'),
        Command::CountDigit(9),
        "Movement count (1–9999; 0 only trailing)",
        "Counts",
    ),
    Binding::new(
        KeyCode::Char('['),
        Command::PreviousInstrument,
        "Previous instrument",
        "Instruments",
    ),
    Binding::new(
        KeyCode::Char(']'),
        Command::NextInstrument,
        "Next instrument",
        "Instruments",
    ),
    Binding::new(
        KeyCode::Char('-'),
        Command::DecreaseTempo,
        "Decrease BPM by 1",
        "Playback",
    ),
    Binding::new(
        KeyCode::Char('+'),
        Command::IncreaseTempo,
        "Increase BPM by 1",
        "Playback",
    ),
    Binding::new(
        KeyCode::Char('f'),
        Command::LowerNote,
        "Lower note one semitone",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('r'),
        Command::RaiseNote,
        "Raise note one semitone",
        "Notes",
    ),
    Binding::new(KeyCode::Char('?'), Command::ToggleHelp, "Toggle help", "Application"),
    Binding::new(KeyCode::Esc, Command::DismissHelp, "Close help", "Application"),
    Binding::new(KeyCode::Up, Command::ScrollHelpUp, "Scroll help up", "Help"),
    Binding::new(KeyCode::Down, Command::ScrollHelpDown, "Scroll help down", "Help"),
    Binding::new(KeyCode::Char('q'), Command::Quit, "Quit", "Application"),
    Binding::new(KeyCode::Char('Q'), Command::Quit, "Quit", "Application"),
    Binding::new(
        KeyCode::Char(' '),
        Command::TogglePlayback,
        "Start/stop playback",
        "Playback",
    ),
    Binding::new(KeyCode::Char('k'), Command::MoveCursorUp, "Move up", "Navigation"),
    Binding::new(KeyCode::Char('j'), Command::MoveCursorDown, "Move down", "Navigation"),
    Binding::new(KeyCode::Char('h'), Command::MoveCursorLeft, "Move left", "Navigation"),
    Binding::new(KeyCode::Char('l'), Command::MoveCursorRight, "Move right", "Navigation"),
    Binding::new(
        KeyCode::Char('z'),
        Command::InsertNote {
            pitch: NotePitch::C,
            octave_offset: 0,
        },
        "Insert C",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('x'),
        Command::InsertNote {
            pitch: NotePitch::D,
            octave_offset: 0,
        },
        "Insert D",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('c'),
        Command::InsertNote {
            pitch: NotePitch::E,
            octave_offset: 0,
        },
        "Insert E",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('v'),
        Command::InsertNote {
            pitch: NotePitch::F,
            octave_offset: 0,
        },
        "Insert F",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('b'),
        Command::InsertNote {
            pitch: NotePitch::G,
            octave_offset: 0,
        },
        "Insert G",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('n'),
        Command::InsertNote {
            pitch: NotePitch::A,
            octave_offset: 0,
        },
        "Insert A",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('m'),
        Command::InsertNote {
            pitch: NotePitch::B,
            octave_offset: 0,
        },
        "Insert B",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char(','),
        Command::InsertNote {
            pitch: NotePitch::C,
            octave_offset: 1,
        },
        "Insert C one octave up",
        "Notes",
    ),
    Binding::new(KeyCode::Delete, Command::DeleteNote, "Delete note", "Notes"),
    Binding::new(KeyCode::Char('a'), Command::DecreaseOctave, "Decrease octave", "Notes"),
    Binding::new(KeyCode::Char('s'), Command::IncreaseOctave, "Increase octave", "Notes"),
];

/// Resolve terminal input using editor mode and pending-count state.
/// Counts are capped before arithmetic and consumed by one movement command.
pub fn resolve(key: KeyEvent, state: &mut App) -> Option<Command> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let mut modifiers = key.modifiers;
    if matches!(key.code, KeyCode::Char(_)) {
        modifiers.remove(KeyModifiers::SHIFT);
    }
    let binding = DEFAULT_BINDINGS.iter().find(|binding| {
        binding.key == key.code
            && binding.modifiers == modifiers
            && if state.help_visible {
                matches!(
                    binding.command,
                    Command::Quit
                        | Command::ToggleHelp
                        | Command::DismissHelp
                        | Command::ScrollHelpUp
                        | Command::ScrollHelpDown
                )
            } else {
                binding.modes.contains(&state.mode)
                    && !matches!(
                        binding.command,
                        Command::DismissHelp | Command::ScrollHelpUp | Command::ScrollHelpDown
                    )
            }
    });
    if key.kind == KeyEventKind::Repeat {
        // Repeated digits never extend a count; toggles and note entry are press-only.
        if binding.is_some_and(|binding| !binding.command.allows_repeat()) {
            return None;
        }
    }
    let Some(binding) = binding else {
        state.pending_count = None;
        return None;
    };
    if let Command::CountDigit(digit) = binding.command {
        if digit != 0 || state.pending_count.is_some() {
            state.pending_count = Some(
                state
                    .pending_count
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(u16::from(digit))
                    .min(9999),
            );
        }
        return None;
    }
    let count = state.pending_count.take();
    let (rows, tracks) = match binding.command {
        Command::MoveCursorUp => (-1, 0),
        Command::MoveCursorDown => (1, 0),
        Command::MoveCursorLeft => (0, -1),
        Command::MoveCursorRight => (0, 1),
        command => {
            return if count.is_none() || matches!(command, Command::Quit | Command::ToggleHelp | Command::EnterNormal) {
                Some(command)
            } else {
                None
            }
        }
    };
    let count = i32::from(count.unwrap_or(1));
    Some(Command::MoveCursor {
        rows: rows * count,
        tracks: tracks * count,
    })
}

/// Dispatch one real key through the shared resolver.
pub fn handle_key(key: KeyEvent, state: &mut App) {
    if let Some(command) = resolve(key, state) {
        state.handle_command(command);
    }
}

/// Listen for terminal keys on a background thread.
///
/// # Panics
///
/// The listener thread panics if the terminal cannot be polled or read.
#[must_use]
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
        handle_key(key, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) {
        handle_key(KeyEvent::new_with_kind(code, modifiers, kind), app);
    }

    fn sequence(app: &mut App, keys: &str) {
        for ch in keys.chars() {
            key(app, KeyCode::Char(ch), KeyModifiers::NONE, KeyEventKind::Press);
        }
    }

    fn escape(app: &mut App) {
        key(app, KeyCode::Esc, KeyModifiers::NONE, KeyEventKind::Press);
    }

    #[test]
    fn actual_sequences_distinguish_piano_modes_and_reversed_rectangles() {
        let mut app = App::default();
        assert_eq!(app.mode, Mode::Normal);
        sequence(&mut app, "zxc");
        assert_eq!(app.pattern.get_step(0, 0).unwrap().note, None);
        sequence(&mut app, "iv");
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(app.pattern.get_step(0, 0).unwrap().note.unwrap().pitch, NotePitch::F);
        sequence(&mut app, "2j"); // Insert digits don't count.
        assert_eq!(app.cursor_row, 1);
        escape(&mut app);
        sequence(&mut app, "16j2lv3k1h");
        assert_eq!((app.cursor_row, app.cursor_track), (14, 1));
        assert_eq!(app.selection_anchor, Some((17, 2)));
        for row in 0..64 {
            for track in 0..4 {
                assert_eq!(
                    app.is_selected(row, track),
                    (14..=17).contains(&row) && (1..=2).contains(&track)
                );
            }
        }
        sequence(&mut app, "v");
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.selection_anchor, None);
        sequence(&mut app, "v");
        escape(&mut app);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.selection_anchor, None);
    }

    #[test]
    fn counts_are_bounded_consumed_once_and_invalid_sequences_never_edit() {
        let mut app = App::default();
        sequence(&mut app, "0");
        assert_eq!(app.pending_count, None);
        sequence(&mut app, "999999999999999999999999999999");
        assert_eq!(app.pending_count, Some(9999));
        assert_eq!(
            resolve(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE), &mut app),
            Some(Command::MoveCursor { rows: 9999, tracks: 0 })
        );
        assert_eq!(app.pending_count, None);
        sequence(&mut app, "9999j9999l");
        assert_eq!((app.cursor_row, app.cursor_track), (63, 3));
        sequence(&mut app, "9999k9999h");
        assert_eq!((app.cursor_row, app.cursor_track), (0, 0));
        for invalid in ["2i", "2v", "2z", "2x", "2d", "2y", "2p", "2r", "2]", "2+", "2=", "2 "] {
            sequence(&mut app, invalid);
            assert_eq!(app.pending_count, None, "{invalid}");
            assert_eq!(app.mode, Mode::Normal);
            assert_eq!(app.bpm, 120.0);
            assert_eq!(app.pattern.instrument(0), crate::tracker::Instrument::Pluck);
            assert_eq!(app.pattern.get_step(0, 0).unwrap().note, None);
            assert!(matches!(app.playback_state, crate::state::PlaybackState::Stopped));
        }
        sequence(&mut app, "2");
        escape(&mut app);
        sequence(&mut app, "j");
        assert_eq!(app.cursor_row, 1);
    }

    #[test]
    fn help_preserves_modes_selection_and_playback_but_discards_counts() {
        for mode_keys in ["", "i", "v2j2l"] {
            let mut app = App::default();
            sequence(&mut app, " ");
            sequence(&mut app, mode_keys);
            let (mode, anchor, row, track) = (app.mode, app.selection_anchor, app.cursor_row, app.cursor_track);
            sequence(&mut app, "12?");
            assert!(app.help_visible);
            assert_eq!(app.pending_count, None);
            sequence(&mut app, "ivzj2 ");
            assert_eq!(
                (app.mode, app.selection_anchor, app.cursor_row, app.cursor_track),
                (mode, anchor, row, track)
            );
            assert!(matches!(app.playback_state, crate::state::PlaybackState::Playing));
            escape(&mut app);
            assert!(!app.help_visible);
            assert_eq!((app.mode, app.selection_anchor), (mode, anchor));
            assert_eq!(app.pattern.get_step(track, row).unwrap().note, None);
        }
    }

    #[test]
    fn repeats_releases_modifiers_and_adjustments_follow_registry() {
        let mut app = App::default();
        sequence(&mut app, "1");
        key(&mut app, KeyCode::Char('6'), KeyModifiers::NONE, KeyEventKind::Repeat);
        key(&mut app, KeyCode::Char('6'), KeyModifiers::NONE, KeyEventKind::Release);
        assert_eq!(app.pending_count, Some(1));
        key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE, KeyEventKind::Release);
        assert_eq!(app.pending_count, Some(1));
        sequence(&mut app, "6j");
        key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE, KeyEventKind::Repeat);
        assert_eq!(app.cursor_row, 17);
        for ch in ['i', 'v', '?', 'q', ' '] {
            key(&mut app, KeyCode::Char(ch), KeyModifiers::NONE, KeyEventKind::Repeat);
        }
        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.help_visible && !app.shutdown);
        sequence(&mut app, "2");
        key(&mut app, KeyCode::Char('j'), KeyModifiers::CONTROL, KeyEventKind::Press);
        assert_eq!(app.pending_count, None);
        assert_eq!(app.cursor_row, 17);
        sequence(&mut app, "i");
        for (ch, command) in [
            ('f', Command::LowerNote),
            ('r', Command::RaiseNote),
            ('a', Command::DecreaseOctave),
            ('s', Command::IncreaseOctave),
            ('+', Command::IncreaseTempo),
            (']', Command::NextInstrument),
        ] {
            for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
                assert_eq!(
                    resolve(
                        KeyEvent::new_with_kind(KeyCode::Char(ch), modifiers, KeyEventKind::Repeat),
                        &mut app
                    ),
                    Some(command)
                );
            }
            for modifiers in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
                assert_eq!(resolve(KeyEvent::new(KeyCode::Char(ch), modifiers), &mut app), None);
            }
        }
        key(&mut app, KeyCode::Char('v'), KeyModifiers::NONE, KeyEventKind::Repeat);
        assert_eq!(app.pattern.get_step(0, 17).unwrap().note, None);
        key(&mut app, KeyCode::Char('Q'), KeyModifiers::SHIFT, KeyEventKind::Press);
        assert!(app.shutdown);
    }

    #[test]
    fn navigation_and_global_controls_work_in_every_mode_and_edits_only_in_insert() {
        for mode_keys in ["", "i", "v"] {
            let mut app = App::default();
            sequence(&mut app, mode_keys);
            let mode = app.mode;
            sequence(&mut app, "jjllkh+-][ ");
            assert_eq!((app.cursor_row, app.cursor_track), (1, 1));
            assert_eq!(app.mode, mode);
            assert_eq!(app.bpm, 120.0);
            assert!(matches!(app.playback_state, crate::state::PlaybackState::Playing));
            sequence(&mut app, "z");
            key(&mut app, KeyCode::Delete, KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(app.pattern.get_step(1, 1).unwrap().note, None);
        }
        let mut app = App::default();
        sequence(&mut app, "izsr");
        let note = app.pattern.get_step(0, 0).unwrap().note.unwrap();
        assert_eq!(note.pitch, NotePitch::Cs);
        assert_eq!(note.octave, 4);
        assert_eq!(app.octave, 5);
        escape(&mut app);
        sequence(&mut app, "xfrzas");
        key(&mut app, KeyCode::Delete, KeyModifiers::NONE, KeyEventKind::Press);
        assert_eq!(app.pattern.get_step(0, 0).unwrap().note, Some(note));
        assert_eq!(app.octave, 5);
        sequence(&mut app, "i");
        key(&mut app, KeyCode::Delete, KeyModifiers::NONE, KeyEventKind::Press);
        assert_eq!(app.pattern.get_step(0, 0).unwrap().note, None);
    }

    #[test]
    fn registry_has_unique_mode_modifier_keys_and_metadata() {
        for (index, binding) in DEFAULT_BINDINGS.iter().enumerate() {
            assert!(!binding.description.is_empty() && !binding.category.is_empty());
            for mode in binding.modes {
                let mut app = App {
                    mode: *mode,
                    help_visible: matches!(
                        binding.command,
                        Command::DismissHelp | Command::ScrollHelpUp | Command::ScrollHelpDown
                    ),
                    ..App::default()
                };
                assert!(!DEFAULT_BINDINGS[..index].iter().any(|other| other.key == binding.key
                    && other.modifiers == binding.modifiers
                    && other.modes.contains(mode)
                    && !matches!(other.command, Command::EnterNormal)));
                let result = resolve(KeyEvent::new(binding.key, binding.modifiers), &mut app);
                if matches!(binding.command, Command::CountDigit(_)) {
                    assert_eq!(result, None);
                } else {
                    assert!(result.is_some(), "{binding:?}");
                }
            }
        }
    }
}
