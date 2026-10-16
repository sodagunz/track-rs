use crate::tracker::{Instrument, NotePitch};

/// User actions, independent of terminal input and the current editor state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    ShowPattern,
    ShowArrangement,
    ShowMixer,
    ShowInstrument,
    PreviousPattern,
    NextPattern,
    NewPattern,
    AddTrack,
    DuplicatePattern,
    PreviousOrder,
    NextOrder,
    AppendOrder,
    RemoveOrder,
    OrderEarlier,
    OrderLater,
    ToggleSong,
    OpenPrompt,
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
    RestartPlayback,
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
    HalveLength,
    DoubleLength,
    PreviousInstrument,
    NextInstrument,
    SetInstrument(Instrument),
    DecreaseTempo,
    IncreaseTempo,
    FirstTrack,
    FirstRow,
    LastRow,
    LastTrack,
    LowerOctave,
    RaiseOctave,
    LowerNote,
    RaiseNote,
    DecreaseOctave,
    IncreaseOctave,
}

impl Command {
    pub(crate) const fn ignores_count(self) -> bool {
        matches!(
            self,
            Self::Quit
                | Self::ToggleHelp
                | Self::EnterNormal
                | Self::ShowPattern
                | Self::ShowArrangement
                | Self::ShowMixer
                | Self::ShowInstrument
        )
    }

    pub(crate) const fn available_in_arrangement(self) -> bool {
        matches!(
            self,
            Self::PreviousPattern
                | Self::NextPattern
                | Self::NewPattern
                | Self::DuplicatePattern
                | Self::AddTrack
                | Self::PreviousOrder
                | Self::NextOrder
                | Self::AppendOrder
                | Self::RemoveOrder
                | Self::OrderEarlier
                | Self::OrderLater
                | Self::ToggleSong
                | Self::Undo
                | Self::Redo
                | Self::OpenPrompt
                | Self::TogglePlayback
                | Self::RestartPlayback
                | Self::DecreaseTempo
                | Self::IncreaseTempo
        )
    }

    pub(crate) const fn available_in_voice_view(self) -> bool {
        matches!(
            self,
            Self::MoveCursorLeft
                | Self::MoveCursorRight
                | Self::MoveCursor { rows: 0, .. }
                | Self::FirstTrack
                | Self::LastTrack
                | Self::PreviousInstrument
                | Self::NextInstrument
                | Self::SetInstrument(_)
                | Self::Undo
                | Self::Redo
                | Self::OpenPrompt
                | Self::TogglePlayback
                | Self::RestartPlayback
                | Self::ToggleSong
                | Self::DecreaseTempo
                | Self::IncreaseTempo
                | Self::EnterNormal
        )
    }

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
                | Self::LowerOctave
                | Self::RaiseOctave
                | Self::LowerNote
                | Self::RaiseNote
                | Self::DecreaseOctave
                | Self::IncreaseOctave
        )
    }
}
