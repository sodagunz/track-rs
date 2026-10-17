use std::time::{Duration, Instant};

use crate::{
    state::App,
    transport::{MetronomeEvent, PlaybackRoute, Transport, TransportPhase},
};

fn ticks(tick: u64, bpm: u64) -> Duration {
    Duration::from_nanos((tick * 60_000_000_000 + bpm * 480) / (bpm * 960))
}

#[test]
fn full_destination_bar_precedes_pinned_cursor_at_all_tempos() {
    for bpm in [20, 120, 300] {
        for (meter, bar, interval) in [
            ("3/4", 2880, 960),
            ("7/8", 3360, 480),
            ("6/8", 2880, 1440),
            ("1/64", 60, 60),
        ] {
            let mut app = App::default();
            app.execute_prompt(&format!("meter {meter}"));
            app.execute_prompt("end 481t"); // Count-in isn't this partial pattern's length.
            app.project.bpm = f64::from(u32::try_from(bpm).unwrap());
            let now = Instant::now();
            let mut clock = Transport::default();
            assert!(clock.start_count_in(now, &app.project, 0, 1));
            let target = app.project.patterns[0].row_tick(1).unwrap();
            for beat in 0..bar / interval {
                let at = now + ticks(beat * interval, bpm);
                assert!(clock
                    .update_project(at, true, &app.project, PlaybackRoute::Pattern(0), 4)
                    .is_none());
                assert!(clock.clock_snapshot().is_none());
                let Some(MetronomeEvent::Click(event)) = clock.take_metronome_event() else {
                    panic!("count click")
                };
                assert_eq!(event.accent, beat == 0);
                assert_eq!(event.scheduled_at, at);
                assert_eq!(clock.deadline(), Some(now + ticks((beat + 1) * interval, bpm)));
            }
            let end = now + ticks(bar, bpm);
            assert!(clock
                .update_project(
                    end.checked_sub(Duration::from_nanos(1)).unwrap(),
                    true,
                    &app.project,
                    PlaybackRoute::Pattern(0),
                    4
                )
                .is_none());
            let event = clock
                .update_project(end, true, &app.project, PlaybackRoute::Pattern(0), 4)
                .unwrap();
            assert_eq!(event.tick, target);
            assert_eq!(event.scheduled_at, end);
            assert_eq!(clock.phase(), TransportPhase::Playing);
            assert!(clock
                .update_project(end, true, &app.project, PlaybackRoute::Pattern(0), 4)
                .is_none());
        }
    }
}

#[test]
fn app_orchestration_counts_without_playhead_or_document_changes_and_cancels() {
    for ongoing_click in [false, true] {
        let mut app = App {
            metronome: ongoing_click,
            cursor_row: 3,
            ..App::default()
        };
        app.toggle_playback();
        let now = Instant::now();
        let mut clock = Transport::default();
        clock.update_app(now, &mut app, || {});
        clock.take_metronome_event();
        let revision = app.revision();
        let playhead = app.playback_tick;
        assert!(clock.start_count_in(now, &app.project, 0, 3));
        for millis in [0, 500, 1000, 1500] {
            assert!(clock
                .update_app(now + Duration::from_millis(millis), &mut app, || panic!(
                    "counting must not stop again"
                ))
                .is_empty());
            assert_eq!(app.playback_tick, playhead);
            assert_eq!(app.revision(), revision);
            assert!(!app.is_dirty());
            assert!(matches!(clock.take_metronome_event(), Some(MetronomeEvent::Click(_))));
        }
        clock.update_app(now + Duration::from_secs(2), &mut app, || {});
        assert_eq!(clock.phase(), TransportPhase::Playing);
        assert_eq!(app.playback_tick, app.project.patterns[0].row_tick(3).unwrap());
        // This target is between denominator clicks, so the count tail stops.
        assert_eq!(clock.take_metronome_event(), Some(MetronomeEvent::Stop));
        assert!(clock.start_count_in(now + Duration::from_secs(3), &app.project, 0, 0));
        app.toggle_playback();
        let mut stopped = false;
        assert!(clock
            .update_app(now + Duration::from_secs(3), &mut app, || stopped = true)
            .is_empty());
        assert!(stopped);
        assert_eq!(clock.phase(), TransportPhase::Stopped);
        assert!(clock.deadline().is_none());
    }
}

