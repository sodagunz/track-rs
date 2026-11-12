use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{
    bindings::handle_key,
    history::MAX_MUTATIONS,
    project::Project,
    state::{App, Mode, PlaybackState},
    tracker::{Note, NotePitch, Pattern},
    transport::{collect_row, PlaybackRoute, Transport},
};

fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    handle_key(KeyEvent::new(code, modifiers), app);
}
fn plain(app: &mut App, code: KeyCode) {
    key(app, code, KeyModifiers::NONE);
}
fn ctrl(app: &mut App, code: KeyCode) {
    key(app, code, KeyModifiers::CONTROL);
}
fn text(app: &mut App, value: &str) {
    for ch in value.chars() {
        plain(app, KeyCode::Char(ch));
    }
}
fn note() -> Note {
    Note {
        length_ticks: crate::tracker::NoteLength::default(),
        pitch: NotePitch::C,
        octave: 4,
    }
}
fn playing(app: &App) -> bool {
    matches!(app.playback_state, PlaybackState::Playing)
}
fn tick(app: &mut App, clock: &mut Transport, now: Instant) -> Option<crate::transport::PlaybackEvent> {
    if app.take_stop_request() {
        clock.stop();
    }
    let event = clock.update_project(
        now,
        playing(app),
        &app.project,
        if app.playback_mode == crate::state::PlaybackMode::Song {
            PlaybackRoute::Song
        } else {
            PlaybackRoute::Pattern(app.playing_pattern)
        },
        4,
    )?;
    app.playback_row = event.row.row;
    app.playing_pattern = event.pattern;
    app.playing_order = event.order.unwrap_or(0);
    Some(event)
}

#[test]
fn actual_keys_build_aab_save_reload_and_keep_editing_separate() {
    let mut app = App::default();
    text(&mut app, "iz");
    plain(&mut app, KeyCode::Esc);
    ctrl(&mut app, KeyCode::Char('a')); // A/A
    ctrl(&mut app, KeyCode::Char('d')); // B is a deep duplicate, selected but not appended
    assert_eq!(app.selected_pattern, 1);
    assert_eq!(app.project.patterns[0], app.project.patterns[1]);
    text(&mut app, "ix"); // replace only B's C with D
    plain(&mut app, KeyCode::Esc);
    assert_ne!(app.project.patterns[0], app.project.patterns[1]);
    ctrl(&mut app, KeyCode::Char('a'));
    assert_eq!(app.project.order, [0, 0, 1]);
    plain(&mut app, KeyCode::Char('O'));
    plain(&mut app, KeyCode::Char(' '));
    let mut clock = Transport::default();
    let start = Instant::now();
    let event = tick(&mut app, &mut clock, start).unwrap();
    assert_eq!((event.order, event.pattern, event.row.row), (Some(0), 0, 0));
    assert_eq!(
        collect_row(
            &app.project.patterns[event.pattern],
            app.project.instruments(),
            event.row
        )[0]
        .note
        .pitch,
        NotePitch::C
    );
    text(&mut app, "ix"); // edit B while A plays
    plain(&mut app, KeyCode::Esc);
    plain(&mut app, KeyCode::PageUp);
    plain(&mut app, KeyCode::PageDown);
    assert!(playing(&app));
    assert!(!app.take_stop_request());
    let event = tick(&mut app, &mut clock, start + Duration::from_secs(8)).unwrap();
    assert_eq!((event.order, event.pattern, event.row.row), (Some(1), 0, 0));
    let event = tick(&mut app, &mut clock, start + Duration::from_secs(16)).unwrap();
    assert_eq!((event.order, event.pattern, event.row.row), (Some(2), 1, 0));
    let event = tick(&mut app, &mut clock, start + Duration::from_secs(24)).unwrap();
    assert_eq!((event.order, event.pattern, event.row.row), (Some(0), 0, 0));

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("A A B.json");
    text(&mut app, &format!(":w {}", path.display()));
    plain(&mut app, KeyCode::Enter);
    assert!(!app.is_dirty());
    let saved = app.project.clone();
    ctrl(&mut app, KeyCode::Char('n'));
    assert!(app.is_dirty());
    text(&mut app, "u");
    assert!(!app.is_dirty());
    text(&mut app, &format!(":e {}", path.display()));
    plain(&mut app, KeyCode::Enter);
    assert_eq!(app.project, saved);
    assert!(!playing(&app));
    assert!(app.take_stop_request());
}

