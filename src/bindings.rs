use std::{
    sync::mpsc::{channel, Receiver},
    thread::spawn,
    time::Duration,
};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{command::Command, state::App, tracker::NotePitch};

/// A default keybinding and the metadata used to present it in help.
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    pub key: KeyCode,
    pub command: Command,
    pub description: &'static str,
    pub category: &'static str,
}

impl Binding {
    const fn new(key: KeyCode, command: Command, description: &'static str, category: &'static str) -> Self {
        Self {
            key,
            command,
            description,
            category,
        }
    }
}

/// The single source of truth for default controls and their descriptions.
pub const DEFAULT_BINDINGS: &[Binding] = &[
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

/// Resolve a press or permitted repeat; modified shortcuts and releases are ignored.
#[must_use]
pub fn resolve(key: KeyEvent) -> Option<Command> {
    // Shift is already represented in printable characters such as uppercase Q.
    let mut modifiers = key.modifiers;
    if matches!(key.code, KeyCode::Char(_)) {
        modifiers.remove(KeyModifiers::SHIFT);
    }
    if !modifiers.is_empty() || key.kind == KeyEventKind::Release {
        return None;
    }

    DEFAULT_BINDINGS.iter().find_map(|binding| {
        (binding.key == key.code && (key.kind == KeyEventKind::Press || binding.command.allows_repeat()))
            .then_some(binding.command)
    })
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
        if let Some(command) = resolve(key) {
            state.handle_command(command);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjustment_keys_normalize_shift_repeat_and_reject_modifiers() {
        for (key, command) in [
            ('[', Command::PreviousInstrument),
            (']', Command::NextInstrument),
            ('-', Command::DecreaseTempo),
            ('+', Command::IncreaseTempo),
            ('f', Command::LowerNote),
            ('r', Command::RaiseNote),
        ] {
            for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
                for kind in [KeyEventKind::Press, KeyEventKind::Repeat] {
                    assert_eq!(
                        resolve(KeyEvent::new_with_kind(KeyCode::Char(key), modifiers, kind)),
                        Some(command)
                    );
                }
                assert_eq!(
                    resolve(KeyEvent::new_with_kind(
                        KeyCode::Char(key),
                        modifiers,
                        KeyEventKind::Release
                    )),
                    None
                );
            }
            for modifiers in [
                KeyModifiers::CONTROL,
                KeyModifiers::ALT,
                KeyModifiers::SHIFT | KeyModifiers::CONTROL,
                KeyModifiers::SHIFT | KeyModifiers::ALT,
            ] {
                assert_eq!(resolve(KeyEvent::new(KeyCode::Char(key), modifiers)), None);
            }
        }
        assert_eq!(resolve(KeyEvent::new(KeyCode::Char('='), KeyModifiers::NONE)), None);
    }

    #[test]
    fn resolves_default_keys_and_shifted_quit() {
        assert_eq!(
            resolve(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE)),
            Some(Command::MoveCursorUp)
        );
        assert_eq!(
            resolve(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT)),
            Some(Command::Quit)
        );
        assert_eq!(
            resolve(KeyEvent::new(KeyCode::Char(','), KeyModifiers::NONE)),
            Some(Command::InsertNote {
                pitch: NotePitch::C,
                octave_offset: 1
            })
        );
        assert_eq!(resolve(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)), None);
        assert_eq!(resolve(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)), None);
    }

    #[test]
    fn releases_and_repeated_toggles_do_not_dispatch() {
        let event = |code, kind| KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind);
        assert_eq!(resolve(event(KeyCode::Char('k'), KeyEventKind::Release)), None);
        assert_eq!(resolve(event(KeyCode::Char(' '), KeyEventKind::Repeat)), None);
        assert_eq!(resolve(event(KeyCode::Char('q'), KeyEventKind::Repeat)), None);
        assert_eq!(resolve(event(KeyCode::Char('?'), KeyEventKind::Repeat)), None);
        assert_eq!(
            resolve(event(KeyCode::Down, KeyEventKind::Repeat)),
            Some(Command::ScrollHelpDown)
        );
        assert_eq!(
            resolve(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT)),
            Some(Command::ToggleHelp)
        );
        assert_eq!(
            resolve(event(KeyCode::Esc, KeyEventKind::Press)),
            Some(Command::DismissHelp)
        );
        assert_eq!(
            resolve(event(KeyCode::Char('j'), KeyEventKind::Repeat)),
            Some(Command::MoveCursorDown)
        );
    }

    #[test]
    fn registry_has_unique_keys_and_help_metadata() {
        for (index, binding) in DEFAULT_BINDINGS.iter().enumerate() {
            assert_ne!(binding.description, "");
            assert_ne!(binding.category, "");
            assert!(!DEFAULT_BINDINGS[..index].iter().any(|other| other.key == binding.key));
            assert_eq!(
                resolve(KeyEvent::new(binding.key, KeyModifiers::NONE)),
                Some(binding.command)
            );
        }
    }
}
