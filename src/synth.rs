//! Standalone three-oscillator voice. No allocation or external state during rendering.
//!
//! Phases restart at zero. Saw/square use `PolyBLEP` edge correction; triangle is
//! analytic (not band-limited). Frequencies stop at 45% of the sample rate.
//! The mixer divides by three, then applies a fixed 0.25 output trim. Resonance
//! can still boost peaks: this is headroom, not a limiter.

// Keep the oscillator/filter equations recognizable for manual DSP experiments.
#![allow(clippy::suboptimal_flops)]

use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Oscillator {
    pub waveform: Waveform,
    pub coarse: f64,
    pub fine: f64,
    pub gain: f64,
}

impl Default for Oscillator {
    fn default() -> Self {
        Self {
            waveform: Waveform::Saw,
            coarse: 0.0,
            fine: 0.0,
            gain: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub attack: f64,
    pub decay: f64,
    pub sustain: f64,
    pub release: f64,
}

/// Parameter values only; waveform and envelope edits take effect at gate-on.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SynthPatch {
    pub oscillators: [Oscillator; 3],
    pub cutoff: f64,
    pub q: f64,
    pub envelope: Envelope,
}

impl Default for SynthPatch {
    fn default() -> Self {
        let mut oscillators = [Oscillator::default(); 3];
        oscillators[0].gain = 1.0;
        Self {
            oscillators,
            cutoff: 4000.0,
            q: 0.707,
            envelope: Envelope {
                attack: 0.01,
                decay: 0.15,
                sustain: 0.7,
                release: 0.2,
            },
        }
    }
}

fn within(value: f64, min: f64, max: f64) -> bool {
    value.is_finite() && (min..=max).contains(&value)
}

impl SynthPatch {
    /// Rejects invalid values rather than silently changing the requested patch.
    ///
    /// # Errors
    /// Returns an error for non-finite or out-of-range parameters.
    pub fn validate(&self) -> Result<(), &'static str> {
        for osc in self.oscillators {
            if !within(osc.coarse, -24.0, 24.0) || !within(osc.fine, -100.0, 100.0) || !within(osc.gain, 0.0, 1.0) {
                return Err("oscillator tuning or gain out of range");
            }
        }
        if !within(self.cutoff, 20.0, 20_000.0) || !within(self.q, 0.5, 4.0) {
            return Err("filter cutoff or Q out of range");
        }
        let env = self.envelope;
        if !within(env.attack, 0.0, 10.0)
            || !within(env.decay, 0.0, 10.0)
            || !within(env.sustain, 0.0, 1.0)
            || !within(env.release, 0.0, 10.0)
        {
            return Err("envelope out of range");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

/// One monophonic voice; the caller owns gate timing and retrigger retirement.
///
/// Construction validates the rate (8–192 kHz). Gate-on resets filter and phases.
/// Live tuning/gain/cutoff/Q targets use a 10 ms exponential time constant.
#[derive(Clone)]
pub struct SynthVoice {
    rate: f64,
    smoothing: f64,
    target: SynthPatch,
    current: SynthPatch,
    envelope: Envelope,
    waveforms: [Waveform; 3],
    phase: [f64; 3],
    frequency: f64,
    filter: [f64; 2],
    stage: Stage,
    level: f64,
    release_step: f64,
}

impl SynthVoice {
    /// Prepare a silent voice.
    ///
    /// # Errors
    /// Returns an error for an invalid patch or a rate outside 8–192 kHz.
    pub fn new(patch: SynthPatch, sample_rate: f64) -> Result<Self, &'static str> {
        patch.validate()?;
        if !within(sample_rate, 8000.0, 192_000.0) {
            return Err("sample rate must be between 8000 and 192000 Hz");
        }
        Ok(Self {
            rate: sample_rate,
            smoothing: -(-1.0 / (sample_rate * 0.01)).exp_m1(),
            target: patch,
            current: patch,
            envelope: patch.envelope,
            waveforms: patch.oscillators.map(|osc| osc.waveform),
            phase: [0.0; 3],
            frequency: 440.0,
            filter: [0.0; 2],
            stage: Stage::Done,
            level: 0.0,
            release_step: 0.0,
        })
    }

    /// Restart at the given frequency in Hz.
    ///
    /// # Errors
    /// Frequencies outside 1–24000 Hz (including non-finite values) are rejected.
    pub fn gate_on(&mut self, frequency: f64) -> Result<(), &'static str> {
        if !within(frequency, 1.0, 24_000.0) {
            return Err("note frequency out of range");
        }
        self.frequency = frequency;
        self.envelope = self.target.envelope;
        self.waveforms = self.target.oscillators.map(|osc| osc.waveform);
        self.phase = [0.0; 3];
        self.filter = [0.0; 2];
        self.level = 0.0;
        self.stage = Stage::Attack;
        Ok(())
    }