#[test]
fn first_note_is_emitted_once_with_gate_from_actual_musical_start() {
    let mut app = App {
        cursor_row: 3,
        ..App::default()
    };
    app.insert_note_under_cursor(Some(crate::tracker::Note {
        pitch: crate::tracker::NotePitch::C,
        octave: 4,
        length_ticks: crate::tracker::NoteLength::default(),
    }));
    app.toggle_playback();
    let now = Instant::now();
    let mut clock = Transport::default();

    assert!(clock.start_count_in(now, &app.project, 0, 3));
    for millis in [0, 500, 1000, 1500, 1999] {
        assert!(clock
            .update_app(now + Duration::from_millis(millis), &mut app, || {})
            .is_empty());
        clock.take_metronome_event();
    }
    let notes = clock.update_app(now + Duration::from_secs(2), &mut app, || {});
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].duration, Duration::from_millis(125));
    assert!(clock
        .update_app(now + Duration::from_secs(2), &mut app, || {})
        .is_empty());
    assert_eq!(
        clock.clock_snapshot().unwrap().anchor(),
        (now + Duration::from_secs(2), 720)
    );
}

#[test]
fn off_grid_event_and_exact_gate_keep_the_count_completion_anchor() {
    use crate::musical_time::{DurationTicks, Tick};
    let mut app = App::default();
    app.edit(|edit| {
        edit.set_event(
            0,
            0,
            Tick(721),
            Some(crate::tracker::PatternEvent {
                start_tick: Tick(721),
                duration_ticks: DurationTicks::new(320).unwrap(),
                pitch: crate::tracker::NotePitch::C,
                octave: 4,
            }),
        );
    });
    app.restart_playback();
    app.take_stop_request();
    let now = Instant::now();
    let mut clock = Transport::default();
    assert!(clock.start_count_in(now, &app.project, 0, 0));
    let end = now + Duration::from_secs(2);
    assert!(clock.update_app(end, &mut app, || {}).is_empty());
    let at = end + ticks(721, 120);
    assert!(clock
        .update_app(at.checked_sub(Duration::from_nanos(1)).unwrap(), &mut app, || {})
        .is_empty());
    let notes = clock.update_app(at, &mut app, || {});
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].duration, ticks(320, 120));
    assert_eq!(app.playback_tick, Tick(721));
    assert!(clock.update_app(at, &mut app, || {}).is_empty());
}

#[test]
fn failed_load_does_not_cancel_count_or_move_its_deadline() {
    let mut app = App::default();
    app.toggle_playback();
    let now = Instant::now();
    let mut clock = Transport::default();
    assert!(clock.start_count_in(now, &app.project, 0, 0));
    clock.update_app(now, &mut app, || {});
    clock.take_metronome_event();
    let deadline = clock.deadline();
    app.execute_prompt("e! no-such-count-in-project.json");
    clock.update_app(now + Duration::from_millis(100), &mut app, || {
        panic!("failed load must keep count")
    });
    assert_eq!(clock.deadline(), deadline);
    assert!(matches!(clock.phase(), TransportPhase::Counting { .. }));
}

#[test]
fn structural_route_load_and_shutdown_cancellation_reuse_app_stop_policy() {
    for action in ["end 1001t", "meter 7/8", "song", "load", "shutdown"] {
        let mut app = App::default();
        app.restart_playback();
        let now = Instant::now();
        let mut clock = Transport::default();
        clock.update_app(now, &mut app, || {});
        assert!(clock.start_count_in(now, &app.project, 0, 0));
        clock.update_app(now, &mut app, || {});
        clock.take_metronome_event();
        match action {
            "song" => app.handle_command(crate::command::Command::ToggleSong),
            "load" => {
                let file = tempfile::NamedTempFile::new().unwrap();
                crate::project_file::save(&app.project, file.path()).unwrap();
                app.execute_prompt(&format!("e! {}", file.path().display()));
            }
            "shutdown" => app.shutdown = true,
            command => app.execute_prompt(command),
        }
        let mut stopped = false;
        clock.update_app(now + Duration::from_millis(100), &mut app, || stopped = true);
        assert!(stopped, "{action}");
        assert!(!matches!(clock.phase(), TransportPhase::Counting { .. }), "{action}");
        assert!(clock.take_metronome_event().is_none());
    }
}

