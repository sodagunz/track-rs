use std::time::{Duration, Instant};

use crate::{
    command::Command,
    musical_time::{DurationTicks, Tick},
    state::App,
    tracker::{NotePitch, PatternEvent},
    transport::Transport,
};

fn duration(ticks: u64) -> Duration {
    Duration::from_nanos((ticks * 500_000_000 + 480) / 960)
}

fn trace(app: &mut App, until: u64) -> Vec<(Duration, usize, NotePitch, Duration)> {
    app.restart_playback();
    let now = Instant::now();
    let mut at = now;
    let mut transport = Transport::default();
    let mut events = Vec::new();
    while at < now + duration(until) {
        events.extend(
            transport
                .update_app(at, app, || {})
                .into_iter()
                .map(|event| (at.duration_since(now), event.track, event.note.pitch, event.duration)),
        );
        let next = transport.deadline().unwrap();
        assert!(next > at, "every visited boundary must advance");
        at = next;
    }
    events
}

#[test]
fn mixed_grid_quantize_clipboard_and_reload_keep_exact_repeated_song_timing() {
    let mut app = App::default();
    app.execute_prompt("end 5761t");
    app.execute_prompt("meter 3/4");
    app.execute_prompt("grid 3");
    app.cursor_row = 9;
    app.execute_prompt("grid 64 bar");
    for (track, start, gate, pitch) in [
        (0, 160, 320, NotePitch::C),
        (1, 700, 61439, NotePitch::D),
        (0, 2888, 1, NotePitch::E),
        (0, 5760, 1, NotePitch::F),
    ] {
        app.project.patterns[0]
            .replace_event(
                track,
                Tick(start),
                Some(PatternEvent {
                    start_tick: Tick(start),
                    duration_ticks: DurationTicks::new(gate).unwrap(),
                    pitch,
                    octave: 4,
                }),
            )
            .unwrap();
    }
    app.cursor_row = 0;
    app.handle_command(Command::ToggleVisual);
    app.handle_command(Command::LastRow);
    app.execute_prompt("quantize"); // Only track zero; hidden 700t on track one stays exact.
    assert!(app.status.contains("2/3 events move"));
    assert_eq!(app.project.patterns[0].tracks[1].events()[0].start_tick, Tick(700));
    app.cursor_row = 0;
    app.handle_command(Command::ToggleVisual);
    app.handle_command(Command::LastRow);
    app.handle_command(Command::LastTrack);
    app.handle_command(Command::Copy);
    app.handle_command(Command::NewPattern);
    app.handle_command(Command::FirstRow);
    app.handle_command(Command::FirstTrack);
    app.execute_prompt("end 3361t");
    app.execute_prompt("meter 7/8");
    app.execute_prompt("grid 64 bar");
    app.handle_command(Command::Paste);
    assert_eq!(app.project.patterns[1].event_count(), 3); // 5760t is outside B, not its crossing
                                                          // gates.
    app.handle_command(Command::AppendOrder);
    app.handle_command(Command::PreviousPattern);
    app.handle_command(Command::AppendOrder);
    assert_eq!(app.project.order, [0, 1, 0]);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mixed-song.json");
    app.execute_prompt(&format!("w {}", path.display()));
    app.execute_prompt(&format!("e {}", path.display()));
    assert!(!app.is_dirty());
    app.handle_command(Command::ToggleSong);
    let cycle = 5761 + 3361 + 5761;
    let mut expected = Vec::new();
    for (prefix, include_tail) in [(0, true), (5761, false), (9122, true)] {
        for (tick, track, pitch, gate) in [
            (320, 0, NotePitch::C, 320),
            (700, 1, NotePitch::D, 61439),
            (2895, 0, NotePitch::E, 1),
            (5760, 0, NotePitch::F, 1),
        ] {
            if include_tail || tick < 3361 {
                expected.push((duration(prefix + tick), track, pitch, duration(gate)));
            }
        }
    }
    let original = trace(&mut app, cycle);
    assert_eq!(original, expected);
    app.handle_command(Command::TogglePlayback);
    // A radically different projection cannot change starts, gates or occurrence offsets.
    for index in 0..app.project.patterns.len() {
        app.selected_pattern = index;
        app.cursor_row = 0;
        app.execute_prompt("grid 1");
        app.execute_prompt("grid 5 bar");
    }
    assert_eq!(trace(&mut app, cycle), expected);
}
