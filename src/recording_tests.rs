use crate::{
    bindings::handle_key_at,
    command::Command,
    musical_time::Tick,
    state::{App, Mode, PlaybackMode, PlaybackState},
    tracker::NotePitch,
    transport::{NoteEvent, Transport, TransportPhase},
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::time::{Duration, Instant};

fn ticks(tick: u64) -> Duration {
    Duration::from_nanos((tick * 60_000_000_000 + 120 * 480) / (120 * 960))
}
fn key(app: &mut App, at: Instant, code: KeyCode) {
    handle_key_at(KeyEvent::new(code, KeyModifiers::NONE), app, at);
}
fn prompt(app: &mut App, at: Instant, text: &str) {
    key(app, at, KeyCode::Char(':'));
    for ch in text.chars() {
        key(app, at, KeyCode::Char(ch));
    }
    key(app, at, KeyCode::Enter);
}
struct Rig {
    app: App,
    clock: Transport,
    now: Instant,
}
impl Rig {
    fn new(count: bool) -> Self {
        let now = Instant::now();
        let mut app = App::default();
        app.project.patterns[0].resize_rows(16);
        prompt(&mut app, now, "record on");
        if count {
            prompt(&mut app, now, "countin 1");
        }
        key(&mut app, now, KeyCode::Char(' '));
        key(&mut app, now, KeyCode::Char('i'));
        let mut rig = Self {
            app,
            clock: Transport::default(),
            now,
        };
        rig.advance(0);
        rig
    }
    fn advance(&mut self, tick: u64) -> Vec<NoteEvent> {
        let notes = self.clock.update_app(self.now + ticks(tick), &mut self.app, || {});
        self.clock.take_metronome_event();
        notes
    }
    fn hit(&mut self, tick: u64, ch: char) {
        self.advance(tick);
        key(&mut self.app, self.now + ticks(tick), KeyCode::Char(ch));
    }
    fn normal(&mut self, tick: u64, text: &str) {
        key(&mut self.app, self.now + ticks(tick), KeyCode::Esc);
        prompt(&mut self.app, self.now + ticks(tick), text);
    }
}

#[test]
fn real_controls_are_atomic_session_only_and_count_input_is_preview_only() {
    let mut rig = Rig::new(true);
    assert!(matches!(rig.clock.phase(), TransportPhase::Counting { .. }));
    let revision = rig.app.revision();
    rig.hit(100, 'z');
    assert!(rig.app.take_audition().is_some());
    assert_eq!(rig.app.revision(), revision);
    assert_eq!(rig.app.cursor_row, 0);
    rig.advance(3839);
    assert!(!rig.app.capture_eligible());
    rig.hit(3840, 'x');
    assert!(rig.app.capture_eligible());
    assert_eq!(
        rig.app.project.patterns[0].event_at(0, Tick(0)).unwrap().pitch,
        NotePitch::D
    );
    rig.normal(3840, "record off");
    assert!(!rig.app.recording.armed);
    for text in [
        "record",
        "record ON",
        "record on extra",
        "countin",
        "countin 2",
        "countin on",
        "countin 0 extra",
    ] {
        prompt(&mut rig.app, rig.now + ticks(3840), text);
        assert!(!rig.app.recording.armed);
        assert!(rig.app.recording.count_in);
    }
    let file = tempfile::NamedTempFile::new().unwrap();
    prompt(
        &mut rig.app,
        rig.now + ticks(3840),
        &format!("w {}", file.path().display()),
    );
    let json = std::fs::read_to_string(file.path()).unwrap();
    assert!(!json.contains("armed"));
    assert!(!json.contains("count_in"));
    prompt(&mut rig.app, rig.now + ticks(3840), "record on");
    prompt(&mut rig.app, rig.now + ticks(3840), "e! missing-record-project.json");
    assert!(rig.app.recording.armed);
    prompt(
        &mut rig.app,
        rig.now + ticks(3840),
        &format!("e! {}", file.path().display()),
    );
    assert!(!rig.app.recording.armed);
    assert!(rig.app.recording.count_in);
}

#[test]
fn nearest_ties_forward_fixed_gate_replacement_and_pinned_track() {
    let mut rig = Rig::new(false);
    rig.app.entry_duration = Some(crate::musical_time::DurationTicks::new(320).unwrap());
    rig.app.edit_step = 4;
    rig.hit(119, 'z');
    rig.hit(120, 'x'); // half of the 240-tick grid, forward to 240
    key(&mut rig.app, rig.now + ticks(120), KeyCode::Right);
    rig.hit(121, 's'); // last input at same exact tick wins, still pinned to track 0
    let pattern = &rig.app.project.patterns[0];
    assert_eq!(pattern.event_at(0, Tick(0)).unwrap().pitch, NotePitch::C);
    let event = pattern.event_at(0, Tick(240)).unwrap();
    assert_eq!(event.pitch, NotePitch::Cs);
    assert_eq!(event.duration_ticks.ticks(), 320);
    assert_eq!(pattern.tracks[1].events(), []);
    assert_eq!(rig.app.cursor_row, 0);
    assert_eq!(rig.app.cursor_track, 1);
    assert_eq!(rig.app.take_audition().unwrap().track, 0);
    rig.normal(121, "record off");
    key(&mut rig.app, rig.now + ticks(121), KeyCode::Char('u'));
    assert_eq!(rig.app.project.patterns[0].tracks[0].events(), []);
    handle_key_at(
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        &mut rig.app,
        rig.now + ticks(121),
    );
    assert_eq!(rig.app.project.patterns[0].tracks[0].events().len(), 2);
}

#[test]
fn preview_is_not_duplicated_by_future_dispatch_and_next_pass_plays() {
    let mut rig = Rig::new(false);
    rig.hit(120, 'z');
    assert!(rig.app.take_audition().is_some());
    assert!(rig.advance(240).is_empty());
    assert_eq!(rig.advance(3840 + 240).len(), 1);
}

#[test]
fn loop_edge_wraps_to_zero_and_pass_history_is_separate() {
    let mut rig = Rig::new(false);
    rig.hit(240, 'z');
    rig.hit(3830, 'x');
    assert_eq!(
        rig.app.project.patterns[0].event_at(0, Tick(0)).unwrap().pitch,
        NotePitch::D
    );
    assert!(rig.advance(3840).is_empty()); // monitored wrap occurrence
    rig.hit(3840 + 480, 'c');
    key(&mut rig.app, rig.now + ticks(4320), KeyCode::Esc);
    key(&mut rig.app, rig.now + ticks(4320), KeyCode::Char('u'));
    assert!(matches!(rig.app.playback_state, PlaybackState::Stopped));
    assert!(rig.app.recording.armed);
    assert!(rig.app.recording.target.is_none());
    assert!(rig.app.project.patterns[0].event_at(0, Tick(480)).is_none());
    assert!(rig.app.project.patterns[0].event_at(0, Tick(240)).is_some());
    key(&mut rig.app, rig.now + ticks(4320), KeyCode::Char('u'));
    assert_eq!(rig.app.project.patterns[0].tracks[0].events(), []);
}

#[test]
fn mode_table_repeat_guards_and_ordinary_navigation_never_record_hidden_notes() {
    let mut rig = Rig::new(false);
    let revision = rig.app.revision();
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        handle_key_at(
            KeyEvent::new_with_kind(KeyCode::Char('z'), KeyModifiers::NONE, kind),
            &mut rig.app,
            rig.now,
        );
    }
    assert_eq!(rig.app.revision(), revision);
    key(&mut rig.app, rig.now, KeyCode::Esc);
    key(&mut rig.app, rig.now, KeyCode::Enter); // Audition
    rig.hit(240, 'z');
    assert_eq!(rig.app.mode, Mode::Audition);
    assert_eq!(rig.app.revision(), revision);
    key(&mut rig.app, rig.now + ticks(240), KeyCode::Esc);
    for ch in "2j".chars() {
        key(&mut rig.app, rig.now + ticks(240), KeyCode::Char(ch));
    }
    assert_eq!(rig.app.revision(), revision);
    rig.normal(240, "record off");
    key(&mut rig.app, rig.now + ticks(240), KeyCode::Char('i'));
    rig.hit(480, 'z');
    assert_eq!(rig.app.revision(), revision);
    assert!(rig.app.take_audition().is_some());
    key(&mut rig.app, rig.now + ticks(480), KeyCode::Char(' '));
    rig.hit(500, 'z'); // stopped Insert still step-writes
    assert_ne!(rig.app.revision(), revision);
}

