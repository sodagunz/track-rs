//! Persistent stereo topology-preserving state-variable filters. Targets are prepared
//! on the control thread; the callback only interpolates and advances fixed-size state.
#![allow(clippy::suboptimal_flops, clippy::float_cmp)] // Keep the published TPT equations
                                                       // recognizable.
use crate::project::ChannelSettings;
use std::f64::consts::{FRAC_1_SQRT_2, PI, SQRT_2};

#[derive(Clone, Copy, Debug)]
pub struct Target {
    g: f64,
    a: f64,
    wet: f64,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            g: 0.1,
            a: 1.0,
            wet: 0.0,
        }
    }
}

pub fn targets(settings: ChannelSettings, rate: f64) -> [Target; 4] {
    let frequencies = [
        200.0,
        4000.0,
        f64::from(settings.hpf_hz.unwrap_or(20.0)),
        f64::from(settings.lpf_hz.unwrap_or(20_000.0)),
    ];
    std::array::from_fn(|i| {
        let a = match i {
            0 => 10.0_f64.powf(f64::from(settings.low_shelf_db) / 40.0),
            1 => 10.0_f64.powf(f64::from(settings.high_shelf_db) / 40.0),
            _ => 1.0,
        };
        // Keep the shelf's warped internal frequency below Nyquist as well.
        let warp = match i {
            0 => 1.0 / a.sqrt(),
            1 => a.sqrt(),
            _ => 1.0,
        };
        let g = ((PI * frequencies[i].min(rate * 0.45) / rate).tan() * warp).min((PI * 0.45).tan());
        Target {
            g,
            a,
            wet: if i < 2 || (i == 2 && settings.hpf_hz.is_some()) || (i == 3 && settings.lpf_hz.is_some()) {
                1.0
            } else {
                0.0
            },
        }
    })
}

#[derive(Default)]
struct Filter {
    current: Target,
    target: Target,
    remaining: u32,
    // Independent integrators preserve stereo width and outlive individual voices.
    state: [[f64; 2]; 2],
}

impl Filter {
    const fn set(&mut self, target: Target, frames: u32) {
        self.target = target;
        self.remaining = frames;
    }

    const fn reset(&mut self) {
        self.current = self.target;
        self.remaining = 0;
        self.state = [[0.0; 2]; 2];
    }

    fn process(&mut self, input: [f64; 2], kind: usize) -> [f64; 2] {
        if self.remaining == 0 && (self.current.wet == 0.0 || (kind < 2 && self.current.a == 1.0)) {
            self.state = [[0.0; 2]; 2];
            return input;
        }
        if self.remaining > 0 {
            let n = f64::from(self.remaining);
            self.current.g += (self.target.g - self.current.g) / n;
            self.current.a += (self.target.a - self.current.a) / n;
            self.current.wet += (self.target.wet - self.current.wet) / n;
            self.remaining -= 1;
            if self.remaining == 0 {
                self.current = self.target;
                if self.current.wet == 0.0 || (kind < 2 && self.current.a == 1.0) {
                    self.state = [[0.0; 2]; 2];
                    return input;
                }
            }
        }
        let Target { g, a, wet } = self.current;
        let k = 1.0 / FRAC_1_SQRT_2; // Butterworth, fixed non-resonant Q.
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let (m0, m1, m2) = match kind {
            0 => (1.0, SQRT_2 * (a - 1.0), a * a - 1.0),
            1 => (a * a, SQRT_2 * (1.0 - a) * a, 1.0 - a * a),
            2 => (1.0, -k, -1.0),
            _ => (0.0, 0.0, 1.0),
        };
        std::array::from_fn(|channel| {
            let x = input[channel];
            let [ic1, ic2] = self.state[channel];
            let v3 = x - ic2;
            let v1 = a1 * ic1 + a2 * v3;
            let v2 = ic2 + a2 * ic1 + a3 * v3;
            self.state[channel] = [2.0 * v1 - ic1, 2.0 * v2 - ic2];
            // Exact dry identity at neutral shelves and Off, with warm filter memory.
            x + wet * ((m0 - 1.0) * x + m1 * v1 + m2 * v2)
        })
    }
}

#[derive(Default)]
pub struct Tone {
    filters: [Filter; 4],
    active: bool,
}

impl Tone {
    pub fn set(&mut self, targets: [Target; 4], frames: u32) {
        self.active = true;
        for (filter, target) in self.filters.iter_mut().zip(targets) {
            filter.set(target, frames);
        }
    }

    pub fn reset(&mut self) {
        for filter in &mut self.filters {
            filter.reset();
        }
    }

