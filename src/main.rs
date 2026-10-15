use track_rs::{
    audio::AudioEngine,
    bindings,
    state::{App, PlaybackState},
    ui::TUI,
};

use std::{
    io::stdout,
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Startup:
    let mut audio_engine = AudioEngine::new()?;
    let mut tui = TUI::new(stdout())?;
    let mut app_state = App::default();

    let listener = bindings::listen();

    // Main loop
    main_loop(&mut app_state, &mut tui, &mut audio_engine, &listener);

    tui.destroy()?;

    Ok(())
}

fn main_loop<W: std::io::Write>(
    app_state: &mut App,
    tui: &mut TUI<W>,
    audio_engine: &mut AudioEngine,
    listener: &Receiver<crossterm::event::KeyEvent>,
) {
    let frame_period = Duration::from_millis(50);
    let mut next_frame = Instant::now();
    let mut transport = track_rs::transport::Transport::default();
    let mut was_playing = false;
    let mut pending = None;
    loop {
        if let Some(key) = pending.take() {
            bindings::handle_key(key, app_state);
        }
        let playing = !app_state.shutdown && matches!(app_state.playback_state, PlaybackState::Playing);
        let stop_requested = app_state.take_stop_request();
        if stop_requested {
            transport.stop();
        }
        if stop_requested || (was_playing && !playing) {
            audio_engine.stop();
        }
        was_playing = playing;
        let now = Instant::now();
        if let Some(row) = transport.update(
            now,
            playing,
            app_state.project.patterns[app_state.selected_pattern].num_rows,
            app_state.project.bpm,
            track_rs::transport::DEFAULT_ROWS_PER_BEAT,
        ) {
            app_state.playback_row = row.row;
            for event in track_rs::transport::collect_row(
                &app_state.project.patterns[app_state.selected_pattern],
                app_state.project.instruments(),
                row,
            ) {
                audio_engine.play_event(event);
            }
        }
        if app_state.shutdown {
            break;
        }
        if now >= next_frame {
            tui.draw(app_state);
            next_frame = Instant::now() + frame_period;
        }
        let wake = transport
            .deadline()
            .map_or(next_frame, |deadline| deadline.min(next_frame));
        match listener.recv_timeout(wake.saturating_duration_since(Instant::now())) {
            Ok(key) => pending = Some(key),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                audio_engine.stop();
                break;
            }
        }
    }
}
