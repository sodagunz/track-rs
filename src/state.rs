use crate::{
    command::Command,
    tracker::{Note, Pattern},
};

#[derive(Clone, Copy, Debug)]
pub enum PlaybackState {
    Stopped,
    Playing,
}

#[derive(Clone, Debug)]
pub struct App {
    pub pattern: Pattern,
    pub cursor_row: usize,
    pub cursor_track: usize,
    pub playback_row: usize,
    pub playback_state: PlaybackState,
    pub octave: u8,
    pub bpm: f64,
    pub shutdown: bool,
}

pub enum CursorOffset {
    Neg(usize),
    Pos(usize),
    None,
}

impl CursorOffset {
    fn apply_to(&self, position: usize) -> usize {
        match self {
            CursorOffset::Pos(delta) => position.checked_add(*delta).unwrap_or(position),
            CursorOffset::Neg(delta) => position.checked_sub(*delta).unwrap_or(position),
            CursorOffset::None => position,
        }
    }
}

impl Default for App {
    fn default() -> Self {
        let default_rows = 64;
        let default_tracks = 4;
        App {
            pattern: Pattern::new(default_rows, default_tracks),
            cursor_row: 0,
            cursor_track: 0,
            octave: 4,
            playback_row: 0,
            playback_state: PlaybackState::Stopped,
            bpm: 120.0,
            shutdown: false,
        }
    }
}

impl App {
    /// Apply an editor action without depending on terminal key events.
    pub fn handle_command(&mut self, command: Command) {
        match command {
            Command::Quit => self.shutdown(),
            Command::TogglePlayback => self.toggle_playback(),
            Command::MoveCursorUp => self.offset_cursor(&CursorOffset::Neg(1), &CursorOffset::None),
            Command::MoveCursorDown => self.offset_cursor(&CursorOffset::Pos(1), &CursorOffset::None),
            Command::MoveCursorLeft => self.offset_cursor(&CursorOffset::None, &CursorOffset::Neg(1)),
            Command::MoveCursorRight => self.offset_cursor(&CursorOffset::None, &CursorOffset::Pos(1)),
            Command::InsertNote { pitch, octave_offset } => self.insert_note_under_cursor(Some(Note {
                pitch,
                octave: self.octave.saturating_add(octave_offset).min(9),
            })),
            Command::DeleteNote => self.insert_note_under_cursor(None),
            Command::DecreaseOctave => self.octave = self.octave.saturating_sub(1).min(9),
            Command::IncreaseOctave => self.octave = self.octave.saturating_add(1).min(9),
        }
    }

    pub fn offset_cursor(&mut self, dr: &CursorOffset, dt: &CursorOffset) {
        let max_rows = self.pattern.num_rows;
        let max_tracks = self.pattern.tracks.len();

        self.cursor_row = dr.apply_to(self.cursor_row).clamp(0, max_rows - 1);
        self.cursor_track = dt.apply_to(self.cursor_track).clamp(0, max_tracks - 1);
    }

    pub fn shutdown(&mut self) {
        self.shutdown = true;
    }

    pub fn toggle_playback(&mut self) {
        match self.playback_state {
            PlaybackState::Stopped => {
                self.playback_state = PlaybackState::Playing;
                self.playback_row = 0; // TODO: feat(Loop) loop_start_cursor/playback_start_cursor
            }
            PlaybackState::Playing => {
                self.playback_state = PlaybackState::Stopped;
                // TODO: Kill all sounds
            }
        }
    }

    pub fn insert_note_under_cursor(&mut self, note: Option<Note>) {
        self.pattern
            .set_step(self.cursor_track, self.cursor_row, note)
            .expect("Cursor should be whitin bounds");
    }

    pub fn advance_playback_row(&mut self) {
        if let PlaybackState::Playing = self.playback_state {
            self.playback_row = (self.playback_row + 1) % self.pattern.num_rows;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::NotePitch;

    #[test]
    fn commands_move_edit_and_delete_at_the_selected_cell() {
        let mut app = App::default();
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
        let mut app = App::default();
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
