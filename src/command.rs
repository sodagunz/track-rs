use crate::tracker::{Instrument, NotePitch};

/// User actions, independent of terminal input and the current editor state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
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
        !matches!(self, Self::Quit | Self::TogglePlayback | Self::ToggleHelp)
    }
}
