use track_rs::{audio::AudioEngine, bindings, state::App, ui::TUI};

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

    let mut pending = None;
    loop {
        if let Some(key) = pending.take() {
            bindings::handle_key(key, app_state);
        }
        let now = Instant::now();
        for event in transport.update_app(now, app_state, || audio_engine.stop()) {
            audio_engine.play_event(event);
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
