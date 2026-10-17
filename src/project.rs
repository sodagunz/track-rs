//! Document data only; track voices are shared by every pattern.
use crate::tracker::{Instrument, Pattern};

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub bpm: f64,
    pub patterns: Vec<Pattern>,
    pub order: Vec<usize>,
    pub(crate) instruments: Vec<Instrument>,
    pub(crate) channels: Vec<ChannelSettings>,
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
            channels: vec![ChannelSettings::default(); tracks],
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

/// Document targets only. `None` volume means silence, not a non-finite number.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelSettings {
    pub volume_db: Option<f32>,
    pub pan: f32,
}

impl Default for ChannelSettings {
    fn default() -> Self {
        Self {
            volume_db: Some(0.0),
            pan: 0.0,
        }
    }
}

impl ChannelSettings {
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.volume_db
            .is_none_or(|v| v.is_finite() && (-60.0..=6.0).contains(&v))
            && self.pan.is_finite()
            && (-100.0..=100.0).contains(&self.pan)
    }

    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // Clamp before narrowing the bounded key count.
    pub fn adjusted(mut self, row: usize, steps: i32) -> Self {
        if row == 0 {
            let value = self.volume_db.unwrap_or(-61.0) + f32::from(steps.clamp(-9999, 9999) as i16);
            self.volume_db = if value < -60.0 { None } else { Some(value.min(6.0)) };
        } else {
            self.pan = (self.pan + f32::from(steps.clamp(-9999, 9999) as i16)).clamp(-100.0, 100.0);
        }
        self
    }

    #[must_use]
    pub fn label(self, row: usize) -> String {
        if row == 0 {
            self.volume_db.map_or_else(|| "-inf dB".into(), |v| format!("{v:+} dB"))
        } else if self.pan < 0.0 {
            format!("L{}", -self.pan)
        } else if self.pan > 0.0 {
            format!("R{}", self.pan)
        } else {
            "Centre".into()
        }
    }
}

impl Project {
    #[must_use]
    pub fn channels(&self) -> &[ChannelSettings] {
        &self.channels
    }
}