#[test]
fn disarm_cancels_count_and_song_never_counts_or_captures() {
    let mut rig = Rig::new(true);
    rig.normal(120, "record off");
    rig.advance(120);
    assert_eq!(rig.clock.phase(), TransportPhase::Stopped);
    prompt(&mut rig.app, rig.now + ticks(120), "record on");
    rig.app.handle_command(Command::ToggleSong);
    key(&mut rig.app, rig.now + ticks(120), KeyCode::Char(' '));
    key(&mut rig.app, rig.now + ticks(120), KeyCode::Char('i'));
    rig.advance(120);
    assert_eq!(rig.app.playback_mode, PlaybackMode::Song);
    assert_eq!(rig.clock.phase(), TransportPhase::Playing);
    assert!(rig.app.recording_label().contains("unavailable"));
    let revision = rig.app.revision();
    rig.hit(240, 'z');
    assert_eq!(rig.app.revision(), revision);
}

#[test]
fn stale_and_out_of_order_receipts_drop_without_writing() {
    let mut rig = Rig::new(false);
    rig.advance(960);
    let revision = rig.app.revision();
    key(&mut rig.app, rig.now + ticks(100), KeyCode::Char('z'));
    assert!(rig.app.status.contains("late"));
    assert_eq!(rig.app.revision(), revision);
    rig.hit(1000, 'z');
    let revision = rig.app.revision();
    key(&mut rig.app, rig.now + ticks(999), KeyCode::Char('x'));
    assert!(rig.app.status.contains("out-of-order"));
    assert_eq!(rig.app.revision(), revision);
}

