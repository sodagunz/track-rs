use crate::{
    bindings::handle_key,
    command::Command,
    musical_time::{DurationTicks, Tick},
    state::{App, Mode, PlaybackState},
    tracker::{NotePitch, PatternEvent},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(app: &mut App, code: KeyCode) {
    handle_key(KeyEvent::new(code, KeyModifiers::NONE), app);
}
fn text(app: &mut App, text: &str) {
    for ch in text.chars() {
        key(app, KeyCode::Char(ch));
    }
}
fn prompt(app: &mut App, command: &str) {
    text(app, ":");
    assert!(app.prompt.is_some());
    text(app, command);
    key(app, KeyCode::Enter);
}
fn seed(app: &mut App, track: usize, start: u64, duration: u32) {
    app.project.patterns[0]
        .replace_event(
            track,
            Tick(start),
            Some(PatternEvent {
                start_tick: Tick(start),
                duration_ticks: DurationTicks::new(duration).unwrap(),
                pitch: NotePitch::C,
                octave: 4,
            }),
        )
        .unwrap();
}
fn identities(app: &App) -> Vec<crate::tracker::EventId> {
    app.project.patterns[0]
        .tracks
        .iter()
        .flat_map(|track| {
            track
                .events()
                .iter()
                .map(|event| track.event_id(event.start_tick).unwrap())
        })
        .collect()
}

#[test]
fn real_visual_quantize_previews_then_moves_mixed_grids_with_exact_gates_and_ids() {
    let mut app = App::default();
    prompt(&mut app, "end 5761t");
    prompt(&mut app, "meter 3/4");
    prompt(&mut app, "grid 3");
    app.cursor_row = 9;
    prompt(&mut app, "grid 64 bar");
    for (track, start, duration) in [(0, 100, 61439), (0, 500, 320), (0, 2897, 1), (1, 2918, 200)] {
        seed(&mut app, track, start, duration);
    }
    let before = app.project.clone();
    let ids = identities(&app);
    app.cursor_row = 0;
    app.handle_command(Command::TogglePlayback);
    text(&mut app, "vgjgl");
    let anchor = app.selection_anchor;
    let revision = app.revision();
    prompt(&mut app, "quantize?");
    assert!(app.status.contains("4/4 events move; 0 collisions; 0 end hits"));
    assert_eq!(app.project, before);
    assert_eq!(app.revision(), revision);
    assert_eq!(app.selection_anchor, anchor);
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    prompt(&mut app, "quantize");
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.selection_anchor, None);
    assert!(matches!(app.playback_state, PlaybackState::Stopped));
    assert_eq!(identities(&app), ids);
    let pattern = &app.project.patterns[0];
    assert_eq!(
        pattern.tracks[0]
            .events()
            .iter()
            .map(|event| (event.start_tick.0, event.duration_ticks.ticks()))
            .collect::<Vec<_>>(),
        [(0, 61439), (640, 320), (2895, 1)]
    );
    assert_eq!(pattern.tracks[1].events()[0].start_tick, Tick(2925));
    assert_eq!(pattern.length_ticks(), before.patterns[0].length_ticks());
    assert_eq!(pattern.grid(), before.patterns[0].grid());
    let after = app.project.clone();
    app.handle_command(Command::Undo);
    assert_eq!(app.project, before);
    assert_eq!(identities(&app), ids);
    app.handle_command(Command::Redo);
    assert_eq!(app.project, after);
    assert_eq!(identities(&app), ids);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("quantized.json");
    crate::project_file::save(&app.project, &path).unwrap();
    assert_eq!(crate::project_file::load(&path).unwrap(), after);
}

#[test]
fn selected_hidden_collisions_reject_the_whole_edit_and_preserve_redo_and_selection() {
    let mut app = App::default();
    seed(&mut app, 0, 15, 1);
    seed(&mut app, 0, 30, 320);
    seed(&mut app, 1, 350, 200);
    app.handle_command(Command::IncreaseTempo);
    app.handle_command(Command::Undo);
    let before = app.project.clone();
    let revision = app.revision();
    let ids = identities(&app);
    app.handle_command(Command::TogglePlayback);
    text(&mut app, "vjl");
    let anchor = app.selection_anchor;
    for command in ["quantize?", "quantize", "quantize!", "quantize 4", "end! 1t", "q!"] {
        prompt(&mut app, command);
        assert_eq!(app.project, before, "{command}");
        assert_eq!(app.revision(), revision);
        assert_eq!(app.selection_anchor, anchor);
        assert_eq!(app.mode, Mode::Visual);
        assert!(matches!(app.playback_state, PlaybackState::Playing));
    }
    prompt(&mut app, "quantize");
    assert!(app.status.contains("1 collisions"));
    assert!(app.status.contains("first T1 @ 0t"));
    assert_eq!(identities(&app), ids);
    key(&mut app, KeyCode::Esc);
    app.handle_command(Command::Redo);
    assert_eq!(app.project.bpm.to_bits(), 121.0_f64.to_bits());
}

