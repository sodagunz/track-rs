//! Transient peak telemetry: one audio writer, one UI reader, no document state.
use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

const CLIP_HOLD: Duration = Duration::from_secs(2);

#[derive(Default)]
struct Peak([AtomicU32; 2]);

impl Peak {
    fn record(&self, sample: (f32, f32)) {
        if sample.0 == 0.0 && sample.1 == 0.0 {
            return;
        }
        for (peak, value) in self.0.iter().zip(<[f32; 2]>::from(sample)) {
            // Positive float bits sort numerically. Non-finite audio must not
            // look like silence; infinity is an explicit telemetry error marker.
            let value = if value.is_finite() { value.abs() } else { f32::INFINITY };
            if value > 0.0 {
                peak.fetch_max(value.to_bits(), Ordering::Relaxed);
            }
        }
    }

    fn take(&self) -> [f32; 2] {
        self.0
            .each_ref()
            .map(|peak| f32::from_bits(peak.swap(0, Ordering::Relaxed)))
    }
}

pub(crate) struct Meters {
    channels: Vec<Peak>,
    master: Peak,
    pub(crate) reset_ack: AtomicU32,
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            channels: (0..crate::project_file::MAX_TRACKS).map(|_| Peak::default()).collect(),
            master: Peak::default(),
            reset_ack: AtomicU32::new(0),
        }
    }
}

impl Meters {
    pub(crate) fn channel(&self, track: usize, sample: (f32, f32)) {
        self.channels[track].record(sample);
    }

    pub(crate) fn master(&self, sample: (f32, f32)) {
        self.master.record(sample);
    }

    // Only the audio writer clears the bank, ordered after voice cancellation.
    pub(crate) fn reset(&self, generation: u32) {
        for peak in &self.channels {
            peak.take();
        }
        self.master.take();
        self.reset_ack.store(generation, Ordering::Release);
    }
}

/// UI-owned, unsaved meter display. Decay is 24 dB/s; clip/error holds for 2 s.
#[derive(Clone, Debug, Default)]
pub struct MeterDisplay {
    pub channels: Vec<[f32; 2]>,
    pub clipped: bool,
    pub invalid: bool,
    hold: Duration,
    generation: u32,
}

impl MeterDisplay {
    pub(crate) fn update(&mut self, meters: &Meters, generation: u32, tracks: usize, elapsed: Duration) {
        if self.generation != generation {
            *self = Self {
                generation,
                ..Self::default()
            };
        }
        self.channels.resize(tracks, [0.0; 2]);
        if meters.reset_ack.load(Ordering::Acquire) != generation {
            self.channels.fill([0.0; 2]);
            return;
        }
        let decay = 10.0_f32.powf(-24.0 * elapsed.as_secs_f32() / 20.0);
        for (display, peak) in self.channels.iter_mut().zip(&meters.channels) {
            for (display, value) in display.iter_mut().zip(peak.take()) {
                *display = (*display * decay).max(value.min(f32::MAX));
            }
        }
        let master = meters.master.take();
        self.hold = self.hold.saturating_sub(elapsed);
        if self.hold.is_zero() {
            self.clipped = false;
            self.invalid = false;
        }
        if master.iter().any(|value| *value > 1.0) {
            self.clipped = true;
            self.invalid |= master.iter().any(|value| !value.is_finite());
            self.hold = CLIP_HOLD;
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn accumulated_peaks_are_consumed_and_isolated() {
        let meters = Meters::default();
        meters.channel(0, (-0.75, 0.25));
        meters.channel(0, (0.5, -0.5));
        let mut display = MeterDisplay::default();
        display.update(&meters, 0, 2, Duration::ZERO);
        assert_eq!(display.channels, [[0.75, 0.5], [0.0, 0.0]]);
        display.update(&meters, 0, 2, Duration::from_secs(1));
        assert!(0.75_f32.mul_add(-10.0_f32.powf(-1.2), display.channels[0][0]).abs() < 1e-6);
    }

    #[test]
    fn decay_and_clip_hold_follow_time_and_reset_waits_for_audio() {
        let meters = Meters::default();
        meters.channel(0, (1.0, 0.5));
        meters.master((1.2, 0.0));
        let mut a = MeterDisplay::default();
        a.update(&meters, 0, 1, Duration::ZERO);
        assert!(a.clipped);
        let mut b = MeterDisplay::default();
        meters.channel(0, (1.0, 0.5));
        b.update(&meters, 0, 1, Duration::ZERO);
        for _ in 0..10 {
            a.update(&meters, 0, 1, Duration::from_millis(100));
        }
        b.update(&meters, 0, 1, Duration::from_secs(1));
        assert!((a.channels[0][0] - b.channels[0][0]).abs() < 1e-6);
        assert!(a.clipped);
        a.update(&meters, 0, 1, Duration::from_secs(1));
        assert!(!a.clipped);
        meters.master((f32::NAN, f32::NEG_INFINITY));
        a.update(&meters, 0, 1, Duration::ZERO);
        assert!(a.clipped && a.invalid);
        a.update(&meters, 1, 2, Duration::ZERO);
        assert!(!a.clipped && !a.invalid);
        assert_eq!(a.channels, [[0.0; 2]; 2]);
        meters.reset(1);
        a.update(&meters, 1, 2, Duration::ZERO);
        assert!(!a.clipped);
        assert_eq!(a.channels, [[0.0; 2]; 2]);
    }
}