#[test]
fn arming_mid_pass_pins_playing_pattern_and_eligibility_resumes_without_retargeting() {
    let mut rig = Rig::new(false);
    rig.normal(100, "record off");
    rig.app.project.patterns.push(crate::tracker::Pattern::new(16, 4));
    rig.app.selected_pattern = 1;
    rig.app.cursor_track = 2;
    prompt(&mut rig.app, rig.now + ticks(100), "record on");
    key(&mut rig.app, rig.now + ticks(100), KeyCode::Char('i'));
    rig.hit(240, 'z');
    assert_eq!(rig.app.project.patterns[1].tracks[2].events(), []);
    assert!(!rig.app.capture_eligible());
    assert_eq!(rig.app.recording.target, Some((0, 2)));
    rig.app.selected_pattern = 0;
    rig.app.cursor_track = 1;
    rig.hit(480, 'x');
    assert!(rig.app.project.patterns[0].event_at(2, Tick(480)).is_some());
    assert_eq!(rig.app.project.patterns[0].tracks[1].events(), []);
    key(&mut rig.app, rig.now + ticks(480), KeyCode::Esc);
    key(&mut rig.app, rig.now + ticks(480), KeyCode::F(2));
    let revision = rig.app.revision();
    rig.hit(720, 'z');
    assert_eq!(rig.app.revision(), revision);
    key(&mut rig.app, rig.now + ticks(720), KeyCode::F(1));
    key(&mut rig.app, rig.now + ticks(720), KeyCode::Char('i'));
    rig.hit(960, 'c');
    key(&mut rig.app, rig.now + ticks(960), KeyCode::Esc);
    key(&mut rig.app, rig.now + ticks(960), KeyCode::Char('u'));
    assert!(rig.app.project.patterns[0].event_at(2, Tick(960)).is_none());
    assert!(rig.app.project.patterns[0].event_at(2, Tick(480)).is_some());
}

