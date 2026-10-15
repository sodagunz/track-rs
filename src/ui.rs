use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::Rect,
    prelude::CrosstermBackend,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::{bindings::DEFAULT_BINDINGS, command::Command, state::App, tracker::PatternStep};

pub struct TUI<W: std::io::Write> {
    pub terminal: Terminal<CrosstermBackend<W>>,
}

impl<W: std::io::Write> TUI<W> {
    /// Enter raw mode and initialize the alternate-screen terminal.
    ///
    /// # Errors
    /// Returns an error if enabling raw mode, entering the alternate screen,
    /// or initializing the terminal fails.
    pub fn new(writer: W) -> anyhow::Result<Self> {
        enable_raw_mode()?;
        let mut w = writer;
        execute!(w, EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(w);
        let terminal = Terminal::new(backend)?;

        Ok(Self { terminal })
    }

    /// Restore normal terminal mode and show the cursor.
    ///
    /// # Errors
    /// Returns an error if disabling raw mode, leaving the alternate screen,
    /// or showing the cursor fails.
    pub fn destroy(&mut self) -> anyhow::Result<()> {
        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;

        Ok(())
    }

    /// Render the current application state.
    ///
    /// # Panics
    /// Panics if drawing to the terminal fails or the pattern's track lengths
    /// are inconsistent with its row count.
    pub fn draw(&mut self, state: &App) {
        self.terminal.draw(|f| render(f, state)).unwrap();
    }
}

fn render(f: &mut Frame, state: &App) {
    let area = f.area();
    let title_rows = u16::from(area.height >= 6);
    let status_rows = u16::from(area.height >= 3);
    let hint_rows = u16::from(area.height > 0);
    if title_rows > 0 {
        draw_title_bar(f, Rect::new(area.x, area.y, area.width, title_rows), state);
    }
    draw_pattern_editor(
        f,
        Rect::new(
            area.x,
            area.y + title_rows,
            area.width,
            area.height.saturating_sub(title_rows + status_rows + hint_rows),
        ),
        state,
    );
    if status_rows > 0 {
        draw_status_bar(
            f,
            Rect::new(area.x, area.bottom() - hint_rows - status_rows, area.width, status_rows),
            state,
        );
    }
    if hint_rows > 0 {
        f.render_widget(
            Paragraph::new(format!(
                "{} {} Help",
                state.mode.label(),
                command_keys(Command::ToggleHelp)
            )),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }
    if state.help_visible {
        draw_help(f, state);
    }
}

fn key_label(key: crossterm::event::KeyCode) -> String {
    use crossterm::event::KeyCode;
    match key {
        KeyCode::Char(' ') => "Space".into(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Delete => "Delete".into(),
        other => format!("{other:?}"),
    }
}

fn binding_label(binding: &crate::bindings::Binding) -> String {
    let prefix = if binding.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
        "Ctrl-"
    } else {
        ""
    };
    format!("{prefix}{}", key_label(binding.key))
}

fn command_keys(command: Command) -> String {
    DEFAULT_BINDINGS
        .iter()
        .filter(|binding| binding.command == command)
        .map(binding_label)
        .collect::<Vec<_>>()
        .join("/")
}

fn help_bindings() -> Vec<&'static crate::bindings::Binding> {
    let mut bindings: Vec<_> = DEFAULT_BINDINGS.iter().collect();
    // Stable sorting keeps aliases and controls in registry order within each category.
    bindings.sort_by_key(|binding| binding.category);
    bindings
}

fn draw_help(f: &mut Frame, state: &App) {
    let screen = f.area();
    let width = screen.width.min(64);
    let height = screen.height.min(24);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, area);
    // On small screens omit borders rather than spending scarce content rows.
    let bordered = width >= 40 && height >= 8;
    let block = Block::default()
        .title("Keybindings")
        .borders(if bordered { Borders::ALL } else { Borders::NONE });
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 {
        return;
    }
    let close = format!(
        "{}/{} close",
        command_keys(Command::ToggleHelp),
        command_keys(Command::DismissHelp)
    );
    f.render_widget(Paragraph::new(close), Rect::new(inner.x, inner.y, inner.width, 1));
    if inner.height < 2 {
        return;
    }
    let bindings = help_bindings();
    let start = state.help_scroll.min(bindings.len().saturating_sub(1));
    let mut lines = Vec::new();
    for binding in &bindings[start..] {
        lines.push(Line::from(format!(
            "[{}] {}: {} ({})",
            binding.category,
            binding_label(binding),
            binding.description,
            binding
                .modes
                .iter()
                .map(|mode| mode.label())
                .collect::<Vec<_>>()
                .join("/")
        )));
    }
    let footer_rows = u16::from(inner.height >= 4);
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1 - footer_rows),
    );
    if footer_rows > 0 {
        let footer = format!(
            "{}/{} scroll  {}/{}",
            command_keys(Command::ScrollHelpUp),
            command_keys(Command::ScrollHelpDown),
            start + 1,
            DEFAULT_BINDINGS.len()
        );
        f.render_widget(
            Paragraph::new(footer),
            Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
        );
    }
}

