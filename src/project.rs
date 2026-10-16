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

    /// Check the resulting rectangular cell count before allocating columns.
    #[must_use]
    pub fn can_append_track(&self) -> bool {
        use crate::project_file::{MAX_CELLS, MAX_ROWS, MAX_TRACKS};
        let tracks = self.instruments.len();
        if !(1..MAX_TRACKS).contains(&tracks) || self.patterns.is_empty() {
            return false;
        }
        self.patterns
            .iter()
            .try_fold(0usize, |total, pattern| {
                if !(1..=MAX_ROWS).contains(&pattern.num_rows) || pattern.tracks.len() != tracks {
                    return None;
                }
                total.checked_add(pattern.num_rows.checked_mul(tracks + 1)?)
            })
            .is_some_and(|total| total <= MAX_CELLS)
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
