use crate::{
    command::Command,
    history::{Edit, History, Revision},
    tracker::{Note, Pattern},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Visual,
}

impl Mode {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PlaybackState {
    Stopped,
    Playing,
}

#[derive(Clone, Debug)]
pub struct App {
    pub(crate) history: History,
    // Track-major owned snapshot; None cells are part of the rectangle.
    pub(crate) clipboard: Vec<Vec<Option<Note>>>,
    pub edit_step: usize,
    pub mode: Mode,
    pub selection_anchor: Option<(usize, usize)>,
    pub pending_count: Option<u16>,
    pub pattern: Pattern,
    pub cursor_row: usize,
    pub cursor_track: usize,
    /// UI mirror of the transport row; the runtime transport owns advancement.
    pub playback_row: usize,
    pub playback_state: PlaybackState,
    pub octave: u8,
    pub bpm: f64,
    pub shutdown: bool,
    pub help_visible: bool,
    /// Index of the first binding shown; independent of terminal dimensions.
    pub help_scroll: usize,
}

pub enum CursorOffset {
    Neg(usize),
    Pos(usize),
    None,
}

impl CursorOffset {
    const fn apply_to(&self, position: usize) -> usize {
        match self {
            Self::Pos(delta) => position.saturating_add(*delta),
            Self::Neg(delta) => position.saturating_sub(*delta),
            Self::None => position,
        }
    }
}

impl Default for App {
    fn default() -> Self {
        let default_rows = 64;
        let default_tracks = 4;
        Self {
            history: History::default(),
            clipboard: Vec::new(),
            edit_step: 0,
            mode: Mode::Normal,
            selection_anchor: None,
            pending_count: None,
            pattern: Pattern::new(default_rows, default_tracks),
            cursor_row: 0,
            cursor_track: 0,
            octave: 4,
            playback_row: 0,
            playback_state: PlaybackState::Stopped,
            bpm: 120.0,
            shutdown: false,
            help_visible: false,
            help_scroll: 0,
        }
    }
}

impl App {
    /// Whether a cell lies in the inclusive Visual rectangle.
    #[must_use]
    pub fn is_selected(&self, row: usize, track: usize) -> bool {
        self.selection_anchor.is_some_and(|(anchor_row, anchor_track)| {
            (anchor_row.min(self.cursor_row)..=anchor_row.max(self.cursor_row)).contains(&row)
                && (anchor_track.min(self.cursor_track)..=anchor_track.max(self.cursor_track)).contains(&track)
        })
    }

