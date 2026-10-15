use crate::tracker::{Note, Pattern};

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
