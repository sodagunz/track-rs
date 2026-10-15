use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::CrosstermBackend,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(2), Constraint::Min(0), Constraint::Length(3)].as_ref())
        .split(f.area());

    draw_title_bar(f, chunks[0], state);
    draw_pattern_editor(f, chunks[1], state);
    draw_status_bar(f, chunks[2], state);
    // Reserve a screen-edge hint even when the normal status layout cannot fit.
    let area = f.area();
    if area.height > 0 {
        f.render_widget(
            Paragraph::new(format!("{} Help", command_keys(Command::ToggleHelp))),
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

fn command_keys(command: Command) -> String {
    DEFAULT_BINDINGS
        .iter()
        .filter(|binding| binding.command == command)
        .map(|binding| key_label(binding.key))
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
            "[{}] {}: {}",
            binding.category,
            key_label(binding.key),
            binding.description
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

fn draw_pattern_editor(f: &mut Frame, area: Rect, app_state: &App) {
    let pattern = &app_state.pattern;
    let num_tracks = pattern.tracks.len();
    let header_cells =
        (0..num_tracks).map(|i| Cell::from(format!("Track {}", i + 1)).style(Style::default().fg(Color::Green)));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let mut rows = vec![];
    for row_idx in 0..pattern.num_rows {
        let mut row_cells = vec![];
        for track_idx in 0..num_tracks {
            let step = pattern
                .get_step(track_idx, row_idx)
                .copied()
                .expect("Should be able to clone a step");
            let cell_str = format_step(step); // TODO: Trait "drawable" or whatever, impl on
                                              // patternstep

            // Highlighting
            let mut style = Style::default();
            if row_idx == app_state.cursor_row && track_idx == app_state.cursor_track {
                style = style.bg(Color::Green).fg(Color::Black); // Highlight cursor
            }

            if matches!(app_state.playback_state, crate::state::PlaybackState::Playing)
                && row_idx == app_state.playback_row
            {
                style = style.bg(Color::DarkGray); // Highlight playback caret
            }

            row_cells.push(Cell::from(cell_str).style(style));
        }
        rows.push(Row::new(row_cells));
    }

    let track_width = 10; // FIXME: fixed width is not guaranteed
    let constraints = std::iter::repeat_n(Constraint::Length(track_width), num_tracks).collect::<Vec<_>>();

    let table = Table::new(rows, &constraints)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Pattern Editor"));

    f.render_widget(table, area);
}

fn draw_status_bar(f: &mut Frame, area: Rect, app_state: &App) {
    let status_text = format!(
        "State: {:?} | Pos: {:02},{:02} | Playback Row: {:02} | BPM: {:.1} | Octave {:02}",
        app_state.playback_state,
        app_state.cursor_row,
        app_state.cursor_track,
        app_state.playback_row,
        app_state.bpm,
        app_state.octave
    );
    let paragraph = Paragraph::new(status_text).block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(paragraph, area);
}

fn format_step(step: PatternStep) -> String {
    step.note
        .map_or_else(|| " . ".to_string(), |note| format!("{}{}", note.pitch, note.octave))
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
    fn help_groups_categories_stably_without_losing_aliases() {
        let bindings = help_bindings();
        assert_eq!(bindings.len(), DEFAULT_BINDINGS.len());
        assert!(bindings.windows(2).all(|pair| pair[0].category <= pair[1].category));
        for binding in DEFAULT_BINDINGS {
            assert_eq!(bindings.iter().filter(|entry| entry.key == binding.key).count(), 1);
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
                    key_label(binding.key),
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
        assert!(screen.contains("Track 1"));
        assert!(screen.contains("C#4"));
        assert!(screen.contains("BPM: 121.0"));
    }
}
