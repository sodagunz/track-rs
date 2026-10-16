#![allow(clippy::precedence)]

use crate::{
    tracker::{Instrument, Note},
    transport::NoteEvent,
};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

pub const DEFAULT_VOICE_GAIN: f32 = 0.5;
pub const DEFAULT_PLUCK_DECAY: f32 = 0.5;
pub const DEFAULT_PLUCK_DAMPING: f32 = 0.5;
pub const DEFAULT_NOTE_DURATION: Duration = Duration::from_millis(200);
pub const DEFAULT_FADE_IN: f64 = 0.02;
pub const DEFAULT_FADE_OUT: f64 = 0.2;
pub const DEFAULT_CHORUS_AMOUNT: f32 = 1.0;
const PERCUSSION_ATTACK: f64 = 0.001;
const KICK_GAIN: f32 = 0.36;
const SNARE_NOISE_GAIN: f32 = 0.3;
const SNARE_TONE_GAIN: f32 = 0.14;
const HIHAT_GAIN: f32 = 0.3;

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, FromSample, SizedSample, Stream, SupportedStreamConfig,
};
use fundsp::{
    math::{cos_hz, xerp11, AttoHash},
    prelude32::{
        chorus, dcblock, highpass_hz, lfo, lowpass_hz, moog, noise, pan, pass, pluck, saw_hz, sine, sine_hz, square_hz,
        zero, AudioUnit, BlockRateAdapter, Fade, Net, ReplayMode, Sequencer,
    },
    realseq::SequencerBackend,
};

type Renderer = BlockRateAdapter;

struct Output {
    renderer: Renderer,
    replacements: Receiver<Renderer>,
    retired: Sender<Renderer>,
}

impl Output {
    fn sample(&mut self) -> (f32, f32) {
        while let Ok(renderer) = self.replacements.try_recv() {
            let old = std::mem::replace(&mut self.renderer, renderer);
            let _ = self.retired.send(old);
        }
        self.renderer.get_stereo()
    }
}

fn prepare(sample_rate: f64) -> (Sequencer, Renderer) {
    let mut sequencer = Sequencer::new(0, 2, ReplayMode::None);
    let mut net = build_net(sample_rate, sequencer.backend());
    (sequencer, BlockRateAdapter::new(Box::new(net.backend())))
}

fn replace_graph(sequencer: &mut Sequencer, replacements: &Sender<Renderer>, sample_rate: f64) {
    let (next, renderer) = prepare(sample_rate);
    *sequencer = next;
    let _ = replacements.send(renderer);
}

pub struct AudioEngine {
    sequencer: Sequencer,
    replacements: Sender<Renderer>,
    retired: Receiver<Renderer>,
    sample_rate: f64,
    dry: bool,
    // Keep the output stream alive for as long as the engine.
    _stream: Stream,
}

impl AudioEngine {
    /// Create and start the default audio output stream.
    ///
    /// # Errors
    /// Returns an error if building or starting the output stream fails.
    ///
    /// # Panics
    /// Panics if no default output device or configuration is available, or if
    /// the device uses a sample format other than F32, I16, or U16.
    pub fn new() -> anyhow::Result<Self> {
        Self::with_dry_output(false)
    }

    /// Create an output stream, optionally bypassing chorus.
    ///
    /// # Errors
    /// Returns an error if building or starting the stream fails.
    ///
    /// # Panics
    /// Panics if the default device/configuration or sample format is unsupported.
    pub fn with_dry_output(dry: bool) -> anyhow::Result<Self> {
        let host = cpal::default_host();

        let device = host
            .default_output_device()
            .expect("failed to find a default output device");
        let config = device.default_output_config().expect("no default output config");

        let sample_rate = f64::from(config.sample_rate());

        let (sequencer, renderer) = prepare(sample_rate);
        let (replacements, receiver) = mpsc::channel();
        let (retired_sender, retired) = mpsc::channel();
        let output = Output {
            renderer,
            replacements: receiver,
            retired: retired_sender,
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build_stream::<f32>(config, &device, output)?,
            cpal::SampleFormat::I16 => build_stream::<i16>(config, &device, output)?,
            cpal::SampleFormat::U16 => build_stream::<u16>(config, &device, output)?,
            _ => panic!("Unsupported sample format"),
        };

        stream.play()?;
        Ok(Self {
            sequencer,
            replacements,
            retired,
            sample_rate,
            dry,
            _stream: stream,
        })
    }