#[test]
fn save_closes_pass_and_unchanged_hits_do_not_dirty_or_add_undo() {
    let mut rig = Rig::new(false);
    rig.hit(240, 'z');
    let file = tempfile::NamedTempFile::new().unwrap();
    rig.normal(240, &format!("w {}", file.path().display()));
    assert!(!rig.app.is_dirty());
    key(&mut rig.app, rig.now + ticks(240), KeyCode::Char('i'));
    rig.hit(241, 'z'); // unchanged pitch and gate at 240
    assert!(!rig.app.is_dirty());
    rig.hit(480, 'x');
    assert!(rig.app.is_dirty());
    key(&mut rig.app, rig.now + ticks(480), KeyCode::Esc);
    key(&mut rig.app, rig.now + ticks(480), KeyCode::Char('u'));
    assert!(!rig.app.is_dirty());
    assert!(rig.app.project.patterns[0].event_at(0, Tick(240)).is_some());
    assert!(rig.app.project.patterns[0].event_at(0, Tick(480)).is_none());
}

#[test]
fn replacement_restores_original_gate_and_off_grid_events_survive_undo() {
    use crate::{musical_time::DurationTicks, tracker::PatternEvent};
    let mut rig = Rig::new(false);
    rig.app.edit(|edit| {
        edit.set_event(
            0,
            0,
            Tick(240),
            Some(PatternEvent {
                start_tick: Tick(240),
                duration_ticks: DurationTicks::new(700).unwrap(),
                pitch: NotePitch::E,
                octave: 3,
            }),
        );
        edit.set_event(
            0,
            0,
            Tick(241),
            Some(PatternEvent {
                start_tick: Tick(241),
                duration_ticks: DurationTicks::new(17).unwrap(),
                pitch: NotePitch::F,
                octave: 2,
            }),
        );
    });
    let original = rig.app.project.clone();
    rig.hit(100, 'z');
    rig.hit(120, 'x');
    assert_eq!(
        rig.app.project.patterns[0]
            .event_at(0, Tick(240))
            .unwrap()
            .duration_ticks
            .ticks(),
        240
    );
    assert_eq!(
        rig.app.project.patterns[0].event_at(0, Tick(241)),
        original.patterns[0].event_at(0, Tick(241))
    );
    key(&mut rig.app, rig.now + ticks(120), KeyCode::Esc);
    key(&mut rig.app, rig.now + ticks(120), KeyCode::Char('u'));
    assert_eq!(rig.app.project, original);
}

#[test]
fn cancellation_restart_controls_retain_arm_and_recount_only_pattern_starts() {
    let mut rig = Rig::new(true);
    rig.advance(480);
    handle_key_at(
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL),
        &mut rig.app,
        rig.now + ticks(480),
    );
    rig.advance(480);
    assert_eq!(rig.clock.phase(), TransportPhase::Counting { beat: 1, total: 4 });
    assert!(rig.app.recording.armed);
    key(&mut rig.app, rig.now + ticks(480), KeyCode::Char(' '));
    rig.advance(480);
    assert_eq!(rig.clock.phase(), TransportPhase::Stopped);
    assert!(rig.app.recording.armed);
    key(&mut rig.app, rig.now + ticks(480), KeyCode::Esc);
    prompt(&mut rig.app, rig.now + ticks(480), "restart");
    rig.advance(480);
    assert!(matches!(rig.clock.phase(), TransportPhase::Counting { .. }));
    rig.normal(480, "end 1001t");
    rig.advance(480);
    assert_eq!(rig.clock.phase(), TransportPhase::Stopped);
}

#[test]
fn capture_budget_failure_is_atomic_but_still_previews() {
    use crate::{
        musical_time::DurationTicks,
        tracker::{Pattern, PatternEvent},
    };
    let mut rig = Rig::new(false);
    let mut pattern = Pattern::new(4096, 4);
    for start in 1000..1000 + crate::project_file::MAX_TRACK_EVENTS as u64 {
        pattern
            .replace_event(
                0,
                Tick(start),
                Some(PatternEvent {
                    start_tick: Tick(start),
                    duration_ticks: DurationTicks::new(1).unwrap(),
                    pitch: NotePitch::C,
                    octave: 4,
                }),
            )
            .unwrap();
    }
    rig.app.project.patterns[0] = pattern;
    let original = rig.app.project.clone();
    let revision = rig.app.revision();
    rig.hit(120, 'x');
    assert_eq!(rig.app.project, original);
    assert_eq!(rig.app.revision(), revision);
    assert_ne!(rig.app.status, "");
    assert!(rig.app.take_audition().is_some());
}

