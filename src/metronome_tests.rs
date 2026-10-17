use crate::{
    bindings::handle_key,
    command::Command,
    state::{App, Mode},
    tracker::Pattern,
    transport::{ClickEvent, MetronomeEvent, Transport},
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::time::{Duration, Instant};

fn prompt(app: &mut App, command: &str) {
    handle_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE), app);
    assert!(app.prompt.is_some());
    for ch in command.chars() {
        handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), app);
    }
    handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), app);
}
fn ticks(ticks: u64, bpm: u64) -> Duration {
    Duration::from_nanos((ticks * 60_000_000_000 + bpm * 480) / (bpm * 960))
}
fn clicks(app: &mut App, until: Duration) -> Vec<ClickEvent> {
    app.restart_playback();
    let mut clock = Transport::default();
    let now = Instant::now();
    let mut at = now;
    let mut events = Vec::new();
    while at < now + until {
        clock.update_app(at, app, || {});
        if let Some(MetronomeEvent::Click(event)) = clock.take_metronome_event() {
            events.push(event);
        }
        assert!(clock.take_metronome_event().is_none());
        let next = clock.deadline().unwrap();
        assert!(next > at);
        at = next;
    }
    events
}

#[test]
fn denominator_units_and_bar_accents_follow_meter_not_edit_grid_at_all_tempos() {
    for bpm in [20, 120, 300] {
        for (meter, bar, unit) in [
            ("3/4", 2880, 960),
            ("7/8", 3360, 480),
            ("6/8", 2880, 480),
            ("1/64", 60, 60),
        ] {
            let mut app = App::default();
            app.execute_prompt(&format!("end {}t", bar * 2));
            app.execute_prompt(&format!("meter {meter}"));
            app.execute_prompt("grid 3");
            app.project.bpm = f64::from(u32::try_from(bpm).unwrap());
            prompt(&mut app, "metronome on");
            let events = clicks(&mut app, ticks(bar * 2, bpm));
            assert_eq!(
                events
                    .iter()
                    .map(|event| (event.absolute_tick, event.accent))
                    .collect::<Vec<_>>(),
                (0..bar * 2 / unit)
                    .map(|index| (index * unit, (index * unit).is_multiple_of(bar)))
                    .collect::<Vec<_>>(),
                "{meter} at {bpm}"
            );
            let origin = events[0].scheduled_at;
            for event in events {
                assert_eq!(
                    event.scheduled_at.duration_since(origin),
                    ticks(event.absolute_tick, bpm)
                );
            }
        }
    }
}

#[test]
fn song_partial_occurrences_reset_click_phase_without_bar_padding() {
    let mut app = App::default();
    app.execute_prompt("end 1001t");
    app.execute_prompt("meter 7/8");
    app.project.patterns.push(Pattern::new(2, 4));
    app.selected_pattern = 1;
    app.execute_prompt("end 241t");
    app.execute_prompt("meter 3/4");
    app.project.order = vec![0, 1, 0];
    app.handle_command(Command::ToggleSong);
    prompt(&mut app, "metronome on");
    let events = clicks(&mut app, ticks(2244, 120));
    assert_eq!(
        events
            .iter()
            .map(|event| (event.absolute_tick, event.order, event.accent))
            .collect::<Vec<_>>(),
        [
            (0, Some(0), true),
            (480, Some(0), false),
            (960, Some(0), false),
            (1001, Some(1), true),
            (1242, Some(2), true),
            (1722, Some(2), false),
            (2202, Some(2), false),
            (2243, Some(0), true)
        ]
    );
}

