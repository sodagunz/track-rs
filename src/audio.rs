#![allow(clippy::precedence)]

#[path = "audio_click.rs"]
mod click;

use crate::{
    tracker::{Instrument, Note},
    transport::NoteEvent,
};
use std::sync::{
    mpsc::{self, Receiver, Sender},
    Arc,
};
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

const RETRIGGER_SECONDS: f64 = 0.005;

// The outer envelope gates effects too. Renderers are built on the control
// thread; fixed track slots only render, move, and retire them in the callback.
struct GatedVoice {
    renderer: Renderer,
    elapsed: f64,
    duration: Option<f64>,
    midi_token: Option<u64>,
    release_level: f32,
    attack: f64,
    release: f64,
    sample_period: f64,
}

impl GatedVoice {
    fn new(event: NoteEvent, sample_rate: f64, dry: bool, seed: u64) -> Self {
        let mut unit = voice(event.note, event.instrument, dry, seed);
        unit.set_sample_rate(sample_rate);
        unit.allocate();
        let duration = event.duration.as_secs_f64();
        let (attack, release) = envelope_times(event.instrument, duration);
        Self {
            renderer: BlockRateAdapter::new(unit),
            elapsed: 0.0,
            duration: Some(duration),
            midi_token: None,
            release_level: 1.0,
            attack,
            release,
            sample_period: sample_rate.recip(),
        }
    }

    fn held(event: NoteEvent, token: u64, sample_rate: f64, dry: bool, seed: u64) -> Self {
        let mut voice = Self::new(event, sample_rate, dry, seed);
        voice.duration = None;
        voice.midi_token = Some(token);
        (voice.attack, voice.release) = preset_envelope_times(event.instrument);
        voice
    }

    fn release_gate(&mut self, token: u64) {
        if self.midi_token == Some(token) && self.duration.is_none() {
            self.duration = Some(self.elapsed);
            self.release_level = if self.attack == 0.0 {
                1.0
            } else {
                smooth_gain(self.elapsed / self.attack)
            };
            self.attack = 0.0;
        }
    }

    fn sample(&mut self) -> Option<(f32, f32)> {
        let release_gain = if let Some(duration) = self.duration {
            if duration == 0.0 || self.elapsed >= duration + self.release {
                return None;
            }
            if self.release == 0.0 {
                1.0
            } else {
                smooth_gain((duration + self.release - self.elapsed) / self.release)
            }
        } else {
            1.0
        };
        let attack_gain = if self.attack == 0.0 {
            1.0
        } else {
            smooth_gain(self.elapsed / self.attack)
        };
        let gain = attack_gain * release_gain * self.release_level;
        let (l, r) = self.renderer.get_stereo();
        self.elapsed += self.sample_period;
        Some((l * gain, r * gain))
    }
}

#[allow(clippy::cast_possible_truncation)]
fn smooth_gain(phase: f64) -> f32 {
    let x = phase.clamp(0.0, 1.0);
    (x * x * x * x.mul_add(x.mul_add(6.0, -15.0), 10.0)) as f32
}

enum OutputCommand {
    MidiPreview {
        track: usize,
        voice: GatedVoice,
    },
    StopMidiPreview,
    ReleaseMidiPreview(u64),
    Click(Option<click::ClickVoice>),
    ResetMeters(u32),
    Channel {
        track: usize,
        gains: (f32, f32),
        tone: [crate::tone::Target; 4],
        sample_rate: f64,
    },
    Preview {
        track: usize,
        voice: GatedVoice,
    },
    Replace(Renderer),
    Trigger {
        track: usize,
        voice: GatedVoice,
    },
}

// A persistent 10 ms linear ramp, advanced even while a track is silent.
struct ChannelProcessor {
    current: (f32, f32),
    target: (f32, f32),
    remaining: u32,
    tone: crate::tone::Tone,
}

impl Default for ChannelProcessor {
    fn default() -> Self {
        Self {
            current: (1.0, 1.0),
            target: (1.0, 1.0),
            remaining: 0,
            tone: crate::tone::Tone::default(),
        }
    }
}

fn channel_gains(settings: crate::project::ChannelSettings) -> (f32, f32) {
    let gain = settings.volume_db.map_or(0.0, |db| 10.0_f32.powf(db / 20.0));
    (
        gain * (1.0 - settings.pan.max(0.0) / 100.0),
        gain * (1.0 + settings.pan.min(0.0) / 100.0),
    )
}

impl ChannelProcessor {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn set(&mut self, target: (f32, f32), sample_rate: f64) {
        self.target = target;
        self.remaining = (sample_rate * 0.01).round().max(1.0) as u32;
    }

    fn reset(&mut self) {
        self.current = self.target;
        self.remaining = 0;
        self.tone.reset();
    }

    #[allow(clippy::cast_precision_loss)]
    fn process(&mut self, sample: (f32, f32)) -> (f32, f32) {
        if self.remaining > 0 {
            self.current.0 += (self.target.0 - self.current.0) / self.remaining as f32;
            self.current.1 += (self.target.1 - self.current.1) / self.remaining as f32;
            self.remaining -= 1;
        }
        let sample = self.tone.process(sample);
        (sample.0 * self.current.0, sample.1 * self.current.1)
    }
}

#[derive(Default)]
struct TrackVoices {
    active: Option<GatedVoice>,
    releasing: Option<(GatedVoice, f64)>,
    mixer: ChannelProcessor,
}

impl TrackVoices {
    fn clear(&mut self, retired: &Sender<Renderer>) {
        if let Some(voice) = self.active.take() {
            let _ = retired.send(voice.renderer);
        }
        if let Some((voice, _)) = self.releasing.take() {
            let _ = retired.send(voice.renderer);
        }
    }

    fn trigger(&mut self, voice: GatedVoice, retired: &Sender<Renderer>) {
        if let Some((old, _)) = self.releasing.take() {
            let _ = retired.send(old.renderer);
        }
        self.releasing = self.active.take().map(|old| (old, RETRIGGER_SECONDS));
        self.active = Some(voice);
    }

    fn sample(&mut self, retired: &Sender<Renderer>) -> (f32, f32) {
        let mut sample = (0.0, 0.0);
        if let Some(voice) = &mut self.active {
            if let Some(value) = voice.sample() {
                sample = value;
            } else if let Some(old) = self.active.take() {
                let _ = retired.send(old.renderer);
            }
        }
        if let Some((voice, remaining)) = &mut self.releasing {
            let value = if *remaining > 0.0 { voice.sample() } else { None };
            if let Some((l, r)) = value {
                let gain = smooth_gain(*remaining / RETRIGGER_SECONDS);
                sample.0 = l.mul_add(gain, sample.0);
                sample.1 = r.mul_add(gain, sample.1);
                *remaining -= voice.sample_period;
            } else if let Some((old, _)) = self.releasing.take() {
                let _ = retired.send(old.renderer);
            }
        }
        self.mixer.process(sample)
    }
}

// Preview slots are independent so live entry never steals a transport voice.
struct Output {
    renderer: Renderer,
    commands: Receiver<OutputCommand>,
    retired: Sender<Renderer>,
    tracks: Vec<TrackVoices>,
    previews: Vec<TrackVoices>,
    midi_previews: Vec<TrackVoices>,
    meters: Arc<crate::meter::Meters>,
    channel_samples: Vec<(f32, f32)>,
    click: Option<click::ClickVoice>,
}

impl Output {
    fn new(renderer: Renderer, commands: Receiver<OutputCommand>, retired: Sender<Renderer>) -> Self {
        Self {
            renderer,
            commands,
            retired,
            meters: Arc::default(),
            channel_samples: vec![(0.0, 0.0); crate::project_file::MAX_TRACKS],
            click: None,
            midi_previews: (0..crate::project_file::MAX_TRACKS)
                .map(|_| TrackVoices::default())
                .collect(),
            previews: (0..crate::project_file::MAX_TRACKS)
                .map(|_| TrackVoices::default())
                .collect(),
            tracks: (0..crate::project_file::MAX_TRACKS)
                .map(|_| TrackVoices::default())
                .collect(),
        }
    }

    fn sample(&mut self) -> (f32, f32) {
        // One FIFO orders cancellation before the next run's triggers, even when
        // the control thread sends commands while this callback is draining it.
        while let Ok(command) = self.commands.try_recv() {
            match command {
                OutputCommand::Click(voice) => self.click = voice,
                OutputCommand::ResetMeters(generation) => self.meters.reset(generation),
                OutputCommand::Channel {
                    track,
                    gains,
                    tone,
                    sample_rate,
                } => {
                    for slots in [&mut self.tracks, &mut self.previews, &mut self.midi_previews] {
                        if let Some(track) = slots.get_mut(track) {
                            track.mixer.set(gains, sample_rate);
                            track.mixer.tone.set(tone, track.mixer.remaining);
                        }
                    }
                }
                OutputCommand::Replace(renderer) => {
                    self.click = None;
                    for track in self
                        .tracks
                        .iter_mut()
                        .chain(&mut self.previews)
                        .chain(&mut self.midi_previews)
                    {
                        track.clear(&self.retired);
                        track.mixer.reset();
                    }
                    let old = std::mem::replace(&mut self.renderer, renderer);
                    let _ = self.retired.send(old);
                }
                OutputCommand::ReleaseMidiPreview(token) => {
                    for track in &mut self.midi_previews {
                        if let Some(voice) = &mut track.active {
                            voice.release_gate(token);
                        }
                    }
                }
                OutputCommand::StopMidiPreview => {
                    for track in &mut self.midi_previews {
                        track.clear(&self.retired);
                    }
                }
                OutputCommand::MidiPreview { track, voice } => {
                    if let Some(track) = self.midi_previews.get_mut(track) {
                        track.trigger(voice, &self.retired);
                    } else {
                        let _ = self.retired.send(voice.renderer);
                    }
                }
                OutputCommand::Preview { track, voice } => {
                    if let Some(track) = self.previews.get_mut(track) {
                        track.trigger(voice, &self.retired);
                    } else {
                        let _ = self.retired.send(voice.renderer);
                    }
                }
                OutputCommand::Trigger { track, voice } => {
                    if let Some(track) = self.tracks.get_mut(track) {
                        track.trigger(voice, &self.retired);
                    } else {
                        let _ = self.retired.send(voice.renderer);
                    }
                }
            }
        }
        let mut sample = self.renderer.get_stereo();
        for (track, channel) in self.tracks.iter_mut().zip(&mut self.channel_samples) {
            *channel = track.sample(&self.retired);
            sample.0 += channel.0;
            sample.1 += channel.1;
        }
        for (index, (preview, channel)) in self.previews.iter_mut().zip(&self.channel_samples).enumerate() {
            let mut preview = preview.sample(&self.retired);
            let midi = self.midi_previews[index].sample(&self.retired);
            preview.0 += midi.0;
            preview.1 += midi.1;
            self.meters
                .channel(index, (channel.0 + preview.0, channel.1 + preview.1));
            sample.0 += preview.0;
            sample.1 += preview.1;
        }
        if let Some(voice) = &mut self.click {
            if let Some(value) = voice.sample() {
                sample.0 += value;
                sample.1 += value;
            } else {
                self.click = None;
            }
        }
        self.meters.master(sample);
        sample
    }
}