fn draw_title_bar(f: &mut Frame, area: Rect, _: &App) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .title(Line::from("track-rs").centered());
    f.render_widget(block, area);
}

fn draw_pattern_editor(f: &mut Frame, area: Rect, state: &App) {
    const GUTTER: u16 = 5;
    const TRACK_WIDTH: u16 = 10;
    if area.is_empty() {
        return;
    }
    let pattern = &state.pattern;
    let visible_tracks = usize::from(area.width.saturating_sub(GUTTER) / TRACK_WIDTH)
        .max(usize::from(area.width > GUTTER))
        .min(pattern.tracks.len());
    let first_track = state
        .cursor_track
        .saturating_add(1)
        .saturating_sub(visible_tracks)
        .min(pattern.tracks.len().saturating_sub(visible_tracks));
    let header_rows = u16::from(area.height >= 2);
    let visible_rows = usize::from(area.height - header_rows);
    let first_row = state
        .cursor_row
        .saturating_add(1)
        .saturating_sub(visible_rows)
        .min(pattern.num_rows.saturating_sub(visible_rows));
    if header_rows > 0 {
        let mut header = vec![Span::raw("Row  ")];
        for track in first_track..first_track + visible_tracks {
            header.push(Span::styled(
                format!("{:<10}", format!("{} {}", track + 1, pattern.instrument(track).label())),
                Style::default().fg(Color::Green),
            ));
        }
        f.render_widget(
            Paragraph::new(Line::from(header)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }
    for (offset, row) in (first_row..pattern.num_rows).take(visible_rows).enumerate() {
        let playing = matches!(state.playback_state, crate::state::PlaybackState::Playing) && row == state.playback_row;
        let background = if playing {
            Color::DarkGray
        } else if row % 2 == 0 {
            Color::Reset
        } else {
            Color::Black
        };
        let foreground = if background == Color::Reset {
            Color::Reset
        } else {
            Color::Gray
        };
        // Boundaries use absolute rows and consume no extra viewport height.
        let (boundary, color) = if row % 16 == 0 {
            ("━", Color::Gray)
        } else if row % 4 == 0 {
            ("─", Color::DarkGray)
        } else {
            (" ", Color::DarkGray)
        };
        let mut spans = vec![Span::styled(
            format!("{}{:02}{} ", if playing { "▶" } else { " " }, row, boundary),
            Style::default().fg(if playing { Color::Cyan } else { color }),
        )];
        for track in first_track..first_track + visible_tracks {
            let step = pattern.get_step(track, row).copied().expect("Pattern row must exist");
            let style = if row == state.cursor_row && track == state.cursor_track {
                Style::default().bg(Color::Green).fg(Color::Black)
            } else if state.is_selected(row, track) {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else if playing {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            spans.push(Span::styled(format_step(step), style));
            spans.push(Span::raw("       "));
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().fg(foreground).bg(background)),
            Rect::new(
                area.x,
                area.y + header_rows + u16::try_from(offset).unwrap_or(0),
                area.width,
                1,
            ),
        );
    }
}

fn draw_status_bar(f: &mut Frame, area: Rect, state: &App) {
    let status = format!(
        "{}{}{} BPM: {:.1} Oct:{} Step:{} {:?} P:{:02} Pos:{:02},{:02}",
        state.mode.label(),
        state
            .pending_count
            .map_or_else(String::new, |count| format!(" Count:{count}")),
        state.selection_anchor.map_or_else(String::new, |(row, track)| format!(
            " Sel:{},{}–{},{}",
            row.min(state.cursor_row),
            track.min(state.cursor_track),
            row.max(state.cursor_row),
            track.max(state.cursor_track)
        )),
        state.bpm,
        state.octave,
        state.edit_step,
        state.playback_state,
        state.playback_row,
        state.cursor_row,
        state.cursor_track,
    );
    f.render_widget(Paragraph::new(status), area);
}

fn format_step(step: PatternStep) -> String {
    step.note.map_or_else(
        || " . ".to_string(),
        |note| format!("{:<3}", format!("{}{}", note.pitch, note.octave)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::{Note, NotePitch};
    use ratatui::backend::TestBackend;

    fn screen(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    #[test]
    fn instrument_headers_fit_existing_columns_without_moving_highlights() {
        use crate::tracker::Instrument;
        for instrument in Instrument::ALL {
            let mut app = App::default();
            app.pattern.set_instrument(1, instrument);
            app.cursor_track = 1;
            app.playback_row = 1;
            app.handle_command(Command::TogglePlayback);
            app.playback_row = 1;
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal.draw(|frame| render(frame, &app)).unwrap();
            let text = screen(&terminal);
            assert!(text.contains(&format!("2 {}", instrument.label())));
            assert!(text.contains("1 Pluck"));
            assert!(format!("2 {}", instrument.label()).len() <= 10);
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(15, 2)].bg, Color::Green);
            assert_eq!(buffer[(15, 3)].bg, Color::DarkGray);
        }
    }

    #[test]
    fn absolute_gutters_and_boundaries_survive_scrolling() {
        for (cursor, first) in [(0, 0), (20, 13), (63, 56)] {
            let app = App {
                cursor_row: cursor,
                ..App::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(45, 9)).unwrap();
            terminal.draw(|f| draw_pattern_editor(f, f.area(), &app)).unwrap();
            let buffer = terminal.backend().buffer();
            for offset in 0..8_u16 {
                let row = first + usize::from(offset);
                let y = offset + 1;
                let gutter: String = (0..5).map(|x| buffer[(x, y)].symbol()).collect();
                let (mark, color) = if row % 16 == 0 {
                    ("━", Color::Gray)
                } else if row % 4 == 0 {
                    ("─", Color::DarkGray)
                } else {
                    (" ", Color::DarkGray)
                };
                assert_eq!(gutter, format!(" {row:02}{mark} "));
                assert_eq!(buffer[(3, y)].fg, color);
            }
            let y = u16::try_from(cursor - first + 1).unwrap();
            assert_eq!(buffer[(5, y)].bg, Color::Green);
        }
    }

    #[test]
    fn cursor_wins_playback_and_unselected_notes_remain_visible() {
        let mut app = App {
            playback_state: crate::state::PlaybackState::Playing,
            ..App::default()
        };
        for (track, pitch) in [(0, NotePitch::C), (1, NotePitch::Cs), (2, NotePitch::D)] {
            app.pattern.set_step(track, 0, Some(Note { pitch, octave: 4 })).unwrap();
        }
        let mut terminal = Terminal::new(TestBackend::new(45, 4)).unwrap();
        terminal.draw(|f| draw_pattern_editor(f, f.area(), &app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 1)].symbol(), "▶");
        assert_eq!(buffer[(0, 1)].fg, Color::Cyan);
        assert_eq!(buffer[(5, 1)].bg, Color::Green);
        assert_eq!(buffer[(5, 1)].fg, Color::Black);
        for (x, expected) in [(5, "C4 "), (15, "C#4"), (25, "D4 ")] {
            let text: String = (x..x + 3).map(|x| buffer[(x, 1)].symbol()).collect();
            assert_eq!(text, expected);
        }
        assert_eq!(buffer[(15, 1)].bg, Color::DarkGray);
        assert_ne!(buffer[(15, 1)].fg, Color::Black);
        assert_eq!(buffer[(15, 2)].bg, Color::Black);
        assert_eq!(buffer[(44, 1)].bg, Color::DarkGray);
    }

    #[test]
    fn alternating_backgrounds_cover_the_row_and_stay_anchored_when_scrolling() {
        for (cursor, first) in [(0, 0), (20, 13), (63, 56)] {
            let app = App {
                cursor_row: cursor,
                ..App::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(48, 9)).unwrap();
            terminal.draw(|f| draw_pattern_editor(f, f.area(), &app)).unwrap();
            let buffer = terminal.backend().buffer();
            for offset in 0..8_u16 {
                let row = first + usize::from(offset);
                let background = if row % 2 == 0 { Color::Reset } else { Color::Black };
                for x in [0, 4, 8, 15, 44, 47] {
                    assert_eq!(buffer[(x, offset + 1)].bg, background, "row {row}, x {x}");
                }
            }
        }
    }

    #[test]
    fn narrow_viewport_follows_selected_track_and_last_row_not_playback() {
        let mut app = App {
            cursor_row: 63,
            cursor_track: 3,
            playback_state: crate::state::PlaybackState::Playing,
            ..App::default()
        };
        app.pattern.set_instrument(3, crate::tracker::Instrument::HiHat);
        let mut terminal = Terminal::new(TestBackend::new(15, 6)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        assert!(screen(&terminal).contains("4 HiHat"));
        assert!(screen(&terminal).contains("63"));
        assert!(screen(&terminal).contains("? Help"));
        assert_eq!(terminal.backend().buffer()[(5, 3)].bg, Color::Green);
        for width in 0..16 {
            for height in 0..4 {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|f| render(f, &app)).unwrap();
                app.help_visible = true;
                terminal.draw(|f| render(f, &app)).unwrap();
                app.help_visible = false;
            }
        }
    }

    #[test]
    fn help_groups_categories_stably_without_losing_aliases() {
        let bindings = help_bindings();
        assert_eq!(bindings.len(), DEFAULT_BINDINGS.len());
        assert!(bindings.windows(2).all(|pair| pair[0].category <= pair[1].category));
        for binding in DEFAULT_BINDINGS {
            assert_eq!(
                bindings
                    .iter()
                    .filter(|entry| entry.key == binding.key
                        && entry.command == binding.command
                        && entry.modifiers == binding.modifiers
                        && entry.modes == binding.modes)
                    .count(),
                1
            );
        }
        for category in bindings.iter().map(|binding| binding.category) {
            let displayed: Vec<_> = bindings
                .iter()
                .filter(|binding| binding.category == category)
                .map(|binding| binding.key)
                .collect();
            let registered: Vec<_> = DEFAULT_BINDINGS
                .iter()
                .filter(|binding| binding.category == category)
                .map(|binding| binding.key)
                .collect();
            assert_eq!(displayed, registered);
        }
    }

    #[test]
    fn help_scrolls_to_every_binding_and_displays_close_controls() {
        for (width, height) in [(80, 30), (32, 8), (24, 6)] {
            let mut app = App::default();
            app.handle_command(Command::ToggleHelp);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for binding in help_bindings() {
                terminal.draw(|frame| render(frame, &app)).unwrap();
                let text = screen(&terminal);
                assert!(text.contains("?/Esc close"));
                // Wrapping may insert padding between words.
                let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                let expected: String = format!(
                    "[{}] {}: {}",
                    binding.category,
                    binding_label(binding),
                    binding.description
                )
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
                assert!(
                    compact.contains(&expected),
                    "missing {expected} at {width}x{height}: {text}"
                );
                app.handle_command(Command::ScrollHelpDown);
            }
        }
    }

    #[test]
    fn real_keys_render_modes_counts_and_scrolled_selection_with_cursor_priority() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::default();
        for ch in "16j2lv3k1h12".chars() {
            crate::bindings::handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut app);
        }
        app.handle_command(Command::TogglePlayback);
        app.playback_row = 15;
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text = screen(&terminal);
        assert!(text.contains("VISUAL Count:12 Sel:13,1–16,2"));
        let buffer = terminal.backend().buffer();
        // Cursor follows scrolling; selected cells take precedence over playback.
        assert_eq!(buffer[(15, 5)].bg, Color::Green);
        assert_eq!(buffer[(25, 5)].bg, Color::Blue);
        app.cursor_row = 15;
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(15, 5)].bg, Color::Green);
        assert_eq!(buffer[(25, 5)].bg, Color::Blue);
        assert_eq!(buffer[(0, 5)].symbol(), "▶");
        for (keys, mode) in [("", "NORMAL"), ("i", "INSERT"), ("v", "VISUAL")] {
            let mut app = App::default();
            for ch in keys.chars() {
                crate::bindings::handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut app);
            }
            for (width, height) in [(20, 3), (12, 2), (6, 1)] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|frame| render(frame, &app)).unwrap();
                assert!(screen(&terminal).contains(mode));
            }
        }
    }

    #[test]
    fn insert_step_is_visible_in_status_and_registry_help() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::default();
        app.handle_command(Command::EnterInsert);
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        for step in [0, 1, 2, 4] {
            terminal.draw(|frame| render(frame, &app)).unwrap();
            assert!(screen(&terminal).contains(&format!("Step:{step}")));
            crate::bindings::handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), &mut app);
        }
        app.handle_command(Command::ToggleHelp);
        app.help_scroll = help_bindings()
            .iter()
            .position(|binding| binding.command == Command::CycleEditStep)
            .unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text = screen(&terminal);
        assert!(text.contains("Tab") && text.contains("0/1/2/4"));
    }

    #[test]
    fn tiny_viewports_render_without_panicking() {
        for (width, height) in [(0, 0), (1, 1), (2, 2), (10, 3), (20, 5)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut app = App::default();
            terminal.draw(|frame| render(frame, &app)).unwrap();
            app.handle_command(Command::ToggleHelp);
            terminal.draw(|frame| render(frame, &app)).unwrap();
        }
    }

    #[test]
    fn renders_pattern_and_status_without_a_terminal() {
        let mut app = App::default();
        app.insert_note_under_cursor(Some(Note {
            pitch: NotePitch::C,
            octave: 4,
        }));

        app.handle_command(Command::RaiseNote);
        app.handle_command(Command::IncreaseTempo);
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();

        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(screen.contains("? Help"));
        assert!(screen.contains("track-rs"));
        assert!(screen.contains("1 Pluck"));
        assert!(screen.contains("C#4"));
        assert!(screen.contains("BPM: 121.0"));
    }
}