    /// Apply an editor action without depending on terminal key events.
    pub fn handle_command(&mut self, command: Command) {
        match command {
            Command::ToggleHelp => {
                self.pending_count = None;
                self.help_visible = !self.help_visible;
                self.help_scroll = 0;
            }
            Command::DismissHelp => {
                self.pending_count = None;
                self.help_visible = false;
            }
            Command::ScrollHelpUp | Command::ScrollHelpDown => {
                if self.help_visible {
                    self.help_scroll = if command == Command::ScrollHelpUp {
                        self.help_scroll.saturating_sub(1)
                    } else {
                        self.help_scroll
                            .saturating_add(1)
                            .min(crate::bindings::DEFAULT_BINDINGS.len().saturating_sub(1))
                    };
                }
            }
            Command::Quit => self.shutdown(),
            _ if self.help_visible => {}
            Command::EnterInsert => {
                self.mode = Mode::Insert;
                self.selection_anchor = None;
            }
            Command::EnterNormal => {
                self.mode = Mode::Normal;
                self.selection_anchor = None;
                self.pending_count = None;
            }
            Command::ToggleVisual => {
                if self.mode == Mode::Visual {
                    self.mode = Mode::Normal;
                    self.selection_anchor = None;
                } else {
                    self.mode = Mode::Visual;
                    self.selection_anchor = Some((self.cursor_row, self.cursor_track));
                }
            }
            Command::CountDigit(_) => {}
            Command::MoveCursor { rows, tracks } => {
                let offset = |value: i32| {
                    if value < 0 {
                        CursorOffset::Neg(value.unsigned_abs() as usize)
                    } else {
                        CursorOffset::Pos(value.unsigned_abs() as usize)
                    }
                };
                self.offset_cursor(&offset(rows), &offset(tracks));
            }
            Command::TogglePlayback => self.toggle_playback(),
            Command::MoveCursorUp => self.offset_cursor(&CursorOffset::Neg(1), &CursorOffset::None),
            Command::MoveCursorDown => self.offset_cursor(&CursorOffset::Pos(1), &CursorOffset::None),
            Command::MoveCursorLeft => self.offset_cursor(&CursorOffset::None, &CursorOffset::Neg(1)),
            Command::MoveCursorRight => self.offset_cursor(&CursorOffset::None, &CursorOffset::Pos(1)),
            Command::InsertNote { pitch, octave_offset } => self.enter_note(Note {
                pitch,
                octave: self.octave.saturating_add(octave_offset).min(9),
            }),
            Command::CycleEditStep => self.cycle_edit_step(),
            Command::Copy | Command::Cut => self.copy_or_cut(command == Command::Cut),
            Command::Paste => self.paste(),
            Command::Undo => self.history.undo(&mut self.pattern, &mut self.bpm),
            Command::Redo => self.history.redo(&mut self.pattern, &mut self.bpm),
            Command::SetInstrument(instrument) => {
                let track = self.cursor_track;
                self.edit(|edit| edit.set_instrument(track, instrument));
            }
            Command::PreviousInstrument | Command::NextInstrument => {
                let instrument = self
                    .pattern
                    .instrument(self.cursor_track)
                    .cycle(command == Command::NextInstrument);
                let track = self.cursor_track;
                self.edit(|edit| edit.set_instrument(track, instrument));
            }
            Command::DecreaseTempo | Command::IncreaseTempo => {
                let bpm = (self.bpm + if command == Command::IncreaseTempo { 1.0 } else { -1.0 }).clamp(20.0, 300.0);
                self.edit(|edit| edit.set_tempo(bpm));
            }
            Command::LowerNote | Command::RaiseNote => {
                if let Some(note) = self
                    .pattern
                    .get_step(self.cursor_track, self.cursor_row)
                    .and_then(|step| step.note)
                {
                    self.insert_note_under_cursor(Some(note.transpose(if command == Command::RaiseNote {
                        1
                    } else {
                        -1
                    })));
                }
            }
            Command::DeleteNote => self.clear_selection(),
            Command::DecreaseOctave => self.octave = self.octave.saturating_sub(1).min(9),
            Command::IncreaseOctave => self.octave = self.octave.saturating_add(1).min(9),
        }
    }

    const fn cycle_edit_step(&mut self) {
        self.edit_step = match self.edit_step {
            0 => 1,
            1 => 2,
            2 => 4,
            _ => 0,
        };
    }

    fn enter_note(&mut self, note: Note) {
        if self.pattern.get_step(self.cursor_track, self.cursor_row).is_some() {
            self.insert_note_under_cursor(Some(note));
            if self.mode == Mode::Insert {
                self.offset_cursor(&CursorOffset::Pos(self.edit_step), &CursorOffset::None);
            }
        }
    }

    fn copy_or_cut(&mut self, cut: bool) {
        self.copy_rectangle();
        if cut {
            self.clear_rectangle();
        }
        self.finish_selection();
    }

    fn rectangle(&self) -> (usize, usize, usize, usize) {
        let (row, track) = if self.mode == Mode::Visual {
            self.selection_anchor.unwrap_or((self.cursor_row, self.cursor_track))
        } else {
            (self.cursor_row, self.cursor_track)
        };
        (
            row.min(self.cursor_row),
            row.max(self.cursor_row).min(self.pattern.num_rows.saturating_sub(1)),
            track.min(self.cursor_track),
            track
                .max(self.cursor_track)
                .min(self.pattern.tracks.len().saturating_sub(1)),
        )
    }