fn prepare(sample_rate: f64) -> (Sequencer, Renderer) {
    let mut sequencer = Sequencer::new(0, 2, ReplayMode::None);
    let mut net = build_net(sample_rate, sequencer.backend());
    (sequencer, BlockRateAdapter::new(Box::new(net.backend())))
}

fn replace_graph(sequencer: &mut Sequencer, replacements: &Sender<OutputCommand>, sample_rate: f64) {
    let (next, renderer) = prepare(sample_rate);
    *sequencer = next;
    let _ = replacements.send(OutputCommand::Replace(renderer));
}

fn sync_channel_targets(
    sender: &Sender<OutputCommand>,
    cached: &mut Vec<crate::project::ChannelSettings>,
    settings: &[crate::project::ChannelSettings],
    sample_rate: f64,
) {
    if cached == settings {
        return;
    }
    let any_solo = settings.iter().any(|setting| setting.solo);
    let previous_solo = cached.iter().any(|setting| setting.solo);
    for (track, &setting) in settings.iter().enumerate() {
        let changed = cached.get(track).is_none_or(|previous| {
            let was_audible = previous.audible(previous_solo);
            *previous != setting || was_audible != setting.audible(any_solo)
        });
        if setting.is_valid() && changed {
            let _ = sender.send(OutputCommand::Channel {
                track,
                gains: if setting.audible(any_solo) {
                    channel_gains(setting)
                } else {
                    (0.0, 0.0)
                },
                tone: crate::tone::targets(setting, sample_rate),
                sample_rate,
            });
        }
    }
    cached.clear();
    cached.extend_from_slice(settings);
}

pub struct AudioEngine {
    sequencer: Sequencer,
    replacements: Sender<OutputCommand>,
    retired: Receiver<Renderer>,
    sample_rate: f64,
    dry: bool,
    channel_targets: Vec<crate::project::ChannelSettings>,
    meters: Arc<crate::meter::Meters>,
    meter_generation: u32,
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
        let output = Output::new(renderer, receiver, retired_sender);
        let meters = Arc::clone(&output.meters);

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
            channel_targets: Vec::new(),
            meters,
            meter_generation: 0,
            _stream: stream,
        })
    }

    /// Legacy audition: always 200 ms, independent of the note's musical length.
    /// Legacy sequencer audition bypasses project track settings; use `play_event` for tracks.
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
    /// Send only changed targets, before scheduling transport or audition events.
    pub fn sync_channels(&mut self, settings: &[crate::project::ChannelSettings]) {
        sync_channel_targets(
            &self.replacements,
            &mut self.channel_targets,
            settings,
            self.sample_rate,
        );
    }

    pub fn play_event(&mut self, event: NoteEvent) {
        self.collect_retired();
        if event.track >= crate::project_file::MAX_TRACKS {
            return;
        }
        let _ = self.replacements.send(OutputCommand::Trigger {
            track: event.track,
            voice: GatedVoice::new(event, self.sample_rate, self.dry, rand::random()),
        });
    }

    /// Separate fixed-gain click; cancellation never resets project or preview voices.
    pub fn metronome_event(&mut self, event: crate::transport::MetronomeEvent) {
        let voice = match event {
            crate::transport::MetronomeEvent::Click(event) => {
                Some(click::ClickVoice::new(self.sample_rate, event.accent))
            }
            crate::transport::MetronomeEvent::Stop => None,
        };
        let _ = self.replacements.send(OutputCommand::Click(voice));
    }

    /// Cancel only MIDI-owned previews, leaving terminal and transport voices intact.
    pub fn stop_midi_preview(&mut self) {
        let _ = self.replacements.send(OutputCommand::StopMidiPreview);
    }

    /// Release one physical key without cancelling its instrument's release tail.
    pub fn release_midi_preview(&mut self, token: u64) {
        let _ = self.replacements.send(OutputCommand::ReleaseMidiPreview(token));
    }

    pub fn midi_preview_event(&mut self, event: NoteEvent, token: u64) {
        self.collect_retired();
        let _ = self.replacements.send(OutputCommand::MidiPreview {
            track: event.track,
            voice: GatedVoice::held(event, token, self.sample_rate, self.dry, rand::random()),
        });
    }

    /// Audition without retriggering a song voice on the same track.
    pub fn preview_event(&mut self, event: NoteEvent) {
        self.collect_retired();
        if event.track < crate::project_file::MAX_TRACKS {
            let _ = self.replacements.send(OutputCommand::Preview {
                track: event.track,
                voice: GatedVoice::new(event, self.sample_rate, self.dry, rand::random()),
            });
        }
    }

    /// Replace the entire prepared graph, cancelling voices and effect tails.
    /// The callback switches before its next sample; retired graphs are freed here.
    pub fn stop(&mut self) {
        self.collect_retired();
        replace_graph(&mut self.sequencer, &self.replacements, self.sample_rate);
        self.meter_generation = self.meter_generation.wrapping_add(1);
        let _ = self
            .replacements
            .send(OutputCommand::ResetMeters(self.meter_generation));
    }

    /// Consume peaks since the previous UI sample; never called from the callback.
    pub fn sample_meters(&self, display: &mut crate::meter::MeterDisplay, tracks: usize, elapsed: Duration) {
        display.update(&self.meters, self.meter_generation, tracks, elapsed);
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
    let (attack, release) = envelope_times(instrument, seconds);
    if seconds > 0.0 {
        sequencer.push_relative(
            0.0,
            seconds + release,
            Fade::Smooth,
            attack,
            release,
            voice(note, instrument, dry, seed),
        );
    }
}

// Duration is the note-off time, not the end of the release tail.
// Keep a short gate's attack brief without truncating the preset's release.
fn envelope_times(instrument: Instrument, duration: f64) -> (f64, f64) {
    let (attack, release) = preset_envelope_times(instrument);
    (attack.min(duration * 0.25), release)
}

const fn preset_envelope_times(instrument: Instrument) -> (f64, f64) {
    match instrument {
        Instrument::Pad => (0.35, 0.6),
        Instrument::Drone => (0.2, 0.45),
        Instrument::Sustain => (0.008, 0.04),
        Instrument::OpenHat => (PERCUSSION_ATTACK, 0.08),
        Instrument::Kick | Instrument::Snare | Instrument::HiHat => (PERCUSSION_ATTACK, DEFAULT_FADE_OUT),
        Instrument::Pluck | Instrument::Sine | Instrument::Square | Instrument::Saw => {
            (DEFAULT_FADE_IN, DEFAULT_FADE_OUT)
        }
    }
}

