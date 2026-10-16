use std::{
    sync::mpsc::{channel, Receiver},
    thread::spawn,
    time::{Duration, Instant},
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
    pub prefix: Option<KeyCode>,
    pub modifiers: KeyModifiers,
    pub modes: &'static [Mode],
    pub command: Command,
    pub description: &'static str,
    pub category: &'static str,
}

impl Binding {
    pub(crate) const fn available_in_view(&self, view: crate::state::View) -> bool {
        (!(matches!(self.key, KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down)
            && matches!(
                self.command,
                Command::MoveCursorLeft | Command::MoveCursorRight | Command::MoveCursorUp | Command::MoveCursorDown
            ))
            || matches!(view, crate::state::View::Mixer | crate::state::View::Pattern))
            && self.command.available_in_view(view)
            && (!matches!(self.command, Command::MixerValue | Command::MixerReset)
                || matches!(view, crate::state::View::Mixer))
    }

    pub(crate) const fn description_in_view(&self, view: crate::state::View) -> &'static str {
        if matches!(view, crate::state::View::Pattern) {
            match self.command {
                Command::MoveCursorLeft => return "Previous track",
                Command::MoveCursorRight => return "Next track",
                Command::MoveCursorUp => return "Previous row",
                Command::MoveCursorDown => return "Next row",
                _ => {}
            }
        }
        if matches!(view, crate::state::View::Mixer) {
            match self.command {
                Command::MoveCursorLeft => return "Previous channel / decrease in Insert",
                Command::MoveCursorRight => return "Next channel / increase in Insert",
                Command::MoveCursorUp => return "Previous parameter",
                Command::MoveCursorDown => return "Next parameter",
                Command::FirstRow => return "First parameter",
                Command::LastRow => return "Last parameter",
                Command::EnterInsert => return "Enter mixer adjustment mode",
                Command::LowerOctave => return "Decrease selected controls",
                Command::RaiseOctave => return "Increase selected controls",
                Command::MixerReset => return "Reset cell/rectangle to neutral",
                _ => {}
            }
        }
        self.description
    }

    pub(crate) fn available_in_mode(&self, view: crate::state::View, mode: Mode) -> bool {
        if !self.available_in_view(view) || !self.modes.contains(&mode) {
            return false;
        }
        if view == crate::state::View::Pattern
            && matches!(
                self.command,
                Command::MoveCursorLeft | Command::MoveCursorRight | Command::MoveCursorUp | Command::MoveCursorDown
            )
            && matches!(self.key, KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down)
            && !matches!(mode, Mode::Insert | Mode::Audition)
        {
            return false;
        }
        if view == crate::state::View::Mixer
            && matches!(self.command, Command::LowerOctave | Command::RaiseOctave)
            && mode != Mode::Visual
        {
            return false;
        }
        if mode == Mode::Audition
            && (view != crate::state::View::Pattern
                || matches!(
                    self.command,
                    Command::DeleteNote | Command::CycleEditStep | Command::HalveLength | Command::DoubleLength
                ))
        {
            return false;
        }
        if view == crate::state::View::Pattern && matches!(mode, Mode::Insert | Mode::Audition) {
            return matches!(
                self.command,
                Command::InsertNote { .. }
                    | Command::HalveLength
                    | Command::DoubleLength
                    | Command::CycleEditStep
                    | Command::DecreaseOctave
                    | Command::IncreaseOctave
                    | Command::DeleteNote
                    | Command::PreviousInstrument
                    | Command::NextInstrument
                    | Command::DecreaseTempo
                    | Command::IncreaseTempo
                    | Command::EnterNormal
                    | Command::TogglePlayback
                    | Command::RestartPlayback
                    | Command::ToggleHelp
                    | Command::ShowPattern
                    | Command::ShowArrangement
                    | Command::ShowMixer
                    | Command::ShowInstrument
            ) || (self.prefix.is_none()
                && matches!(self.key, KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down));
        }
        true
    }

    fn matches_context(&self, state: &App) -> bool {
        self.available_in_mode(state.view, state.mode)
    }

    const fn new(key: KeyCode, command: Command, description: &'static str, category: &'static str) -> Self {
        Self {
            key,
            prefix: None,
            modifiers: KeyModifiers::NONE,
            modes: match command {
                Command::InsertNote { .. }
                | Command::HalveLength
                | Command::DoubleLength
                | Command::CycleEditStep
                | Command::DecreaseOctave
                | Command::IncreaseOctave => &[Mode::Insert, Mode::Audition],
                Command::OpenPrompt | Command::Undo | Command::Redo | Command::Paste => &[Mode::Normal],
                Command::Copy | Command::Cut | Command::EnterInsert | Command::ToggleVisual => {
                    &[Mode::Normal, Mode::Visual]
                }
                _ => &[Mode::Normal, Mode::Insert, Mode::Visual, Mode::Audition],
            },
            command,
            description,
            category,
        }
    }
}

