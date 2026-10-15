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
    let mut last_frame = Instant::now();
    let frame_period = Duration::from_millis(50); // UI refresh rate

    let mut last_playback_time: Option<Instant> = None;

    loop {
        if app_state.shutdown {
            break;
        }

        // Play audio
        let seconds_per_beat = 60.0 / app_state.bpm;
        let rows_per_beat = 4.0;
        let seconds_per_row = seconds_per_beat / rows_per_beat;

        assert!((seconds_per_row > 0.0), "Too fast a BPM champ!"); // FIXME: Tempo should be
                                                                   // ridiculously high for this
                                                                   // assert to even trigger

        let playback_interval = Duration::from_secs_f64(seconds_per_row);

        match app_state.playback_state {
            PlaybackState::Playing => {
                let now = Instant::now();
                let mut trigger_playback_audio = false;

                if let Some(last_time) = last_playback_time {
                    if now.duration_since(last_time) >= playback_interval {
                        last_playback_time = Some(now);
                        trigger_playback_audio = true;
                        app_state.advance_playback_row();
                    }
                } else {
                    last_playback_time = Some(now);
                    trigger_playback_audio = true;
                }

                if trigger_playback_audio {
                    let current_row = app_state.playback_row;
                    for track_idx in 0..app_state.pattern.tracks.len() {
                        if let Some(step) = app_state.pattern.get_step(track_idx, current_row).copied() {
                            if let Some(note) = step.note {
                                audio_engine.play(note); // FIXME: send async message to audio
                                                         // thread?
                            }
                        }
                    }
                }
            }
            PlaybackState::Stopped => {
                last_playback_time = None;
            }
        }

        // Handle input
        // TODO: move to its own thingy
        bindings::handle(listener, app_state);

        // Draw UI
        // TODO: move to its own thingy, renderer maybe?
        if last_frame.elapsed() >= frame_period {
            tui.draw(app_state);
            last_frame = Instant::now();
        }

        // Prevent Busy-Waiting
        // Calculates smallest potential wait time until next frame or next playback tick
        let time_until_next_frame = frame_period.saturating_sub(last_frame.elapsed());
        let time_until_next_tick = match (app_state.playback_state, last_playback_time) {
            (PlaybackState::Playing, Some(last_time)) => playback_interval.saturating_sub(last_time.elapsed()),
            (PlaybackState::Playing, None) => Duration::from_secs(0),
            (_, _) => time_until_next_frame,
        };

        let suggested_sleep = std::cmp::min(
            std::cmp::min(time_until_next_tick, time_until_next_frame),
            Duration::from_millis(10),
        );

        let _sleep_time = if suggested_sleep >= Duration::from_millis(5) {
            suggested_sleep
        } else {
            Duration::from_millis(1)
        };

        // thread::sleep(sleep_time);
    }
}
