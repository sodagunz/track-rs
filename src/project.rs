//! Document data only; track voices are shared by every pattern.
use crate::tracker::{Instrument, Pattern};

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub bpm: f64,
    pub patterns: Vec<Pattern>,
    pub order: Vec<usize>,
    pub(crate) instruments: Vec<Instrument>,
}

impl Default for Project {
    fn default() -> Self {
        Self::new(64, 4)
    }
}

impl Project {
    #[must_use]
    pub fn new(rows: usize, tracks: usize) -> Self {
        Self {
            bpm: 120.0,
            patterns: vec![Pattern::new(rows, tracks)],
            order: vec![0],
            instruments: vec![Instrument::default(); tracks],
        }
    }

    #[must_use]
    pub fn instruments(&self) -> &[Instrument] {
        &self.instruments
    }

    #[must_use]
    pub fn instrument(&self, track: usize) -> Instrument {
        self.instruments.get(track).copied().unwrap_or_default()
    }

    pub fn set_instrument(&mut self, track: usize, instrument: Instrument) {
        if let Some(value) = self.instruments.get_mut(track) {
            *value = instrument;
        }
    }
}
