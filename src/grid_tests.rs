use crate::{
    command::Command,
    musical_time::{Grid, GridMap, Meter, Tick},
    state::{App, Mode, PlaybackState},
    tracker::NotePitch,
    transport::Transport,
};
use std::time::{Duration, Instant};

fn enter(app: &mut App, pitch: NotePitch) {
    app.handle_command(Command::EnterInsert);
    app.handle_command(Command::InsertNote {
        pitch,
        octave_offset: 0,
    });
    app.handle_command(Command::EnterNormal);
}

#[test]
fn indexed_projection_matches_grid_neighbors_without_a_dense_matrix() {
    for (n, d) in [(3, 4), (7, 8), (6, 8), (1, 64)] {
        for divisions in [1, 3, 5, 6, 64] {
            let meter = Meter::new(n, d).unwrap();
            let mut map = GridMap::new(meter, Tick(meter.bar_ticks() * 3 + 1), Grid::new(divisions).unwrap()).unwrap();
            map.set_bar_grid(Tick(meter.bar_ticks()), Grid::new(64).unwrap())
                .unwrap();
            let mut tick = Tick(0);
            for row in 0..map.row_count() {
                assert_eq!(map.row_tick(row), Some(tick));
                assert_eq!(map.row_at(tick), Some(row));
                tick = map.neighbors(Tick(tick.0 + 1)).unwrap().1;
            }
            assert_eq!(tick, map.end());
            assert_eq!(map.row_tick(map.row_count()), Some(map.end()));
            assert!(map.row_tick(map.row_count() + 1).is_none());
        }
    }
    let map = GridMap::new(Meter::new(4, 4).unwrap(), Tick(983_040), Grid::new(64).unwrap()).unwrap();
    assert_eq!(map.row_count(), 65_536);
    assert_eq!(map.row_tick(65_535), Some(Tick(983_025)));
}

