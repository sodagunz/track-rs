use anyhow::anyhow;
use fundsp::math::midi_hz;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    pub pitch: NotePitch,
    pub octave: u8,
}

impl Note {
    #[allow(clippy::cast_precision_loss)]
    pub fn freq(self) -> f64 {
        // freq = 440.0 * 2^((midi_note - 69) / 12)
        let pitch_class = match self.pitch {
            NotePitch::C => 0,
            NotePitch::Cs => 1,
            NotePitch::D => 2,
            NotePitch::Ds => 3,
            NotePitch::E => 4,
            NotePitch::F => 5,
            NotePitch::Fs => 6,
            NotePitch::G => 7,
            NotePitch::Gs => 8,
            NotePitch::A => 9,
            NotePitch::As => 10,
            NotePitch::B => 11,
        };

        let midi_note = (i32::from(self.octave) * 12) + pitch_class + 12;

        midi_hz(midi_note as f64)
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
    pub fn new(num_rows: usize, num_tracks: usize) -> Self {
        Pattern {
            num_rows,
            tracks: vec![vec![PatternStep::default(); num_rows]; num_tracks],
        }
    }

    pub fn get_step(&self, track_idx: usize, row_idx: usize) -> Option<&PatternStep> {
        self.tracks.get(track_idx)?.get(row_idx)
    }

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
