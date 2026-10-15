#![allow(clippy::precedence)]

use crate::{tracker::Note, transport::NoteEvent};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

pub const DEFAULT_VOICE_GAIN: f32 = 0.5;
pub const DEFAULT_PLUCK_DECAY: f32 = 0.5;
pub const DEFAULT_PLUCK_DAMPING: f32 = 0.5;
pub const DEFAULT_NOTE_DURATION: Duration = Duration::from_millis(200);
pub const DEFAULT_FADE_IN: f64 = 0.02;
pub const DEFAULT_FADE_OUT: f64 = 0.2;
pub const DEFAULT_CHORUS_AMOUNT: f32 = 1.0;

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, FromSample, SizedSample, Stream, SupportedStreamConfig,
};
use fundsp::{
    math::{cos_hz, xerp11, AttoHash},
    prelude32::{
        chorus, dcblock, follow, lfo, moog, multipass, pan, pass, pluck, shared, split, var, zero, AudioUnit,
        BlockRateAdapter, Fade, Net, ReplayMode, Sequencer, U2,
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

fn prepare(sample_rate: f64, dry: bool) -> (Sequencer, Renderer) {
    let mut sequencer = Sequencer::new(0, 1, ReplayMode::None);
    let mut net = build_net(sample_rate, sequencer.backend(), dry);
    (sequencer, BlockRateAdapter::new(Box::new(net.backend())))
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

        let (sequencer, renderer) = prepare(sample_rate, dry);
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
        schedule_note(&mut self.sequencer, note, DEFAULT_NOTE_DURATION);
    }
    pub fn play_event(&mut self, event: NoteEvent) {
        self.collect_retired();
        schedule_note(&mut self.sequencer, event.note, event.duration);
    }

    /// Replace the entire prepared graph, cancelling voices and effect tails.
    /// The callback switches before its next sample; retired graphs are freed here.
    pub fn stop(&mut self) {
        self.collect_retired();
        let (sequencer, renderer) = prepare(self.sample_rate, self.dry);
        self.sequencer = sequencer;
        let _ = self.replacements.send(renderer);
    }

    fn collect_retired(&self) {
        while self.retired.try_recv().is_ok() {}
    }
}

fn schedule_note(sequencer: &mut Sequencer, note: Note, duration: Duration) {
    let pitch_hz = note.freq();
    // let pitch = lfo(move |t| pitch_hz * xerp11(1.0, 1.0, 0.5 * (sin_hz(6.0, t) + sin_hz(6.1,
    // t))));
    // The synthesis graph uses f32, so rounding the f64 note frequency is required.
    #[allow(clippy::cast_possible_truncation)]
    let pitch_hz = pitch_hz as f32;
    let waveform = Net::wrap(Box::new(
        zero() >> pluck(pitch_hz, DEFAULT_PLUCK_DECAY, DEFAULT_PLUCK_DAMPING) * DEFAULT_VOICE_GAIN,
    ));
    let filter = Net::wrap(Box::new(
        (pass() | lfo(move |t| (xerp11(400.0, 10000.0, cos_hz(0.1, t)), 0.6))) >> moog(),
    ));

    let mut note = Box::new(waveform >> filter >> dcblock());
    let seed: u64 = rand::random();
    // Give the note its own random seed.
    note.ping(false, AttoHash::new(seed));
    let seconds = duration.as_secs_f64();
    if seconds > 0.0 {
        sequencer.push_relative(
            0.0,
            seconds,
            Fade::Smooth,
            DEFAULT_FADE_IN.min(seconds),
            DEFAULT_FADE_OUT.min(seconds),
            note,
        );
    }
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

fn build_net(sample_rate: f64, sequencer_backend: SequencerBackend, dry: bool) -> Net {
    // let room_size = 10.0;
    // let reverb_amount = shared(0.25);
    // let reverb_time = 2.0;
    // let reverb_diffusion = 0.5;
    let chorus_amount = shared(DEFAULT_CHORUS_AMOUNT);

    let mut net = Net::wrap(Box::new(sequencer_backend));
    // let (reverb, reverb_id) = Net::wrap_id(create_reverb(room_size, reverb_time,
    // reverb_diffusion));
    let (phaser, _phaser_id) = Net::wrap_id(Box::new(multipass::<U2>()));
    let (flanger, _flanger_id) = Net::wrap_id(Box::new(multipass::<U2>()));
    net = net >> pan(0.0);
    if dry {
        net.set_sample_rate(sample_rate);
        return net;
    }
    // Smooth chorus and reverb amounts to prevent discontinuities.
    net = net
        >> ((1.0 - var(&chorus_amount) >> follow(0.01) >> split()) * multipass()
            & (var(&chorus_amount) >> follow(0.01) >> split())
                * 2.0
                * (chorus(0, 0.0, 0.03, 0.2) | chorus(1, 0.0, 0.03, 0.2)));
    net = net >> phaser >> flanger;
    // net = net
    //     >> ((1.0 - var(&reverb_amount) >> follow(0.01) >> split::<U2>()) * multipass()
    //         & (var(&reverb_amount) >> follow(0.01) >> split::<U2>()) * reverb);

    net.set_sample_rate(sample_rate);

    net
}

// FIXME: this implementation of reverb requires building in release mode, otherwise we underrun
// ALSA. I'll comment it for now, until I find out how to turn it on only on release builds.
// fn create_reverb(room_size: f32, time: f32, diffusion: f32) -> Box<dyn AudioUnit> {
//     //Box::new(reverb3_stereo(time, diffusion, highshelf_hz(5000.0, 1.0, db_amp(-1.0))))
//     Box::new(reverb2_stereo(
//         room_size,
//         time,
//         diffusion,
//         1.0,
//         highshelf_hz(5000.0, 1.0, db_amp(-1.0)),
//     ))
//     //Box::new(reverb4_stereo(room_size, time))
// }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::NotePitch;

    #[test]
    fn stop_cancels_voices_tails_and_queued_notes_without_resurrection() {
        for dry in [false, true] {
            let (mut seq, renderer) = prepare(48_000.0, dry);
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
            schedule_note(&mut seq, note, Duration::from_secs(2));
            let mut heard = false;
            for _ in 0..10_000 {
                let (l, r) = output.sample();
                assert!(l.is_finite() && r.is_finite());
                heard |= l.abs() + r.abs() > 1e-6;
            }
            assert!(heard);
            schedule_note(&mut seq, note, Duration::from_secs(2));
            let (mut next, renderer) = prepare(48_000.0, dry);
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
            let (_, renderer) = prepare(48_000.0, dry);
            tx.send(renderer).unwrap();
            for _ in 0..100_000 {
                assert_eq!(output.sample(), (0.0, 0.0));
            }
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
        let mut sequencer = Sequencer::new(0, 1, ReplayMode::All);
        let backend = sequencer.backend();
        let mut net = build_net(48_000.0, backend, false);
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
