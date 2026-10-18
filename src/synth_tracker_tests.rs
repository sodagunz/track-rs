#![allow(clippy::float_cmp)]

use crate::{
    bindings::handle_key,
    command::Command,
    project::Project,
    project_file,
    state::App,
    synth::{SynthPatch, Waveform},
    tracker::Instrument,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn patches_are_independent_retained_and_undoable_without_note_edits() {
    let mut app = App::default();
    let notes = app.project.patterns.clone();
    let original = app.project.synth_patch(0);
    let edited = SynthPatch {
        cutoff: 321.0,
        ..original
    };
    app.edit(|edit| edit.set_synth_patch(0, edited));
    assert!(app.is_dirty());
    assert_eq!(app.project.synth_patch(1), original);
    assert_eq!(app.project.synth_patch(0), edited);
    let revision = app.revision();
    app.edit(|edit| {
        edit.set_synth_patch(0, edited);
        edit.set_synth_patch(128, original);
        edit.set_synth_patch(0, SynthPatch { q: f64::NAN, ..edited });
    });
    assert_eq!(app.revision(), revision);
    app.handle_command(Command::SetInstrument(Instrument::Synth));
    app.handle_command(Command::SetInstrument(Instrument::Sine));
    app.handle_command(Command::SetInstrument(Instrument::Synth));
    assert_eq!(app.project.synth_patch(0), edited);
    for _ in 0..4 {
        app.handle_command(Command::Undo);
    }
    assert!(!app.is_dirty());
    assert_eq!(app.project.synth_patch(0), original);
    for _ in 0..4 {
        app.handle_command(Command::Redo);
    }
    assert_eq!(app.project.synth_patch(0), edited);
    assert_eq!(app.project.patterns, notes);
}

#[test]
fn append_undo_redo_keeps_patch_alignment_and_values() {
    let mut app = App::default();
    let tracks = app.project.instruments().len();
    app.handle_command(Command::AddTrack);
    assert_eq!(app.project.synth_patches().len(), tracks + 1);
    let patch = SynthPatch {
        q: 3.0,
        ..SynthPatch::default()
    };
    app.edit(|edit| edit.set_synth_patch(tracks, patch));
    app.handle_command(Command::Undo);
    app.handle_command(Command::Undo);
    assert_eq!(app.project.synth_patches().len(), tracks);
    app.handle_command(Command::Redo);
    app.handle_command(Command::Redo);
    assert_eq!(app.project.synth_patch(tracks), patch);
    assert_eq!(app.project.synth_patch(tracks - 1), SynthPatch::default());
    assert_eq!(app.project.channels().len(), app.project.synth_patches().len());
}

#[test]
fn v4_patch_round_trip_defaults_and_invalid_loads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("synth.json");
    let mut project = Project::new(4, 2);
    let mut history = crate::history::History::default();
    let mut settings = SynthPatch {
        cutoff: 20_000.0,
        q: 4.0,
        ..SynthPatch::default()
    };
    settings.oscillators[2].waveform = Waveform::Triangle;
    settings.envelope.release = 10.0;
    history.edit(&mut project, |edit| edit.set_synth_patch(1, settings));
    project.set_instrument(1, Instrument::Synth);
    project_file::save(&project, &path).unwrap();
    assert_eq!(project_file::load(&path).unwrap(), project);
    let encoded: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(encoded["version"], 4);
    let mut app = App {
        project: project.clone(),
        ..App::default()
    };
    app.handle_command(Command::TogglePlayback);
    let before = app.clone();
    for mutation in 0..7 {
        let mut corrupt = encoded.clone();
        match mutation {
            0 => corrupt["synth_patches"] = serde_json::json!([]),
            1 => corrupt["synth_patches"][1]["q"] = serde_json::json!(4.1),
            2 => corrupt["synth_patches"][1]["cutoff"] = serde_json::json!(0),
            3 => corrupt["synth_patches"][1]["oscillators"][0]["gain"] = serde_json::json!(2),
            4 => corrupt["synth_patches"][1]["envelope"]["release"] = serde_json::json!(11),
            5 => corrupt["synth_patches"][1]["envelope"]["typo"] = serde_json::json!(1),
            _ => corrupt["synth_patches"][1]["q"] = serde_json::json!("NaN"),
        }
        std::fs::write(&path, serde_json::to_vec(&corrupt).unwrap()).unwrap();
        assert!(project_file::load(&path).is_err(), "mutation {mutation}");
        app.execute_prompt(&format!("e! {}", path.display()));
        assert_eq!(app.project, before.project);
        assert_eq!(app.revision(), before.revision());
        assert!(matches!(app.playback_state, crate::state::PlaybackState::Playing));
    }
    let mut missing = encoded;
    missing.as_object_mut().unwrap().remove("synth_patches");
    std::fs::write(&path, serde_json::to_vec(&missing).unwrap()).unwrap();
    let migrated = project_file::load(&path).unwrap();
    assert_eq!(migrated.synth_patches(), &[SynthPatch::default(); 2]);
    assert_eq!(migrated.instruments(), project.instruments());
    assert_eq!(migrated.patterns, project.patterns);
    project_file::save(&project, &path).unwrap();
    let previous = std::fs::read(&path).unwrap();
    project.synth_patches.pop();
    assert!(project_file::save(&project, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), previous);
}

#[test]
fn f4_selects_synth_and_auditions_without_a_write() {
    let mut app = App::default();
    let key = |app: &mut App, code| handle_key(KeyEvent::new(code, KeyModifiers::NONE), app);
    key(&mut app, KeyCode::F(4));
    key(&mut app, KeyCode::Char('['));
    assert_eq!(app.project.instrument(0), Instrument::Synth);
    let before = app.project.clone();
    let revision = app.revision();
    key(&mut app, KeyCode::Enter);
    let preview = app.take_audition().unwrap();
    assert_eq!(preview.instrument, Instrument::Synth);
    assert_eq!(preview.track, 0);
    assert_eq!(app.project, before);
    assert_eq!(app.revision(), revision);
    app.handle_command(Command::Undo);
    assert!(!app.is_dirty());
}