    pub fn play(&mut self, note: Note) {
        self.collect_retired();
        schedule_instrument(
            &mut self.sequencer,
            note,
            Instrument::Pluck,
            DEFAULT_NOTE_DURATION,
            self.dry,
        );
    }
    pub fn play_event(&mut self, event: NoteEvent) {
        self.collect_retired();
        schedule_instrument(
            &mut self.sequencer,
            event.note,
            event.instrument,
            event.duration,
            self.dry,
        );
    }

    /// Replace the entire prepared graph, cancelling voices and effect tails.
    /// The callback switches before its next sample; retired graphs are freed here.
    pub fn stop(&mut self) {
        self.collect_retired();
        replace_graph(&mut self.sequencer, &self.replacements, self.sample_rate);
    }

    fn collect_retired(&self) {
        while self.retired.try_recv().is_ok() {}
    }
}

#[cfg(test)]
fn schedule_note(sequencer: &mut Sequencer, note: Note, duration: Duration) {
    schedule_instrument(sequencer, note, Instrument::Pluck, duration, false);
}

fn schedule_instrument(sequencer: &mut Sequencer, note: Note, instrument: Instrument, duration: Duration, dry: bool) {
    schedule_voice(sequencer, note, instrument, duration, dry, rand::random());
}

fn schedule_voice(
    sequencer: &mut Sequencer,
    note: Note,
    instrument: Instrument,
    duration: Duration,
    dry: bool,
    seed: u64,
) {
    let seconds = duration.as_secs_f64();
    let attack = match instrument {
        // Preserve the drum transient instead of fading through most of it.
        Instrument::Kick | Instrument::Snare | Instrument::HiHat => PERCUSSION_ATTACK,
        _ => DEFAULT_FADE_IN,
    };
    if seconds > 0.0 {
        sequencer.push_relative(
            0.0,
            seconds,
            Fade::Smooth,
            attack.min(seconds * 0.25),
            DEFAULT_FADE_OUT.min(seconds * 0.25),
            voice(note, instrument, dry, seed),
        );
    }
}

/// Construct a seeded stereo voice. The sequencer supplies the gate's attack/release.
/// Kick sweeps 150 to 45 Hz; snare uses 180 Hz plus noise; hi-hat uses high-passed
/// noise. All percussion ignores note pitch. Only Pluck uses the legacy chorus.
fn voice(note: Note, instrument: Instrument, dry: bool, seed: u64) -> Box<dyn AudioUnit> {
    #[allow(clippy::cast_possible_truncation)]
    let hz = note.freq() as f32;
    let waveform = match instrument {
        Instrument::Pluck => {
            let waveform = Net::wrap(Box::new(
                zero() >> pluck(hz, DEFAULT_PLUCK_DECAY, DEFAULT_PLUCK_DAMPING) * DEFAULT_VOICE_GAIN,
            ));
            let filter = Net::wrap(Box::new(
                (pass() | lfo(move |t| (xerp11(400.0, 10000.0, cos_hz(0.1, t)), 0.6))) >> moog(),
            ));
            waveform >> filter >> dcblock()
        }
        Instrument::Sine => Net::wrap(Box::new(sine_hz(hz) * 0.18)),
        Instrument::Square => Net::wrap(Box::new(square_hz(hz) * 0.12)),
        Instrument::Saw => Net::wrap(Box::new(saw_hz(hz) * 0.16)),
        Instrument::Kick => Net::wrap(Box::new(
            (lfo(|t: f32| 105.0_f32.mul_add((-t * 35.0).exp(), 45.0)) >> sine())
                * lfo(|t: f32| KICK_GAIN * (-t * 16.0).exp()),
        )),
        Instrument::Snare => Net::wrap(Box::new(
            ((noise() >> highpass_hz(1200.0, 0.7) >> lowpass_hz(10000.0, 0.7)) * SNARE_NOISE_GAIN
                + sine_hz(180.0) * SNARE_TONE_GAIN)
                * lfo(|t: f32| (-t * 24.0).exp()),
        )),
        Instrument::HiHat => Net::wrap(Box::new(
            (noise() >> highpass_hz(7000.0, 0.7)) * lfo(|t: f32| HIHAT_GAIN * (-t * 65.0).exp()),
        )),
    };
    let mut stereo = waveform >> pan(0.0);
    if instrument == Instrument::Pluck && !dry {
        stereo = stereo >> (chorus(0, 0.0, 0.03, 0.2) | chorus(1, 0.0, 0.03, 0.2)) * (2.0 * DEFAULT_CHORUS_AMOUNT);
    }
    let mut voice: Box<dyn AudioUnit> = Box::new(stereo);
    voice.ping(false, AttoHash::new(seed));
    voice
}