#[test]
fn pattern_loop_is_pinned_until_explicit_stop_restart_or_mode_toggle() {
    let mut app = App::default();
    ctrl(&mut app, KeyCode::Char('n'));
    app.project.patterns[0] = Pattern::new(2, 4);
    app.project.patterns[1] = Pattern::new(3, 4);
    plain(&mut app, KeyCode::PageUp);
    plain(&mut app, KeyCode::Char(' '));
    let mut clock = Transport::default();
    let start = Instant::now();
    assert_eq!(tick(&mut app, &mut clock, start).unwrap().pattern, 0);
    plain(&mut app, KeyCode::PageDown);
    text(&mut app, "iz");
    plain(&mut app, KeyCode::Esc);
    assert_eq!(app.project.patterns[1].get_step(0, 0).unwrap().note, Some(note()));
    assert_eq!(app.project.patterns[0].get_step(0, 0).unwrap().note, None);
    let event = tick(&mut app, &mut clock, start + Duration::from_millis(250)).unwrap();
    assert_eq!((event.pattern, event.row.row), (0, 0));
    // Even a direct route request cannot retarget an active transport.
    assert_eq!(
        clock
            .update_project(
                start + Duration::from_millis(375),
                true,
                &app.project,
                PlaybackRoute::Pattern(1),
                4
            )
            .unwrap()
            .pattern,
        0
    );
    plain(&mut app, KeyCode::Char(' '));
    plain(&mut app, KeyCode::Char(' '));
    let event = tick(&mut app, &mut clock, start + Duration::from_secs(1)).unwrap();
    assert_eq!((event.pattern, event.row.row), (1, 0));
    plain(&mut app, KeyCode::Char('O'));
    assert!(!playing(&app));
    assert!(tick(&mut app, &mut clock, start + Duration::from_secs(2)).is_none());
    plain(&mut app, KeyCode::Char(' '));
    let event = tick(&mut app, &mut clock, start + Duration::from_secs(3)).unwrap();
    assert_eq!((event.order, event.pattern, event.row.row), (Some(0), 0, 0));
}

#[test]
fn variable_lengths_repeated_entries_and_huge_delays_keep_anchored_deadlines() {
    let mut project = Project::new(2, 1);
    project.patterns.push(Pattern::new(3, 1));
    project.order = vec![0, 0, 1]; // seven rows, not five
    let mut clock = Transport::default();
    let start = Instant::now();
    for (tick, order, pattern, row) in [
        (0, 0, 0, 0),
        (1, 0, 0, 1),
        (2, 1, 0, 0),
        (3, 1, 0, 1),
        (4, 2, 1, 0),
        (6, 2, 1, 2),
        (7, 0, 0, 0),
        (700_000_004, 2, 1, 0),
    ] {
        let now = start + Duration::from_millis(tick * 125);
        let event = clock
            .update_project(now, true, &project, PlaybackRoute::Song, 4)
            .unwrap();
        assert_eq!((event.order, event.pattern, event.row.row), (Some(order), pattern, row));
        assert_eq!(clock.deadline(), Some(now + Duration::from_millis(125)));
        assert!(clock
            .update_project(now, true, &project, PlaybackRoute::Song, 4)
            .is_none());
    }
    let boundary = clock.deadline().unwrap();
    project.bpm = 60.0;
    assert!(clock
        .update_project(
            boundary.checked_sub(Duration::from_nanos(1)).unwrap(),
            true,
            &project,
            PlaybackRoute::Song,
            4
        )
        .is_none());
    assert_eq!(clock.deadline(), Some(boundary));
    let now = boundary + Duration::from_millis(250 * 7 * 1_000_000 + 250 + 17);
    let event = clock
        .update_project(now, true, &project, PlaybackRoute::Song, 4)
        .unwrap();
    assert_eq!((event.order, event.pattern, event.row.row), (Some(2), 1, 2));
    assert_eq!(event.row.duration, Duration::from_millis(250));
    assert_eq!(clock.deadline(), Some(now + Duration::from_millis(233)));
    clock.stop();
    let event = clock
        .update_project(now, true, &project, PlaybackRoute::Song, 4)
        .unwrap();
    assert_eq!((event.order, event.row.row), (Some(0), 0));
}