/// The single source of truth for default controls and their descriptions.
pub const DEFAULT_BINDINGS: &[Binding] = &[
    Binding {
        modes: &[Mode::Normal],
        ..Binding::new(
            KeyCode::Enter,
            Command::EnterAudition,
            "Play without writing (F1)",
            "Notes",
        )
    },
    Binding::new(
        KeyCode::Char('s'),
        Command::InsertNote {
            pitch: NotePitch::Cs,
            octave_offset: 0,
        },
        "Insert Cs",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('d'),
        Command::InsertNote {
            pitch: NotePitch::Ds,
            octave_offset: 0,
        },
        "Insert Ds",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('g'),
        Command::InsertNote {
            pitch: NotePitch::Fs,
            octave_offset: 0,
        },
        "Insert Fs",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('h'),
        Command::InsertNote {
            pitch: NotePitch::Gs,
            octave_offset: 0,
        },
        "Insert Gs",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('j'),
        Command::InsertNote {
            pitch: NotePitch::As,
            octave_offset: 0,
        },
        "Insert As",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('q'),
        Command::InsertNote {
            pitch: NotePitch::C,
            octave_offset: 1,
        },
        "Insert C (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('2'),
        Command::InsertNote {
            pitch: NotePitch::Cs,
            octave_offset: 1,
        },
        "Insert Cs (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('w'),
        Command::InsertNote {
            pitch: NotePitch::D,
            octave_offset: 1,
        },
        "Insert D (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('3'),
        Command::InsertNote {
            pitch: NotePitch::Ds,
            octave_offset: 1,
        },
        "Insert Ds (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('e'),
        Command::InsertNote {
            pitch: NotePitch::E,
            octave_offset: 1,
        },
        "Insert E (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('r'),
        Command::InsertNote {
            pitch: NotePitch::F,
            octave_offset: 1,
        },
        "Insert F (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('5'),
        Command::InsertNote {
            pitch: NotePitch::Fs,
            octave_offset: 1,
        },
        "Insert Fs (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('t'),
        Command::InsertNote {
            pitch: NotePitch::G,
            octave_offset: 1,
        },
        "Insert G (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('6'),
        Command::InsertNote {
            pitch: NotePitch::Gs,
            octave_offset: 1,
        },
        "Insert Gs (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('y'),
        Command::InsertNote {
            pitch: NotePitch::A,
            octave_offset: 1,
        },
        "Insert A (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('7'),
        Command::InsertNote {
            pitch: NotePitch::As,
            octave_offset: 1,
        },
        "Insert As (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('u'),
        Command::InsertNote {
            pitch: NotePitch::B,
            octave_offset: 1,
        },
        "Insert B (upper octave)",
        "Notes",
    ),
    Binding::new(
        KeyCode::PageUp,
        Command::DecreaseOctave,
        "Decrease entry octave",
        "Notes",
    ),
    Binding::new(
        KeyCode::PageDown,
        Command::IncreaseOctave,
        "Increase entry octave",
        "Notes",
    ),
    Binding::new(
        KeyCode::Left,
        Command::MoveCursorLeft,
        "Previous channel / decrease in Insert",
        "Navigation",
    ),
    Binding::new(
        KeyCode::Right,
        Command::MoveCursorRight,
        "Next channel / increase in Insert",
        "Navigation",
    ),
    Binding::new(KeyCode::Up, Command::MoveCursorUp, "Previous parameter", "Navigation"),
    Binding::new(KeyCode::Down, Command::MoveCursorDown, "Next parameter", "Navigation"),
    Binding {
        modes: &[Mode::Normal],
        ..Binding::new(KeyCode::Char('m'), Command::MixerMute, "Toggle channel mute", "Mixer")
    },
    Binding {
        modes: &[Mode::Normal],
        ..Binding::new(KeyCode::Char('M'), Command::MixerSolo, "Toggle channel solo", "Mixer")
    },
    Binding {
        modes: &[Mode::Insert, Mode::Audition],
        ..Binding::new(
            KeyCode::Enter,
            Command::MixerValue,
            "Enter value: dB, pan %, or cutoff Hz/off",
            "Mixer",
        )
    },
    Binding {
        modes: &[Mode::Normal, Mode::Visual],
        ..Binding::new(
            KeyCode::Char('d'),
            Command::MixerReset,
            "Reset selected mixer cell",
            "Mixer",
        )
    },
    Binding {
        modes: &[Mode::Normal, Mode::Visual],
        ..Binding::new(
            KeyCode::Char('x'),
            Command::MixerReset,
            "Reset selected mixer cell",
            "Mixer",
        )
    },
    Binding {
        modes: &[Mode::Insert, Mode::Audition],
        ..Binding::new(
            KeyCode::Backspace,
            Command::MixerReset,
            "Reset selected mixer cell",
            "Mixer",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        ..Binding::new(
            KeyCode::Enter,
            Command::AuditionInstrument,
            "Audition voice (F4, stopped)",
            "Playback",
        )
    },
    Binding {
        modes: &[Mode::Normal, Mode::Insert, Mode::Visual, Mode::Audition],
        ..Binding::new(KeyCode::F(1), Command::ShowPattern, "Pattern view", "Application")
    },
    Binding {
        modes: &[Mode::Normal, Mode::Insert, Mode::Visual, Mode::Audition],
        ..Binding::new(
            KeyCode::F(2),
            Command::ShowArrangement,
            "Arrangement view",
            "Application",
        )
    },
    Binding {
        modes: &[Mode::Normal, Mode::Insert, Mode::Visual, Mode::Audition],
        ..Binding::new(KeyCode::F(3), Command::ShowMixer, "Mixer view", "Application")
    },
    Binding {
        modes: &[Mode::Normal, Mode::Insert, Mode::Visual, Mode::Audition],
        ..Binding::new(
            KeyCode::F(4),
            Command::ShowInstrument,
            "Instrument editor",
            "Application",
        )
    },
    Binding {
        prefix: Some(KeyCode::Char('g')),
        ..Binding::new(KeyCode::Char('h'), Command::FirstTrack, "First track", "Navigation")
    },
    Binding {
        prefix: Some(KeyCode::Char('g')),
        ..Binding::new(KeyCode::Char('k'), Command::FirstRow, "First row", "Navigation")
    },
    Binding {
        prefix: Some(KeyCode::Char('g')),
        ..Binding::new(KeyCode::Char('j'), Command::LastRow, "Last row", "Navigation")
    },
    Binding {
        prefix: Some(KeyCode::Char('g')),
        ..Binding::new(KeyCode::Char('l'), Command::LastTrack, "Last track", "Navigation")
    },
    Binding::new(KeyCode::Char('{'), Command::HalveLength, "Halve note length", "Editing"),
    Binding::new(
        KeyCode::Char('}'),
        Command::DoubleLength,
        "Double note length",
        "Editing",
    ),
    Binding {
        modifiers: KeyModifiers::CONTROL,
        ..Binding::new(
            KeyCode::Char(' '),
            Command::RestartPlayback,
            "Restart from beginning (:restart)",
            "Playback",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        modifiers: KeyModifiers::CONTROL,
        ..Binding::new(
            KeyCode::Char('t'),
            Command::AddTrack,
            "Append track to all patterns",
            "Arrangement",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        ..Binding::new(
            KeyCode::PageUp,
            Command::PreviousPattern,
            "Previous edit pattern",
            "Arrangement",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        ..Binding::new(
            KeyCode::PageDown,
            Command::NextPattern,
            "Next edit pattern",
            "Arrangement",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        modifiers: KeyModifiers::CONTROL,
        ..Binding::new(
            KeyCode::Char('n'),
            Command::NewPattern,
            "Create blank pattern (same dimensions)",
            "Arrangement",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        modifiers: KeyModifiers::CONTROL,
        ..Binding::new(
            KeyCode::Char('d'),
            Command::DuplicatePattern,
            "Duplicate edit pattern",
            "Arrangement",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        modifiers: KeyModifiers::CONTROL,
        ..Binding::new(
            KeyCode::Left,
            Command::PreviousOrder,
            "Select previous order entry",
            "Arrangement",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        modifiers: KeyModifiers::CONTROL,
        ..Binding::new(
            KeyCode::Right,
            Command::NextOrder,
            "Select next order entry",
            "Arrangement",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        modifiers: KeyModifiers::CONTROL,
        ..Binding::new(
            KeyCode::Char('a'),
            Command::AppendOrder,
            "Append edit pattern to order (stops)",
            "Arrangement",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        modifiers: KeyModifiers::CONTROL,
        ..Binding::new(
            KeyCode::Char('x'),
            Command::RemoveOrder,
            "Remove selected order entry (stops)",
            "Arrangement",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        modifiers: KeyModifiers::CONTROL,
        ..Binding::new(
            KeyCode::Up,
            Command::OrderEarlier,
            "Move order entry earlier (stops)",
            "Arrangement",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        modifiers: KeyModifiers::CONTROL,
        ..Binding::new(
            KeyCode::Down,
            Command::OrderLater,
            "Move order entry later (stops)",
            "Arrangement",
        )
    },
    Binding {
        modes: &[Mode::Normal],
        ..Binding::new(
            KeyCode::Char('O'),
            Command::ToggleSong,
            "Toggle Pattern/Song playback (stops)",
            "Arrangement",
        )
    },
    Binding::new(
        KeyCode::Char(':'),
        Command::OpenPrompt,
        "w [path], e[!] path, q[!], wq; Esc cancels",
        "Files",
    ),
    Binding {
        modes: &[Mode::Normal, Mode::Visual],
        ..Binding::new(
            KeyCode::Char('x'),
            Command::DeleteNote,
            "Clear cell/rectangle; finish selection",
            "Clipboard",
        )
    },
    Binding::new(
        KeyCode::Char('d'),
        Command::Cut,
        "Copy then clear; finish selection",
        "Clipboard",
    ),
    Binding::new(
        KeyCode::Char('y'),
        Command::Copy,
        "Copy cell/rectangle; finish selection",
        "Clipboard",
    ),
    Binding::new(
        KeyCode::Char('p'),
        Command::Paste,
        "Paste snapshot (overwrite, clip edges)",
        "Clipboard",
    ),
    Binding::new(
        KeyCode::Tab,
        Command::CycleEditStep,
        "Cycle entry step 0/1/2/4 rows",
        "Notes",
    ),
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
        KeyCode::Char('A'),
        Command::LowerNote,
        "Lower note one semitone",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('S'),
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
        "Stop / play from cursor",
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
    Binding::new(
        KeyCode::Delete,
        Command::DeleteNote,
        "Clear cell/rectangle without copying",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('a'),
        Command::LowerOctave,
        "Lower notes one octave",
        "Notes",
    ),
    Binding::new(
        KeyCode::Char('s'),
        Command::RaiseOctave,
        "Raise notes one octave",
        "Notes",
    ),
];

/// Resolve terminal input using editor mode and pending-count state.
/// Counts are capped before arithmetic and consumed by one movement command.
pub fn resolve(key: KeyEvent, state: &mut App) -> Option<Command> {
    if state.prompt.is_some() || key.kind == KeyEventKind::Release {
        return None;
    }
    let code = terminal_code(key);
    if key.kind == KeyEventKind::Repeat && state.suppressed_repeat {
        return None;
    }
    if key.kind == KeyEventKind::Press {
        state.suppressed_repeat = false;
    }
    let mut modifiers = key.modifiers;
    if matches!(code, KeyCode::Char(ch) if ch != ' ') {
        modifiers.remove(KeyModifiers::SHIFT);
    }
    if !state.help_visible
        && modifiers.is_empty()
        && DEFAULT_BINDINGS
            .iter()
            .any(|binding| binding.prefix == Some(code) && binding.matches_context(state))
    {
        if key.kind == KeyEventKind::Press {
            state.pending_prefix = code.as_char();
            state.status.clear();
        }
        return None;
    }
    if state.pending_prefix.is_some() && key.kind == KeyEventKind::Repeat {
        return None;
    }
    let prefix = state.pending_prefix.take().map(KeyCode::Char);
    let binding = DEFAULT_BINDINGS.iter().find(|binding| {
        (binding.prefix == prefix || matches!(code, KeyCode::F(1..=4)))
            && binding.key == code
            && binding.modifiers == modifiers
            && (!matches!(binding.command, Command::InsertNote { .. }) || !key.modifiers.contains(KeyModifiers::SHIFT))
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
                binding.matches_context(state)
                    && !matches!(
                        binding.command,
                        Command::DismissHelp | Command::ScrollHelpUp | Command::ScrollHelpDown
                    )
            }
    });
    if prefix.is_some() {
        state.suppressed_repeat = true;
        state.pending_count = None;
        return binding.map(|binding| binding.command).or_else(|| match code {
            KeyCode::Esc => Some(Command::EnterNormal),
            KeyCode::Char('?') if modifiers.is_empty() => Some(Command::ToggleHelp),
            _ => None,
        });
    }
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
        let before = state.pending_count;
        extend_count(state, digit);
        if state.pending_count != before {
            state.status.clear();
        }
        return None;
    }
    counted_command(binding.command, state.pending_count.take(), state.view)
}

fn counted_command(command: Command, count: Option<u16>, view: crate::state::View) -> Option<Command> {
    let (rows, tracks) = match command {
        Command::MoveCursorUp => (-1, 0),
        Command::MoveCursorDown => (1, 0),
        Command::MoveCursorLeft => (0, -1),
        Command::MoveCursorRight => (0, 1),
        command => {
            if view == crate::state::View::Mixer && matches!(command, Command::LowerOctave | Command::RaiseOctave) {
                return Some(Command::MixerAdjust(
                    if command == Command::RaiseOctave { 1 } else { -1 } * i32::from(count.unwrap_or(1)),
                ));
            }
            return if count.is_none() || command.ignores_count() {
                Some(command)
            } else {
                None
            };
        }
    };
    let count = i32::from(count.unwrap_or(1));
    Some(Command::MoveCursor {
        rows: rows * count,
        tracks: tracks * count,
    })
}

fn terminal_code(key: KeyEvent) -> KeyCode {
    // Crossterm decodes legacy Ctrl-Space (NUL) as Null + CONTROL.
    if key.code == KeyCode::Null && key.modifiers == KeyModifiers::CONTROL {
        KeyCode::Char(' ')
    } else {
        key.code
    }
}

fn extend_count(state: &mut App, digit: u8) {
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
}

#[derive(Clone, Debug)]
pub(crate) struct MixerGesture {
    key: (KeyCode, KeyModifiers),
    started: Instant,
    last: Instant,
    revision: crate::history::Revision,
}

/// Dispatch one real key through the shared resolver.
pub fn handle_key(key: KeyEvent, state: &mut App) {
    handle_key_at(key, state, Instant::now());
}

pub(crate) fn handle_key_at(key: KeyEvent, state: &mut App, now: Instant) {
    let previous = state.mixer_gesture.take();
    let eligible = state.view == crate::state::View::Mixer
        && !state.help_visible
        && state.prompt.is_none()
        && state.pending_count.is_none()
        && state.pending_prefix.is_none()
        && key.kind != KeyEventKind::Release
        && key.modifiers.is_empty()
        && ((state.mode == Mode::Insert
            && matches!(key.code, KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l')))
            || (state.mode == Mode::Visual && matches!(key.code, KeyCode::Char('a' | 's'))));
    let before = state.revision();
    dispatch_key(key, state);
    if eligible && state.revision() != before {
        let prior = previous.filter(|gesture| {
            gesture.key == (key.code, key.modifiers)
                && gesture.revision == before
                && now.duration_since(gesture.last) <= Duration::from_millis(400)
                && now.duration_since(gesture.started) < Duration::from_secs(2)
        });
        if prior.is_some() {
            state.history.join_channel_edits(before);
        }
        state.mixer_gesture = Some(MixerGesture {
            key: (key.code, key.modifiers),
            started: prior.map_or(now, |gesture| gesture.started),
            last: now,
            revision: state.revision(),
        });
    } else if eligible {
        // Clamped repeats keep the gesture alive without creating history.
        state.mixer_gesture =
            previous.filter(|gesture| gesture.key == (key.code, key.modifiers) && gesture.revision == before);
    }
}

fn dispatch_key(key: KeyEvent, state: &mut App) {
    if state.prompt.is_some() {
        state.pending_prefix = None;
        state.pending_count = None;
        if key.kind == KeyEventKind::Release
            || key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return;
        }
        match key.code {
            KeyCode::Esc => {
                state.prompt = None;
                state.status.clear();
            }
            KeyCode::Enter if key.kind == KeyEventKind::Press => {
                if let Some(text) = state.prompt.take() {
                    state.execute_prompt(&text);
                }
            }
            KeyCode::Backspace => {
                if let Some(text) = &mut state.prompt {
                    if text.pop().is_some() {
                        state.status.clear();
                    }
                }
            }
            KeyCode::Char(ch) => {
                if let Some(text) = &mut state.prompt {
                    text.push(ch);
                    state.status.clear();
                }
            }
            _ => {}
        }
        return;
    }
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
    fn view_switching_restores_pattern_modes_and_selection_without_hidden_edits() {
        use crate::state::View;
        for setup in ["i", "3j2lv2j1h"] {
            let mut app = App::default();
            sequence(&mut app, setup);
            let saved = (app.mode, app.cursor_row, app.cursor_track, app.selection_anchor);
            let revision = app.revision();
            let pending = if app.mode == Mode::Insert { "14" } else { "12g" };
            sequence(&mut app, pending);
            key(&mut app, KeyCode::F(3), KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(app.view, View::Mixer);
            assert_eq!(app.mode, Mode::Normal);
            assert_eq!((app.pending_count, app.pending_prefix), (None, None));
            sequence(&mut app, "asxdypz{");
            assert_eq!(app.mode, Mode::Normal);
            assert_eq!(app.cursor_row, 0);
            assert_eq!(app.revision(), revision);
            sequence(&mut app, "3lgh2l");
            assert_eq!(app.cursor_track, 2);
            key(&mut app, KeyCode::F(4), KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(app.cursor_track, 0);
            sequence(&mut app, "l");
            key(&mut app, KeyCode::F(1), KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(
                (app.mode, app.cursor_row, app.cursor_track, app.selection_anchor),
                saved
            );
            key(&mut app, KeyCode::F(3), KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(app.cursor_track, 2);
            sequence(&mut app, " ");
            assert_eq!(app.playback_row, saved.1);
            sequence(&mut app, "?");
            key(&mut app, KeyCode::F(1), KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(app.view, View::Mixer);
            escape(&mut app);
            sequence(&mut app, ":");
            key(&mut app, KeyCode::F(1), KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(app.view, View::Mixer);
            escape(&mut app);
            key(&mut app, KeyCode::F(1), KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(
                (app.mode, app.cursor_row, app.cursor_track, app.selection_anchor),
                saved
            );
        }
    }

    #[test]
    fn structural_undo_invalidates_saved_selection_and_clamps_each_view() {
        let mut app = App::default();
        key(&mut app, KeyCode::Char('t'), KeyModifiers::CONTROL, KeyEventKind::Press);
        sequence(&mut app, "v2j");
        assert_eq!(app.cursor_track, 4);
        key(&mut app, KeyCode::F(3), KeyModifiers::NONE, KeyEventKind::Press);
        sequence(&mut app, "glu");
        assert_eq!(app.project.instruments().len(), 4);
        key(&mut app, KeyCode::F(1), KeyModifiers::NONE, KeyEventKind::Press);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.selection_anchor, None);
        assert_eq!((app.cursor_row, app.cursor_track), (2, 3));
    }

    #[test]
    fn actual_sequences_distinguish_piano_modes_and_reversed_rectangles() {
        let mut app = App::default();
        assert_eq!(app.mode, Mode::Normal);
        sequence(&mut app, "zxc");
        assert_eq!(
            app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note,
            None
        );
        sequence(&mut app, "iv");
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(
            app.project.patterns[app.selected_pattern]
                .get_step(0, 0)
                .unwrap()
                .note
                .unwrap()
                .pitch,
            NotePitch::F
        );
        for _ in 0..2 {
            key(&mut app, KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Press);
        }
        assert_eq!(app.cursor_row, 2);
        escape(&mut app);
        sequence(&mut app, "16j2lv3k1h");
        assert_eq!((app.cursor_row, app.cursor_track), (15, 1));
        assert_eq!(app.selection_anchor, Some((18, 2)));
        for row in 0..64 {
            for track in 0..4 {
                assert_eq!(
                    app.is_selected(row, track),
                    (15..=18).contains(&row) && (1..=2).contains(&track)
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
            assert_eq!(app.project.bpm, 120.0);
            assert_eq!(app.project.instrument(0), crate::tracker::Instrument::Pluck);
            assert_eq!(
                app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note,
                None
            );
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
            sequence(&mut app, "14?");
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
            assert_eq!(
                app.project.patterns[app.selected_pattern]
                    .get_step(track, row)
                    .unwrap()
                    .note,
                None
            );
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
        for (ch, command) in [
            ('A', Command::LowerNote),
            ('S', Command::RaiseNote),
            ('a', Command::LowerOctave),
            ('s', Command::RaiseOctave),
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
            assert_eq!(
                resolve(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::ALT), &mut app),
                None
            );
        }
        sequence(&mut app, "i");
        key(&mut app, KeyCode::Char('v'), KeyModifiers::NONE, KeyEventKind::Repeat);
        assert_eq!(
            app.project.patterns[app.selected_pattern].get_step(0, 17).unwrap().note,
            None
        );
        key(&mut app, KeyCode::Char('Q'), KeyModifiers::SHIFT, KeyEventKind::Press);
        assert!(!app.shutdown);
        escape(&mut app);
        key(&mut app, KeyCode::Char('Q'), KeyModifiers::SHIFT, KeyEventKind::Press);
        assert!(app.shutdown);
    }

    #[test]
    fn navigation_and_global_controls_work_in_every_mode_and_edits_only_in_insert() {
        for mode_keys in ["", "i", "v"] {
            let mut app = App::default();
            sequence(&mut app, mode_keys);
            let mode = app.mode;
            if mode == Mode::Insert {
                key(&mut app, KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Press);
                key(&mut app, KeyCode::Right, KeyModifiers::NONE, KeyEventKind::Press);
                sequence(&mut app, "+-][ ");
            } else {
                sequence(&mut app, "jjllkh+-][ ");
            }
            assert_eq!((app.cursor_row, app.cursor_track), (1, 1));
            assert_eq!(app.mode, mode);
            assert_eq!(app.project.bpm, 120.0);
            assert!(matches!(app.playback_state, crate::state::PlaybackState::Playing));
            sequence(&mut app, "z");
            key(&mut app, KeyCode::Delete, KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(
                app.project.patterns[app.selected_pattern].get_step(1, 1).unwrap().note,
                None
            );
        }
        let mut app = App::default();
        sequence(&mut app, "iz");
        escape(&mut app);
        sequence(&mut app, "sS");
        let note = app.project.patterns[app.selected_pattern]
            .get_step(0, 0)
            .unwrap()
            .note
            .unwrap();
        assert_eq!(note.pitch, NotePitch::Cs);
        assert_eq!(note.octave, 5);
        assert_eq!(app.octave, 4);
        escape(&mut app);
        sequence(&mut app, "xfrzas");
        key(&mut app, KeyCode::Delete, KeyModifiers::NONE, KeyEventKind::Press);
        assert_eq!(
            app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note,
            None
        );
        assert_eq!(app.octave, 4);
        sequence(&mut app, "i");
        key(&mut app, KeyCode::Delete, KeyModifiers::NONE, KeyEventKind::Press);
        assert_eq!(
            app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note,
            None
        );
    }

    #[test]
    fn directional_g_jumps_follow_view_local_navigation() {
        for (panel, last_row, horizontal) in [
            (1, Some(63), true),
            (2, None, false),
            (3, Some(5), true),
            (4, None, true),
        ] {
            let mut app = App::default();
            key(&mut app, KeyCode::F(panel), KeyModifiers::NONE, KeyEventKind::Press);
            let revision = app.revision();
            sequence(&mut app, "gj");
            assert_eq!(app.cursor_row, last_row.unwrap_or(0), "F{panel}: gj goes down");
            sequence(&mut app, "gk");
            assert_eq!(app.cursor_row, 0, "F{panel}: gk goes up");
            sequence(&mut app, "gl");
            assert_eq!(
                app.cursor_track,
                if horizontal { 3 } else { 0 },
                "F{panel}: gl goes right"
            );
            sequence(&mut app, "gh");
            assert_eq!(app.cursor_track, 0, "F{panel}: gh goes left");
            assert_eq!(app.revision(), revision);
        }
    }

    #[test]
    fn jumps_consume_counts_cancel_invalid_and_ignore_repeats_in_all_modes() {
        for mode in [Mode::Normal, Mode::Visual] {
            let mut app = App { mode, ..App::default() };
            sequence(&mut app, "12gj");
            assert_eq!(app.cursor_row, 63);
            assert_eq!(app.pending_count, None);
            key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE, KeyEventKind::Repeat);
            assert_eq!(app.cursor_row, 63);
            sequence(&mut app, "glgkgh");
            assert_eq!((app.cursor_row, app.cursor_track), (0, 0));
            let revision = app.revision();
            sequence(&mut app, "gz");
            key(&mut app, KeyCode::Char('z'), KeyModifiers::NONE, KeyEventKind::Repeat);
            key(&mut app, KeyCode::Char('s'), KeyModifiers::NONE, KeyEventKind::Repeat);
            assert!(app.suppressed_repeat);
            assert_eq!(app.revision(), revision);
            sequence(&mut app, "j");
            assert!(!app.suppressed_repeat);
            assert_eq!(app.cursor_row, 1);
            assert_eq!(app.pending_prefix, None);
            sequence(&mut app, "g?");
            assert!(app.help_visible);
            assert_eq!(app.pending_prefix, None);
            escape(&mut app);
            sequence(&mut app, "g");
            escape(&mut app);
            assert_eq!(app.pending_prefix, None);
        }
    }

    #[test]
    fn transpose_rectangle_preserves_selection_lengths_and_undoes_atomically() {
        let mut app = App::default();
        sequence(&mut app, "iz}");
        key(&mut app, KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Press);
        key(&mut app, KeyCode::Right, KeyModifiers::NONE, KeyEventKind::Press);
        sequence(&mut app, "x");
        escape(&mut app);
        let before = app.project.clone();
        sequence(&mut app, "vkh");
        let anchor = app.selection_anchor;
        sequence(&mut app, "s");
        for (track, row) in [(0, 0), (1, 1)] {
            let old = before.patterns[0].get_step(track, row).unwrap().note.unwrap();
            let new = app.project.patterns[0].get_step(track, row).unwrap().note.unwrap();
            assert_eq!(new, old.transpose(12));
            assert_eq!(new.length_ticks, old.length_ticks);
        }
        assert_eq!(app.mode, Mode::Visual);
        assert_eq!(app.selection_anchor, anchor);
        assert_eq!((app.cursor_row, app.cursor_track, app.octave), (0, 0, 4));
        app.handle_command(Command::Undo);
        assert_eq!(app.project, before);
        for mode in [Mode::Normal, Mode::Visual] {
            app.mode = mode;
            let revision = app.revision();
            sequence(&mut app, "2s2A2zfr");
            assert_eq!(app.revision(), revision);
        }
    }

    #[test]
    fn feedback_lives_until_an_accepted_action_not_ignored_input() {
        let mut app = App::default();
        sequence(&mut app, ":w");
        handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
        assert!(app.status.contains("No file path"));
        let error = app.status.clone();
        for event in [
            KeyEvent::new_with_kind(KeyCode::Char('j'), KeyModifiers::NONE, KeyEventKind::Release),
            KeyEvent::new_with_kind(KeyCode::Char('i'), KeyModifiers::NONE, KeyEventKind::Repeat),
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
        ] {
            handle_key(event, &mut app);
            assert_eq!(app.status, error);
        }
        sequence(&mut app, "j");
        assert_eq!(app.status, "");
        sequence(&mut app, ":unknown");
        handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
        assert!(app.status.contains("Unknown command"));
        sequence(&mut app, "2");
        assert_eq!(app.status, "");
        app.status = error.clone();
        sequence(&mut app, "g");
        assert_eq!(app.status, "");
        app.status = error.clone();
        sequence(&mut app, "z"); // Invalid chord consumes the prefix, not the message.
        assert_eq!(app.status, error);
        sequence(&mut app, ":literal ? path");
        assert!(!app.help_visible);
        assert_eq!(app.prompt.as_deref(), Some("literal ? path"));
        escape(&mut app);
        assert!(app.prompt.is_none());
        assert_eq!(app.status, "");
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("saved 界 file.json");
        sequence(&mut app, &format!(":w {}", path.display()));
        handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
        assert!(app.status.starts_with("Saved "));
        assert!(app.status.contains(&path.display().to_string()));
        assert!(!app.is_dirty());
        sequence(&mut app, "?");
        assert!(app.help_visible);
        escape(&mut app);
        assert!(!app.help_visible);
    }

    #[test]
    fn chromatic_insert_owns_both_octaves_and_never_treats_notes_as_commands() {
        let pitches = [
            NotePitch::C,
            NotePitch::Cs,
            NotePitch::D,
            NotePitch::Ds,
            NotePitch::E,
            NotePitch::F,
            NotePitch::Fs,
            NotePitch::G,
            NotePitch::Gs,
            NotePitch::A,
            NotePitch::As,
            NotePitch::B,
        ];
        for (offset, keys) in [(0, "zsxdcvgbhnjm"), (1, "q2w3er5t6y7u")] {
            for (ch, pitch) in keys.chars().zip(pitches) {
                let mut app = App::default();
                sequence(&mut app, "i");
                sequence(&mut app, &ch.to_string());
                let note = app.project.patterns[0].get_step(0, 0).unwrap().note.unwrap();
                assert_eq!((note.pitch, note.octave), (pitch, 4 + offset), "{ch}");
                assert_eq!((app.cursor_row, app.cursor_track), (0, 0));
                assert_eq!((app.pending_count, app.pending_prefix), (None, None));
                assert!(!app.shutdown);
                assert!(app.take_audition().is_some());
                let revision = app.revision();
                for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
                    key(&mut app, KeyCode::Char(ch), KeyModifiers::NONE, kind);
                }
                assert_eq!(app.revision(), revision);
                assert!(app.take_audition().is_none());
                for modifiers in [KeyModifiers::SHIFT, KeyModifiers::ALT, KeyModifiers::CONTROL] {
                    key(&mut app, KeyCode::Char(ch), modifiers, KeyEventKind::Press);
                    assert!(app.take_audition().is_none());
                    assert_eq!(app.revision(), revision);
                }
                sequence(&mut app, &ch.to_string());
                assert_eq!(app.revision(), revision);
                assert!(app.take_audition().is_some());
                escape(&mut app);
                sequence(&mut app, "u");
                assert!(app.project.patterns[0].get_step(0, 0).unwrap().note.is_none());
            }
        }
        let mut app = App::default();
        sequence(&mut app, "i");
        let before = app.project.clone();
        sequence(&mut app, "aklAS01489Qip:");
        assert_eq!(app.project, before);
        assert_eq!(app.mode, Mode::Insert);
        assert!(app.prompt.is_none());
        assert!(!app.shutdown);
        key(&mut app, KeyCode::PageDown, KeyModifiers::NONE, KeyEventKind::Press);
        assert_eq!(app.octave, 5);
        assert_eq!(app.selected_pattern, 0);
        key(&mut app, KeyCode::PageUp, KeyModifiers::NONE, KeyEventKind::Press);
        sequence(&mut app, ",");
        assert_eq!(app.project.patterns[0].get_step(0, 0).unwrap().note.unwrap().octave, 5);
    }

    #[test]
    fn audition_plays_chromatic_keys_without_writes_or_transport_changes() {
        use crate::tracker::Instrument;
        for voice in [Instrument::Sine, Instrument::Kick] {
            let mut app = App::default();
            app.handle_command(Command::SetInstrument(voice));
            sequence(&mut app, "iz}");
            escape(&mut app);
            sequence(&mut app, "y ");
            app.edit_step = 4;
            app.playback_row = 17;
            let project = app.project.clone();
            let revision = app.revision();
            let clipboard = app.clipboard.clone();
            let dirty = app.is_dirty();
            key(&mut app, KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(app.mode, Mode::Audition);
            for ch in "zsxdcvgbhnjmq2w3er5t6y7u,".chars() {
                sequence(&mut app, &ch.to_string());
                let event = app.take_audition().unwrap();
                assert_eq!(event.instrument, voice);
                assert_eq!(event.track, 0);
                assert_eq!(event.duration, crate::audio::DEFAULT_NOTE_DURATION);
                assert!(app.take_audition().is_none());
                for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
                    key(&mut app, KeyCode::Char(ch), KeyModifiers::NONE, kind);
                }
                assert!(app.take_audition().is_none());
                assert_eq!(app.project, project);
                assert_eq!(app.revision(), revision);
                assert_eq!(app.is_dirty(), dirty);
                assert_eq!(app.clipboard, clipboard);
                assert_eq!((app.cursor_row, app.cursor_track, app.playback_row), (0, 0, 17));
                assert_eq!(app.selection_anchor, None);
            }
            sequence(&mut app, "{}ipaklAS01489Q:");
            key(&mut app, KeyCode::Delete, KeyModifiers::NONE, KeyEventKind::Press);
            key(&mut app, KeyCode::Tab, KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(app.project, project);
            assert_eq!(app.edit_step, 4);
            assert!(app.prompt.is_none());
            key(&mut app, KeyCode::PageDown, KeyModifiers::NONE, KeyEventKind::Press);
            key(&mut app, KeyCode::Right, KeyModifiers::NONE, KeyEventKind::Press);
            sequence(&mut app, "q");
            let event = app.take_audition().unwrap();
            assert_eq!((event.track, event.note.pitch, event.note.octave), (1, NotePitch::C, 6));
            sequence(&mut app, "z?");
            assert!(app.take_audition().is_none());
            sequence(&mut app, "z");
            assert!(app.take_audition().is_none());
            escape(&mut app);
            sequence(&mut app, "z");
            key(&mut app, KeyCode::F(3), KeyModifiers::NONE, KeyEventKind::Press);
            assert!(app.take_audition().is_none());
            key(&mut app, KeyCode::F(1), KeyModifiers::NONE, KeyEventKind::Press);
            assert_eq!(app.mode, Mode::Audition);
            assert!(app.take_audition().is_none());
            escape(&mut app);
            assert_eq!(app.mode, Mode::Normal);
            sequence(&mut app, "u");
            assert_ne!(app.revision(), revision); // Audition added no history entries.
        }
    }

    #[test]
    fn registry_has_unique_mode_modifier_keys_and_metadata() {
        for (index, binding) in DEFAULT_BINDINGS.iter().enumerate() {
            assert!(!binding.description.is_empty() && !binding.category.is_empty());
            for mode in binding.modes {
                let mut app = App {
                    mode: *mode,
                    view: if matches!(binding.command, Command::MixerValue | Command::MixerReset)
                        || matches!(
                            binding.command,
                            Command::MoveCursorLeft
                                | Command::MoveCursorRight
                                | Command::MoveCursorUp
                                | Command::MoveCursorDown
                        ) && matches!(
                            binding.key,
                            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
                        ) {
                        crate::state::View::Mixer
                    } else if binding.command == Command::AuditionInstrument {
                        crate::state::View::Instrument
                    } else {
                        crate::state::View::Pattern
                    },
                    help_visible: matches!(
                        binding.command,
                        Command::DismissHelp | Command::ScrollHelpUp | Command::ScrollHelpDown
                    ),
                    ..App::default()
                };
                if !app.help_visible && !binding.available_in_mode(app.view, *mode) {
                    continue;
                }
                assert!(!DEFAULT_BINDINGS[..index].iter().any(|other| other.key == binding.key
                    && other.prefix == binding.prefix
                    && other.modifiers == binding.modifiers
                    && other.available_in_mode(app.view, *mode)
                    && matches!(
                        other.command,
                        Command::DismissHelp | Command::ScrollHelpUp | Command::ScrollHelpDown
                    ) == matches!(
                        binding.command,
                        Command::DismissHelp | Command::ScrollHelpUp | Command::ScrollHelpDown
                    )
                    && !matches!(other.command, Command::EnterNormal)));
                app.pending_prefix = binding.prefix.as_ref().and_then(KeyCode::as_char);
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