    fn copy_rectangle(&mut self) {
        let (first_row, last_row, first_track, last_track) = self.rectangle();
        self.clipboard = (first_track..=last_track)
            .map(|track| {
                (first_row..=last_row)
                    .map(|row| self.pattern.get_step(track, row).and_then(|step| step.note))
                    .collect()
            })
            .collect();
    }

    fn clear_rectangle(&mut self) {
        let (first_row, last_row, first_track, last_track) = self.rectangle();
        self.edit(|edit| {
            for track in first_track..=last_track {
                for row in first_row..=last_row {
                    edit.set_cell(track, row, None);
                }
            }
        });
    }

    fn clear_selection(&mut self) {
        self.clear_rectangle();
        self.finish_selection();
    }

    fn finish_selection(&mut self) {
        if self.mode == Mode::Visual {
            self.mode = Mode::Normal;
        }
        self.selection_anchor = None;
    }

    fn paste(&mut self) {
        let (track, row) = (self.cursor_track, self.cursor_row);
        let tracks = self.pattern.tracks.len().saturating_sub(track);
        let rows = self.pattern.num_rows.saturating_sub(row);
        let clipboard = self.clipboard.clone();
        self.edit(|edit| {
            for (dt, cells) in clipboard.iter().take(tracks).enumerate() {
                for (dr, note) in cells.iter().take(rows).enumerate() {
                    edit.set_cell(track + dt, row + dr, *note);
                }
            }
        });
    }

    pub fn offset_cursor(&mut self, dr: &CursorOffset, dt: &CursorOffset) {
        let max_rows = self.pattern.num_rows;
        let max_tracks = self.pattern.tracks.len();

        self.cursor_row = dr.apply_to(self.cursor_row).clamp(0, max_rows.saturating_sub(1));
        self.cursor_track = dt.apply_to(self.cursor_track).clamp(0, max_tracks.saturating_sub(1));
    }

    pub const fn shutdown(&mut self) {
        self.shutdown = true;
    }

    pub const fn toggle_playback(&mut self) {
        match self.playback_state {
            PlaybackState::Stopped => {
                self.playback_state = PlaybackState::Playing;
                self.playback_row = 0; // TODO: feat(Loop) loop_start_cursor/playback_start_cursor
            }
            PlaybackState::Playing => {
                self.playback_state = PlaybackState::Stopped;
                // The runtime observes this transition and cancels audio.
            }
        }
    }

    /// Apply one document transaction, including future multi-cell operations.
    pub fn edit(&mut self, operation: impl FnOnce(&mut Edit<'_>)) {
        self.history.edit(&mut self.pattern, &mut self.bpm, operation);
    }

    /// Stable identity for future saved-revision comparisons, even after eviction.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.history.revision()
    }

    /// Replace the note at the cursor; invalid positions are ignored.
    pub fn insert_note_under_cursor(&mut self, note: Option<Note>) {
        let (track, row) = (self.cursor_track, self.cursor_row);
        self.edit(|edit| edit.set_cell(track, row, note));
    }