/// Construct a seeded stereo voice. The caller supplies the gate's attack/release.
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
        Instrument::Pad => Net::wrap(Box::new(
            (sine_hz(hz * 0.997) + sine_hz(hz * 1.003) + sine_hz(hz * 2.0) * 0.2)
                * lfo(|t: f32| 0.065 * (0.12_f32.mul_add(cos_hz(0.17, t), 1.0))),
        )),
        Instrument::Drone => Net::wrap(Box::new(
            (sine_hz(hz)
                + sine_hz(hz * 2.0) * lfo(|t: f32| 0.1_f32.mul_add(cos_hz(0.11, t), 0.25))
                + sine_hz(hz * 3.0) * 0.1)
                * 0.12,
        )),
        Instrument::Sustain => Net::wrap(Box::new((sine_hz(hz) + sine_hz(hz * 2.0) * 0.15) * 0.15)),
        Instrument::OpenHat => Net::wrap(Box::new(
            (noise() >> highpass_hz(7000.0, 0.7)) * lfo(|t: f32| HIHAT_GAIN * (-t * 8.0).exp()),
        )),
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

    #[test]
    fn click_voices_are_finite_short_and_accented_at_supported_rates() {
        for rate in [8000.0, 44100.0, 48000.0, 96000.0] {
            let mut energies = Vec::new();
            for accent in [false, true] {
                let mut voice = click::ClickVoice::new(rate, accent);
                let mut energy = 0.0;
                let mut samples = 0;
                while let Some(sample) = voice.sample() {
                    assert!(sample.is_finite() && sample.abs() <= 0.101);
                    energy += f64::from(sample).powi(2);
                    samples += 1;
                    assert!(samples <= 961, "bounded ten-millisecond tail");
                }
                assert!(energy > 0.0001);
                assert!(f64::from(samples) / rate <= 0.010 + rate.recip());
                energies.push(energy);
            }
            assert!(energies[1] > energies[0]);
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn click_bypasses_channels_joins_master_meter_and_cancels_without_resetting_notes() {
        use crate::{meter::MeterDisplay, project::ChannelSettings};
        let rate = 8000.0;
        let (_, renderer) = prepare(rate);
        let (sender, receiver) = mpsc::channel();
        let (retired, _rx) = mpsc::channel();
        let mut output = Output::new(renderer, receiver, retired);
        sender
            .send(OutputCommand::Trigger {
                track: 0,
                voice: GatedVoice {
                    renderer: BlockRateAdapter::new(Box::new(fundsp::prelude32::dc((0.99, 0.99)))),
                    elapsed: 1.0,
                    duration: Some(10.0),
                    midi_token: None,
                    release_level: 1.0,
                    attack: 0.001,
                    release: 0.001,
                    sample_period: rate.recip(),
                },
            })
            .unwrap();
        sender
            .send(OutputCommand::Click(Some(click::ClickVoice::new(rate, true))))
            .unwrap();
        for _ in 0..80 {
            output.sample();
        }
        let mut display = MeterDisplay::default();
        display.update(&output.meters, 0, 1, Duration::ZERO);
        assert!(display.clipped); // Click participates in master, not per-channel telemetry.
        assert!(display.channels[0][0] < 1.0);
        let mut cached = Vec::new();
        sync_channel_targets(
            &sender,
            &mut cached,
            &[ChannelSettings {
                muted: true,
                solo: true,
                volume_db: Some(-60.0),
                ..ChannelSettings::default()
            }],
            rate,
        );
        for _ in 0..100 {
            output.sample();
        }
        sender.send(OutputCommand::ResetMeters(1)).unwrap();
        sender
            .send(OutputCommand::Click(Some(click::ClickVoice::new(rate, true))))
            .unwrap();
        let peak = (0..80).map(|_| output.sample().0.abs()).fold(0.0_f32, f32::max);
        assert!(peak > 0.01);
        display.update(&output.meters, 1, 1, Duration::ZERO);
        assert_eq!(display.channels[0], [0.0, 0.0]);
        sender.send(OutputCommand::Click(None)).unwrap();
        assert_eq!(output.sample(), (0.0, 0.0));
        assert!(output.tracks[0].active.is_some());
        sync_channel_targets(&sender, &mut cached, &[ChannelSettings::default()], rate);
        for _ in 0..100 {
            output.sample();
        }
        assert!((output.sample().0 - 0.99).abs() < 1e-6);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn graph_cancellation_discards_clicks_in_fifo_order() {
        let rate = 8000.0;
        let (_, renderer) = prepare(rate);
        let (sender, receiver) = mpsc::channel();
        let (retired, _rx) = mpsc::channel();
        let mut output = Output::new(renderer, receiver, retired);
        sender
            .send(OutputCommand::Click(Some(click::ClickVoice::new(rate, true))))
            .unwrap();
        for _ in 0..10 {
            output.sample();
        }
        assert!(output.click.is_some());
        let (_, replacement) = prepare(rate);
        sender.send(OutputCommand::Replace(replacement)).unwrap();
        assert_eq!(output.sample(), (0.0, 0.0));
        assert!(output.click.is_none());
        sender
            .send(OutputCommand::Click(Some(click::ClickVoice::new(rate, false))))
            .unwrap();
        assert!((0..80).any(|_| output.sample().0.abs() > 0.01));
        sender.send(OutputCommand::Click(None)).unwrap();
        for _ in 0..100 {
            assert_eq!(output.sample(), (0.0, 0.0));
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn mute_and_unmute_preserve_song_and_preview_voice_timing() {
        use crate::project::ChannelSettings;
        for preview in [false, true] {
            for rate in [8_000.0, 48_000.0, 96_000.0] {
                let make_output = || {
                    let (_, renderer) = prepare(rate);
                    let (sender, receiver) = mpsc::channel();
                    let (retired, _rx) = mpsc::channel();
                    (sender, Output::new(renderer, receiver, retired))
                };
                let (sender, mut output) = make_output();
                let (reference_sender, mut reference) = make_output();
                for sender in [&sender, &reference_sender] {
                    let voice = GatedVoice::new(sine_event(0, Duration::from_secs(1)), rate, true, 42);
                    sender
                        .send(if preview {
                            OutputCommand::Preview { track: 0, voice }
                        } else {
                            OutputCommand::Trigger { track: 0, voice }
                        })
                        .unwrap();
                }
                let mut cached = vec![ChannelSettings::default(); 4];
                let mut settings = cached.clone();
                for _ in 0..1000 {
                    assert_eq!(output.sample(), reference.sample());
                }
                settings[0].muted = true;
                sync_channel_targets(&sender, &mut cached, &settings, rate);
                for _ in 0..1000 {
                    output.sample();
                    reference.sample();
                }
                for _ in 0..1000 {
                    assert_eq!(output.sample(), (0.0, 0.0));
                    reference.sample();
                }
                settings[0].muted = false;
                sync_channel_targets(&sender, &mut cached, &settings, rate);
                for _ in 0..1000 {
                    output.sample();
                    reference.sample();
                }
                let mut energy = 0.0;
                for _ in 0..1000 {
                    let sample = output.sample();
                    assert_eq!(sample, reference.sample());
                    energy += sample.0.abs() + sample.1.abs();
                }
                assert!(energy > 0.001);
            }
        }
    }

    #[test]
    fn mute_solo_sync_updates_other_channels_without_redundant_commands() {
        use crate::project::ChannelSettings;
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut cached = Vec::new();
        let mut settings = vec![ChannelSettings::default(); 3];
        sync_channel_targets(&sender, &mut cached, &settings, 48_000.0);
        assert_eq!(receiver.try_iter().count(), 3);
        settings[1].solo = true;
        sync_channel_targets(&sender, &mut cached, &settings, 48_000.0);
        let commands: Vec<_> = receiver.try_iter().collect();
        assert_eq!(commands.len(), 3);
        for command in commands {
            let OutputCommand::Channel { track, gains, .. } = command else {
                panic!("expected channel target")
            };
            assert_eq!(gains, if track == 1 { (1.0, 1.0) } else { (0.0, 0.0) });
        }
        settings[1].muted = true;
        sync_channel_targets(&sender, &mut cached, &settings, 48_000.0);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            OutputCommand::Channel {
                track: 1,
                gains: (0.0, 0.0),
                ..
            }
        ));
        assert!(receiver.try_recv().is_err());
        sync_channel_targets(&sender, &mut cached, &settings, 48_000.0);
        assert!(receiver.try_recv().is_err());
        settings[1].solo = false;
        sync_channel_targets(&sender, &mut cached, &settings, 48_000.0);
        assert_eq!(receiver.try_iter().count(), 3);
        assert!(!settings[1].audible(false));
        assert!(settings[0].audible(false));
    }
    use crate::tracker::NotePitch;

    #[test]
    #[allow(clippy::float_cmp, clippy::too_many_lines)]
    fn meters_measure_processed_song_preview_sum_and_reset_in_fifo_order() {
        use crate::{meter::MeterDisplay, project::ChannelSettings};
        let rate = 8000.0;
        let (_, renderer) = prepare(rate);
        let (sender, receiver) = mpsc::channel();
        let (retired, _rx) = mpsc::channel();
        let mut output = Output::new(renderer, receiver, retired);
        let mut display = MeterDisplay::default();
        let constant = || GatedVoice {
            renderer: BlockRateAdapter::new(Box::new(fundsp::prelude32::dc((0.6, -0.6)))),
            elapsed: 1.0,
            duration: Some(10.0),
            midi_token: None,
            release_level: 1.0,
            attack: 0.001,
            release: 0.001,
            sample_period: 1.0 / rate,
        };
        sender
            .send(OutputCommand::Trigger {
                track: 0,
                voice: constant(),
            })
            .unwrap();
        sender
            .send(OutputCommand::Preview {
                track: 1,
                voice: constant(),
            })
            .unwrap();
        let sample = output.sample();
        assert!((sample.0 - 1.2).abs() < 1e-6);
        assert!(sample.1 < -1.0); // The telemetry does not limit output.
        display.update(&output.meters, 0, 3, Duration::ZERO);
        assert_eq!(display.channels, [[0.6, 0.6], [0.6, 0.6], [0.0, 0.0]]);
        assert!(display.clipped); // Neither channel clips alone.
                                  // A high-pass removes this DC signal before channel telemetry.
        let filtered = ChannelSettings {
            hpf_hz: Some(200.0),
            ..ChannelSettings::default()
        };
        sender
            .send(OutputCommand::Channel {
                track: 0,
                gains: (1.0, 1.0),
                tone: crate::tone::targets(filtered, rate),
                sample_rate: rate,
            })
            .unwrap();
        for _ in 0..800 {
            output.sample();
        }
        output.meters.reset(0);
        output.sample();
        display = MeterDisplay::default();
        display.update(&output.meters, 0, 3, Duration::ZERO);
        assert!(display.channels[0].iter().all(|peak| *peak < 0.001));
        assert_eq!(display.channels[1], [0.6; 2]);
        for (pan, expected) in [(-100.0, [0.6, 0.0]), (100.0, [0.0, 0.6])] {
            sender
                .send(OutputCommand::Channel {
                    track: 0,
                    gains: channel_gains(ChannelSettings {
                        pan,
                        ..ChannelSettings::default()
                    }),
                    tone: crate::tone::targets(ChannelSettings::default(), rate),
                    sample_rate: rate,
                })
                .unwrap();
            for _ in 0..80 {
                output.sample();
            }
            output.meters.reset(0);
            output.sample();
            display = MeterDisplay::default();
            display.update(&output.meters, 0, 3, Duration::ZERO);
            assert!((display.channels[0][0] - expected[0]).abs() < 1e-6);
            assert!((display.channels[0][1] - expected[1]).abs() < 1e-6);
        }
        // Preview and transport are combined on the same channel.
        sender
            .send(OutputCommand::Preview {
                track: 0,
                voice: constant(),
            })
            .unwrap();
        output.sample();
        display = MeterDisplay::default();
        display.update(&output.meters, 0, 3, Duration::ZERO);
        assert!(display.channels[0][1] > 0.6);
        sender
            .send(OutputCommand::Channel {
                track: 0,
                gains: (0.0, 0.0),
                tone: crate::tone::targets(ChannelSettings::default(), rate),
                sample_rate: rate,
            })
            .unwrap();
        for _ in 0..80 {
            output.sample();
        }
        output.meters.reset(0);
        output.sample();
        display = MeterDisplay::default();
        display.update(&output.meters, 0, 3, Duration::ZERO);
        assert_eq!(display.channels[0], [0.0; 2]);
        // Stop/load cancellation clears pending peaks; a new slot starts clean.
        let (_, renderer) = prepare(rate);
        sender.send(OutputCommand::Replace(renderer)).unwrap();
        sender.send(OutputCommand::ResetMeters(1)).unwrap();
        display.update(&output.meters, 1, 4, Duration::ZERO);
        assert!(!display.clipped);
        assert_eq!(display.channels, [[0.0; 2]; 4]);
        output.sample();
        display.update(&output.meters, 1, 4, Duration::ZERO);
        assert_eq!(display.channels, [[0.0; 2]; 4]);
        sender
            .send(OutputCommand::Trigger {
                track: 3,
                voice: constant(),
            })
            .unwrap();
        output.sample();
        display.update(&output.meters, 1, 4, Duration::ZERO);
        assert_eq!(display.channels[3], [0.6; 2]);
        assert_eq!(display.channels[0], [0.0; 2]);
        assert!(!display.clipped);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn tone_commands_change_held_voices_and_survive_retrigger() {
        use crate::project::ChannelSettings;
        let rate = 8000.0;
        let (_, renderer) = prepare(rate);
        let (sender, receiver) = mpsc::channel();
        let (retired, _rx) = mpsc::channel();
        let mut output = Output::new(renderer, receiver, retired);
        sender
            .send(OutputCommand::Trigger {
                track: 0,
                voice: GatedVoice::new(sine_event(0, Duration::from_secs(1)), rate, true, 42),
            })
            .unwrap();
        for _ in 0..400 {
            output.sample();
        }
        let before = output.tracks[0].active.as_ref().unwrap().elapsed;
        sender
            .send(OutputCommand::Channel {
                track: 0,
                gains: (1.0, 1.0),
                tone: crate::tone::targets(
                    ChannelSettings {
                        lpf_hz: Some(20.0),
                        ..ChannelSettings::default()
                    },
                    rate,
                ),
                sample_rate: rate,
            })
            .unwrap();
        for _ in 0..400 {
            output.sample();
        }
        assert!(output.tracks[0].active.as_ref().unwrap().elapsed > before + 0.049);
        let mut energy = 0.0;
        for _ in 0..80 {
            let (l, r) = output.sample();
            energy += l.abs() + r.abs();
        }
        assert!(energy < 0.5);
        sender
            .send(OutputCommand::Trigger {
                track: 0,
                voice: GatedVoice::new(sine_event(0, Duration::from_secs(1)), rate, true, 43),
            })
            .unwrap();
        output.sample();
        assert!(output.tracks[0].releasing.is_some());
        // Muting after the filters must not discard their stored tail.
        let mixer = &mut output.tracks[0].mixer;
        mixer.current = (0.0, 0.0);
        mixer.remaining = 0;
        assert_eq!(mixer.process((1.0, 0.0)), (0.0, 0.0));
        mixer.current = (1.0, 1.0);
        assert!(mixer.process((0.0, 0.0)).0.abs() > 0.0);
        let (_, renderer) = prepare(rate);
        sender.send(OutputCommand::Replace(renderer)).unwrap();
        assert_eq!(output.sample(), (0.0, 0.0));
    }

    #[test]
    fn held_recording_loop_matches_offline_monitor_only_mix() {
        struct Offline {
            output: Output,
            sender: Sender<OutputCommand>,
        }
        impl crate::midi::Output for Offline {
            fn stop(&mut self) {}
            fn play_event(&mut self, event: NoteEvent) {
                self.sender
                    .send(OutputCommand::Trigger {
                        track: event.track,
                        voice: GatedVoice::new(event, 8_000.0, true, 42),
                    })
                    .unwrap();
            }
            fn metronome_event(&mut self, _: crate::transport::MetronomeEvent) {}
            fn midi_preview_event(&mut self, event: NoteEvent, token: u64) {
                self.sender
                    .send(OutputCommand::MidiPreview {
                        track: event.track,
                        voice: GatedVoice::held(event, token, 8_000.0, true, 42),
                    })
                    .unwrap();
            }
            fn release_midi_preview(&mut self, token: u64) {
                self.sender.send(OutputCommand::ReleaseMidiPreview(token)).unwrap();
            }
            fn stop_midi_preview(&mut self) {
                self.sender.send(OutputCommand::StopMidiPreview).unwrap();
            }
        }
        fn output() -> Offline {
            let (_, renderer) = prepare(8_000.0);
            let (sender, receiver) = mpsc::channel();
            let (retired, _) = mpsc::channel();
            Offline {
                output: Output::new(renderer, receiver, retired),
                sender,
            }
        }
        let now = std::time::Instant::now();
        let (mut session, input) = crate::midi::Session::test_input(now);
        let mut app = crate::state::App::default();
        app.project.set_instrument(0, Instrument::Sine);
        app.recording.armed = true;
        app.toggle_playback();
        app.mode = crate::state::Mode::Insert;
        let mut transport = crate::transport::Transport::default();
        transport.update_app(now, &mut app, || {});
        let mut audio = output();
        let mut reference = output();
        input.receive_at(&[0x90, 60, 80], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        assert!(app.audition.is_none());
        // Use the same monitor request and seed without any recorded song events.
        let note = crate::midi::note(60, app.entry_length).unwrap();
        app.preview_note_on(note, 0);
        crate::midi::Output::midi_preview_event(&mut reference, app.audition.take().unwrap(), 1);
        let length = app.project.patterns[0].length_ticks().0;
        for sample in 0..=128_000 {
            let at = now + Duration::from_secs_f64(f64::from(sample) / 8_000.0);
            for event in transport.update_app(at, &mut app, || {}) {
                crate::midi::Output::play_event(&mut audio, event);
            }
            assert_eq!(
                audio.output.sample(),
                reference.output.sample(),
                "mix differs at sample {sample}, loop length {length}"
            );
        }
    }

    #[test]
    fn fake_midi_input_reaches_offline_audio_and_overflow_clears_held_gate() {
        struct Offline {
            output: Output,
            sender: Sender<OutputCommand>,
        }
        impl crate::midi::Output for Offline {
            fn stop(&mut self) {}
            fn play_event(&mut self, _: NoteEvent) {}
            fn metronome_event(&mut self, _: crate::transport::MetronomeEvent) {}
            fn midi_preview_event(&mut self, event: NoteEvent, token: u64) {
                self.sender
                    .send(OutputCommand::MidiPreview {
                        track: event.track,
                        voice: GatedVoice::held(event, token, 8_000.0, true, 42),
                    })
                    .unwrap();
            }
            fn release_midi_preview(&mut self, token: u64) {
                self.sender.send(OutputCommand::ReleaseMidiPreview(token)).unwrap();
            }
            fn stop_midi_preview(&mut self) {
                self.sender.send(OutputCommand::StopMidiPreview).unwrap();
            }
        }
        let now = std::time::Instant::now();
        let (mut session, input) = crate::midi::Session::test_input(now);
        let (_, renderer) = prepare(8_000.0);
        let (sender, receiver) = mpsc::channel();
        let (retired, _rx) = mpsc::channel();
        let mut audio = Offline {
            output: Output::new(renderer, receiver, retired),
            sender,
        };
        let mut app = crate::state::App {
            mode: crate::state::Mode::Audition,
            ..crate::state::App::default()
        };
        app.project.set_instrument(0, Instrument::Sine);
        let mut transport = crate::transport::Transport::default();
        input.receive_at(&[0x90, 60, 80], now);
        session.poll(&mut app, &mut audio, &mut transport, now);
        for _ in 0..8_000 {
            audio.output.sample();
        }
        assert!(audio.output.midi_previews[0]
            .active
            .as_ref()
            .unwrap()
            .duration
            .is_none());
        input.receive_at(&[0x80, 60, 0], now + Duration::from_secs(1));
        session.poll(&mut app, &mut audio, &mut transport, now + Duration::from_secs(1));
        audio.output.sample();
        assert!(audio.output.midi_previews[0]
            .active
            .as_ref()
            .unwrap()
            .duration
            .is_some());
        for _ in 0..1700 {
            audio.output.sample();
        }
        assert!(audio.output.midi_previews[0].active.is_none());
        input.receive_at(&[0x90, 64, 80], now + Duration::from_secs(2));
        session.poll(&mut app, &mut audio, &mut transport, now + Duration::from_secs(2));
        audio.output.sample();
        assert!(audio.output.midi_previews[0].active.is_some());
        for _ in 0..256 {
            input.receive_at(&[0x90, 67, 80], now + Duration::from_secs(3));
        }
        session.poll(&mut app, &mut audio, &mut transport, now + Duration::from_secs(3));
        assert_eq!(audio.output.sample(), (0.0, 0.0));
        assert!(audio.output.midi_previews[0].active.is_none());
        assert!(app.status.contains("overflow"));
        assert!(!app.recording.armed);
    }

    #[test]
    fn held_midi_gates_keep_pitch_and_release_at_physical_noteoff() {
        for rate in [8_000_u32, 44_100, 48_000] {
            for instrument in [
                Instrument::Sine,
                Instrument::Pad,
                Instrument::Drone,
                Instrument::Sustain,
                Instrument::Kick,
            ] {
                let event = NoteEvent {
                    instrument,
                    ..sine_event(0, DEFAULT_NOTE_DURATION)
                };
                let mut held = GatedVoice::held(event, 7, f64::from(rate), true, 42);
                let mut reference = GatedVoice::new(
                    NoteEvent {
                        duration: Duration::from_secs(2),
                        ..event
                    },
                    f64::from(rate),
                    true,
                    42,
                );
                // Both paths use the same oscillator and preset envelope once the attack completes.
                for index in 0..rate {
                    let sample = held.sample().unwrap();
                    let expected = reference.sample().unwrap();
                    assert!(
                        sample.0.is_finite() && sample.1.is_finite(),
                        "{instrument:?} at {rate} Hz sample {index}; timed reference: {expected:?}"
                    );
                    if index > rate / 2 {
                        assert_eq!(sample, expected);
                    }
                }
                assert!(held.duration.is_none());
                held.release_gate(6);
                assert!(held.duration.is_none());
                held.release_gate(7);
                let release = held.release;
                let tail: Vec<_> = (0..rate).map(|_| held.sample()).collect();
                assert!(tail[0].is_some());
                let end =
                    usize::try_from(Duration::from_secs_f64(release).as_nanos() * u128::from(rate) / 1_000_000_000)
                        .unwrap();
                assert!(tail[end + 2..].iter().all(Option::is_none));
                if instrument != Instrument::Kick {
                    assert!(tail[..end / 4].iter().flatten().any(|s| s.0.abs() > 0.0001));
                }
            }
        }
    }

    #[test]
    fn held_midi_retriggers_are_bounded_and_old_tokens_cannot_release_replacements() {
        let (_, renderer) = prepare(8_000.0);
        let (sender, receiver) = mpsc::channel();
        let (retired, _rx) = mpsc::channel();
        let mut output = Output::new(renderer, receiver, retired);
        for token in 1..=32 {
            sender
                .send(OutputCommand::MidiPreview {
                    track: 0,
                    voice: GatedVoice::held(sine_event(0, DEFAULT_NOTE_DURATION), token, 8_000.0, true, 42),
                })
                .unwrap();
            sender.send(OutputCommand::ReleaseMidiPreview(token - 1)).unwrap();
            output.sample();
            let slot = &output.midi_previews[0];
            assert!(slot.active.as_ref().unwrap().duration.is_none());
            assert_eq!(slot.active.as_ref().unwrap().midi_token, Some(token));
            assert_eq!(slot.releasing.is_some(), token > 1);
        }
        for _ in 0..50 {
            output.sample();
        }
        assert!(output.midi_previews[0].releasing.is_none());
        sender.send(OutputCommand::StopMidiPreview).unwrap();
        sender
            .send(OutputCommand::MidiPreview {
                track: 0,
                voice: GatedVoice::held(sine_event(0, DEFAULT_NOTE_DURATION), 33, 8_000.0, true, 42),
            })
            .unwrap();
        sender.send(OutputCommand::ReleaseMidiPreview(32)).unwrap();
        output.sample();
        assert!(output.midi_previews[0].active.as_ref().unwrap().duration.is_none());
        sender.send(OutputCommand::ReleaseMidiPreview(33)).unwrap();
        for _ in 0..1700 {
            output.sample();
        }
        assert!(output.midi_previews[0].active.is_none());
    }

    #[test]
    fn midi_release_and_reset_leave_song_samples_bit_exact() {
        let make = || {
            let (_, renderer) = prepare(8_000.0);
            let (sender, receiver) = mpsc::channel();
            let (retired, rx) = mpsc::channel();
            let output = Output::new(renderer, receiver, retired);
            sender
                .send(OutputCommand::Trigger {
                    track: 0,
                    voice: GatedVoice::new(sine_event(0, Duration::from_secs(2)), 8_000.0, true, 42),
                })
                .unwrap();
            (output, sender, rx)
        };
        let (mut reference, _sender, _rx) = make();
        let (mut output, sender, _retired) = make();
        sender
            .send(OutputCommand::MidiPreview {
                track: 0,
                voice: GatedVoice::held(sine_event(0, DEFAULT_NOTE_DURATION), 1, 8_000.0, true, 43),
            })
            .unwrap();
        sender.send(OutputCommand::ReleaseMidiPreview(1)).unwrap();
        sender.send(OutputCommand::StopMidiPreview).unwrap();
        for _ in 0..8_000 {
            assert_eq!(output.sample(), reference.sample());
        }
    }

    #[test]
    fn held_midi_uses_live_channel_processing_and_meters() {
        let (_, renderer) = prepare(8_000.0);
        let (sender, receiver) = mpsc::channel();
        let (retired, _rx) = mpsc::channel();
        let mut output = Output::new(renderer, receiver, retired);
        sender
            .send(OutputCommand::MidiPreview {
                track: 0,
                voice: GatedVoice::held(sine_event(0, DEFAULT_NOTE_DURATION), 1, 8_000.0, true, 42),
            })
            .unwrap();
        for _ in 0..4_000 {
            output.sample();
        }
        assert!(output.sample().0.abs() > 0.0001);
        let mut display = crate::meter::MeterDisplay::default();
        display.update(&output.meters, 0, 1, Duration::from_millis(33));
        assert!(display.channels[0][0] > 0.0);
        sender
            .send(OutputCommand::Channel {
                track: 0,
                gains: (0.0, 0.5),
                tone: crate::tone::targets(crate::project::ChannelSettings::default(), 8_000.0),
                sample_rate: 8_000.0,
            })
            .unwrap();
        for _ in 0..81 {
            output.sample();
        }
        let panned = output.sample();
        assert_eq!(panned.0, 0.0);
        assert!(panned.1.abs() > 0.0001);
        sender
            .send(OutputCommand::Channel {
                track: 0,
                gains: (0.0, 0.0),
                tone: crate::tone::targets(crate::project::ChannelSettings::default(), 8_000.0),
                sample_rate: 8_000.0,
            })
            .unwrap();
        for _ in 0..81 {
            output.sample();
        }
        assert_eq!(output.sample(), (0.0, 0.0));
        assert!(output.midi_previews[0].active.as_ref().unwrap().duration.is_none());
    }

    #[test]
    fn early_noteoff_releases_from_current_attack_level() {
        let mut held = GatedVoice::held(
            NoteEvent {
                instrument: Instrument::Pad,
                ..sine_event(0, DEFAULT_NOTE_DURATION)
            },
            1,
            8_000.0,
            true,
            42,
        );
        for _ in 0..80 {
            held.sample();
        }
        let level = smooth_gain(held.elapsed / held.attack);
        held.release_gate(1);
        assert_eq!(held.release_level, level);
        assert_eq!(held.attack, 0.0);
        for _ in 0..80 {
            held.sample();
        }
        held.release_gate(1);
        assert_eq!(held.release_level, level);
        assert!(held.release_level < 0.001);
    }

    #[test]
    fn held_gate_zero_fades_are_finite_and_immediate_release_retires() {
        let mut held = GatedVoice::held(sine_event(0, DEFAULT_NOTE_DURATION), 1, 8_000.0, true, 42);
        held.attack = 0.0;
        held.release = 0.0;
        let sample = held.sample().unwrap();
        assert!(sample.0.is_finite() && sample.1.is_finite());
        held.release_gate(1);
        assert!(held.sample().is_none());
        let mut immediate = GatedVoice::held(sine_event(0, DEFAULT_NOTE_DURATION), 2, 8_000.0, true, 42);
        immediate.release_gate(2);
        assert!(immediate.sample().is_none());
    }

    #[test]
    fn midi_cancellation_preserves_terminal_and_song_voices() {
        let (_, renderer) = prepare(8_000.0);
        let (sender, receiver) = mpsc::channel();
        let (retired, _rx) = mpsc::channel();
        let mut output = Output::new(renderer, receiver, retired);
        let voice = || GatedVoice::new(sine_event(0, DEFAULT_NOTE_DURATION), 8_000.0, true, 42);
        sender
            .send(OutputCommand::Trigger {
                track: 0,
                voice: voice(),
            })
            .unwrap();
        sender
            .send(OutputCommand::Preview {
                track: 0,
                voice: voice(),
            })
            .unwrap();
        sender
            .send(OutputCommand::MidiPreview {
                track: 0,
                voice: voice(),
            })
            .unwrap();
        output.sample();
        assert!(output.midi_previews[0].active.is_some());
        sender.send(OutputCommand::StopMidiPreview).unwrap();
        output.sample();
        assert!(output.midi_previews[0].active.is_none());
        assert!(output.tracks[0].active.is_some());
        assert!(output.previews[0].active.is_some());
    }

    #[test]
    fn preview_renders_without_stealing_song_voice_and_expires() {
        let (_, renderer) = prepare(8_000.0);
        let (sender, receiver) = mpsc::channel();
        let (retired, _rx) = mpsc::channel();
        let mut output = Output::new(renderer, receiver, retired);
        sender
            .send(OutputCommand::Trigger {
                track: 0,
                voice: GatedVoice::new(sine_event(0, Duration::from_secs(10)), 8_000.0, true, 42),
            })
            .unwrap();
        output.sample();
        for _ in 0..20 {
            sender
                .send(OutputCommand::Preview {
                    track: 0,
                    voice: GatedVoice::new(sine_event(0, DEFAULT_NOTE_DURATION), 8_000.0, true, 42),
                })
                .unwrap();
        }
        output.sample();
        assert!(output.tracks[0].releasing.is_none());
        assert!(output.tracks[0].active.as_ref().unwrap().elapsed > 0.0);
        assert!(output.previews[0].active.is_some());
        assert!(output.previews[0].releasing.is_some());
        let mut energy = 0.0;
        for _ in 0..8_000 {
            let sample = output.previews[0].sample(&output.retired);
            energy += sample.0.abs() + sample.1.abs();
        }
        assert!(energy > 0.0);
        assert!(output.previews[0].active.is_none());
        assert!(output.previews[0].releasing.is_none());
        assert!(output.tracks[0].active.is_some());
    }

    #[test]
    #[allow(clippy::float_cmp, clippy::cast_precision_loss)]
    fn channel_balance_neutral_endpoints_gain_and_sample_rate_ramps() {
        use crate::project::ChannelSettings;
        assert_eq!(channel_gains(ChannelSettings::default()), (1.0, 1.0));
        for (pan, expected) in [(-100.0, (1.0, 0.0)), (100.0, (0.0, 1.0))] {
            assert_eq!(
                channel_gains(ChannelSettings {
                    pan,
                    ..ChannelSettings::default()
                }),
                expected
            );
        }
        assert_eq!(
            channel_gains(ChannelSettings {
                volume_db: None,
                pan: 20.0,
                ..ChannelSettings::default()
            }),
            (0.0, 0.0)
        );
        let boosted = channel_gains(ChannelSettings {
            volume_db: Some(6.0),
            pan: 0.0,
            ..ChannelSettings::default()
        });
        assert!(boosted.0 > 1.99 && boosted.0 < 2.0);
        for rate in [8_000_u32, 44_100, 48_000, 96_000] {
            let mut mixer = ChannelProcessor::default();
            assert_eq!(mixer.process((0.25, -0.5)), (0.25, -0.5));
            mixer.set((0.0, 1.0), f64::from(rate));
            let frames = rate / 100;
            let mut previous = 1.0;
            for _ in 0..frames {
                let value = mixer.process((1.0, -0.5));
                assert!(value.0 <= previous && value.0 >= 0.0);
                assert!(previous - value.0 <= 1.01 / frames as f32);
                assert_eq!(value.1, -0.5);
                previous = value.0;
            }
            assert!(previous.abs() < 1e-6);
            mixer.set(boosted, f64::from(rate));
            for _ in 0..frames {
                let sample = mixer.process((2.0, -3.0));
                assert!(sample.0.is_finite() && sample.1.is_finite());
            }
            assert!(mixer.process((2.0, 2.0)).0 > 3.9); // No hidden limiter.
            mixer.reset();
            assert_eq!(mixer.remaining, 0);
            assert_eq!(mixer.current, mixer.target);
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn output_channel_edits_change_held_notes_and_tails_without_retriggering() {
        for rate in [8_000_u32, 48_000, 96_000] {
            let (_, renderer) = prepare(f64::from(rate));
            let (sender, receiver) = mpsc::channel();
            let (retired, _rx) = mpsc::channel();
            let mut output = Output::new(renderer, receiver, retired);
            sender
                .send(OutputCommand::Trigger {
                    track: 0,
                    voice: GatedVoice::new(sine_event(0, Duration::from_millis(100)), f64::from(rate), false, 42),
                })
                .unwrap();
            let mut energy = 0.0;
            for _ in 0..rate / 20 {
                let (l, r) = output.sample();
                energy += l.abs() + r.abs();
            }
            assert!(energy > 0.01);
            sender
                .send(OutputCommand::Channel {
                    track: 0,
                    gains: (0.0, 1.0),
                    tone: crate::tone::targets(crate::project::ChannelSettings::default(), 48_000.0),
                    sample_rate: f64::from(rate),
                })
                .unwrap();
            for _ in 0..rate / 100 {
                output.sample();
            }
            let mut right = 0.0;
            for _ in 0..rate / 100 {
                let (l, r) = output.sample();
                assert!(l.abs() < 1e-7);
                right += r.abs();
            }
            assert!(right > 0.001);
            // Past note-off: the same processor still controls the release.
            for _ in 0..rate / 20 {
                output.sample();
            }
            sender
                .send(OutputCommand::Channel {
                    track: 0,
                    gains: (1.0, 0.0),
                    tone: crate::tone::targets(crate::project::ChannelSettings::default(), 48_000.0),
                    sample_rate: f64::from(rate),
                })
                .unwrap();
            for _ in 0..rate / 100 {
                output.sample();
            }
            let mut left = 0.0;
            for _ in 0..rate / 100 {
                let (l, r) = output.sample();
                assert!(r.abs() < 1e-7);
                left += l.abs();
            }
            assert!(left > 0.001);
            // A retrigger crossfade is summed before balance, too.
            sender
                .send(OutputCommand::Trigger {
                    track: 0,
                    voice: GatedVoice::new(sine_event(0, Duration::from_secs(1)), f64::from(rate), true, 43),
                })
                .unwrap();
            for _ in 0..rate / 100 {
                assert!(output.sample().1.abs() < 1e-7);
            }
            // Track 1 remains neutral; muting track 0 cannot silence it.
            sender
                .send(OutputCommand::Channel {
                    track: 0,
                    gains: (0.0, 0.0),
                    tone: crate::tone::targets(crate::project::ChannelSettings::default(), 48_000.0),
                    sample_rate: f64::from(rate),
                })
                .unwrap();
            sender
                .send(OutputCommand::Trigger {
                    track: 1,
                    voice: GatedVoice::new(sine_event(1, Duration::from_secs(1)), f64::from(rate), true, 44),
                })
                .unwrap();
            for _ in 0..rate / 20 {
                output.sample();
            }
            let mut stereo = (0.0, 0.0);
            for _ in 0..rate / 100 {
                let (l, r) = output.sample();
                stereo.0 += l.abs();
                stereo.1 += r.abs();
            }
            assert!(stereo.0 > 0.001 && stereo.1 > 0.001);
            let (_, renderer) = prepare(f64::from(rate));
            sender.send(OutputCommand::Replace(renderer)).unwrap();
            assert_eq!(output.sample(), (0.0, 0.0));
            assert_eq!(output.tracks[0].mixer.remaining, 0);
            assert_eq!(output.tracks[0].mixer.target, (0.0, 0.0));
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn control_sync_coalesces_edits_history_append_and_load_before_triggers() {
        use crate::{command::Command, project::ChannelSettings, state::App};
        let mut app = App::default();
        let (sender, receiver) = mpsc::channel();
        let mut cached = Vec::new();
        sync_channel_targets(&sender, &mut cached, app.project.channels(), 48_000.0);
        assert_eq!(receiver.try_iter().count(), 4);
        sync_channel_targets(&sender, &mut cached, app.project.channels(), 48_000.0);
        assert_eq!(receiver.try_iter().count(), 0);
        app.edit(|edit| {
            edit.set_channel(
                2,
                ChannelSettings {
                    volume_db: None,
                    pan: 50.0,
                    ..ChannelSettings::default()
                },
            );
        });
        for command in [None, Some(Command::Undo), Some(Command::Redo)] {
            if let Some(command) = command {
                app.handle_command(command);
            }
            sync_channel_targets(&sender, &mut cached, app.project.channels(), 48_000.0);
            let commands: Vec<_> = receiver.try_iter().collect();
            assert_eq!(commands.len(), 1);
            assert!(matches!(commands[0], OutputCommand::Channel { track: 2, .. }));
        }
        app.handle_command(Command::AddTrack);
        sync_channel_targets(&sender, &mut cached, app.project.channels(), 48_000.0);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            OutputCommand::Channel { track: 4, .. }
        ));
        app.handle_command(Command::Undo);
        sync_channel_targets(&sender, &mut cached, app.project.channels(), 48_000.0);
        assert_eq!(receiver.try_iter().count(), 0);
        app.handle_command(Command::Redo);
        sync_channel_targets(&sender, &mut cached, app.project.channels(), 48_000.0);
        assert!(matches!(
            receiver.try_recv().unwrap(),
            OutputCommand::Channel { track: 4, .. }
        ));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loaded.json");
        let mut project = crate::project::Project::new(1, 1);
        project.channels[0] = ChannelSettings {
            volume_db: None,
            pan: -100.0,
            ..ChannelSettings::default()
        };
        crate::project_file::save(&project, &path).unwrap();
        app.execute_prompt(&format!("e! {}", path.display()));
        assert_eq!(app.project, project);
        sync_channel_targets(&sender, &mut cached, app.project.channels(), 48_000.0);
        let (_, renderer) = prepare(48_000.0);
        sender.send(OutputCommand::Replace(renderer)).unwrap();
        sender
            .send(OutputCommand::Trigger {
                track: 0,
                voice: GatedVoice::new(sine_event(0, Duration::from_secs(1)), 48_000.0, true, 1),
            })
            .unwrap();
        let (_, renderer) = prepare(48_000.0);
        let (retired, _rx) = mpsc::channel();
        let mut output = Output::new(renderer, receiver, retired);
        for _ in 0..1000 {
            assert_eq!(output.sample(), (0.0, 0.0));
        }
        assert!(output.tracks[0].active.is_some());
        assert_eq!(output.tracks[0].mixer.target, (0.0, 0.0));
    }

    fn sine_event(track: usize, duration: Duration) -> NoteEvent {
        NoteEvent {
            track,
            instrument: Instrument::Sine,
            note: Note {
                pitch: NotePitch::A,
                octave: 4,
                length_ticks: crate::tracker::NoteLength::default(),
            },
            beats: 0.25,
            duration,
        }
    }

    #[test]
    fn transport_512_tick_gates_hold_until_noteoff_then_release_in_both_paths() {
        let rate = 48_000_u32;
        let mut pattern = crate::tracker::Pattern::new(32, 1);
        let mut note = sine_event(0, Duration::ZERO).note;
        note.length_ticks = crate::tracker::NoteLength::try_from(512).unwrap();
        pattern.set_step(0, 0, Some(note)).unwrap();
        for instrument in [Instrument::Pad, Instrument::Drone, Instrument::Sustain] {
            let event = crate::transport::collect_row(
                &pattern,
                &[instrument],
                crate::transport::RowEvent {
                    row: 0,
                    beats: 0.25,
                    duration: Duration::from_millis(125),
                },
            )[0];
            assert_eq!(event.duration, Duration::from_secs(4));
            let release = match instrument {
                Instrument::Pad => 0.6,
                Instrument::Drone => 0.45,
                Instrument::Sustain => 0.04,
                _ => unreachable!(),
            };
            for legacy in [false, true] {
                let (mut seq, renderer) = prepare(f64::from(rate));
                let (sender, receiver) = mpsc::channel();
                let (retired, _retired_rx) = mpsc::channel();
                if legacy {
                    schedule_voice(&mut seq, note, instrument, event.duration, false, 42);
                } else {
                    sender
                        .send(OutputCommand::Trigger {
                            track: 0,
                            voice: GatedVoice::new(event, f64::from(rate), false, 42),
                        })
                        .unwrap();
                }
                let mut output = Output::new(renderer, receiver, retired);
                let mut raw = voice(note, instrument, false, 42);
                raw.set_sample_rate(f64::from(rate));
                raw.allocate();
                let mut reference = BlockRateAdapter::new(raw);
                let (mut reference_seq, mut legacy_reference) = prepare(f64::from(rate));
                if legacy {
                    schedule_voice(&mut reference_seq, note, instrument, Duration::from_secs(10), false, 42);
                }
                let end = 192_000;
                let tail =
                    usize::try_from(Duration::from_secs_f64(release).as_nanos() * u128::from(rate) / 1_000_000_000)
                        .unwrap();
                let mut early_energy = 0.0_f64;
                let mut late_energy = 0.0_f64;
                let mut reference_early = 0.0_f64;
                let mut reference_late = 0.0_f64;
                for i in 0..end + tail + 100 {
                    let actual = output.sample();
                    let expected = if legacy {
                        legacy_reference.get_stereo()
                    } else {
                        reference.get_stereo()
                    };
                    if (168_000..end).contains(&i) {
                        assert!(
                            (actual.0 - expected.0).abs() < 0.00001,
                            "{instrument:?}, legacy={legacy}: pre-noteoff sample {i}: {} != {}",
                            actual.0,
                            expected.0
                        );
                    }
                    if (end..end + tail / 4).contains(&i) {
                        early_energy += f64::from(actual.0 * actual.0);
                        reference_early += f64::from(expected.0 * expected.0);
                    }
                    if (end + tail * 3 / 4..end + tail).contains(&i) {
                        late_energy += f64::from(actual.0 * actual.0);
                        reference_late += f64::from(expected.0 * expected.0);
                    }
                    if i >= end + tail + 2 {
                        assert_eq!(actual, (0.0, 0.0));
                    }
                }
                assert!(
                    early_energy / reference_early > 0.8,
                    "{instrument:?}: missing release after noteoff, legacy={legacy}"
                );
                assert!(
                    late_energy / reference_late < 0.02,
                    "{instrument:?}: release did not decline, legacy={legacy}"
                );
            }
        }
    }

    #[test]
    fn zero_gates_are_silent_and_subsample_gates_remain_finite() {
        for instrument in Instrument::ALL {
            for duration in [Duration::ZERO, Duration::from_nanos(1)] {
                let event = NoteEvent {
                    instrument,
                    ..sine_event(0, duration)
                };
                let mut gated = GatedVoice::new(event, 48_000.0, false, 42);
                let (mut seq, mut renderer) = prepare(48_000.0);
                schedule_voice(&mut seq, event.note, instrument, duration, false, 42);
                for _ in 0..30000 {
                    let sample = gated.sample().unwrap_or((0.0, 0.0));
                    let legacy = renderer.get_stereo();
                    assert!(
                        [sample.0, sample.1, legacy.0, legacy.1].into_iter().all(f32::is_finite),
                        "{instrument:?} {duration:?}: gated={sample:?}, legacy={legacy:?}"
                    );
                    if duration.is_zero() {
                        assert_eq!(sample, (0.0, 0.0));
                        assert_eq!(legacy, (0.0, 0.0));
                    }
                }
                assert!(gated.sample().is_none());
                assert_eq!(renderer.get_stereo(), (0.0, 0.0));
            }
        }
    }

    #[test]
    fn gates_render_onset_hold_and_release_at_multiple_rates_and_row_periods() {
        for rate in [8_000_u32, 44_100, 48_000] {
            for milliseconds in [50, 125, 750] {
                for ticks in [1_u16, 16, 4096] {
                    let mut pattern = crate::tracker::Pattern::new(1, 1);
                    let mut event = sine_event(0, Duration::ZERO);
                    event.note.length_ticks = crate::tracker::NoteLength::try_from(ticks).unwrap();
                    pattern.set_step(0, 0, Some(event.note)).unwrap();
                    let event = crate::transport::collect_row(
                        &pattern,
                        &[Instrument::Sine],
                        crate::transport::RowEvent {
                            row: 0,
                            beats: 0.25,
                            duration: Duration::from_millis(milliseconds),
                        },
                    )[0];
                    let mut voice = GatedVoice::new(event, f64::from(rate), true, 42);
                    assert_eq!(voice.sample().unwrap(), (0.0, 0.0));
                    let mut count = 1_u64;
                    let mut energy = 0.0_f64;
                    let mut last = 0.0_f32;
                    let mut max_jump = 0.0_f32;
                    while let Some((l, r)) = voice.sample() {
                        assert!(l.is_finite() && r.is_finite());
                        energy += f64::from(r.mul_add(r, l * l));
                        max_jump = max_jump.max((l - last).abs());
                        last = l;
                        count += 1;
                    }
                    let rendered = Duration::from_secs_f64(f64::from(u32::try_from(count).unwrap()) / f64::from(rate));
                    assert!(
                        rendered.abs_diff(event.duration + Duration::from_secs_f64(voice.release))
                            <= Duration::from_secs_f64(2.0 / f64::from(rate)),
                        "{rate} Hz, {milliseconds} ms row, {ticks} ticks: {rendered:?}"
                    );
                    assert!(energy > 0.001);
                    let final_sample_bound =
                        0.13_f32.mul_add(smooth_gain(voice.sample_period / voice.release), 0.00001);
                    assert!(
                        last.abs() <= final_sample_bound,
                        "release: {last} > {final_sample_bound} at {rate} Hz"
                    );
                    assert!(max_jump < 0.06, "discontinuous gate: {max_jump}");
                    assert!(voice.sample().is_none());
                }
            }
        }
    }

    #[test]
    fn track_retrigger_fades_old_voice_and_bounds_releases_while_other_tracks_mix() {
        let (_, renderer) = prepare(48_000.0);
        let (triggers, replacements) = mpsc::channel();
        let (retired, retired_rx) = mpsc::channel();
        let mut output = Output::new(renderer, replacements, retired);
        let event = NoteEvent {
            instrument: Instrument::Pluck,
            ..sine_event(0, Duration::from_secs(10))
        };
        triggers
            .send(OutputCommand::Trigger {
                track: 0,
                voice: GatedVoice::new(event, 48_000.0, true, 42),
            })
            .unwrap();
        for _ in 0..4800 {
            output.sample();
        }
        let new = NoteEvent {
            instrument: Instrument::Sine,
            note: Note {
                octave: 5,
                ..event.note
            },
            ..event
        };
        triggers
            .send(OutputCommand::Trigger {
                track: 0,
                voice: GatedVoice::new(new, 48_000.0, true, 43),
            })
            .unwrap();
        let mut reference = GatedVoice::new(new, 48_000.0, true, 43);
        let mut difference = 0.0_f32;
        for i in 0..4800 {
            let sample = output.sample();
            let expected = reference.sample().unwrap();
            if i < 200 {
                difference += (sample.0 - expected.0).abs();
            }
            if i > 241 {
                assert_eq!(sample, expected, "old voice survived release");
            }
        }
        assert!(difference > 0.1, "old voice was cut rather than faded");
        assert!(output.tracks[0].releasing.is_none());
        let other = sine_event(1, Duration::from_secs(10));
        triggers
            .send(OutputCommand::Trigger {
                track: 1,
                voice: GatedVoice::new(other, 48_000.0, true, 44),
            })
            .unwrap();
        let mut second = GatedVoice::new(other, 48_000.0, true, 44);
        for _ in 0..4800 {
            let a = reference.sample().unwrap();
            let b = second.sample().unwrap();
            assert_eq!(output.sample(), (a.0 + b.0, a.1 + b.1));
        }
        for _ in 0..100 {
            triggers
                .send(OutputCommand::Trigger {
                    track: 0,
                    voice: GatedVoice::new(event, 48_000.0, false, 45),
                })
                .unwrap();
            for _ in 0..10 {
                output.sample();
            }
            assert!(output.tracks[0].active.is_some());
            assert!(output.tracks[0].releasing.is_some());
            assert!(output.tracks[1].active.is_some());
        }
        assert!(retired_rx.try_iter().count() >= 99);
        for _ in 0..300 {
            output.sample();
        }
        assert!(output.tracks[0].releasing.is_none());
    }

    #[test]
    fn track_fifo_cancellation_discards_queued_and_releasing_voices() {
        let (_, renderer) = prepare(48_000.0);
        let (reset, replacements) = mpsc::channel();
        let triggers = reset.clone();
        let (retired, _retired_rx) = mpsc::channel();
        let mut output = Output::new(renderer, replacements, retired);
        let event = sine_event(0, Duration::from_secs(100));
        for _ in 0..2 {
            triggers
                .send(OutputCommand::Trigger {
                    track: 0,
                    voice: GatedVoice::new(event, 48_000.0, false, 42),
                })
                .unwrap();
            for _ in 0..100 {
                output.sample();
            }
        }
        assert!(output.tracks[0].releasing.is_some());
        triggers
            .send(OutputCommand::Trigger {
                track: 1,
                voice: GatedVoice::new(event, 48_000.0, false, 42),
            })
            .unwrap();
        let (_, renderer) = prepare(48_000.0);
        reset.send(OutputCommand::Replace(renderer)).unwrap();
        for _ in 0..1000 {
            assert_eq!(output.sample(), (0.0, 0.0));
        }
        assert!(output
            .tracks
            .iter()
            .all(|track| track.active.is_none() && track.releasing.is_none()));
        let restart = sine_event(0, Duration::from_millis(50));
        triggers
            .send(OutputCommand::Trigger {
                track: 0,
                voice: GatedVoice::new(restart, 48_000.0, true, 43),
            })
            .unwrap();
        let mut reference = GatedVoice::new(restart, 48_000.0, true, 43);
        for _ in 0..13000 {
            assert_eq!(output.sample(), reference.sample().unwrap_or((0.0, 0.0)));
        }
        assert!(output.tracks[0].active.is_none());
    }

    #[test]
    fn held_gate_crosses_pattern_and_song_loop_and_ignores_later_tempo() {
        use crate::{command::Command, state::App, transport::Transport};
        let mut app = App::default();
        app.project.patterns = vec![crate::tracker::Pattern::new(1, 4), crate::tracker::Pattern::new(2, 4)];
        app.project.order = vec![0, 1];
        app.project.instruments[0] = Instrument::Sine;
        let mut note = sine_event(0, Duration::ZERO).note;
        note.length_ticks = crate::tracker::NoteLength::try_from(256).unwrap();
        app.project.patterns[0].set_step(0, 0, Some(note)).unwrap();
        app.handle_command(Command::ToggleSong);
        app.take_stop_request();
        app.handle_command(Command::TogglePlayback);
        let mut clock = Transport::default();
        let start = std::time::Instant::now();
        let event = clock.update_app(start, &mut app, || panic!("unexpected cancellation"))[0];
        assert_eq!(event.duration, Duration::from_secs(2));
        let mut voice = GatedVoice::new(event, 48_000.0, true, 42);
        for _ in 0..6000 {
            voice.sample();
        }
        app.project.bpm = 60.0;
        assert!(clock
            .update_app(start + Duration::from_millis(125), &mut app, || panic!(
                "boundary cancelled gate"
            ))
            .is_empty());
        assert_eq!(app.playing_pattern, 1);
        for _ in 0..24000 {
            voice.sample();
        }
        let next = clock.update_app(start + Duration::from_millis(625), &mut app, || {
            panic!("loop cancelled gate")
        })[0];
        assert_eq!(app.playing_pattern, 0);
        assert_eq!(next.duration, Duration::from_secs(4));
        assert!(voice.sample().is_some());
        assert_eq!(voice.duration, Some(2.0));
    }

    fn render_output(instruments: &[Instrument], rate: u32, event: NoteEvent) -> Vec<(f32, f32)> {
        let (_, renderer) = prepare(f64::from(rate));
        let (sender, receiver) = mpsc::channel();
        let (retired, _retired_rx) = mpsc::channel();
        let mut output = Output::new(renderer, receiver, retired);
        // These preset regressions need only their populated channels, not 128
        // silent song/preview pairs per sample. Mixer capacity has separate tests.
        output.tracks.truncate(instruments.len());
        output.previews.truncate(instruments.len());
        output.channel_samples.truncate(instruments.len());
        for (track, &instrument) in instruments.iter().enumerate() {
            sender
                .send(OutputCommand::Trigger {
                    track,
                    voice: GatedVoice::new(
                        NoteEvent {
                            track,
                            instrument,
                            ..event
                        },
                        f64::from(rate),
                        false,
                        42,
                    ),
                })
                .unwrap();
        }
        let tail = instruments
            .iter()
            .map(|&instrument| envelope_times(instrument, event.duration.as_secs_f64()).1)
            .fold(0.0_f64, f64::max);
        let count = usize::try_from(
            (event.duration + Duration::from_secs_f64(tail) + Duration::from_millis(20)).as_nanos() * u128::from(rate)
                / 1_000_000_000,
        )
        .unwrap();
        let samples = (0..count).map(|_| output.sample()).collect();
        assert!(output
            .tracks
            .iter()
            .all(|track| track.active.is_none() && track.releasing.is_none()));
        samples
    }

    fn rms(samples: &[(f32, f32)]) -> f64 {
        (samples.iter().map(|(l, r)| f64::from(l * l + r * r) / 2.0).sum::<f64>()
            / f64::from(u32::try_from(samples.len()).unwrap()))
        .sqrt()
    }

    #[test]
    fn sustained_production_gates_hold_energy_follow_pitch_and_release() {
        for rate in [8_000_u32, 44_100, 48_000] {
            for bpm in [20_u32, 120, 300] {
                for octave in [2, 4, 5] {
                    let event = NoteEvent {
                        note: Note {
                            octave,
                            ..sine_event(0, Duration::ZERO).note
                        },
                        ..sine_event(0, Duration::from_secs_f64(240.0 / f64::from(bpm)))
                    };
                    for instrument in [Instrument::Pad, Instrument::Drone, Instrument::Sustain] {
                        let samples = render_output(&[instrument], rate, event);
                        assert_eq!(samples, render_output(&[instrument], rate, event));
                        let end =
                            usize::try_from(event.duration.as_nanos() * u128::from(rate) / 1_000_000_000).unwrap();
                        let early = rms(&samples[end / 4..end / 2]);
                        let late = rms(&samples[end / 2..end * 3 / 4]);
                        assert!((0.015..0.13).contains(&early), "{instrument:?}: {early}");
                        assert!(late > early * 0.25, "{instrument:?} died early: {early}, {late}");
                        assert_eq!(samples[0], (0.0, 0.0));
                        assert!(
                            samples.windows(2).all(|s| (s[1].0 - s[0].0).abs() < 0.15),
                            "{instrument:?} discontinuity at {rate} Hz"
                        );
                        assert!(samples.iter().all(|(l, r)| l.is_finite() && r.is_finite()));
                        assert!(rms(&samples[end - end / 16..end]) > 0.001);
                        let (attack, release) = envelope_times(instrument, event.duration.as_secs_f64());
                        assert!(attack <= event.duration.as_secs_f64() * 0.25);
                        assert!(release > 0.0);
                        let release_samples = usize::try_from(
                            Duration::from_secs_f64(release).as_nanos() * u128::from(rate) / 1_000_000_000,
                        )
                        .unwrap();
                        let total_end = end + release_samples;
                        assert!(rms(&samples[end..end + release_samples / 4]) > 0.001);
                        assert!(rms(&samples[total_end - release_samples / 8..total_end]) < late * 0.03);
                        assert!(samples[total_end - 1].0.abs() < 0.0001);
                        assert!(samples[total_end + 2..].iter().all(|s| *s == (0.0, 0.0)));
                        // Spectral projection tolerates Pad's beating, unlike zero crossings.
                        let window = &samples[end / 4..end * 3 / 4];
                        let power = |ratio: f64| {
                            let mut real = 0.0;
                            let mut imaginary = 0.0;
                            for (index, sample) in window.iter().enumerate().step_by(8) {
                                let phase = std::f64::consts::TAU
                                    * event.note.freq()
                                    * ratio
                                    * f64::from(u32::try_from(index).unwrap())
                                    / f64::from(rate);
                                real = f64::from(sample.0).mul_add(phase.cos(), real);
                                imaginary = f64::from(sample.0).mul_add(phase.sin(), imaginary);
                            }
                            real.mul_add(real, imaginary * imaginary)
                        };
                        let fundamental = [0.997, 1.0, 1.003].into_iter().map(power).fold(0.0_f64, f64::max);
                        assert!(
                            fundamental > power(0.7) * 20.0 && fundamental > power(1.3) * 20.0,
                            "{instrument:?} does not follow {} Hz",
                            event.note.freq()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn production_short_gates_keep_full_preset_release_after_noteoff() {
        for rate in [44_100_u32, 48_000] {
            for micros in [3125, 7813, 46875] {
                let event = sine_event(0, Duration::from_micros(micros));
                for instrument in Instrument::ALL {
                    let samples = render_output(&[instrument], rate, event);
                    let end = usize::try_from(event.duration.as_nanos() * u128::from(rate) / 1_000_000_000).unwrap();
                    assert_eq!(samples[0], (0.0, 0.0));
                    assert!(rms(&samples[..end]) > 0.0001, "{instrument:?} at {rate} Hz");
                    assert!(samples.iter().all(|(l, r)| l.is_finite() && r.is_finite()));
                    assert!(rms(&samples[end..end + 32]) > 0.00001, "{instrument:?}: missing tail");
                    let release = envelope_times(instrument, event.duration.as_secs_f64()).1;
                    let total_end = end
                        + usize::try_from(
                            Duration::from_secs_f64(release).as_nanos() * u128::from(rate) / 1_000_000_000,
                        )
                        .unwrap();
                    assert!(samples[total_end + 2..].iter().all(|s| *s == (0.0, 0.0)));
                }
            }
        }
    }

    #[test]
    fn production_open_hat_has_a_longer_normalized_tail_and_balanced_attack() {
        for rate in [44_100_u32, 48_000] {
            let event = sine_event(0, Duration::from_secs(1));
            let closed = render_output(&[Instrument::HiHat], rate, event);
            let open = render_output(&[Instrument::OpenHat], rate, event);
            let index = |ms: usize| usize::try_from(rate).unwrap() * ms / 1000;
            let onset = |s: &[(f32, f32)]| rms(&s[..index(5)]);
            let tail = |s: &[(f32, f32)]| rms(&s[index(100)..index(200)]) / onset(s);
            assert!(onset(&open) > 0.03 && onset(&open) < 0.15);
            assert!((0.8..1.5).contains(&(onset(&open) / onset(&closed))));
            assert!(tail(&open) > tail(&closed) * 20.0);
            assert!(tail(&open) > 0.1);
        }
    }

    #[test]
    fn production_four_track_sustained_mix_has_headroom_and_bounded_retriggers() {
        let instruments = [
            Instrument::Pad,
            Instrument::Drone,
            Instrument::Sustain,
            Instrument::OpenHat,
        ];
        let samples = render_output(&instruments, 48_000, sine_event(0, Duration::from_secs(4)));
        assert!(samples.iter().all(|(l, r)| l.abs() < 1.0 && r.abs() < 1.0));
        assert!(rms(&samples[48_000..96_000]) > 0.025);
        let (_, renderer) = prepare(48_000.0);
        let (sender, receiver) = mpsc::channel();
        let (retired, retired_rx) = mpsc::channel();
        let mut output = Output::new(renderer, receiver, retired);
        for _ in 0..100 {
            for (track, instrument) in instruments.into_iter().enumerate() {
                sender
                    .send(OutputCommand::Trigger {
                        track,
                        voice: GatedVoice::new(
                            NoteEvent {
                                instrument,
                                ..sine_event(track, Duration::from_secs(10))
                            },
                            48_000.0,
                            false,
                            42,
                        ),
                    })
                    .unwrap();
            }
            for _ in 0..480 {
                let (l, r) = output.sample();
                assert!(l.abs() < 1.0 && r.abs() < 1.0);
            }
            assert_eq!(output.tracks.iter().filter(|t| t.active.is_some()).count(), 4);
            assert!(output.tracks.iter().all(|t| t.releasing.is_none()));
        }
        assert!(retired_rx.try_iter().count() >= 396);
        let (_, renderer) = prepare(48_000.0);
        sender.send(OutputCommand::Replace(renderer)).unwrap();
        for _ in 0..1000 {
            assert_eq!(output.sample(), (0.0, 0.0));
        }
    }

    fn render_presets(instruments: &[Instrument], dry: bool, duration: Duration) -> Vec<(f32, f32)> {
        let (mut seq, mut renderer) = prepare(48_000.0);
        for instrument in instruments {
            schedule_voice(
                &mut seq,
                Note {
                    length_ticks: crate::tracker::NoteLength::default(),
                    pitch: NotePitch::C,
                    octave: 4,
                },
                *instrument,
                duration,
                dry,
                42,
            );
        }
        let release = instruments
            .iter()
            .map(|&instrument| envelope_times(instrument, duration.as_secs_f64()).1)
            .fold(0.0_f64, f64::max);
        let count = usize::try_from(
            (duration + Duration::from_secs_f64(release) + Duration::from_millis(20)).as_nanos() * 48_000
                / 1_000_000_000,
        )
        .unwrap();
        (0..count).map(|_| renderer.get_stereo()).collect()
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
                    let release = envelope_times(instrument, duration.as_secs_f64()).1;
                    let end = usize::try_from(
                        (duration + Duration::from_secs_f64(release)).as_nanos() * 48_000 / 1_000_000_000,
                    )
                    .unwrap();
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
        assert!(samples[34800..].iter().all(|sample| *sample == (0.0, 0.0)));
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
        for instrument in [
            Instrument::Kick,
            Instrument::Snare,
            Instrument::HiHat,
            Instrument::OpenHat,
        ] {
            let mut low = voice(
                Note {
                    length_ticks: crate::tracker::NoteLength::default(),
                    pitch: NotePitch::C,
                    octave: 0,
                },
                instrument,
                true,
                42,
            );
            let mut high = voice(
                Note {
                    length_ticks: crate::tracker::NoteLength::default(),
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
            let mut output = Output::new(renderer, replacements, retired);
            let note = Note {
                length_ticks: crate::tracker::NoteLength::default(),
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
            tx.send(OutputCommand::Replace(renderer)).unwrap();
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
            tx.send(OutputCommand::Replace(renderer)).unwrap();
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
        let mut output = Output::new(renderer, replacements, retired);
        let mut app = App::default();
        let note = Note {
            length_ticks: crate::tracker::NoteLength::default(),
            pitch: NotePitch::C,
            octave: 4,
        };
        app.project.patterns[0].set_step(0, 8, Some(note)).unwrap();
        // A long old gate must not be reconstructed when starting later.
        tx.send(OutputCommand::Trigger {
            track: 0,
            voice: GatedVoice::new(
                NoteEvent {
                    instrument: Instrument::Pluck,
                    note,
                    ..sine_event(0, Duration::from_secs(10))
                },
                48_000.0,
                false,
                42,
            ),
        })
        .unwrap();
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
            tx.send(OutputCommand::Trigger {
                track: event.track,
                voice: GatedVoice::new(event, 48_000.0, false, 43),
            })
            .unwrap();
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
                length_ticks: crate::tracker::NoteLength::default(),
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
