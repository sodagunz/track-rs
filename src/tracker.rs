use anyhow::anyhow;
use fundsp::math::midi_hz;
use serde::{Deserialize, Serialize};

/// Built-in voices. Percussion ignores note pitch and uses fixed drum tuning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotePitch {
    C,
    #[serde(rename = "C#")]
    Cs,
    D,
    #[serde(rename = "D#")]
    Ds,
    E,
    F,
    #[serde(rename = "F#")]
    Fs,
    G,
    #[serde(rename = "G#")]
    Gs,
    A,
    #[serde(rename = "A#")]
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

/// Validated musical gate: sixteen ticks per row, one through 4096 ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct NoteLength(u16);

impl NoteLength {
    pub const TICKS_PER_ROW: u16 = 16;

    #[must_use]
    pub const fn ticks(self) -> u16 {
        self.0
    }

    #[must_use]
    pub fn adjusted(self, double: bool) -> Self {
        Self(if double {
            self.0.saturating_mul(2).min(4096)
        } else {
            (self.0 / 2).max(1)
        })
    }
}

impl Default for NoteLength {
    fn default() -> Self {
        Self(Self::TICKS_PER_ROW)
    }
}

impl TryFrom<u16> for NoteLength {
    type Error = &'static str;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if (1..=4096).contains(&value) {
            Ok(Self(value))
        } else {
            Err("Note length must be 1..=4096 ticks")
        }
    }
}

impl From<NoteLength> for u16 {
    fn from(value: NoteLength) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub length_ticks: NoteLength,
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
            length_ticks: self.length_ticks,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PatternStep {
    pub note: Option<Note>,
}

pub type Track = Vec<PatternStep>;

#[derive(Clone, Debug, PartialEq, Eq)]
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
