use super::*;

fn event(duration: Duration) -> NoteEvent {
    NoteEvent {
        track: 0,
        instrument: Instrument::Synth,
        note: Note {
            pitch: crate::tracker::NotePitch::A,
            octave: 4,
            length_ticks: crate::tracker::NoteLength::default(),
        },
        beats: 1.0,
        duration,
    }
}

fn output(rate: f64) -> (Sender<OutputCommand>, SyncSender<(usize, SynthPatch)>, Output) {
    let (_, renderer) = prepare(rate);
    let (sender, receiver) = mpsc::channel();
    let (retired, _) = mpsc::channel();
    let (updates, patches) = mpsc::sync_channel(crate::project_file::MAX_TRACKS);
    let mut output = Output::new(renderer, receiver, retired);
    output.synth_updates = patches;
    (sender, updates, output)
}

#[test]
#[allow(clippy::cast_possible_truncation)]
fn routed_synth_matches_its_adsr_without_outer_shaping_or_release_cap() {
    for rate in [8000, 12_000, 48_000, 96_000] {
        let note = event(Duration::from_millis(20));
        let mut patch = SynthPatch {
            cutoff: 20_000.0,
            q: 4.0,
            ..SynthPatch::default()
        };
        patch.envelope.attack = 0.1; // Gate-off interrupts attack.
        patch.envelope.release = 1.0; // Longer than every legacy preset's release.
        let (sender, _, mut output) = output(f64::from(rate));
        sender
            .send(OutputCommand::Trigger {
                track: 0,
                voice: GatedVoice::with_patch(note, patch, f64::from(rate), true, 42),
            })
            .unwrap();
        let mut reference = SynthVoice::new(patch, f64::from(rate)).unwrap();
        reference.gate_on(note.note.freq()).unwrap();
        let mut elapsed = 0.0;
        let mut tail_energy = 0.0;
        for i in 0..rate * 2 {
            if elapsed >= note.duration.as_secs_f64() {
                reference.gate_off();
            }
            let sample = reference.sample() as f32;
            let actual = output.sample();
            assert_eq!(actual, (sample, sample));
            assert!(actual.0.is_finite());
            if i > rate / 2 {
                tail_energy += actual.0.abs();
            }
            elapsed += f64::from(rate).recip();
        }
        assert!(tail_energy > 0.01);
        assert!(output.tracks[0].active.is_none());
    }
}

#[test]
#[allow(clippy::cast_possible_truncation)]
fn held_midi_synth_uses_token_release_and_live_targets_during_tail() {
    let rate = 8000;
    let note = event(DEFAULT_NOTE_DURATION);
    let mut patch = SynthPatch::default();
    patch.envelope.release = 1.0;
    let (sender, updates, mut output) = output(f64::from(rate));
    sender
        .send(OutputCommand::MidiPreview {
            track: 0,
            voice: GatedVoice::held_with_patch(note, patch, 7, f64::from(rate), true, 42),
        })
        .unwrap();
    sender.send(OutputCommand::ReleaseMidiPreview(6)).unwrap();
    let mut reference = SynthVoice::new(patch, f64::from(rate)).unwrap();
    reference.gate_on(note.note.freq()).unwrap();
    for _ in 0..rate {
        let sample = reference.sample() as f32;
        assert_eq!(output.sample(), (sample, sample));
    }
    assert!(output.midi_previews[0].active.as_ref().unwrap().duration.is_none());
    sender.send(OutputCommand::ReleaseMidiPreview(7)).unwrap();
    reference.gate_off();
    for i in 0..rate + 2 {
        if i == rate / 4 {
            patch.cutoff = 20.0;
            patch.q = 4.0;
            patch.oscillators[0].fine = -100.0;
            patch.oscillators[0].gain = 0.2;
            patch.oscillators[0].waveform = crate::synth::Waveform::Square;
            patch.envelope.release = 10.0; // Does not extend the existing release.
            updates.try_send((0, patch)).unwrap();
            reference.apply_patch(patch).unwrap();
        }
        let sample = reference.sample() as f32;
        assert_eq!(output.sample(), (sample, sample));
    }
    assert!(output.midi_previews[0].active.is_none());
}

#[test]
fn synth_retrigger_preview_isolation_channel_processing_and_stop() {
    let rate = 8000.0;
    let (sender, updates, mut output) = output(rate);
    let note = event(Duration::from_secs(3));
    for token in 1..=32 {
        sender
            .send(OutputCommand::MidiPreview {
                track: 0,
                voice: GatedVoice::held(note, token, rate, true, 42),
            })
            .unwrap();
        output.sample();
    }
    assert!(output.midi_previews[0].active.is_some());
    assert!(output.midi_previews[0].releasing.is_some());
    sender
        .send(OutputCommand::Trigger {
            track: 0,
            voice: GatedVoice::new(note, rate, true, 42),
        })
        .unwrap();
    sender
        .send(OutputCommand::Preview {
            track: 0,
            voice: GatedVoice::new(note, rate, true, 42),
        })
        .unwrap();
    sender
        .send(OutputCommand::Channel {
            track: 0,
            gains: (0.0, 1.0),
            tone: crate::tone::targets(crate::project::ChannelSettings::default(), rate),
            sample_rate: rate,
        })
        .unwrap();
    let mut patch = SynthPatch::default();
    patch.oscillators[0].gain = 0.5;
    updates.try_send((0, patch)).unwrap();
    for _ in 0..1000 {
        output.sample();
    }
    assert!(output.midi_previews[0].releasing.is_none());
    assert!(output.tracks[0].active.is_some());
    assert!(output.previews[0].active.is_some());
    let mut right = 0.0;
    for _ in 0..1000 {
        let sample = output.sample();
        assert_eq!(sample.0, 0.0);
        right += sample.1.abs();
    }
    assert!(right > 0.01);
    sender.send(OutputCommand::StopMidiPreview).unwrap();
    output.sample();
    assert!(output.midi_previews[0].active.is_none());
    assert!(output.tracks[0].active.is_some());
    assert!(output.previews[0].active.is_some());
    sender.send(OutputCommand::Replace(prepare(rate).1)).unwrap();
    assert_eq!(output.sample(), (0.0, 0.0));
    for slot in [&output.tracks[0], &output.previews[0], &output.midi_previews[0]] {
        assert!(slot.active.is_none() && slot.releasing.is_none());
    }
}

#[test]
fn patch_updates_are_bounded_retryable_and_noop_aware() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let mut cached = Vec::new();
    let patches = [SynthPatch::default(); 2];
    sync_synth_targets(&sender, &mut cached, &patches);
    assert_eq!(cached, [Some(patches[0]), None]);
    assert_eq!(receiver.try_recv().unwrap(), (0, patches[0]));
    sync_synth_targets(&sender, &mut cached, &patches);
    assert_eq!(receiver.try_recv().unwrap(), (1, patches[1]));
    sync_synth_targets(&sender, &mut cached, &patches);
    assert!(receiver.try_recv().is_err());
    let invalid = [SynthPatch {
        q: f64::NAN,
        ..patches[0]
    }];
    sync_synth_targets(&sender, &mut cached, &invalid);
    assert_eq!(cached, patches.map(Some));
    assert!(receiver.try_recv().is_err());
}