#[test]
fn pass_history_respects_retention_and_never_merges_across_loops() {
    let mut rig = Rig::new(false);
    for pass in 0..crate::history::MAX_TRANSACTIONS as u64 + 4 {
        rig.hit(pass * 3840 + 240, if pass.is_multiple_of(2) { 'z' } else { 'x' });
    }
    rig.app.handle_command(Command::EnterNormal);
    for _ in 0..crate::history::MAX_TRANSACTIONS {
        rig.app.handle_command(Command::Undo);
    }
    let oldest = rig.app.revision();
    rig.app.handle_command(Command::Undo);
    assert_eq!(rig.app.revision(), oldest);
    assert!(rig.app.project.patterns[0].event_at(0, Tick(240)).is_some());
}

#[test]
fn capture_uses_committed_tempo_segment_not_the_ui_bpm() {
    let mut rig = Rig::new(false);
    rig.app.project.bpm = 60.0;
    rig.hit(120, 'z'); // before the committed 125ms boundary: old segment tick 120
    assert!(rig.app.project.patterns[0].event_at(0, Tick(240)).is_some());
    rig.hit(480, 'x'); // at 250ms: new segment tick 360, ties forward to 480
    assert!(rig.app.project.patterns[0].event_at(0, Tick(480)).is_some());
    let snapshot = rig.clock.clock_snapshot().unwrap();
    assert_eq!(snapshot.tick_at(rig.now + ticks(480)), Some(360));
    assert_eq!(snapshot.previous_tick(), 240);
    assert_eq!(snapshot.next_tick(), 480);
}

#[test]
fn armed_controls_count_full_destination_meter_at_all_supported_tempos() {
    for bpm in [20_u32, 120, 300] {
        for (meter, bar) in [("3/4", 2880_u64), ("7/8", 3360), ("6/8", 2880)] {
            let now = Instant::now();
            let mut app = App::default();
            app.project.bpm = f64::from(bpm);
            prompt(&mut app, now, "end 481t");
            prompt(&mut app, now, &format!("meter {meter}"));
            prompt(&mut app, now, "record on");
            prompt(&mut app, now, "countin 1");
            app.cursor_row = 1;
            key(&mut app, now, KeyCode::Char(' '));
            key(&mut app, now, KeyCode::Char('i'));
            let mut clock = Transport::default();
            assert_eq!(clock.update_app(now, &mut app, || {}).len(), 0);
            let end =
                now + Duration::from_nanos((bar * 60_000_000_000 + u64::from(bpm) * 480) / (u64::from(bpm) * 960));
            clock.update_app(end.checked_sub(Duration::from_nanos(1)).unwrap(), &mut app, || {});
            key(
                &mut app,
                end.checked_sub(Duration::from_nanos(1)).unwrap(),
                KeyCode::Char('x'),
            );
            assert!(app.project.patterns[0].event_at(0, Tick(240)).is_none());
            assert_eq!(app.playback_tick, Tick(240));
            clock.update_app(end, &mut app, || {});
            key(&mut app, end, KeyCode::Char('z'));
            assert_eq!(
                app.project.patterns[0].event_at(0, Tick(240)).unwrap().pitch,
                NotePitch::C
            );
            assert!(app.capture_eligible());
        }
    }
}

#[test]
fn net_noop_pass_does_not_consume_history_or_leave_document_dirty() {
    let mut rig = Rig::new(false);
    rig.hit(0, 'z');
    rig.normal(0, "record off");
    let file = tempfile::NamedTempFile::new().unwrap();
    prompt(&mut rig.app, rig.now, &format!("w {}", file.path().display()));
    prompt(&mut rig.app, rig.now, "record on");
    key(&mut rig.app, rig.now, KeyCode::Char('i'));
    rig.hit(1, 'x');
    rig.hit(2, 'z');
    assert!(!rig.app.is_dirty());
    key(&mut rig.app, rig.now + ticks(2), KeyCode::Esc);
    key(&mut rig.app, rig.now + ticks(2), KeyCode::Char('u'));
    assert!(rig.app.project.patterns[0].event_at(0, Tick(0)).is_none());
}

