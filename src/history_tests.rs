#![allow(clippy::float_cmp, clippy::unnecessary_literal_unwrap)]

use crate::{
    bindings::handle_key,
    command::Command,
    history::{MAX_MUTATIONS, MAX_TRANSACTIONS},
    state::{App, Mode, PlaybackState},
    tracker::{Instrument, Note, NotePitch},
    transport::{collect_row, Transport},
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::time::Instant;

fn key(app: &mut App, ch: char, modifiers: KeyModifiers) {
    handle_key(KeyEvent::new(KeyCode::Char(ch), modifiers), app);
}

#[test]
fn every_document_edit_round_trips_with_revision_identity() {
    let mut app = App::default();
    for command in [
        Command::InsertNote {
            pitch: NotePitch::C,
            octave_offset: 0,
        },
        Command::InsertNote {
            pitch: NotePitch::D,
            octave_offset: 0,
        },
        Command::LowerNote,
        Command::RaiseNote,
        Command::DeleteNote,
        Command::SetInstrument(Instrument::Kick),
        Command::PreviousInstrument,
        Command::NextInstrument,
        Command::DecreaseTempo,
        Command::IncreaseTempo,
    ] {
        let before = (
            app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note,
            app.project.instrument(0),
            app.project.bpm,
            app.revision(),
        );
        app.handle_command(command);
        let after = (
            app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note,
            app.project.instrument(0),
            app.project.bpm,
            app.revision(),
        );
        assert_ne!(before.3, after.3, "{command:?}");
        app.handle_command(Command::Undo);
        assert_eq!(
            (
                app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note,
                app.project.instrument(0),
                app.project.bpm,
                app.revision()
            ),
            before
        );
        app.handle_command(Command::Redo);
        assert_eq!(
            (
                app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note,
                app.project.instrument(0),
                app.project.bpm,
                app.revision()
            ),
            after
        );
    }
}

#[test]
fn noops_invalid_targets_and_navigation_preserve_redo() {
    let mut app = App::default();
    let initial = app.revision();
    app.handle_command(Command::IncreaseTempo);
    let edited = app.revision();
    app.handle_command(Command::Undo);
    app.insert_note_under_cursor(None);
    let pattern = app.selected_pattern;
    app.edit(|edit| {
        edit.set_cell(
            pattern,
            0,
            0,
            Some(Note {
                length_ticks: crate::tracker::NoteLength::default(),
                pitch: NotePitch::C,
                octave: 4,
            }),
        );
        edit.set_cell(pattern, 0, 0, None);
        edit.set_tempo(130.0);
        edit.set_tempo(120.0);
        edit.set_instrument(0, Instrument::Kick);
        edit.set_instrument(0, Instrument::Pluck);
    });
    app.handle_command(Command::LowerNote);
    app.handle_command(Command::RaiseNote);
    app.handle_command(Command::SetInstrument(Instrument::Pluck));
    let pattern = app.selected_pattern;
    app.edit(|edit| {
        edit.set_cell(pattern, usize::MAX, 0, None);
        edit.set_instrument(usize::MAX, Instrument::Kick);
        edit.set_tempo(f64::NAN);
        edit.set_tempo(0.0);
    });
    app.cursor_row = usize::MAX;
    app.insert_note_under_cursor(Some(Note {
        length_ticks: crate::tracker::NoteLength::default(),
        pitch: NotePitch::C,
        octave: 4,
    }));
    app.handle_command(Command::IncreaseOctave);
    app.handle_command(Command::ToggleVisual);
    app.handle_command(Command::RestartPlayback);
    app.advance_playback_row();
    assert_eq!(app.revision(), initial);
    app.handle_command(Command::Redo);
    assert_eq!(app.revision(), edited);
    assert_eq!(app.octave, 5);
    assert_eq!(app.mode, Mode::Visual);
    assert_eq!(app.playback_row, 1);
    assert!(matches!(app.playback_state, PlaybackState::Playing));

    for (bpm, command) in [(20.0, Command::DecreaseTempo), (300.0, Command::IncreaseTempo)] {
        let mut app = App {
            project: crate::project::Project {
                bpm,
                ..crate::project::Project::default()
            },
            ..App::default()
        };
        let revision = app.revision();
        app.handle_command(command);
        assert_eq!(app.revision(), revision);
    }
    for (pitch, octave, command) in [
        (NotePitch::C, 0, Command::LowerNote),
        (NotePitch::B, 9, Command::RaiseNote),
    ] {
        let mut app = App::default();
        app.insert_note_under_cursor(Some(Note {
            length_ticks: crate::tracker::NoteLength::default(),
            pitch,
            octave,
        }));
        let revision = app.revision();
        app.handle_command(command);
        assert_eq!(app.revision(), revision);
        app.insert_note_under_cursor(Some(Note {
            length_ticks: crate::tracker::NoteLength::default(),
            pitch,
            octave,
        }));
        assert_eq!(app.revision(), revision);
    }
}

#[test]
fn divergence_and_eviction_never_reuse_revision_tokens() {
    let mut app = App::default();
    let initial = app.revision();
    app.handle_command(Command::IncreaseTempo);
    let discarded = app.revision();
    app.handle_command(Command::Undo);
    app.handle_command(Command::DecreaseTempo);
    let branch = app.revision();
    assert_ne!(discarded, branch);
    app.handle_command(Command::Redo);
    assert_eq!(app.revision(), branch);
    for index in 0..MAX_TRANSACTIONS + 10 {
        app.handle_command(if index % 2 == 0 {
            Command::IncreaseTempo
        } else {
            Command::DecreaseTempo
        });
    }
    let tip = app.revision();
    for _ in 0..MAX_TRANSACTIONS {
        app.handle_command(Command::Undo);
    }
    let oldest = app.revision();
    assert_ne!(oldest, initial);
    app.handle_command(Command::Undo);
    assert_eq!(app.revision(), oldest);
    for _ in 0..MAX_TRANSACTIONS {
        app.handle_command(Command::Redo);
    }
    assert_eq!(app.revision(), tip);
}

#[test]
fn bulk_seam_is_atomic_and_mutation_budget_evicts_oversized_edits() {
    let mut app = App::default();
    let initial = app.revision();
    let note = Some(Note {
        length_ticks: crate::tracker::NoteLength::default(),
        pitch: NotePitch::F,
        octave: 4,
    });
    let pattern = app.selected_pattern;
    app.edit(|edit| {
        edit.set_cell(pattern, 0, 0, note);
        edit.set_cell(pattern, 1, 1, note);
        edit.set_instrument(1, Instrument::Snare);
        edit.set_tempo(140.0);
    });
    app.handle_command(Command::Undo);
    assert_eq!(app.revision(), initial);
    assert_eq!(
        app.project.patterns[app.selected_pattern].get_step(1, 1).unwrap().note,
        None
    );
    assert_eq!(app.project.instrument(1), Instrument::Pluck);
    assert_eq!(app.project.bpm, 120.0);
    app.handle_command(Command::Redo);
    assert_eq!(
        app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note,
        note
    );
    assert_eq!(
        app.project.patterns[app.selected_pattern].get_step(1, 1).unwrap().note,
        note
    );
    assert_eq!(
        app.project.patterns[app.selected_pattern].get_step(0, 1).unwrap().note,
        None
    );

    let mut app = App {
        project: crate::project::Project::new(MAX_MUTATIONS + 1, 1),
        ..App::default()
    };
    let pattern = app.selected_pattern;
    app.edit(|edit| {
        for row in 0..=MAX_MUTATIONS {
            edit.set_cell(pattern, 0, row, note);
        }
    });
    let oversized = app.revision();
    app.handle_command(Command::Undo);
    assert_eq!(app.revision(), oversized);
    assert_eq!(
        app.project.patterns[app.selected_pattern]
            .get_step(0, MAX_MUTATIONS)
            .unwrap()
            .note,
        note
    );
    app.handle_command(Command::IncreaseTempo);
    app.handle_command(Command::Undo);
    assert_eq!(app.revision(), oversized);
}

#[test]
fn actual_history_keys_respect_mode_modifiers_counts_help_and_event_kind() {
    let mut app = App::default();
    key(&mut app, 'i', KeyModifiers::NONE);
    key(&mut app, 'z', KeyModifiers::NONE);
    let edited = app.revision();
    key(&mut app, 'u', KeyModifiers::NONE); // Upper B, not undo in Insert.
    assert_ne!(app.revision(), edited);
    key(&mut app, 'z', KeyModifiers::NONE);
    let edited = app.revision();
    key(&mut app, 'r', KeyModifiers::CONTROL);
    assert_eq!(app.revision(), edited);
    handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &mut app);
    for ch in "2u".chars() {
        key(&mut app, ch, KeyModifiers::NONE);
    }
    assert_eq!(app.revision(), edited);
    assert_eq!(app.pending_count, None);
    key(&mut app, 'u', KeyModifiers::ALT);
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        handle_key(
            KeyEvent::new_with_kind(KeyCode::Char('u'), KeyModifiers::NONE, kind),
            &mut app,
        );
    }
    assert_eq!(app.revision(), edited);
    key(&mut app, '?', KeyModifiers::NONE);
    key(&mut app, 'u', KeyModifiers::NONE);
    assert_eq!(app.revision(), edited);
    key(&mut app, '?', KeyModifiers::NONE);
    key(&mut app, 'u', KeyModifiers::NONE);
    let undone = app.revision();
    assert_ne!(undone, edited);
    for ch in "2".chars() {
        key(&mut app, ch, KeyModifiers::NONE);
    }
    key(&mut app, 'r', KeyModifiers::CONTROL);
    assert_eq!(app.revision(), undone);
    key(&mut app, 'r', KeyModifiers::NONE);
    assert_eq!(app.revision(), undone);
    key(&mut app, 'r', KeyModifiers::CONTROL);
    assert_eq!(app.revision(), edited);
    key(&mut app, 'v', KeyModifiers::NONE);
    key(&mut app, 'u', KeyModifiers::NONE);
    assert_eq!(app.revision(), edited);
}

