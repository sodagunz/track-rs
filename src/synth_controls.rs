//! Concrete F4 controls. Patch values stay independent of terminal/editor state.
use crate::synth::{SynthPatch, Waveform};

#[derive(Clone, Copy, Debug)]
pub enum Control {
    Wave(usize),
    Coarse(usize),
    Fine(usize),
    Gain(usize),
    Cutoff,
    Q,
    Attack,
    Decay,
    Sustain,
    Release,
}

pub const CONTROLS: [Control; 18] = [
    Control::Wave(0),
    Control::Coarse(0),
    Control::Fine(0),
    Control::Gain(0),
    Control::Wave(1),
    Control::Coarse(1),
    Control::Fine(1),
    Control::Gain(1),
    Control::Wave(2),
    Control::Coarse(2),
    Control::Fine(2),
    Control::Gain(2),
    Control::Cutoff,
    Control::Q,
    Control::Attack,
    Control::Decay,
    Control::Sustain,
    Control::Release,
];
const WAVES: [Waveform; 4] = [Waveform::Sine, Waveform::Saw, Waveform::Square, Waveform::Triangle];

impl Control {
    pub(crate) const fn group(self) -> &'static str {
        match self {
            Self::Wave(i) | Self::Coarse(i) | Self::Fine(i) | Self::Gain(i) => ["OSC 1", "OSC 2", "OSC 3"][i],
            Self::Cutoff | Self::Q => "FILTER",
            _ => "AMP",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Wave(_) => "Waveform",
            Self::Coarse(_) => "Coarse",
            Self::Fine(_) => "Fine",
            Self::Gain(_) => "Gain",
            Self::Cutoff => "Cutoff",
            Self::Q => "Q",
            Self::Attack => "Attack",
            Self::Decay => "Decay",
            Self::Sustain => "Sustain",
            Self::Release => "Release",
        }
    }

    pub(crate) const fn policy(self) -> &'static str {
        match self {
            Self::Wave(_) | Self::Attack | Self::Decay | Self::Sustain | Self::Release => "next note",
            _ => "live 10ms",
        }
    }

    // Minimum, maximum, keyboard step, display unit. Gain/sustain are linear, not dB.
    const fn numeric(self) -> (f64, f64, f64, &'static str) {
        match self {
            Self::Coarse(_) => (-24.0, 24.0, 1.0, "st"),
            Self::Fine(_) => (-100.0, 100.0, 1.0, "ct"),
            Self::Gain(_) | Self::Sustain => (0.0, 1.0, 0.01, ""),
            Self::Cutoff => (20.0, 20_000.0, 100.0, "Hz"),
            Self::Q => (0.5, 4.0, 0.05, ""),
            _ => (0.0, 10.0, 0.01, "s"),
        }
    }

    const fn value_mut(self, patch: &mut SynthPatch) -> Option<&mut f64> {
        Some(match self {
            Self::Wave(_) => return None,
            Self::Coarse(i) => &mut patch.oscillators[i].coarse,
            Self::Fine(i) => &mut patch.oscillators[i].fine,
            Self::Gain(i) => &mut patch.oscillators[i].gain,
            Self::Cutoff => &mut patch.cutoff,
            Self::Q => &mut patch.q,
            Self::Attack => &mut patch.envelope.attack,
            Self::Decay => &mut patch.envelope.decay,
            Self::Sustain => &mut patch.envelope.sustain,
            Self::Release => &mut patch.envelope.release,
        })
    }

    pub(crate) fn value(self, mut patch: SynthPatch) -> String {
        if let Self::Wave(i) = self {
            return format!("{:?}", patch.oscillators[i].waveform);
        }
        let value = *self.value_mut(&mut patch).expect("numeric control");
        let unit = self.numeric().3;
        // Keep small exact edits visible instead of rounding them to zero.
        format!("{value}{}{}", if unit.is_empty() { "" } else { " " }, unit)
    }

    pub(crate) fn hint(self) -> String {
        if matches!(self, Self::Wave(_)) {
            return "sine / saw / square / triangle".into();
        }
        let (min, max, _, unit) = self.numeric();
        let extra = match self {
            Self::Attack | Self::Decay | Self::Release => " (ms allowed)",
            Self::Cutoff => " (kHz allowed)",
            Self::Gain(_) | Self::Sustain => " linear",
            _ => "",
        };
        format!("{min}..{max}{}{unit}{extra}", if unit.is_empty() { "" } else { " " })
    }

    pub(crate) fn adjusted(self, mut patch: SynthPatch, steps: i32) -> SynthPatch {
        if let Self::Wave(i) = self {
            let index = WAVES
                .iter()
                .position(|wave| *wave == patch.oscillators[i].waveform)
                .unwrap_or(0);
            let index = (i64::try_from(index).unwrap_or(0) + i64::from(steps)).rem_euclid(4);
            patch.oscillators[i].waveform = WAVES[usize::try_from(index).unwrap_or(0)];
        } else {
            let (min, max, step, _) = self.numeric();
            let value = self.value_mut(&mut patch).expect("numeric control");
            // Quantize keyboard steps to their decimal precision, not a long float tail.
            *value = (f64::from(steps.clamp(-9999, 9999)).mul_add(step, *value) * 1_000_000.0)
                .round()
                .clamp(min * 1_000_000.0, max * 1_000_000.0)
                / 1_000_000.0;
        }
        patch
    }

    pub(crate) fn reset(self, mut patch: SynthPatch) -> SynthPatch {
        let mut default = SynthPatch::default();
        if let Self::Wave(i) = self {
            patch.oscillators[i].waveform = default.oscillators[i].waveform;
        } else {
            *self.value_mut(&mut patch).expect("numeric control") =
                *self.value_mut(&mut default).expect("numeric control");
        }
        patch
    }

    pub(crate) fn entered(self, mut patch: SynthPatch, text: &str) -> anyhow::Result<SynthPatch> {
        let text = text.trim().to_ascii_lowercase();
        if let Self::Wave(i) = self {
            patch.oscillators[i].waveform = match text.as_str() {
                "sine" => Waveform::Sine,
                "saw" => Waveform::Saw,
                "square" => Waveform::Square,
                "triangle" => Waveform::Triangle,
                _ => anyhow::bail!("Expected {}", self.hint()),
            };
        } else {
            let (_, _, _, unit) = self.numeric();
            let (number, scale) = if matches!(self, Self::Attack | Self::Decay | Self::Release) && text.ends_with("ms")
            {
                (text.strip_suffix("ms").expect("matched suffix"), 0.001)
            } else if matches!(self, Self::Cutoff) && text.ends_with("khz") {
                (text.strip_suffix("khz").expect("matched suffix"), 1000.0)
            } else if !unit.is_empty() && text.ends_with(&unit.to_ascii_lowercase()) {
                (
                    text.strip_suffix(&unit.to_ascii_lowercase()).expect("matched suffix"),
                    1.0,
                )
            } else {
                (text.as_str(), 1.0)
            };
            let value = number
                .trim()
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("Expected {}", self.hint()))?
                * scale;
            let (min, max, _, _) = self.numeric();
            anyhow::ensure!(
                value.is_finite() && (min..=max).contains(&value),
                "Expected {}",
                self.hint()
            );
            *self.value_mut(&mut patch).expect("numeric control") = value;
        }
        patch.validate().map_err(anyhow::Error::msg)?;
        Ok(patch)
    }
}