#[test]
fn mid_unit_start_toggle_and_late_wakes_never_replay_or_burst_clicks() {
    let mut app = App::default();
    app.execute_prompt("meter 1/64");
    app.execute_prompt("grid 64");
    app.cursor_row = 1;
    app.handle_command(Command::TogglePlayback);
    app.metronome = true;
    let now = Instant::now();
    let mut clock = Transport::default();
    clock.update_app(now, &mut app, || {});
    assert!(clock.take_metronome_event().is_none()); // tick 15 is not a denominator unit.
    assert_eq!(clock.deadline(), Some(now + ticks(45, 120)));
    let at = now + ticks(45, 120);
    clock.update_app(at.checked_sub(Duration::from_nanos(1)).unwrap(), &mut app, || {});
    assert!(clock.take_metronome_event().is_none());
    assert_eq!(clock.deadline(), Some(at));
    clock.update_app(at, &mut app, || {});
    let Some(MetronomeEvent::Click(event)) = clock.take_metronome_event() else {
        panic!("unit click");
    };
    assert_eq!(event.tick.0, 60);
    assert!(event.accent); // Every unit is a complete 1/64 bar.
    app.metronome = false;
    clock.update_app(at, &mut app, || panic!("disabling click must not stop notes"));
    assert_eq!(clock.take_metronome_event(), Some(MetronomeEvent::Stop));
    app.metronome = true;
    clock.update_app(at + Duration::from_millis(1), &mut app, || {});
    assert!(clock.take_metronome_event().is_none());
    clock.update_app(now + Duration::from_millis(201), &mut app, || {});
    let Some(MetronomeEvent::Click(event)) = clock.take_metronome_event() else {
        panic!("latest fresh click");
    };
    assert!(now + Duration::from_millis(201) - event.scheduled_at < ticks(60, 120));
    clock.update_app(now + Duration::from_millis(201), &mut app, || {});
    assert!(clock.take_metronome_event().is_none());
}

#[test]
fn stale_quarter_clicks_are_skipped_without_replay() {
    let mut app = App {
        metronome: true,
        ..App::default()
    };
    app.restart_playback();
    let mut clock = Transport::default();
    let now = Instant::now();
    clock.update_app(now, &mut app, || {});
    clock.take_metronome_event();
    clock.update_app(now + Duration::from_millis(1100), &mut app, || {});
    assert!(clock.take_metronome_event().is_none());
    let mut at = clock.deadline().unwrap();
    while at < now + Duration::from_millis(1500) {
        clock.update_app(at, &mut app, || {});
        assert!(clock.take_metronome_event().is_none());
        at = clock.deadline().unwrap();
    }
    clock.update_app(at, &mut app, || {});
    let Some(MetronomeEvent::Click(event)) = clock.take_metronome_event() else {
        panic!("next fresh quarter");
    };
    assert_eq!(event.scheduled_at, now + Duration::from_millis(1500));
}

#[test]
fn tempo_segments_retime_future_clicks_with_notes_and_preserve_committed_note_boundary() {
    let mut app = App {
        metronome: true,
        ..App::default()
    };
    app.restart_playback();
    let now = Instant::now();
    let mut clock = Transport::default();
    clock.update_app(now, &mut app, || {});
    clock.take_metronome_event();
    app.edit(|edit| edit.set_tempo(60.0));
    assert_eq!(clock.deadline(), Some(now + Duration::from_millis(125)));
    for ms in [125, 375, 625] {
        clock.update_app(now + Duration::from_millis(ms), &mut app, || {
            panic!("tempo is nonstructural")
        });
        assert!(clock.take_metronome_event().is_none());
    }
    clock.update_app(now + Duration::from_millis(875), &mut app, || {});
    let Some(MetronomeEvent::Click(event)) = clock.take_metronome_event() else {
        panic!("retimed quarter");
    };
    assert_eq!(event.tick.0, 960);
    assert_eq!(event.scheduled_at, now + Duration::from_millis(875));
}