#[test]
fn playback_keeps_deadline_and_reads_restored_future_events() {
    let mut app = App::default();
    let mut transport = Transport::default();
    app.handle_command(Command::TogglePlayback);
    transport.update(Instant::now(), true, 64, app.project.bpm, 4).unwrap();
    let boundary = transport.deadline().unwrap();
    app.playback_row = 7;
    app.cursor_row = 1;
    let note = Some(Note {
        length_ticks: crate::tracker::NoteLength::default(),
        pitch: NotePitch::D,
        octave: 5,
    });
    let pattern = app.selected_pattern;
    app.edit(|edit| {
        edit.set_cell(pattern, 0, 1, note);
        edit.set_instrument(0, Instrument::Kick);
        edit.set_tempo(150.0);
    });
    app.handle_command(Command::Undo);
    assert_eq!(transport.deadline(), Some(boundary));
    assert_eq!(app.playback_row, 7);
    assert_eq!(app.cursor_row, 1);
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    let event = transport.update(boundary, true, 64, app.project.bpm, 4).unwrap();
    assert_eq!(event.row, 1);
    assert!(collect_row(
        &app.project.patterns[app.selected_pattern],
        app.project.instruments(),
        event
    )
    .is_empty());
    let next_boundary = transport.deadline();
    app.handle_command(Command::Redo);
    assert_eq!(transport.deadline(), next_boundary);
    let notes = collect_row(
        &app.project.patterns[app.selected_pattern],
        app.project.instruments(),
        event,
    );
    assert_eq!(notes[0].note, note.unwrap());
    assert_eq!(notes[0].instrument, Instrument::Kick);
    assert_eq!(app.project.bpm, 150.0);
}
