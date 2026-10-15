use anyhow::anyhow;
use fundsp::math::midi_hz;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotePitch {
    C,
    Cs,
    D,
    Ds,
    E,
    F,
    Fs,
    G,
    Gs,
    A,
    As,
    B,
}

impl NotePitch {
    const fn semitone(self) -> i32 {
        match self {
            Self::C => 0,
            Self::Cs => 1,
            Self::D => 2,
            Self::Ds => 3,
            Self::E => 4,
            Self::F => 5,
            Self::Fs => 6,
            Self::G => 7,
            Self::Gs => 8,
            Self::A => 9,
            Self::As => 10,
            Self::B => 11,
        }
    }
}

impl std::fmt::Display for NotePitch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::C => "C",
            Self::Cs => "C#",
            Self::D => "D",
            Self::Ds => "D#",
            Self::E => "E",
            Self::F => "F",
            Self::Fs => "F#",
            Self::G => "G",
            Self::Gs => "G#",
            Self::A => "A",
            Self::As => "A#",
            Self::B => "B",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    pub pitch: NotePitch,
    pub octave: u8,
}

impl Note {
    /// Transpose chromatically, clamping to C0 through B9.
    #[must_use]
    pub fn transpose(self, semitones: i8) -> Self {
        let index = (i32::from(self.octave) * 12 + self.pitch.semitone() + i32::from(semitones)).clamp(0, 119);
        let pitch = match index % 12 {
            0 => NotePitch::C,
            1 => NotePitch::Cs,
            2 => NotePitch::D,
            3 => NotePitch::Ds,
            4 => NotePitch::E,
            5 => NotePitch::F,
            6 => NotePitch::Fs,
            7 => NotePitch::G,
            8 => NotePitch::Gs,
            9 => NotePitch::A,
            10 => NotePitch::As,
            _ => NotePitch::B,
        };
        Self {
            pitch,
            octave: u8::try_from(index / 12).unwrap_or(9),
        }
    }

    #[must_use]
    pub fn freq(self) -> f64 {
        // freq = 440.0 * 2^((midi_note - 69) / 12)
        let pitch_class = self.pitch.semitone();

        let midi_note = (i32::from(self.octave) * 12) + pitch_class + 12;

        midi_hz(f64::from(midi_note))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PatternStep {
    pub note: Option<Note>,
}

pub type Track = Vec<PatternStep>;

#[derive(Clone, Debug)]
pub struct Pattern {
    pub num_rows: usize,
    pub tracks: Vec<Track>,
}

impl Pattern {
    #[must_use]
    pub fn new(num_rows: usize, num_tracks: usize) -> Self {
        Self {
            num_rows,
            tracks: vec![vec![PatternStep::default(); num_rows]; num_tracks],
        }
    }

    #[must_use]
    pub fn get_step(&self, track_idx: usize, row_idx: usize) -> Option<&PatternStep> {
        self.tracks.get(track_idx)?.get(row_idx)
    }

    /// Replace the note at the specified track and row.
    ///
    /// # Errors
    /// Returns an error if the track or row index is outside the pattern.
    pub fn set_step(&mut self, track_idx: usize, row_idx: usize, new_note: Option<Note>) -> anyhow::Result<()> {
        self.tracks
            .get_mut(track_idx)
            .ok_or_else(|| anyhow!("Accessing inexistant track"))?
            .get_mut(row_idx)
            .ok_or_else(|| anyhow!("Accessing inexistant row"))?
            .note = new_note;
        Ok(())
    }
}