#[test]
fn session_command_is_normal_only_atomic_overlay_safe_and_retained_across_loads() {
    let mut app = App::default();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("song.json");
    app.execute_prompt(&format!("w {}", path.display()));
    let revision = app.revision();
    for view in [
        Command::ShowPattern,
        Command::ShowArrangement,
        Command::ShowMixer,
        Command::ShowInstrument,
    ] {
        app.handle_command(view);
        for invalid in ["metronome", "metronome yes", "metronome on extra", "metronome ON"] {
            prompt(&mut app, invalid);
            assert!(!app.metronome);
        }
        prompt(&mut app, "metronome on");
        assert!(app.metronome);
        assert_eq!(app.revision(), revision);
        assert!(!app.is_dirty());
        prompt(&mut app, "metronome toggle");
        assert!(!app.metronome);
    }
    app.handle_command(Command::ShowPattern);
    app.mode = Mode::Insert;
    app.execute_prompt("metronome on");
    assert!(!app.metronome);
    app.handle_command(Command::EnterNormal);
    app.help_visible = true;
    handle_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE), &mut app);
    assert!(app.prompt.is_none());
    app.help_visible = false;
    handle_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE), &mut app);
    for ch in "metronome on".chars() {
        handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut app);
    }
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        handle_key(
            KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, kind),
            &mut app,
        );
        assert!(!app.metronome);
    }
    handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
    assert!(app.metronome);
    app.execute_prompt("e! no-such-metronome-project.json");
    assert!(app.metronome);
    app.execute_prompt(&format!("e {}", path.display()));
    assert!(app.metronome);
    assert!(!app.is_dirty());
    let json = std::fs::read_to_string(path).unwrap();
    assert!(!json.contains("metronome"));
}

#[test]
fn load_and_route_changes_cancel_clicks_but_failed_load_keeps_the_run() {
    let mut app = App {
        metronome: true,
        ..App::default()
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("click-session.json");
    app.execute_prompt(&format!("w {}", path.display()));
    app.restart_playback();
    let now = Instant::now();
    let mut clock = Transport::default();
    clock.update_app(now, &mut app, || {});
    clock.take_metronome_event();
    let deadline = clock.deadline();
    app.execute_prompt("e! no-such-click-session.json");
    clock.update_app(now, &mut app, || panic!("failed load keeps run"));
    assert_eq!(clock.deadline(), deadline);
    assert!(clock.take_metronome_event().is_none());
    app.execute_prompt(&format!("e {}", path.display()));
    let mut cancelled = 0;
    clock.update_app(now, &mut app, || cancelled += 1);
    assert_eq!(cancelled, 1);
    assert!(clock.deadline().is_none());
    assert!(clock.take_metronome_event().is_none());
    assert!(app.metronome);
    app.restart_playback();
    clock.update_app(now, &mut app, || {});
    assert!(matches!(clock.take_metronome_event(), Some(MetronomeEvent::Click(_))));
    app.handle_command(Command::ToggleSong);
    clock.update_app(now, &mut app, || cancelled += 1);
    assert_eq!(cancelled, 2);
    assert!(clock.deadline().is_none());
    assert!(clock.take_metronome_event().is_none());
}

#[test]
fn stop_restart_structural_edits_and_shutdown_cancel_without_stopped_clicks() {
    let mut app = App {
        metronome: true,
        ..App::default()
    };
    let now = Instant::now();
    let mut clock = Transport::default();
    clock.update_app(now, &mut app, || {});
    assert!(clock.take_metronome_event().is_none());
    assert!(clock.deadline().is_none());
    app.restart_playback();
    clock.update_app(now, &mut app, || {});
    assert!(matches!(clock.take_metronome_event(), Some(MetronomeEvent::Click(_))));
    app.handle_command(Command::TogglePlayback);
    let mut cancelled = 0;
    clock.update_app(now, &mut app, || cancelled += 1);
    assert_eq!(cancelled, 1);
    assert!(clock.take_metronome_event().is_none());
    assert!(clock.deadline().is_none());
    assert!(app.metronome);
    app.restart_playback();
    clock.update_app(now, &mut app, || {});
    assert!(matches!(clock.take_metronome_event(), Some(MetronomeEvent::Click(_))));
    app.execute_prompt("meter 7/8");
    clock.update_app(now, &mut app, || cancelled += 1);
    assert_eq!(cancelled, 2);
    assert!(clock.deadline().is_none());
    app.shutdown = true;
    clock.update_app(now, &mut app, || cancelled += 1);
    assert_eq!(cancelled, 3);
    assert!(clock.deadline().is_none());
}
