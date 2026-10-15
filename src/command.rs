use crate::tracker::{Instrument, NotePitch};

/// User actions, independent of terminal input and the current editor state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Undo,
    Redo,
    EnterInsert,
    ToggleVisual,
    EnterNormal,
    CountDigit(u8),
    MoveCursor { rows: i32, tracks: i32 },
    Quit,
    ToggleHelp,
    DismissHelp,
    ScrollHelpUp,
    ScrollHelpDown,
    TogglePlayback,
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorLeft,
    MoveCursorRight,
    InsertNote { pitch: NotePitch, octave_offset: u8 },
    DeleteNote,
    Copy,
    Cut,
    Paste,
    CycleEditStep,
    PreviousInstrument,
    NextInstrument,
    SetInstrument(Instrument),
    DecreaseTempo,
    IncreaseTempo,
    LowerNote,
    RaiseNote,
    DecreaseOctave,
    IncreaseOctave,
}

impl Command {
    /// Whether holding a key should perform this action repeatedly.
    #[must_use]
    pub const fn allows_repeat(self) -> bool {
        matches!(
            self,
            Self::MoveCursorUp
                | Self::MoveCursorDown
                | Self::MoveCursorLeft
                | Self::MoveCursorRight
                | Self::MoveCursor { .. }
                | Self::ScrollHelpUp
                | Self::ScrollHelpDown
                | Self::PreviousInstrument
                | Self::NextInstrument
                | Self::DecreaseTempo
                | Self::IncreaseTempo
                | Self::LowerNote
                | Self::RaiseNote
                | Self::DecreaseOctave
                | Self::IncreaseOctave
        )
    }
}
