//! `FunDSP` adapter for legacy sequencer callers; tracker voices use `SynthVoice` directly.
use fundsp::prelude32::{AudioNode, Frame, U0, U2};

use crate::synth::{SynthPatch, SynthVoice};

#[derive(Clone)]
pub(super) struct SynthNode {
    voice: Option<SynthVoice>,
    frequency: f64,
    rate: f64,
    duration: Option<f64>,
    elapsed: f64,
}

impl SynthNode {
    pub(super) fn new(frequency: f64, rate: f64, duration: Option<f64>) -> Self {
        let mut node = Self {
            voice: None,
            frequency,
            rate,
            duration,
            elapsed: 0.0,
        };
        node.reset();
        node
    }
}

impl AudioNode for SynthNode {
    const ID: u64 = 0x7472_6163_6b73_796e;
    type Inputs = U0;
    type Outputs = U2;

    fn reset(&mut self) {
        self.voice = SynthVoice::new(SynthPatch::default(), self.rate)
            .ok()
            .and_then(|mut voice| {
                voice.gate_on(self.frequency).ok()?;
                Some(voice)
            });
        self.elapsed = 0.0;
    }

    #[allow(clippy::float_cmp)] // An identical rate must not reset a running voice.
    fn set_sample_rate(&mut self, rate: f64) {
        if self.rate != rate {
            self.rate = rate;
            self.reset();
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn tick(&mut self, _: &Frame<f32, U0>) -> Frame<f32, U2> {
        let sample = self.voice.as_mut().map_or(0.0, |voice| {
            if self.duration.is_some_and(|duration| self.elapsed >= duration) {
                voice.gate_off();
            }
            voice.sample() as f32
        });
        self.elapsed += self.rate.recip();
        [sample, sample].into()
    }
}