#[test]
fn order_history_stops_only_real_changes_and_never_leaves_invalid_selection() {
    let mut app = App::default();
    plain(&mut app, KeyCode::Char(' '));
    for code in [KeyCode::Char('x'), KeyCode::Up, KeyCode::Down] {
        ctrl(&mut app, code);
    }
    text(&mut app, "u");
    ctrl(&mut app, KeyCode::Char('r'));
    assert!(playing(&app));
    assert!(!app.take_stop_request());
    ctrl(&mut app, KeyCode::Char('a'));
    assert!(!playing(&app));
    assert!(app.take_stop_request());
    let arranged = app.revision();
    plain(&mut app, KeyCode::Char(' '));
    ctrl(&mut app, KeyCode::Up); // exchanging A with A is a no-op
    assert!(playing(&app));
    assert!(!app.take_stop_request());
    assert_eq!(app.revision(), arranged);
    text(&mut app, "u");
    assert!(!playing(&app));
    assert!(app.take_stop_request());
    assert_eq!((app.project.order.as_slice(), app.selected_order), (&[0][..], 0));
    plain(&mut app, KeyCode::Char(' '));
    ctrl(&mut app, KeyCode::Char('r'));
    assert!(!playing(&app));
    assert!(app.take_stop_request());
    assert_eq!(app.revision(), arranged);
    ctrl(&mut app, KeyCode::Char('n'));
    ctrl(&mut app, KeyCode::Char('a'));
    ctrl(&mut app, KeyCode::Up);
    assert_eq!(app.project.order, [0, 1, 0]);
    ctrl(&mut app, KeyCode::Down);
    assert_eq!(app.project.order, [0, 0, 1]);
    ctrl(&mut app, KeyCode::Left);
    ctrl(&mut app, KeyCode::Right);
    ctrl(&mut app, KeyCode::Char('x'));
    assert_eq!(app.project.order, [0, 0]);
    text(&mut app, "uuu");
    assert_eq!(app.project.order, [0, 0, 1]);
    text(&mut app, "uu"); // undo append and creation, without deleting a referenced pattern
    assert_eq!(app.project.patterns.len(), 1);
    assert_eq!(app.selected_pattern, 0);
    assert!(app.project.order.iter().all(|&i| i < app.project.patterns.len()));
}

#[test]
fn variable_length_song_round_trip_and_note_history_preserve_future_events() {
    let mut app = App {
        project: Project::new(2, 1),
        ..App::default()
    };
    ctrl(&mut app, KeyCode::Char('n'));
    app.project.patterns[1] = Pattern::new(3, 1);
    app.project.order = vec![0, 0, 1];
    app.insert_note_under_cursor(Some(note()));
    plain(&mut app, KeyCode::Char('O'));
    plain(&mut app, KeyCode::Char(' '));
    let start = Instant::now();
    let mut clock = Transport::default();
    tick(&mut app, &mut clock, start).unwrap();
    let boundary = clock.deadline();
    text(&mut app, "u");
    assert!(playing(&app));
    assert!(!app.take_stop_request());
    assert_eq!(clock.deadline(), boundary);
    ctrl(&mut app, KeyCode::Char('r'));
    assert!(playing(&app));
    assert!(!app.take_stop_request());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("variable.json");
    text(&mut app, &format!(":w {}", path.display()));
    plain(&mut app, KeyCode::Enter);
    assert!(!app.is_dirty());
    text(&mut app, &format!(":e {}", path.display()));
    plain(&mut app, KeyCode::Enter);
    assert_eq!(app.project.patterns[1].num_rows(), 3);
    assert_eq!(app.project.order, [0, 0, 1]);
    plain(&mut app, KeyCode::Char(' '));
    assert_eq!(tick(&mut app, &mut clock, start).unwrap().order, Some(0));
    let event = tick(&mut app, &mut clock, start + Duration::from_millis(500)).unwrap();
    assert_eq!((event.order, event.pattern, event.row.row), (Some(2), 1, 0));
    assert_eq!(
        collect_row(&app.project.patterns[1], app.project.instruments(), event.row)[0].note,
        note()
    );
    ctrl(&mut app, KeyCode::Char('x'));
    assert!(tick(&mut app, &mut clock, start + Duration::from_millis(625)).is_none());
    assert_eq!(clock.deadline(), None);
    plain(&mut app, KeyCode::Char(' '));
    let event = tick(&mut app, &mut clock, start + Duration::from_secs(1)).unwrap();
    assert_eq!((event.order, event.row.row), (Some(0), 0));
}