    /// Legacy standalone editor helper; the runtime uses `Transport` instead.
    pub const fn advance_playback_row(&mut self) {
        if matches!(self.playback_state, PlaybackState::Playing) && self.pattern.num_rows > 0 {
            self.playback_row = (self.playback_row + 1) % self.pattern.num_rows;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::NotePitch;

    #[test]
    fn instruments_wrap_isolate_selected_track_and_obey_help() {
        use crate::tracker::Instrument;
        let mut app = App {
            cursor_track: 2,
            cursor_row: 7,
            playback_row: 3,
            playback_state: PlaybackState::Playing,
            ..App::default()
        };
        app.handle_command(Command::PreviousInstrument);
        assert_eq!(app.pattern.instrument(2), Instrument::HiHat);
        app.handle_command(Command::NextInstrument);
        assert_eq!(app.pattern.instrument(2), Instrument::Pluck);
        for expected in Instrument::ALL.into_iter().cycle().skip(1).take(14) {
            app.handle_command(Command::NextInstrument);
            assert_eq!(app.pattern.instrument(2), expected);
            assert_eq!(app.pattern.instrument(0), Instrument::Pluck);
            assert_eq!((app.cursor_track, app.cursor_row, app.playback_row), (2, 7, 3));
            assert!(matches!(app.playback_state, PlaybackState::Playing));
        }
        app.handle_command(Command::SetInstrument(Instrument::Snare));
        app.handle_command(Command::ToggleHelp);
        for command in [
            Command::PreviousInstrument,
            Command::NextInstrument,
            Command::SetInstrument(Instrument::Sine),
        ] {
            app.handle_command(command);
            assert_eq!(app.pattern.instrument(2), Instrument::Snare);
        }
        app.pattern.tracks.push(vec![]);
        assert_eq!(app.pattern.instrument(4), Instrument::Pluck);
        app.pattern.set_instrument(4, Instrument::Kick);
        assert_eq!(app.pattern.instrument(4), Instrument::Kick);
        app.pattern.set_instrument(usize::MAX, Instrument::Kick);
        assert_eq!(app.pattern.instrument(usize::MAX), Instrument::Pluck);
    }

    #[test]
    fn tempo_bounds_preserve_editor_and_playback_state() {
        for playing in [false, true] {
            let mut app = App::default();
            if playing {
                app.handle_command(Command::TogglePlayback);
            }
            app.playback_row = 17;
            app.cursor_row = 5;
            for _ in 0..400 {
                app.handle_command(Command::DecreaseTempo);
            }
            assert_eq!(app.bpm, 20.0);
            app.handle_command(Command::IncreaseTempo);
            assert_eq!(app.bpm, 21.0);
            for _ in 0..400 {
                app.handle_command(Command::IncreaseTempo);
            }
            assert_eq!(app.bpm, 300.0);
            app.handle_command(Command::DecreaseTempo);
            assert_eq!(app.bpm, 299.0);
            assert_eq!(matches!(app.playback_state, PlaybackState::Playing), playing);
            assert_eq!(
                (app.playback_row, app.cursor_row, app.cursor_track, app.octave),
                (17, 5, 0, 4)
            );
        }
    }

    #[test]
    fn semitone_commands_cover_every_pitch_octave_and_limit() {
        use NotePitch::{As, Cs, Ds, Fs, Gs, A, B, C, D, E, F, G};
        let pitches = [C, Cs, D, Ds, E, F, Fs, G, Gs, A, As, B];
        let mut app = App {
            mode: Mode::Insert,
            cursor_row: 3,
            cursor_track: 2,
            ..App::default()
        };
        for command in [Command::LowerNote, Command::RaiseNote] {
            app.handle_command(command);
            assert_eq!(app.pattern.get_step(2, 3).unwrap().note, None);
            for index in 0..120_usize {
                let note = Note {
                    pitch: pitches[index % 12],
                    octave: u8::try_from(index / 12).unwrap(),
                };
                app.insert_note_under_cursor(Some(note));
                app.handle_command(command);
                let expected = if command == Command::RaiseNote {
                    (index + 1).min(119)
                } else {
                    index.saturating_sub(1)
                };
                assert_eq!(
                    app.pattern.get_step(2, 3).unwrap().note,
                    Some(Note {
                        pitch: pitches[expected % 12],
                        octave: u8::try_from(expected / 12).unwrap()
                    })
                );
                assert_eq!(app.octave, 4);
                assert_eq!(app.pattern.get_step(0, 0).unwrap().note, None);
            }
            app.insert_note_under_cursor(None);
        }
    }

    #[test]
    fn help_is_modal_bounded_and_does_not_pause_playback() {
        let mut app = App {
            mode: Mode::Insert,
            ..App::default()
        };
        app.handle_command(Command::DismissHelp);
        assert!(!app.shutdown);
        app.handle_command(Command::TogglePlayback);
        app.handle_command(Command::InsertNote {
            pitch: NotePitch::C,
            octave_offset: 0,
        });
        let note = app.pattern.get_step(0, 0).unwrap().note;
        app.handle_command(Command::ToggleHelp);
        for binding in crate::bindings::DEFAULT_BINDINGS {
            if !matches!(
                binding.command,
                Command::Quit
                    | Command::ToggleHelp
                    | Command::DismissHelp
                    | Command::ScrollHelpUp
                    | Command::ScrollHelpDown
            ) {
                app.handle_command(binding.command);
                assert_eq!(app.bpm, 120.0, "{:?}", binding.command);
                assert_eq!((app.cursor_row, app.cursor_track, app.octave), (0, 0, 4));
                assert_eq!(app.pattern.get_step(0, 0).unwrap().note, note);
                assert!(matches!(app.playback_state, PlaybackState::Playing));
            }
        }
        app.advance_playback_row();
        assert_eq!(app.playback_row, 1);
        for _ in 0..100 {
            app.handle_command(Command::ScrollHelpDown);
        }
        assert_eq!(app.help_scroll, crate::bindings::DEFAULT_BINDINGS.len() - 1);
        for _ in 0..100 {
            app.handle_command(Command::ScrollHelpUp);
        }
        assert_eq!(app.help_scroll, 0);
        app.handle_command(Command::ToggleHelp);
        assert!(!app.help_visible);
        app.handle_command(Command::ToggleHelp);
        app.handle_command(Command::DismissHelp);
        assert!(!app.help_visible);
        app.handle_command(Command::ToggleHelp);
        app.handle_command(Command::Quit);
        assert!(app.shutdown);
    }

    #[test]
    fn commands_move_edit_and_delete_at_the_selected_cell() {
        let mut app = App {
            mode: Mode::Insert,
            ..App::default()
        };
        app.handle_command(Command::MoveCursorDown);
        app.handle_command(Command::MoveCursorRight);
        app.handle_command(Command::InsertNote {
            pitch: NotePitch::D,
            octave_offset: 1,
        });
        assert_eq!(
            app.pattern.get_step(1, 1).unwrap().note,
            Some(Note {
                pitch: NotePitch::D,
                octave: 5
            })
        );
        app.handle_command(Command::DeleteNote);
        assert_eq!(app.pattern.get_step(1, 1).unwrap().note, None);
        app.handle_command(Command::MoveCursorUp);
        app.handle_command(Command::MoveCursorLeft);
        app.handle_command(Command::MoveCursorUp);
        app.handle_command(Command::MoveCursorLeft);
        assert_eq!((app.cursor_row, app.cursor_track), (0, 0));
        for _ in 0..100 {
            app.handle_command(Command::MoveCursorDown);
            app.handle_command(Command::MoveCursorRight);
        }
        assert_eq!((app.cursor_row, app.cursor_track), (63, 3));
    }

    #[test]
    fn octave_commands_and_upper_c_stay_in_range() {
        let mut app = App {
            mode: Mode::Insert,
            ..App::default()
        };
        for _ in 0..20 {
            app.handle_command(Command::IncreaseOctave);
        }
        assert_eq!(app.octave, 9);
        app.handle_command(Command::InsertNote {
            pitch: NotePitch::C,
            octave_offset: 1,
        });
        assert_eq!(app.pattern.get_step(0, 0).unwrap().note.unwrap().octave, 9);
        for _ in 0..20 {
            app.handle_command(Command::DecreaseOctave);
        }
        assert_eq!(app.octave, 0);
    }

    #[test]
    fn playback_commands_restart_at_row_zero_and_quit() {
        let mut app = App::default();
        app.handle_command(Command::TogglePlayback);
        assert!(matches!(app.playback_state, PlaybackState::Playing));
        app.advance_playback_row();
        app.handle_command(Command::TogglePlayback);
        assert!(matches!(app.playback_state, PlaybackState::Stopped));
        app.handle_command(Command::TogglePlayback);
        assert_eq!(app.playback_row, 0);
        app.handle_command(Command::Quit);
        assert!(app.shutdown);
    }
}