#[test]
fn destination_click_is_not_swallowed_by_count_tail_cancellation() {
    let mut app = App {
        metronome: true,
        ..App::default()
    };
    app.restart_playback();
    let now = Instant::now();
    let mut clock = Transport::default();
    clock.update_app(now, &mut app, || {});
    clock.take_metronome_event();
    assert!(clock.start_count_in(now, &app.project, 0, 0));
    clock.update_app(now + Duration::from_secs(2), &mut app, || {});
    let Some(MetronomeEvent::Click(event)) = clock.take_metronome_event() else {
        panic!("destination click")
    };
    assert_eq!(event.tick.0, 0);
    assert!(event.accent);
    assert!(clock.take_metronome_event().is_none());
}

#[test]
fn tempo_keeps_pending_beat_and_retimes_remaining_intervals() {
    let mut app = App::default();
    let now = Instant::now();
    let mut clock = Transport::default();
    assert!(clock.start_count_in(now, &app.project, 0, 0));
    clock.update_project(now, true, &app.project, PlaybackRoute::Pattern(0), 4);
    clock.take_metronome_event();
    app.project.bpm = 60.0;
    clock.update_project(
        now + Duration::from_millis(100),
        true,
        &app.project,
        PlaybackRoute::Pattern(0),
        4,
    );
    assert_eq!(clock.deadline(), Some(now + Duration::from_millis(500)));
    for millis in [500, 1500, 2500] {
        clock.update_project(
            now + Duration::from_millis(millis),
            true,
            &app.project,
            PlaybackRoute::Pattern(0),
            4,
        );
        assert!(matches!(clock.take_metronome_event(), Some(MetronomeEvent::Click(_))));
    }
    let end = now + Duration::from_millis(3500);
    let event = clock
        .update_project(end, true, &app.project, PlaybackRoute::Pattern(0), 4)
        .unwrap();
    assert_eq!(event.scheduled_at, end);
    assert_eq!(event.tick.0, 0);
}

#[test]
fn late_wakes_skip_clicks_and_keep_song_anchor() {
    let app = App::default();
    let now = Instant::now();
    let mut clock = Transport::default();
    assert!(clock.start_count_in(now, &app.project, 0, 0));
    clock.update_project(
        now + Duration::from_millis(1100),
        true,
        &app.project,
        PlaybackRoute::Pattern(0),
        4,
    );
    assert!(clock.take_metronome_event().is_none());
    assert_eq!(clock.deadline(), Some(now + Duration::from_millis(1500)));
    let event = clock
        .update_project(
            now + Duration::from_millis(2350),
            true,
            &app.project,
            PlaybackRoute::Pattern(0),
            4,
        )
        .unwrap();
    assert_eq!(event.tick.0, 480);
    assert_eq!(event.scheduled_at, now + Duration::from_millis(2250));
    assert_eq!(
        clock.clock_snapshot().unwrap().anchor(),
        (now + Duration::from_secs(2), 0)
    );
}

#[test]
fn cancellation_restart_invalid_target_and_generation_are_explicit() {
    let app = App::default();
    let now = Instant::now();
    let mut clock = Transport::default();
    clock.update_project(now, true, &app.project, PlaybackRoute::Pattern(0), 4);
    let old = clock.clock_snapshot().unwrap().generation();
    assert!(clock.start_count_in(now, &app.project, 0, 0));
    assert_eq!(clock.phase(), TransportPhase::Counting { beat: 0, total: 4 });
    assert!(!clock.start_count_in(now, &app.project, usize::MAX, 0));
    assert_eq!(clock.deadline(), Some(now));
    clock.stop();
    assert_eq!(clock.phase(), TransportPhase::Stopped);
    assert!(clock.deadline().is_none());
    assert!(clock.take_metronome_event().is_none());
    assert!(clock.start_count_in(now + Duration::from_secs(1), &app.project, 0, 0));
    assert!(clock
        .update_project(
            now + Duration::from_secs(2),
            false,
            &app.project,
            PlaybackRoute::Pattern(0),
            4
        )
        .is_none());
    assert_eq!(clock.phase(), TransportPhase::Stopped);
    assert!(clock.start_count_in(now + Duration::from_secs(2), &app.project, 0, 0));
    let event = clock
        .update_project(
            now + Duration::from_secs(4),
            true,
            &app.project,
            PlaybackRoute::Pattern(0),
            4,
        )
        .unwrap();
    assert!(event.generation > old);
    assert_eq!(event.tick.0, 0);
}
