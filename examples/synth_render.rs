//! Render plan 020's standalone voice without opening an audio device.
use std::io::{BufWriter, Write};

use track_rs::synth::{SynthPatch, SynthVoice, Waveform};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = if let Some(destination) = std::env::args().nth(1) {
        // create_new avoids overwriting an existing recording.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?
    } else {
        eprintln!("No filename supplied; rendering to an anonymous temporary file.");
        tempfile::tempfile()?
    };
    let mut output = BufWriter::new(file);
    let rate = 48_000_u32;
    let samples = rate * 3;
    let bytes = samples * 2;
    output.write_all(b"RIFF")?;
    output.write_all(&(36 + bytes).to_le_bytes())?;
    output.write_all(b"WAVEfmt ")?;
    output.write_all(&16_u32.to_le_bytes())?;
    output.write_all(&1_u16.to_le_bytes())?; // PCM
    output.write_all(&1_u16.to_le_bytes())?; // mono
    output.write_all(&rate.to_le_bytes())?;
    output.write_all(&(rate * 2).to_le_bytes())?;
    output.write_all(&2_u16.to_le_bytes())?;
    output.write_all(&16_u16.to_le_bytes())?;
    output.write_all(b"data")?;
    output.write_all(&bytes.to_le_bytes())?;

    let mut patch = SynthPatch::default();
    patch.oscillators[1].gain = 0.45;
    patch.oscillators[1].fine = -8.0;
    patch.oscillators[2].waveform = Waveform::Square;
    patch.oscillators[2].coarse = -12.0;
    patch.oscillators[2].gain = 0.3;
    patch.envelope.release = 0.8;
    let mut voice = SynthVoice::new(patch, f64::from(rate))?;
    voice.gate_on(220.0)?;
    let mut clipped = 0;
    for frame in 0..samples {
        if frame == rate {
            patch.cutoff = 500.0;
            patch.q = 2.0;
            voice.apply_patch(patch)?;
        }
        if frame == rate * 2 {
            voice.gate_off();
        }
        let sample = voice.sample();
        if sample.abs() > 1.0 {
            clipped += 1;
        }
        #[allow(clippy::cast_possible_truncation)]
        let pcm = (sample.clamp(-1.0, 1.0) * f64::from(i16::MAX)).round() as i16;
        output.write_all(&pcm.to_le_bytes())?;
    }
    output.flush()?;
    eprintln!("Rendered 3 seconds at 48 kHz; {clipped} clipped PCM samples.");
    Ok(())
}