#[test]
fn pattern_keys_are_normal_press_only_counts_help_and_piano_still_work() {
    let mut app = App::default();
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        handle_key(
            KeyEvent::new_with_kind(KeyCode::Char('n'), KeyModifiers::CONTROL, kind),
            &mut app,
        );
    }
    text(&mut app, "2");
    ctrl(&mut app, KeyCode::Char('n'));
    assert_eq!(app.project.patterns.len(), 1);
    text(&mut app, "?");
    ctrl(&mut app, KeyCode::Char('n'));
    text(&mut app, "?");
    text(&mut app, "izxcvbnm,");
    ctrl(&mut app, KeyCode::Char('n'));
    plain(&mut app, KeyCode::PageDown);
    assert_eq!(app.project.patterns.len(), 1);
    assert_eq!(app.mode, Mode::Insert);
    plain(&mut app, KeyCode::Esc);
    ctrl(&mut app, KeyCode::Char('n'));
    assert!(app.project.patterns[1]
        .tracks
        .iter()
        .all(|track| track.events().is_empty()));
    app.project.patterns[0] = Pattern::new(2, 4);
    app.cursor_row = 63;
    app.selection_anchor = Some((63, 0));
    plain(&mut app, KeyCode::PageUp);
    assert_eq!(app.cursor_row, 1);
    assert_eq!(app.selection_anchor, None);
    assert_eq!(app.pending_count, None);
}

#[test]
fn allocation_limits_and_weighted_history_cover_structural_edits() {
    use crate::project_file::{MAX_CELLS, MAX_ORDER, MAX_PATTERNS};
    let mut app = App::default();
    app.project.patterns = vec![Pattern::new(1, 4); MAX_PATTERNS];
    let revision = app.revision();
    ctrl(&mut app, KeyCode::Char('n'));
    ctrl(&mut app, KeyCode::Char('d'));
    assert_eq!(app.project.patterns.len(), MAX_PATTERNS);
    assert_eq!(app.revision(), revision);
    app.project.order = vec![0; MAX_ORDER];
    plain(&mut app, KeyCode::Char(' '));
    ctrl(&mut app, KeyCode::Char('a'));
    assert_eq!(app.project.order.len(), MAX_ORDER);
    assert!(playing(&app));
    assert!(!app.take_stop_request());
    app.edit(|edit| edit.set_order(vec![MAX_PATTERNS]));
    app.edit(|edit| edit.set_order(vec![]));
    assert_eq!(app.revision(), revision);
    assert!(playing(&app));

    // Empty sparse patterns are not charged for hypothetical cells.
    app.project = Project::new(4096, 128);
    app.project
        .patterns
        .push(Pattern::new((MAX_CELLS - 4096 * 128) / 128, 128));
    ctrl(&mut app, KeyCode::Char('d'));
    assert_eq!(app.project.patterns.len(), 3);
    text(&mut app, "u");
    assert_eq!(app.project.patterns.len(), 2);

    let mut app = App {
        project: Project::new(MAX_MUTATIONS / 32 + 1, 32),
        ..App::default()
    };
    for row in 0..=MAX_MUTATIONS / 32 {
        for track in 0..32 {
            app.project.patterns[0]
                .set_step(
                    track,
                    row,
                    Some(Note {
                        pitch: NotePitch::C,
                        octave: 4,
                        length_ticks: crate::tracker::NoteLength::default(),
                    }),
                )
                .unwrap();
        }
    }
    ctrl(&mut app, KeyCode::Char('d'));
    let revision = app.revision();
    text(&mut app, "u");
    assert_eq!(app.project.patterns.len(), 2); // oversized patch evicted whole
    assert_eq!(app.revision(), revision);
    assert!(app.is_dirty());
}