#[test]
fn exact_cursor_quantize_checks_unselected_destinations_and_ties_forward() {
    let mut app = App::default();
    seed(&mut app, 0, 120, 1);
    seed(&mut app, 0, 240, 320);
    prompt(&mut app, "next-event");
    assert_eq!(app.cursor_tick(), Tick(120));
    let before = app.project.clone();
    prompt(&mut app, "quantize");
    assert!(app.status.contains("1 collisions"));
    assert_eq!(app.project, before);
    app.project.patterns[0].replace_event(0, Tick(240), None).unwrap();
    let id = app.project.patterns[0].tracks[0].event_id(Tick(120));
    prompt(&mut app, "quantize");
    assert_eq!(app.project.patterns[0].tracks[0].events()[0].start_tick, Tick(240));
    assert_eq!(app.project.patterns[0].tracks[0].event_id(Tick(240)), id);
    assert_eq!(app.project.patterns[0].tracks[0].events()[0].duration_ticks.ticks(), 1);
    app.handle_command(Command::Undo);
    assert_eq!(app.project.patterns[0].tracks[0].event_id(Tick(120)), id);
}

#[test]
fn partial_end_hits_are_never_wrapped_clamped_or_silently_dropped() {
    let mut app = App::default();
    prompt(&mut app, "end 239t");
    seed(&mut app, 0, 238, 61439);
    prompt(&mut app, "next-event");
    let before = app.project.clone();
    let revision = app.revision();
    prompt(&mut app, "quantize");
    assert!(app.status.contains("1 end hits"));
    assert_eq!(app.project, before);
    assert_eq!(app.revision(), revision);
    prompt(&mut app, "end 241t");
    prompt(&mut app, "next-event");
    prompt(&mut app, "quantize");
    assert_eq!(app.project.patterns[0].tracks[0].events()[0].start_tick, Tick(240));
    app.handle_command(Command::LastRow);
    let revision = app.revision();
    app.handle_command(Command::TogglePlayback);
    prompt(&mut app, "quantize");
    assert_eq!(app.revision(), revision);
    assert!(matches!(app.playback_state, PlaybackState::Playing));
}

#[test]
fn snapping_across_local_bar_overrides_detects_cross_bar_collisions() {
    let mut app = App::default();
    prompt(&mut app, "end 5760t");
    prompt(&mut app, "meter 3/4");
    prompt(&mut app, "grid 3");
    app.cursor_row = 9;
    prompt(&mut app, "grid 64 bar");
    seed(&mut app, 0, 2875, 1);
    seed(&mut app, 0, 2887, 320);
    app.cursor_row = 0;
    text(&mut app, "vgj");
    let before = app.project.clone();
    prompt(&mut app, "quantize");
    assert!(app.status.contains("first T1 @ 2880t"));
    assert_eq!(app.project, before);
}

#[test]
fn quantize_at_track_capacity_moves_handles_without_temporary_budget_growth() {
    use crate::project_file::MAX_TRACK_EVENTS;
    let mut app = App {
        project: crate::project::Project::new(4096, 1),
        ..App::default()
    };
    prompt(&mut app, "grid 64");
    for index in 0..MAX_TRACK_EVENTS {
        seed(&mut app, 0, if index == 1 { 16 } else { index as u64 * 15 }, 1);
    }
    let id = app.project.patterns[0].tracks[0].event_id(Tick(16));
    text(&mut app, "vgj");
    prompt(&mut app, "quantize");
    assert!(app.status.contains("1/65536 events move; 0 collisions"));
    assert_eq!(app.project.event_count(), MAX_TRACK_EVENTS);
    assert_eq!(app.project.patterns[0].tracks[0].event_id(Tick(15)), id);
    app.handle_command(Command::Undo);
    assert_eq!(app.project.patterns[0].tracks[0].event_id(Tick(16)), id);
    app.handle_command(Command::Redo);
    assert_eq!(app.project.patterns[0].tracks[0].event_id(Tick(15)), id);
}

#[test]
fn visual_prompt_is_pattern_only_and_empty_or_on_grid_quantize_is_a_noop() {
    let mut app = App::default();
    seed(&mut app, 0, 0, 240);
    app.handle_command(Command::TogglePlayback);
    let revision = app.revision();
    text(&mut app, "v");
    prompt(&mut app, "quantize");
    assert_eq!(app.mode, Mode::Visual);
    assert_eq!(app.revision(), revision);
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    key(&mut app, KeyCode::Esc);
    app.handle_command(Command::ShowMixer);
    text(&mut app, "v:");
    assert!(app.prompt.is_none());
    app.handle_command(Command::ShowPattern);
    key(&mut app, KeyCode::Esc);
    app.handle_command(Command::EnterInsert);
    text(&mut app, ":");
    assert!(app.prompt.is_none());
}
