use track_rs::{audio::AudioEngine, bindings, state::App, ui::TUI};

use std::{
    io::stdout,
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let explicit = track_rs::theme::theme_argument(std::env::args_os().skip(1))?;
    let discovered = track_rs::theme::discover(std::env::var_os("XDG_CONFIG_HOME"), std::env::var_os("HOME"));
    let theme = track_rs::theme::load(explicit.as_deref(), discovered.as_deref())?;

    // Validate configuration before opening audio or changing terminal mode.
    let mut audio_engine = AudioEngine::new()?;
    let mut tui = TUI::new(stdout())?;
    let mut app_state = App::default();
    app_state.theme = theme;

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
    let mut last_meter_sample = next_frame;
    let mut transport = track_rs::transport::Transport::default();

    let mut pending = None;
    loop {
        if let Some(key) = pending.take() {
            bindings::handle_key(key, app_state);
        }
        audio_engine.sync_channels(app_state.project.channels());
        let now = Instant::now();
        for event in transport.update_app(now, app_state, || audio_engine.stop()) {
            audio_engine.play_event(event);
        }
        if let Some(event) = transport.take_metronome_event() {
            audio_engine.metronome_event(event);
        }
        if let Some(event) = app_state.take_audition() {
            audio_engine.preview_event(event);
        }
        if app_state.shutdown {
            break;
        }
        if now >= next_frame {
            audio_engine.sample_meters(
                &mut app_state.meters,
                app_state.project.channels().len(),
                now.saturating_duration_since(last_meter_sample),
            );
            last_meter_sample = now;
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
