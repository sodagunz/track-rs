use crate::{
    bindings::handle_key,
    state::{App, Mode},
    synth_controls::CONTROLS,
    tracker::Instrument,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};

fn screen(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn synth_rows_scroll_with_groups_values_policies_and_text_selection() {
    let mut app = App::default();
    app.handle_command(crate::command::Command::ShowInstrument);
    app.handle_command(crate::command::Command::SetInstrument(Instrument::Synth));
    for (row, control) in CONTROLS.iter().enumerate() {
        app.cursor_row = row;
        for (width, height) in [(100, 26), (48, 8), (32, 7), (12, 5), (1, 1), (0, 0)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| super::render(frame, &app)).unwrap();
            let text = screen(&terminal);
            if width >= 32 {
                assert!(
                    text.contains(&format!("> {} {}", control.group(), control.label())),
                    "{row}: {text}"
                );
                assert!(
                    text.contains(&control.value(app.project.synth_patch(0))),
                    "{row}: {text}"
                );
            }
            if width >= 48 {
                assert!(text.contains(control.policy()), "{row}: {text}");
            }
        }
    }
    assert_eq!(app.project.patterns[0].tracks[0].events(), []);
}

#[test]
fn synth_prompt_and_help_show_actual_units_modes_and_next_note_policy() {
    let mut app = App::default();
    app.handle_command(crate::command::Command::ShowInstrument);
    app.handle_command(crate::command::Command::SetInstrument(Instrument::Synth));
    app.cursor_row = 14;
    handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE), &mut app);
    handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
    let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
    terminal.draw(|frame| super::render(frame, &app)).unwrap();
    let text = screen(&terminal);
    assert!(text.contains("AMP Attack [next note; 0..10 s (ms allowed)]"), "{text}");
    assert!(text.contains("INSTRUMENT INSERT"));
    assert!(!text.contains("New:"));
    for width in [0, 1, 12, 40] {
        let mut terminal = Terminal::new(TestBackend::new(width, 6)).unwrap();
        app.prompt = Some("界e\u{301}👩‍💻".into());
        terminal.draw(|frame| super::render(frame, &app)).unwrap();
    }
    app.prompt = None;
    app.mode = Mode::Normal;
    app.help_visible = true;
    app.help_scroll.set(0);
    terminal.draw(|frame| super::render(frame, &app)).unwrap();
    // The registry is the source of truth; editor help must include Synth controls.
    let groups = super::help_groups(app.view, Some(Mode::Insert));
    let descriptions = groups
        .iter()
        .flatten()
        .map(|binding| binding.description)
        .collect::<Vec<_>>();
    assert!(descriptions.contains(&"Enter Synth value / waveform name"));
    assert!(descriptions.contains(&"Reset selected Synth control to default"));
}