#[test]
fn help_prompt_other_views_and_normal_counts_cannot_capture() {
    let mut rig = Rig::new(false);
    let revision = rig.app.revision();
    key(&mut rig.app, rig.now, KeyCode::Char('?'));
    rig.hit(120, 'z');
    assert_eq!(rig.app.revision(), revision);
    key(&mut rig.app, rig.now + ticks(120), KeyCode::Esc);
    key(&mut rig.app, rig.now + ticks(120), KeyCode::Esc);
    key(&mut rig.app, rig.now + ticks(120), KeyCode::Char(':'));
    key(&mut rig.app, rig.now + ticks(120), KeyCode::Char('z'));
    assert_eq!(rig.app.revision(), revision);
    key(&mut rig.app, rig.now + ticks(120), KeyCode::Esc);
    for view in [KeyCode::F(2), KeyCode::F(3), KeyCode::F(4)] {
        key(&mut rig.app, rig.now + ticks(120), view);
        rig.hit(120, 'z');
        assert_eq!(rig.app.revision(), revision);
        assert!(!rig.app.capture_eligible());
    }
}

#[test]
fn session_commands_work_in_each_normal_view_without_history_or_piano_key_theft() {
    let mut app = App::default();
    let now = Instant::now();
    let revision = app.revision();
    for view in 1..=4 {
        key(&mut app, now, KeyCode::F(view));
        prompt(&mut app, now, "record on");
        prompt(&mut app, now, "countin 1");
        assert!(app.recording.armed && app.recording.count_in);
        prompt(&mut app, now, "record toggle");
        prompt(&mut app, now, "countin 0");
        assert!(!app.recording.armed && !app.recording.count_in);
        assert_eq!(app.revision(), revision);
        assert!(!app.is_dirty());
    }
    key(&mut app, now, KeyCode::F(1));
    key(&mut app, now, KeyCode::Char('i'));
    key(&mut app, now, KeyCode::Char('2'));
    assert!(app.project.patterns[0].event_at(0, Tick(0)).is_some());
    assert!(!app.recording.armed);
}

#[test]
fn per_bar_override_and_shortest_loop_use_exact_local_boundaries() {
    let mut rig = Rig::new(false);
    rig.normal(0, "end 7680t");
    rig.app.cursor_row = 16;
    prompt(&mut rig.app, rig.now, "grid 3 bar");
    rig.app.cursor_row = 0;
    key(&mut rig.app, rig.now, KeyCode::Char(' '));
    key(&mut rig.app, rig.now, KeyCode::Char('i'));
    rig.advance(0);
    rig.hit(3840 + 160, 'z');
    assert!(rig.app.project.patterns[0].event_at(0, Tick(4160)).is_some());
    rig.normal(4000, "end! 1t");
    key(&mut rig.app, rig.now + ticks(4000), KeyCode::Char(' '));
    key(&mut rig.app, rig.now + ticks(4000), KeyCode::Char('i'));
    rig.hit(4000, 'x');
    rig.hit(4001, 'c');
    assert_eq!(rig.app.project.patterns[0].tracks[0].events().len(), 1);
    assert_eq!(
        rig.app.project.patterns[0].event_at(0, Tick(0)).unwrap().pitch,
        NotePitch::E
    );
}

#[test]
fn local_grid_changes_activate_on_next_bar_and_partial_end_wraps() {
    let mut rig = Rig::new(false);
    rig.hit(100, 'z');
    rig.normal(100, "grid 3");
    key(&mut rig.app, rig.now + ticks(100), KeyCode::Char('i'));
    rig.hit(180, 'x');
    assert!(rig.app.project.patterns[0].event_at(0, Tick(240)).is_some()); // old grid for bar
    rig.hit(3840 + 180, 'c');
    assert!(rig.app.project.patterns[0].event_at(0, Tick(320)).is_some()); // new grid on next loop
    rig.normal(4020, "end 1001t");
    key(&mut rig.app, rig.now + ticks(4020), KeyCode::Char(' '));
    key(&mut rig.app, rig.now + ticks(4020), KeyCode::Char('i'));
    rig.advance(4020);
    rig.hit(4020 + 995, 'z');
    assert_eq!(
        rig.app.project.patterns[0].event_at(0, Tick(0)).unwrap().pitch,
        NotePitch::C
    );
}
