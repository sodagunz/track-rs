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
                if !(1..=MAX_ROWS).contains(&pattern.num_rows()) || pattern.tracks.len() != tracks {
                    return None;
                }
                total.checked_add(pattern.num_rows().checked_mul(tracks + 1)?)
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
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
    pub volume_db: Option<f32>,
    pub pan: f32,
    #[serde(default)]
    pub low_shelf_db: f32,
    #[serde(default)]
    pub high_shelf_db: f32,
    #[serde(default)]
    pub hpf_hz: Option<f32>,
    #[serde(default)]
    pub lpf_hz: Option<f32>,
}

impl Default for ChannelSettings {
    fn default() -> Self {
        Self {
            muted: false,
            solo: false,
            volume_db: Some(0.0),
            pan: 0.0,
            low_shelf_db: 0.0,
            high_shelf_db: 0.0,
            hpf_hz: None,
            lpf_hz: None,
        }
    }
}

pub const CHANNEL_ROWS: usize = 6;
pub const CHANNEL_LABELS: [&str; CHANNEL_ROWS] = ["Vol dB", "Pan %", "Low dB", "High dB", "HPF Hz", "LPF Hz"];

impl ChannelSettings {
    #[must_use]
    pub const fn audible(self, any_solo: bool) -> bool {
        !self.muted && (!any_solo || self.solo)
    }

    #[must_use]
    pub fn is_valid(self) -> bool {
        self.volume_db
            .is_none_or(|v| v.is_finite() && (-60.0..=6.0).contains(&v))
            && self.pan.is_finite()
            && (-100.0..=100.0).contains(&self.pan)
            && [self.low_shelf_db, self.high_shelf_db]
                .iter()
                .all(|v| v.is_finite() && (-12.0..=12.0).contains(v))
            && [self.hpf_hz, self.lpf_hz]
                .iter()
                .all(|v| v.is_none_or(|v| v.is_finite() && (20.0..=20_000.0).contains(&v)))
    }

    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // Clamp before narrowing the bounded key count.
    pub fn adjusted(mut self, row: usize, steps: i32) -> Self {
        if row == 0 {
            let value = self.volume_db.unwrap_or(-61.0) + f32::from(steps.clamp(-9999, 9999) as i16);
            self.volume_db = if value < -60.0 { None } else { Some(value.min(6.0)) };
        } else if row == 1 {
            self.pan = (self.pan + f32::from(steps.clamp(-9999, 9999) as i16)).clamp(-100.0, 100.0);
        } else if row < 4 {
            let gain = if row == 2 {
                &mut self.low_shelf_db
            } else {
                &mut self.high_shelf_db
            };
            *gain = (*gain + f32::from(steps.clamp(-9999, 9999) as i16)).clamp(-12.0, 12.0);
        } else {
            let cutoff = if row == 4 { &mut self.hpf_hz } else { &mut self.lpf_hz };
            // Apply each step so counted motion has the same Off boundaries as single keys.
            for _ in 0..steps.unsigned_abs().min(9999) {
                *cutoff = match (*cutoff, row, steps.signum()) {
                    (None, 4, 1) => Some(20.0),
                    (None, 5, -1) => Some(20_000.0),
                    (Some(v), 4, -1) if v <= 20.0 => None,
                    (Some(v), 5, 1) if v >= 20_000.0 => None,
                    (Some(v), _, direction) => {
                        let next = v * (f32::from(direction as i16) / 12.0).exp2();
                        if (row == 4 && next < 20.0) || (row == 5 && next > 20_000.0) {
                            None
                        } else {
                            Some(next.clamp(20.0, 20_000.0))
                        }
                    }
                    _ => *cutoff,
                };
            }
        }
        self
    }

    #[must_use]
    pub fn label(self, row: usize) -> String {
        if row >= 4 {
            return if row == 4 { self.hpf_hz } else { self.lpf_hz }.map_or_else(
                || "Off".into(),
                |v| {
                    if v >= 1000.0 {
                        format!("{:.2} kHz", v / 1000.0)
                    } else {
                        format!("{v:.1} Hz")
                    }
                },
            );
        }
        if row >= 2 {
            return format!(
                "{:+} dB",
                if row == 2 {
                    self.low_shelf_db
                } else {
                    self.high_shelf_db
                }
            );
        }
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
