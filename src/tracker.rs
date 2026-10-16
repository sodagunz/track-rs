use anyhow::anyhow;
use fundsp::math::midi_hz;

/// Built-in voices. Percussion ignores note pitch and uses fixed drum tuning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Instrument {
    #[default]
    Pluck,
    Sine,
    Square,
    Saw,
    Kick,
    Snare,
    HiHat,
}

impl Instrument {
    pub const ALL: [Self; 7] = [
        Self::Pluck,
        Self::Sine,
        Self::Square,
        Self::Saw,
        Self::Kick,
        Self::Snare,
        Self::HiHat,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pluck => "Pluck",
            Self::Sine => "Sine",
            Self::Square => "Square",
            Self::Saw => "Saw",
            Self::Kick => "Kick",
            Self::Snare => "Snare",
            Self::HiHat => "HiHat",
        }
    }

    #[must_use]
    pub fn cycle(self, forward: bool) -> Self {
        let index = Self::ALL.iter().position(|instrument| *instrument == self).unwrap_or(0);
        Self::ALL[(index + if forward { 1 } else { Self::ALL.len() - 1 }) % Self::ALL.len()]
    }
}

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
    instruments: Vec<Instrument>,
}

impl Pattern {
    #[must_use]
    pub fn new(num_rows: usize, num_tracks: usize) -> Self {
        Self {
            num_rows,
            tracks: vec![vec![PatternStep::default(); num_rows]; num_tracks],
            instruments: vec![Instrument::default(); num_tracks],
        }
    }

    /// Tracks appended through the public vector default to Pluck. Assignments follow indices.
    #[must_use]
    pub fn instrument(&self, track: usize) -> Instrument {
        self.instruments.get(track).copied().unwrap_or_default()
    }

    /// Assign an existing track; invalid track indices are ignored.
    pub fn set_instrument(&mut self, track: usize, instrument: Instrument) {
        if track < self.tracks.len() {
            self.instruments.resize(self.tracks.len(), Instrument::default());
            self.instruments[track] = instrument;
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
