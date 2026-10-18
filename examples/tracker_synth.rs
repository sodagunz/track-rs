//! Exercise tracker synth routing through the default output device (or CI's null device).
use std::{thread::sleep, time::Duration};

use track_rs::{
    audio::AudioEngine,
    history::History,
    project::Project,
    tracker::{Instrument, Note, NoteLength, NotePitch},
    transport::NoteEvent,
};

fn main() -> anyhow::Result<()> {
    let mut project = Project::new(4, 1);
    project.set_instrument(0, Instrument::Synth);
    let mut patch = project.synth_patch(0);
    patch.oscillators[1].gain = 0.3;
    patch.oscillators[1].fine = 9.0;
    patch.envelope.release = 0.4;
    let mut history = History::default();
    history.edit(&mut project, |edit| edit.set_synth_patch(0, patch));
    let mut audio = AudioEngine::new()?;
    audio.sync_channels(project.channels());
    audio.sync_synth_patches(project.synth_patches());
    let event = NoteEvent {
        track: 0,
        instrument: project.instrument(0),
        note: Note {
            pitch: NotePitch::A,
            octave: 3,
            length_ticks: NoteLength::default(),
        },
        beats: 1.0,
        duration: Duration::from_millis(150),
    };
    audio.play_event(event);
    sleep(Duration::from_millis(250));
    audio.preview_event(event); // Independent from the still-releasing song voice.
    audio.midi_preview_event(event, 1);
    sleep(Duration::from_millis(150));
    patch.cutoff = 300.0;
    patch.q = 2.0;
    history.edit(&mut project, |edit| edit.set_synth_patch(0, patch));
    audio.sync_synth_patches(project.synth_patches());
    sleep(Duration::from_millis(100));
    audio.release_midi_preview(1);
    sleep(Duration::from_millis(500));
    audio.stop();
    sleep(Duration::from_millis(20));
    Ok(())
}