fn build_stream<T>(config: SupportedStreamConfig, device: &Device, mut output: Output) -> anyhow::Result<Stream>
where
    T: SizedSample + cpal::FromSample<f64>,
{
    let mut next_value = move || output.sample();
    let err_fn = |err| eprintln!("an error occurred on stream: {err}");

    let channels = usize::from(config.channels());
    let stream = device.build_output_stream(
        config.into(),
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| write_data(data, channels, &mut next_value),
        err_fn,
        None,
    )?;

    Ok(stream)
}

fn write_data<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> (f32, f32))
where
    T: SizedSample + FromSample<f64>,
{
    for frame in output.chunks_mut(channels) {
        let sample = next_sample();
        let left: T = T::from_sample(f64::from(sample.0));
        let right: T = T::from_sample(f64::from(sample.1));

        for (channel, sample) in frame.iter_mut().enumerate() {
            if channel & 1 == 0 {
                *sample = left;
            } else {
                *sample = right;
            }
        }
    }
}

fn build_net(sample_rate: f64, sequencer_backend: SequencerBackend) -> Net {
    let mut net = Net::wrap(Box::new(sequencer_backend));
    net.set_sample_rate(sample_rate);
    net
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::NotePitch;

    fn render_presets(instruments: &[Instrument], dry: bool, duration: Duration) -> Vec<(f32, f32)> {
        let (mut seq, mut renderer) = prepare(48_000.0);
        for instrument in instruments {
            schedule_voice(
                &mut seq,
                Note {
                    pitch: NotePitch::C,
                    octave: 4,
                },
                *instrument,
                duration,
                dry,
                42,
            );
        }
        (0..48_000).map(|_| renderer.get_stereo()).collect()
    }

    #[test]
    fn percussion_has_an_audible_attack() {
        let mut attacks = Vec::new();
        for instrument in [Instrument::Kick, Instrument::Snare, Instrument::HiHat] {
            let samples = render_presets(&[instrument], true, Duration::from_millis(125));
            let rms = |samples: &[(f32, f32)]| {
                (samples.iter().map(|(l, r)| f64::from(l * l + r * r) / 2.0).sum::<f64>()
                    / f64::from(u32::try_from(samples.len()).unwrap()))
                .sqrt()
            };
            let attack = rms(&samples[..240]);
            let body = rms(&samples[..6000]);
            let peak = samples
                .iter()
                .map(|(l, r)| l.abs().max(r.abs()))
                .fold(0.0_f32, f32::max);
            assert!((0.01..0.15).contains(&body), "{instrument:?} gate RMS: {body}");
            assert!((0.1..0.35).contains(&peak), "{instrument:?} peak: {peak}");
            attacks.push((instrument, attack));
        }
        assert!(
            attacks.iter().all(|(_, attack)| *attack >= 0.03),
            "quiet attacks: {attacks:?}"
        );
        let quietest = attacks.iter().map(|(_, rms)| *rms).fold(f64::INFINITY, f64::min);
        let loudest = attacks.iter().map(|(_, rms)| *rms).fold(0.0_f64, f64::max);
        // Compare onsets, not whole gates: the hi-hat intentionally decays faster.
        assert!(loudest / quietest < 3.0, "unbalanced attacks: {attacks:?}");
    }

    #[test]
    fn every_preset_is_deterministic_finite_audible_and_gated() {
        for instrument in Instrument::ALL {
            for dry in [false, true] {
                for millis in [50, 125, 750] {
                    let duration = Duration::from_millis(millis);
                    let samples = render_presets(&[instrument], dry, duration);
                    assert_eq!(samples, render_presets(&[instrument], dry, duration));
                    assert!(samples.iter().all(|(l, r)| l.is_finite() && r.is_finite()));
                    assert!(samples.iter().any(|(l, r)| l.abs() + r.abs() > 1e-4), "{instrument:?}");
                    let end = usize::try_from(millis * 48).unwrap();
                    assert!(
                        samples[end..].iter().all(|sample| *sample == (0.0, 0.0)),
                        "{instrument:?}"
                    );
                    let (l, r) = samples[end - 1];
                    assert!(l.abs() + r.abs() < 0.001, "{instrument:?}: {l}, {r}");
                    assert_eq!(samples[0], (0.0, 0.0));
                }
            }
        }
    }

    #[test]
    fn mixed_polyphony_is_finite_bounded_and_ends_cleanly() {
        let samples = render_presets(&Instrument::ALL, false, Duration::from_millis(125));
        assert!(samples
            .iter()
            .all(|(l, r)| l.is_finite() && r.is_finite() && l.abs() < 1.0 && r.abs() < 1.0));
        assert!(samples.iter().any(|(l, r)| l.abs() + r.abs() > 0.01));
        assert!(samples[6000..].iter().all(|sample| *sample == (0.0, 0.0)));
    }

    #[test]
    fn percussion_ignores_pitch_and_new_presets_bypass_chorus() {
        for instrument in Instrument::ALL {
            if instrument != Instrument::Pluck {
                assert_eq!(
                    render_presets(&[instrument], false, DEFAULT_NOTE_DURATION),
                    render_presets(&[instrument], true, DEFAULT_NOTE_DURATION)
                );
            }
        }
        for instrument in [Instrument::Kick, Instrument::Snare, Instrument::HiHat] {
            let mut low = voice(
                Note {
                    pitch: NotePitch::C,
                    octave: 0,
                },
                instrument,
                true,
                42,
            );
            let mut high = voice(
                Note {
                    pitch: NotePitch::B,
                    octave: 9,
                },
                instrument,
                true,
                42,
            );
            low.set_sample_rate(48_000.0);
            high.set_sample_rate(48_000.0);
            for _ in 0..10_000 {
                assert_eq!(low.get_stereo(), high.get_stereo());
            }
        }
    }

    #[test]
    fn stop_cancels_voices_tails_and_queued_notes_without_resurrection() {
        for dry in [false, true] {
            let (mut seq, renderer) = prepare(48_000.0);
            let (tx, replacements) = mpsc::channel();
            let (retired, rx) = mpsc::channel();
            let mut output = Output {
                renderer,
                replacements,
                retired,
            };
            let note = Note {
                pitch: NotePitch::C,
                octave: 4,
            };
            for instrument in Instrument::ALL {
                schedule_instrument(&mut seq, note, instrument, Duration::from_secs(2), dry);
            }
            let mut heard = false;
            for _ in 0..10_000 {
                let (l, r) = output.sample();
                assert!(l.is_finite() && r.is_finite());
                heard |= l.abs() + r.abs() > 1e-6;
            }
            assert!(heard);
            for instrument in Instrument::ALL {
                schedule_instrument(&mut seq, note, instrument, Duration::from_secs(2), dry);
            }
            let (mut next, renderer) = prepare(48_000.0);
            tx.send(renderer).unwrap();
            for _ in 0..100_000 {
                assert_eq!(output.sample(), (0.0, 0.0));
            }
            drop(rx.try_recv().unwrap());
            schedule_note(&mut next, note, DEFAULT_NOTE_DURATION);
            assert!((0..10_000).any(|_| {
                let (l, r) = output.sample();
                l.abs() + r.abs() > 1e-6
            }));
            let (_, renderer) = prepare(48_000.0);
            tx.send(renderer).unwrap();
            for _ in 0..100_000 {
                assert_eq!(output.sample(), (0.0, 0.0));
            }
        }
    }

    #[test]
    fn runtime_restart_cancels_old_audio_before_new_notes_and_stopped_time_does_not_seek() {
        use crate::{bindings::handle_key, state::App, transport::Transport};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (mut sequencer, renderer) = prepare(48_000.0);
        let (tx, replacements) = mpsc::channel();
        let (retired, _rx) = mpsc::channel();
        let mut output = Output {
            renderer,
            replacements,
            retired,
        };
        let mut app = App::default();
        let note = Note {
            pitch: NotePitch::C,
            octave: 4,
        };
        app.project.patterns[0].set_step(0, 8, Some(note)).unwrap();
        // A long old gate must not be reconstructed when starting later.
        schedule_instrument(&mut sequencer, note, Instrument::Pluck, Duration::from_secs(10), false);
        assert!((0..10_000).any(|_| {
            let (l, r) = output.sample();
            l.abs() + r.abs() > 1e-6
        }));
        let mut transport = Transport::default();
        let now = std::time::Instant::now();
        handle_key(KeyEvent::new(KeyCode::Null, KeyModifiers::CONTROL), &mut app);
        let notes = transport.update_app(now, &mut app, || replace_graph(&mut sequencer, &tx, 48_000.0));
        assert!(notes.is_empty());
        for _ in 0..20_000 {
            assert_eq!(output.sample(), (0.0, 0.0));
        }
        handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE), &mut app);
        app.cursor_row = 8;
        handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE), &mut app);
        let notes = transport.update_app(now + Duration::from_secs(50), &mut app, || {
            replace_graph(&mut sequencer, &tx, 48_000.0);
        });
        assert_eq!(app.playback_row, 8);
        assert_eq!(notes.len(), 1);
        for event in notes {
            schedule_instrument(&mut sequencer, event.note, event.instrument, event.duration, false);
        }
        assert!((0..10_000).any(|_| {
            let (l, r) = output.sample();
            l.abs() + r.abs() > 1e-6
        }));
        handle_key(KeyEvent::new(KeyCode::Null, KeyModifiers::CONTROL), &mut app);
        assert!(transport
            .update_app(now, &mut app, || replace_graph(&mut sequencer, &tx, 48_000.0))
            .is_empty());
        for _ in 0..100_000 {
            assert_eq!(output.sample(), (0.0, 0.0));
        }
    }

    #[test]
    fn output_maps_mono_and_stereo() {
        let mut mono = [0.0_f32; 4];
        write_data(&mut mono, 1, &mut || (0.25, -0.5));
        assert_eq!(mono, [0.25; 4]);
        let mut stereo = [0.0_f32; 4];
        write_data(&mut stereo, 2, &mut || (0.25, -0.5));
        assert_eq!(stereo, [0.25, -0.5, 0.25, -0.5]);
    }

    #[test]
    fn scheduled_note_produces_finite_audio_without_an_output_device() {
        let mut sequencer = Sequencer::new(0, 2, ReplayMode::All);
        let backend = sequencer.backend();
        let mut net = build_net(48_000.0, backend);
        let mut renderer = BlockRateAdapter::new(Box::new(net.backend()));

        schedule_note(
            &mut sequencer,
            Note {
                pitch: NotePitch::C,
                octave: 4,
            },
            DEFAULT_NOTE_DURATION,
        );

        let mut heard_note = false;
        for _ in 0..48_000 {
            let (left, right) = renderer.get_stereo();
            assert!(left.is_finite() && right.is_finite());
            heard_note |= left.abs() > 1e-6 || right.abs() > 1e-6;
        }
        assert!(heard_note);
    }
}