#[test]
fn real_editor_writes_mixed_triplet_and_256th_bars_without_retiming() {
    let mut app = App::default();
    app.execute_prompt("resize 24");
    app.execute_prompt("meter 3/4");
    app.execute_prompt("grid 3");
    app.cursor_row = 1;
    enter(&mut app, NotePitch::C);
    assert_eq!(app.cursor_tick(), Tick(320));
    app.cursor_row = 9;
    app.execute_prompt("grid 64 bar");
    app.cursor_row = 10;
    enter(&mut app, NotePitch::D);
    let pattern = &app.project.patterns[0];
    assert_eq!(pattern.length_ticks(), Tick(5760));
    assert_eq!(pattern.view_rows(), 9 + 192);
    assert_eq!(
        pattern.tracks[0]
            .events()
            .iter()
            .map(|event| event.start_tick.0)
            .collect::<Vec<_>>(),
        [320, 2895]
    );
    let values = pattern.tracks[0].events().to_vec();
    let ids = values
        .iter()
        .map(|event| pattern.tracks[0].event_id(event.start_tick).unwrap())
        .collect::<Vec<_>>();
    for _ in 0..4 {
        app.execute_prompt("grid 4");
        app.execute_prompt("grid 3");
    }
    assert_eq!(app.project.patterns[0].tracks[0].events(), values);
    for (event, id) in values.iter().zip(ids) {
        assert_eq!(app.project.patterns[0].tracks[0].event_id(event.start_tick), Some(id));
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mixed.json");
    crate::project_file::save(&app.project, &path).unwrap();
    assert_eq!(crate::project_file::load(&path).unwrap(), app.project);
    let mut clock = Transport::default();
    app.restart_playback();
    let now = Instant::now();
    let mut at = now;
    let mut played = Vec::new();
    while at < now + Duration::from_secs(3) {
        played.extend(clock.update_app(at, &mut app, || {}));
        at = clock.deadline().unwrap();
    }
    assert_eq!(
        played.iter().map(|event| event.note.pitch).collect::<Vec<_>>(),
        [NotePitch::C, NotePitch::D]
    );
}

#[test]
fn hidden_events_require_exact_navigation_or_explicit_range_edits() {
    let mut app = App::default();
    app.execute_prompt("grid 64");
    app.cursor_row = 1;
    enter(&mut app, NotePitch::C);
    app.cursor_row = 2;
    enter(&mut app, NotePitch::D);
    app.cursor_row = 0;
    app.execute_prompt("grid 4");
    assert_eq!(app.project.patterns[0].hidden_in_row(0, 0), 2);
    let original = app.project.clone();
    app.handle_command(Command::DeleteNote);
    assert_eq!(app.project, original);
    enter(&mut app, NotePitch::E);
    assert_eq!(app.project.patterns[0].tracks[0].events().len(), 3);
    app.execute_prompt("next-event");
    assert_eq!(app.cursor_tick(), Tick(15));
    app.handle_command(Command::DeleteNote);
    assert_eq!(app.project.patterns[0].tracks[0].events().len(), 2);
    app.handle_command(Command::Undo);
    assert_eq!(app.project.patterns[0].tracks[0].events().len(), 3);
    app.handle_command(Command::MoveCursorUp);
    assert_eq!(app.cursor_tick(), Tick(0));
    app.handle_command(Command::Copy);
    app.handle_command(Command::Paste);
    assert!(app.status.contains("2 hidden events"));
    app.execute_prompt("paste!");
    assert_eq!(app.project.patterns[0].tracks[0].events().len(), 1);
    app.handle_command(Command::Undo);
    app.handle_command(Command::ToggleVisual);
    app.handle_command(Command::DeleteNote);
    assert_eq!(app.project.patterns[0].tracks[0].events(), []);
    assert!(app.status.contains("3 events"));
    app.handle_command(Command::Undo);
    assert_eq!(app.project.patterns[0].tracks[0].events().len(), 3);
}

#[test]
fn old_v4_defaults_and_meter_grouping_round_trip_without_touching_music() {
    let mut app = App::default();
    enter(&mut app, NotePitch::C);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("old-v4.json");
    crate::project_file::save(&app.project, &path).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["patterns"][0].as_object_mut().unwrap().remove("grid");
    value["patterns"][0].as_object_mut().unwrap().remove("bar_grids");
    std::fs::write(&path, value.to_string()).unwrap();
    assert_eq!(crate::project_file::load(&path).unwrap(), app.project);
    let event = app.project.patterns[0].tracks[0].events()[0];
    let id = app.project.patterns[0].tracks[0].event_id(event.start_tick);
    for meter in ["3/4", "6/8", "7/8"] {
        app.execute_prompt(&format!("meter {meter}"));
        crate::project_file::save(&app.project, &path).unwrap();
        assert_eq!(crate::project_file::load(&path).unwrap(), app.project);
        assert_eq!(app.project.patterns[0].tracks[0].events(), [event]);
        assert_eq!(app.project.patterns[0].tracks[0].event_id(event.start_tick), id);
    }
}

#[test]
fn grid_changes_keep_playback_and_meter_conflicts_are_atomic() {
    let mut app = App::default();
    app.restart_playback();
    let mut transport = Transport::default();
    let now = Instant::now();
    transport.update_app(now, &mut app, || {});
    let deadline = transport.deadline();
    app.execute_prompt("grid 3");
    assert!(matches!(app.playback_state, PlaybackState::Playing));
    transport.update_app(now, &mut app, || panic!("grid cannot cancel sound"));
    assert_eq!(transport.deadline(), deadline);
    app.cursor_row = 12;
    app.execute_prompt("grid 64 bar");
    let before = app.project.clone();
    let revision = app.revision();
    app.execute_prompt("meter 3/4");
    assert!(app.status.contains("meter!"));
    assert_eq!(app.project, before);
    assert_eq!(app.revision(), revision);
    app.execute_prompt("meter! 3/4");
    assert!(matches!(app.playback_state, PlaybackState::Stopped));
    assert!(app.project.patterns[0].grid().overrides().next().is_none());
    app.handle_command(Command::Undo);
    assert_eq!(app.project, before);
    app.mode = Mode::Insert;
    app.execute_prompt("grid 6");
    assert!(app.status.contains("Normal mode"));
}