    /// Idempotent: repeated note-offs do not restart the release.
    pub fn gate_off(&mut self) {
        if matches!(self.stage, Stage::Release | Stage::Done) {
            return;
        }
        if self.level == 0.0 || self.envelope.release == 0.0 {
            self.level = 0.0;
            self.stage = Stage::Done;
        } else {
            self.release_step = self.level / (self.envelope.release * self.rate);
            self.stage = Stage::Release;
        }
    }

    /// Invalid updates leave both live and next-note settings unchanged.
    ///
    /// # Errors
    /// Returns an error for an invalid patch.
    pub fn apply_patch(&mut self, patch: SynthPatch) -> Result<(), &'static str> {
        patch.validate()?;
        self.target = patch;
        Ok(())
    }

    #[must_use]
    pub fn finished(&self) -> bool {
        self.stage == Stage::Done
    }

    fn envelope_sample(&mut self) -> f64 {
        loop {
            match self.stage {
                Stage::Attack if self.envelope.attack == 0.0 => {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
                Stage::Decay if self.envelope.decay == 0.0 => {
                    self.level = self.envelope.sustain;
                    self.stage = Stage::Sustain;
                }
                Stage::Attack => {
                    self.level = (self.level + 1.0 / (self.envelope.attack * self.rate)).min(1.0);
                    if self.level >= 1.0 {
                        self.stage = Stage::Decay;
                    }
                    break;
                }
                Stage::Decay => {
                    self.level = (self.level - (1.0 - self.envelope.sustain) / (self.envelope.decay * self.rate))
                        .max(self.envelope.sustain);
                    if self.level <= self.envelope.sustain {
                        self.stage = Stage::Sustain;
                    }
                    break;
                }
                Stage::Release => {
                    self.level = (self.level - self.release_step).max(0.0);
                    if self.level == 0.0 {
                        self.stage = Stage::Done;
                    }
                    break;
                }
                Stage::Sustain | Stage::Done => break,
            }
        }
        self.level
    }

    pub fn sample(&mut self) -> f64 {
        if self.finished() {
            return 0.0;
        }
        let amount = self.smoothing;
        self.current.cutoff += (self.target.cutoff - self.current.cutoff) * amount;
        self.current.q += (self.target.q - self.current.q) * amount;
        let mut mixed = 0.0;
        for i in 0..3 {
            let osc = &mut self.current.oscillators[i];
            let target = self.target.oscillators[i];
            osc.coarse += (target.coarse - osc.coarse) * amount;
            osc.fine += (target.fine - osc.fine) * amount;
            osc.gain += (target.gain - osc.gain) * amount;
            let frequency =
                (self.frequency * 2.0_f64.powf((osc.coarse + osc.fine / 100.0) / 12.0)).min(self.rate * 0.45);
            let dt = frequency / self.rate;
            let phase = self.phase[i];
            let value = match self.waveforms[i] {
                Waveform::Sine => (phase * TAU).sin(),
                Waveform::Saw => 2.0 * phase - 1.0 - poly_blep(phase, dt),
                Waveform::Square => {
                    (if phase < 0.5 { 1.0 } else { -1.0 }) + poly_blep(phase, dt) - poly_blep((phase + 0.5).fract(), dt)
                }
                Waveform::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            };
            mixed += value * osc.gain / 3.0;
            self.phase[i] = (phase + dt).fract();
        }
        // Topology-preserving state-variable filter (trapezoidal integrators).
        let g = (PI * self.current.cutoff.min(self.rate * 0.45) / self.rate).tan();
        let k = 1.0 / self.current.q;
        let a = 1.0 / (1.0 + g * (g + k));
        let v3 = mixed - self.filter[1];
        let v1 = a * self.filter[0] + g * a * v3;
        let v2 = self.filter[1] + g * self.filter[0] * a + g * g * a * v3;
        self.filter = [2.0 * v1 - self.filter[0], 2.0 * v2 - self.filter[1]];
        v2 * self.envelope_sample() * 0.25
    }

    pub fn render(&mut self, output: &mut [f64]) {
        for sample in output {
            *sample = self.sample();
        }
    }
}

fn poly_blep(phase: f64, dt: f64) -> f64 {
    if phase < dt {
        let t = phase / dt;
        2.0 * t - t * t - 1.0
    } else if phase > 1.0 - dt {
        let t = (phase - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_and_mix_through_rendered_output() {
        for rate in [44_100, 48_000, 96_000] {
            let mut patch = SynthPatch {
                envelope: Envelope {
                    attack: 0.0,
                    decay: 0.0,
                    sustain: 1.0,
                    release: 0.1,
                },
                ..SynthPatch::default()
            };
            patch.oscillators[0].waveform = Waveform::Sine;
            patch.oscillators[0].coarse = 12.0;
            let mut one = SynthVoice::new(patch, f64::from(rate)).unwrap();
            patch.oscillators[1] = patch.oscillators[0];
            patch.oscillators[2] = patch.oscillators[0];
            let mut three = SynthVoice::new(patch, f64::from(rate)).unwrap();
            one.gate_on(220.0).unwrap();
            three.gate_on(220.0).unwrap();
            let mut crossings = 0;
            let mut previous = 0.0;
            for _ in 0..rate {
                let sample = one.sample();
                let summed = three.sample();
                assert!((summed - sample * 3.0).abs() < 1e-10);
                if previous < 0.0 && sample >= 0.0 {
                    crossings += 1;
                }
                previous = sample;
            }
            assert!((439..=441).contains(&crossings), "{rate}: {crossings}");
        }
    }

    #[test]
    fn low_pass_attenuates_high_notes() {
        let energy = |cutoff| {
            let mut patch = SynthPatch {
                cutoff,
                ..SynthPatch::default()
            };
            patch.oscillators[0].waveform = Waveform::Sine;
            let mut voice = SynthVoice::new(patch, 48_000.0).unwrap();
            voice.gate_on(4000.0).unwrap();
            let mut sum = 0.0;
            for i in 0..24_000 {
                let sample = voice.sample();
                if i > 12_000 {
                    sum += sample * sample;
                }
            }
            sum
        };
        assert!(energy(200.0) < energy(10_000.0) * 0.001);
    }

    #[test]
    fn decay_sustain_and_live_release_targets() {
        for rate in [44_100, 48_000, 96_000] {
            let patch = SynthPatch::default();
            let mut voice = SynthVoice::new(patch, f64::from(rate)).unwrap();
            voice.gate_on(220.0).unwrap();
            for _ in 0..rate / 2 {
                voice.sample();
            }
            assert!((voice.level - patch.envelope.sustain).abs() < 1e-10);
            voice.gate_off();
            let mut updated = patch;
            updated.cutoff = 20.0;
            updated.q = 4.0;
            updated.oscillators[0].gain = 0.0;
            updated.envelope.release = 10.0;
            voice.apply_patch(updated).unwrap();
            for _ in 0..rate / 10 {
                assert!(voice.sample().is_finite());
            }
            assert!(voice.current.oscillators[0].gain < 0.001);
            assert!((voice.level - 0.35).abs() < 1e-8);
            for _ in 0..rate / 10 + 2 {
                voice.sample();
            }
            assert!(voice.finished());
        }
    }

    #[test]
    fn invalid_construction_and_note_are_atomic() {
        let patch = SynthPatch::default();
        for rate in [0.0, 7999.0, 192_001.0, f64::NAN, f64::INFINITY] {
            assert!(SynthVoice::new(patch, rate).is_err());
        }
        let mut voice = SynthVoice::new(patch, 48_000.0).unwrap();
        assert!(voice.gate_on(f64::NAN).is_err());
        assert!(voice.finished());
        for invalid in [f64::NAN, f64::INFINITY, -1.0, 11.0] {
            let mut bad = patch;
            bad.envelope.attack = invalid;
            assert!(SynthVoice::new(bad, 48_000.0).is_err());
        }
    }

    #[test]
    fn envelope_timing_and_early_release() {
        for rate in [44_100, 48_000, 96_000] {
            let mut voice = SynthVoice::new(SynthPatch::default(), f64::from(rate)).unwrap();
            voice.gate_on(440.0).unwrap();
            for _ in 0..rate / 200 {
                voice.sample();
            }
            assert!((voice.level - 0.5).abs() < 0.003);
            voice.gate_off();
            for _ in 0..rate / 10 {
                voice.sample();
            }
            assert!((voice.level - 0.25).abs() < 0.003);
            voice.gate_off();
            for _ in 0..rate / 10 + 2 {
                voice.sample();
            }
            assert!(voice.finished());
            assert_eq!(voice.sample(), 0.0);
        }
    }

    #[test]
    fn zero_gate_and_zero_stages() {
        let patch = SynthPatch {
            envelope: Envelope {
                attack: 0.0,
                decay: 0.0,
                sustain: 0.6,
                release: 0.0,
            },
            ..SynthPatch::default()
        };
        let mut voice = SynthVoice::new(patch, 48_000.0).unwrap();
        voice.gate_on(440.0).unwrap();
        voice.gate_off();
        assert_eq!(voice.sample(), 0.0);
        voice.gate_on(440.0).unwrap();
        voice.sample();
        assert_eq!(voice.level, 0.6);
        voice.gate_off();
        assert!(voice.finished());
    }

    #[test]
    fn extremes_remain_finite_and_bounded() {
        for rate in [8000, 12_000, 48_000, 96_000] {
            for waveform in [Waveform::Sine, Waveform::Saw, Waveform::Square, Waveform::Triangle] {
                for cutoff in [20.0, 20_000.0] {
                    let patch = SynthPatch {
                        cutoff,
                        q: 4.0,
                        oscillators: [Oscillator {
                            waveform,
                            coarse: 24.0,
                            fine: 100.0,
                            gain: 1.0,
                        }; 3],
                        ..SynthPatch::default()
                    };
                    let mut voice = SynthVoice::new(patch, f64::from(rate)).unwrap();
                    voice.gate_on(24_000.0).unwrap();
                    for _ in 0..rate {
                        let sample = voice.sample();
                        assert!(
                            sample.is_finite() && sample.abs() < 4.0,
                            "{rate} {waveform:?}: {sample}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn updates_are_atomic_and_block_size_independent() {
        let patch = SynthPatch::default();
        let mut a = SynthVoice::new(patch, 48_000.0).unwrap();
        let mut b = SynthVoice::new(patch, 48_000.0).unwrap();
        a.gate_on(220.0).unwrap();
        b.gate_on(220.0).unwrap();
        let mut updated = patch;
        updated.cutoff = 300.0;
        updated.oscillators[1].gain = 0.5;
        updated.oscillators[1].fine = 12.0;
        a.apply_patch(updated).unwrap();
        b.apply_patch(updated).unwrap();
        let mut invalid = updated;
        invalid.q = f64::NAN;
        assert!(a.apply_patch(invalid).is_err());
        let mut whole = [0.0; 2048];
        let mut chunks = [0.0; 2048];
        a.render(&mut whole);
        for chunk in chunks.chunks_mut(31) {
            b.render(chunk);
        }
        assert_eq!(whole, chunks);
        assert_eq!(a.target, updated);
        assert!((a.current.cutoff - updated.cutoff).abs() < 60.0);
    }

    #[test]
    fn next_note_settings_and_retrigger_are_reproducible() {
        let patch = SynthPatch::default();
        let mut voice = SynthVoice::new(patch, 48_000.0).unwrap();
        voice.gate_on(440.0).unwrap();
        let mut first = [0.0; 512];
        voice.render(&mut first);
        voice.gate_off();
        voice.gate_on(440.0).unwrap();
        let mut second = [0.0; 512];
        voice.render(&mut second);
        assert_eq!(first, second);
        let mut edited = patch;
        edited.envelope.attack = 1.0;
        edited.oscillators[0].waveform = Waveform::Square;
        voice.apply_patch(edited).unwrap();
        assert_eq!(voice.envelope, patch.envelope);
        assert_eq!(voice.waveforms[0], Waveform::Saw);
        voice.gate_on(440.0).unwrap();
        assert_eq!(voice.envelope, edited.envelope);
        assert_eq!(voice.waveforms[0], Waveform::Square);
    }
}