    #[allow(clippy::cast_possible_truncation)] // Audio is f32; integrators use f64 headroom.
    pub fn process(&mut self, sample: (f32, f32)) -> (f32, f32) {
        if !self.active {
            return sample;
        }
        if self
            .filters
            .iter()
            .enumerate()
            .all(|(kind, f)| f.remaining == 0 && (f.current.wet == 0.0 || (kind < 2 && f.current.a == 1.0)))
        {
            self.active = false;
            return sample;
        }
        let mut sample = [f64::from(sample.0), f64::from(sample.1)];
        for (kind, filter) in self.filters.iter_mut().enumerate() {
            sample = filter.process(sample, kind);
        }
        (sample[0] as f32, sample[1] as f32)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    use super::*;

    fn response(settings: ChannelSettings, rate: u32, hz: f64) -> f64 {
        let mut tone = Tone::default();
        tone.set(targets(settings, f64::from(rate)), rate / 100);
        tone.reset();
        let mut input_energy = 0.0;
        let mut output_energy = 0.0;
        for i in 0..rate {
            let x = (2.0 * PI * hz * f64::from(i) / f64::from(rate)).sin() as f32;
            let (l, r) = tone.process((x, -x));
            assert_eq!(l, -r);
            if i > rate / 2 {
                input_energy += f64::from(x).powi(2);
                output_energy += f64::from(l).powi(2);
            }
        }
        10.0 * (output_energy / input_energy).log10()
    }

    #[test]
    fn frequency_response_shelves_and_butterworth_filters_at_multiple_rates() {
        for rate in [8000, 44_100, 48_000, 96_000] {
            let neutral = ChannelSettings::default();
            assert!(response(neutral, rate, 337.0).abs() < 1e-6);
            for gain in [-12.0, 12.0] {
                let low = ChannelSettings {
                    low_shelf_db: gain,
                    ..neutral
                };
                assert!((response(low, rate, 20.0) - f64::from(gain)).abs() < 0.1);
                assert!(response(low, rate, f64::from(rate) * 0.4).abs() < 0.1);
                let high = ChannelSettings {
                    high_shelf_db: gain,
                    ..neutral
                };
                assert!(response(high, rate, 40.0).abs() < 0.1);
                if rate >= 44_100 {
                    assert!((response(high, rate, f64::from(rate) * 0.45) - f64::from(gain)).abs() < 0.1);
                }
            }
            let hp = ChannelSettings {
                hpf_hz: Some(400.0),
                ..neutral
            };
            let lp = ChannelSettings {
                lpf_hz: Some(400.0),
                ..neutral
            };
            assert!((response(hp, rate, 400.0) + 3.0103).abs() < 0.02);
            assert!((response(lp, rate, 400.0) + 3.0103).abs() < 0.02);
            assert!(response(hp, rate, 40.0) < -39.0);
            assert!(response(lp, rate, 40.0).abs() < 0.02);
            assert!(response(lp, rate, 2000.0) < -27.0);
        }
    }

    #[test]
    fn rapid_sweeps_bypass_reset_tails_and_nyquist_are_finite() {
        for rate in [1000, 8000, 44_100, 48_000, 96_000] {
            let mut tone = Tone::default();
            let frames = rate / 100;
            for step in 0..1000 {
                let settings = ChannelSettings {
                    low_shelf_db: if step % 2 == 0 { 12.0 } else { -12.0 },
                    high_shelf_db: if step % 3 == 0 { 12.0 } else { -12.0 },
                    hpf_hz: if step % 5 == 0 {
                        None
                    } else {
                        Some(if step % 2 == 0 { 20.0 } else { 20_000.0 })
                    },
                    lpf_hz: if step % 7 == 0 {
                        None
                    } else {
                        Some(if step % 2 == 0 { 20_000.0 } else { 20.0 })
                    },
                    ..ChannelSettings::default()
                };
                let target = targets(settings, f64::from(rate));
                assert!(target
                    .iter()
                    .all(|t| t.g.is_finite() && t.g > 0.0 && t.g <= (PI * 0.45).tan()));
                tone.set(target, frames);
                for i in 0..frames / 2 {
                    let x = if step < 500 {
                        (f64::from(i + step) * 0.27).sin() as f32
                    } else {
                        0.0
                    };
                    let (l, r) = tone.process((x, -0.25 * x));
                    assert!(l.is_finite() && r.is_finite());
                    assert!(l.abs() < 1000.0 && r.abs() < 1000.0);
                }
            }
            tone.set(targets(ChannelSettings::default(), f64::from(rate)), frames);
            for _ in 0..frames {
                tone.process((0.0, 0.0));
            }
            assert_eq!(tone.process((0.123, -0.456)), (0.123, -0.456));
            tone.reset();
            assert!(tone.filters.iter().all(|f| f.state == [[0.0; 2]; 2]));
            assert_eq!(tone.process((0.123, -0.456)), (0.123, -0.456));
        }
    }

    #[test]
    fn transitions_take_ten_ms_and_filters_keep_release_memory() {
        for rate in [8000, 44_100, 48_000, 96_000] {
            let mut tone = Tone::default();
            tone.set(
                targets(
                    ChannelSettings {
                        lpf_hz: Some(400.0),
                        ..ChannelSettings::default()
                    },
                    f64::from(rate),
                ),
                rate / 100,
            );
            for _ in 0..rate / 100 - 1 {
                tone.process((0.0, 0.0));
            }
            assert_eq!(tone.filters[3].remaining, 1);
            tone.process((1.0, 0.0));
            assert_eq!(tone.filters[3].remaining, 0);
            let tail = tone.process((0.0, 0.0));
            assert!(tail.0.abs() > 0.0);
            assert_eq!(tail.1, 0.0);
            tone.reset();
            assert_eq!(tone.process((0.0, 0.0)), (0.0, 0.0));
            assert_eq!(tone.filters[3].current.wet, 1.0);
        }
    }
}
