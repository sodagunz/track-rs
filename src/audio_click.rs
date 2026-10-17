//! Fixed-size procedural click, prepared on the control thread and rendered without allocation.
pub(super) struct ClickVoice {
    elapsed: f64,
    period: f64,
    frequency: f64,
    gain: f64,
}

impl ClickVoice {
    pub(super) fn new(sample_rate: f64, accent: bool) -> Self {
        Self {
            elapsed: 0.0,
            period: sample_rate.recip(),
            frequency: (if accent { 1800.0_f64 } else { 1200.0_f64 }).min(sample_rate * 0.2),
            gain: if accent { 0.10 } else { 0.07 },
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(super) fn sample(&mut self) -> Option<f32> {
        if self.elapsed >= 0.010 {
            return None;
        }
        let attack = (self.elapsed / 0.0005).min(1.0);
        let release = (1.0 - self.elapsed / 0.010).powi(2);
        let sample = (self.elapsed * self.frequency * std::f64::consts::TAU).sin() * self.gain * attack * release;
        self.elapsed += self.period;
        Some(sample as f32)
    }
}
