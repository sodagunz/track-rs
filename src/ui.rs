use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::Rect,
    prelude::CrosstermBackend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::{
    bindings::DEFAULT_BINDINGS,
    command::Command,
    state::{App, Mode, View},
    tracker::PatternStep,
};

mod presentation;

use presentation::{pattern_label, row_label, track_number, Presentation};

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
    let order_rows = u16::from(area.height >= 7 && state.view == View::Pattern);
    let hint_rows = u16::from(area.height > 0 && (state.prompt.is_some() || !state.status.is_empty()));
    let status_rows = u16::from(area.height.saturating_sub(hint_rows) >= 3);
    if title_rows > 0 {
        draw_buffer_line(f, Rect::new(area.x, area.y, area.width, title_rows), state);
    }
    if order_rows > 0 {
        draw_order(f, Rect::new(area.x, area.y + title_rows, area.width, 1), state);
    }
    let draw_panel = match state.view {
        View::Pattern => draw_pattern_editor,
        View::Arrangement => draw_arrangement,
        View::Mixer => draw_mixer,
        View::Instrument => draw_instrument,
    };
    draw_panel(
        f,
        Rect::new(
            area.x,
            area.y + title_rows + order_rows,
            area.width,
            area.height
                .saturating_sub(title_rows + order_rows + status_rows + hint_rows),
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
        let line = state.prompt.as_ref().map_or_else(
            || state.status.clone(),
            |text| {
                let labeled;
                let text = if state.view == View::Mixer && state.mode == Mode::Insert {
                    labeled = format!(
                        "{}: {text}",
                        match state.cursor_row {
                            0 => "Volume dB (-inf allowed)",
                            1 => "Pan % (-100 L, +100 R)",
                            2 => "Low shelf dB (200 Hz)",
                            3 => "High shelf dB (4 kHz)",
                            4 => "HPF Hz (off allowed)",
                            _ => "LPF Hz (off allowed)",
                        }
                    );
                    &labeled
                } else {
                    text
                };
                let (visible, cursor) = presentation::prompt_view(text, area.width);
                if area.width > 0 {
                    f.set_cursor_position((area.x + cursor, area.bottom() - 1));
                }
                visible
            },
        );
        f.render_widget(
            Paragraph::new(line),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }
    if state.help_visible {
        draw_help(f, state);
    }
}

fn draw_instrument(f: &mut Frame, area: Rect, state: &App) {
    use crate::tracker::Instrument;
    let current = state.project.instrument(state.cursor_track);
    let block = Block::default().borders(Borders::ALL).title(format!(
        "Instrument | Track {} | {} | [/] voice | h/l track | Enter audition",
        track_number(state.cursor_track),
        current.label()
    ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let height = usize::from(inner.height);
    let selected = Instrument::ALL.iter().position(|&voice| voice == current).unwrap_or(0);
    let first = selected.saturating_sub(height.saturating_sub(1));
    let lines: Vec<_> = Instrument::ALL
        .iter()
        .skip(first)
        .take(height)
        .map(|&voice| {
            let selected = voice == current;
            Line::styled(
                format!(
                    "{} {}{}",
                    if selected { ">" } else { " " },
                    voice.label(),
                    if selected { " (all patterns)" } else { "" }
                ),
                if selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default()
                },
            )
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_mixer(f: &mut Frame, area: Rect, state: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Mixer | i adjust | Enter value | Esc normal");
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let label_width = 8usize.min(usize::from(inner.width).saturating_sub(8));
    let cell_width = 12usize.min(usize::from(inner.width).saturating_sub(label_width)).max(1);
    let count = (usize::from(inner.width).saturating_sub(label_width) / cell_width).max(1);
    let first = state.cursor_track.saturating_sub(count - 1);
    // Keep the header, selected parameter and its label even at two rows tall.
    let visible = usize::from(inner.height.saturating_sub(1)).max(1);
    let first_row = state.cursor_row.saturating_sub(visible - 1);
    let rows = first_row..(first_row + visible).min(crate::project::CHANNEL_ROWS);
    let mut lines = Vec::new();
    if inner.height > 1 {
        let mut header = vec![Span::raw(format!("{:label_width$}", ""))];
        for track in first..(first + count).min(state.project.channels().len()) {
            header.push(Span::styled(
                format!("{:^cell_width$}", format!("T{}", track_number(track))),
                if track == state.cursor_track {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                },
            ));
        }
        lines.push(Line::from(header));
    }
    for row in rows {
        let mut spans = vec![Span::raw(format!(
            "{:<label_width$}",
            crate::project::CHANNEL_LABELS[row]
        ))];
        for track in first..(first + count).min(state.project.channels().len()) {
            let style = if track == state.cursor_track && row == state.cursor_row {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else if track == state.cursor_track {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            spans.push(Span::styled(
                format!("{:^cell_width$}", state.project.channels()[track].label(row)),
                style,
            ));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn key_label(key: crossterm::event::KeyCode) -> String {
    use crossterm::event::KeyCode;
    match key {
        KeyCode::Char(' ') => "Space".into(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::F(number) => format!("F{number}"),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Delete => "Delete".into(),
        other => format!("{other:?}"),
    }
}

fn binding_label(binding: &crate::bindings::Binding) -> String {
    let mut prefix = String::new();
    for (modifier, label) in [
        (crossterm::event::KeyModifiers::CONTROL, "Ctrl-"),
        (crossterm::event::KeyModifiers::ALT, "Alt-"),
        (crossterm::event::KeyModifiers::SHIFT, "Shift-"),
        (crossterm::event::KeyModifiers::SUPER, "Super-"),
    ] {
        if binding.modifiers.contains(modifier) {
            prefix.push_str(label);
        }
    }
    if let Some(key) = binding.prefix {
        prefix.push_str(&key_label(key));
    }
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

/// Group aliases only when their action and presentation metadata agree.
fn help_groups(view: View, mode: Option<Mode>) -> Vec<Vec<&'static crate::bindings::Binding>> {
    let mut bindings: Vec<_> = DEFAULT_BINDINGS
        .iter()
        .filter(|binding| {
            let shared = [Mode::Normal, Mode::Insert, Mode::Visual]
                .iter()
                .all(|mode| binding.modes.contains(mode));
            binding.available_in_view(view) && mode.map_or(shared, |mode| !shared && binding.modes.contains(&mode))
        })
        .collect();
    bindings.sort_by_key(|binding| binding.category);
    let mut groups: Vec<Vec<&crate::bindings::Binding>> = Vec::new();
    for binding in bindings {
        if let Some(group) = groups.iter_mut().find(|group| {
            let first = group[0];
            first.category == binding.category
                && first.description == binding.description
                && (first.command == binding.command
                    || matches!(
                        (first.command, binding.command),
                        (Command::CountDigit(_), Command::CountDigit(_))
                    ))
        }) {
            group.push(binding);
        } else {
            groups.push(vec![binding]);
        }
    }
    groups
}

fn group_keys(group: &[&crate::bindings::Binding]) -> String {
    if group.len() == 10
        && group
            .iter()
            .all(|binding| matches!(binding.command, Command::CountDigit(_)))
    {
        "1–9/0".into()
    } else {
        group
            .iter()
            .map(|binding| binding_label(binding))
            .collect::<Vec<_>>()
            .join("/")
    }
}

fn help_lines(view: View, mode: Mode, width: u16) -> Vec<Line<'static>> {
    let sections = [
        (None, "Shared"),
        (Some(Mode::Normal), "Normal"),
        (Some(Mode::Insert), "Insert"),
        (Some(Mode::Visual), "Visual"),
    ];
    let key_width = sections
        .iter()
        .flat_map(|(mode, _)| help_groups(view, *mode))
        .map(|group| Line::from(group_keys(&group)).width())
        .max()
        .unwrap_or(0);
    let mut lines = Vec::new();
    if view == View::Mixer {
        lines.push(Line::from(
            "Normal: h/l channel; j/k parameter. Insert: h/l adjust; j/k parameter.",
        ));
        lines.push(Line::from(
            "Counts multiply steps. Pan is stereo balance; centre preserves width, no boost.",
        ));
        lines.push(Line::from(
            "Shelves: 200 Hz/4 kHz, -12..+12 dB. HPF/LPF: 20..20000 Hz or off, semitone steps.",
        ));
        lines.push(Line::from(
            "HPF off: increase from 20 Hz. LPF off: decrease from 20000 Hz. Boosts can clip.",
        ));
    }
    for (section, label) in sections {
        if view == View::Mixer && section == Some(Mode::Visual) {
            continue;
        }
        let active = section == Some(mode);
        lines.push(Line::styled(
            if active {
                format!("{label} (active)")
            } else {
                label.into()
            },
            Style::default().add_modifier(if active { Modifier::BOLD } else { Modifier::empty() }),
        ));
        let mut category = "";
        for group in help_groups(view, section) {
            if category != group[0].category {
                category = group[0].category;
                lines.push(Line::styled(category, Style::default().add_modifier(Modifier::DIM)));
            }
            let keys = group_keys(&group);
            let padding = " ".repeat(key_width.saturating_sub(Line::from(keys.clone()).width()) + 2);
            if usize::from(width) >= key_width + 18 {
                lines.push(Line::from(vec![
                    Span::raw(keys),
                    Span::raw(padding),
                    Span::raw(group[0].description_in_view(view)),
                ]));
            } else {
                lines.push(Line::from(keys));
                lines.push(Line::from(format!("  {}", group[0].description_in_view(view))));
            }
        }
    }
    lines
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
    let bordered = width >= 40 && height >= 8;
    let block = Block::default().borders(if bordered { Borders::ALL } else { Borders::NONE });
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    f.render_widget(
        Paragraph::new(Line::styled(
            format!("Help · {}", state.mode.label()),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let footer_rows = inner.height.saturating_sub(1).min(2);
    let content = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1 - footer_rows);
    let paragraph = Paragraph::new(help_lines(state.view, state.mode, content.width)).wrap(Wrap { trim: false });
    let max = paragraph
        .line_count(content.width)
        .saturating_sub(usize::from(content.height));
    state.help_scroll_max.set(max);
    let start = state.help_scroll.get().min(max);
    state.help_scroll.set(start);
    f.render_widget(paragraph.scroll((u16::try_from(start).unwrap_or(u16::MAX), 0)), content);
    let hints = [
        format!(
            "{}/{} close",
            command_keys(Command::ToggleHelp),
            command_keys(Command::DismissHelp)
        ),
        format!(
            "{}/{} scroll",
            command_keys(Command::ScrollHelpUp),
            command_keys(Command::ScrollHelpDown)
        ),
    ];
    for row in 0..footer_rows {
        f.render_widget(
            Paragraph::new(hints[usize::from(row)].clone()),
            Rect::new(inner.x, inner.bottom() - footer_rows + row, inner.width, 1),
        );
    }
}

fn draw_arrangement(f: &mut Frame, area: Rect, state: &App) {
    if area.height == 0 {
        return;
    }
    f.render_widget(
        Paragraph::new("Arrangement | F1 Pattern | F2 Order"),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let visible = usize::from(area.height.saturating_sub(1));
    let start = state.selected_order.saturating_add(1).saturating_sub(visible);
    let lines: Vec<_> = state
        .project
        .order
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .map(|(index, &pattern)| {
            let selected = index == state.selected_order;
            let playing = matches!(state.playback_state, crate::state::PlaybackState::Playing)
                && state.playback_mode == crate::state::PlaybackMode::Song
                && index == state.playing_order;
            Line::styled(
                format!(
                    "{}{} O{index} P{pattern}  {} rows",
                    if selected { "]" } else { " " },
                    if playing { ">" } else { " " },
                    state.project.patterns[pattern].num_rows
                ),
                if selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else if playing {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            )
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines),
        Rect::new(area.x, area.y + 1, area.width, area.height - 1),
    );
}

fn draw_order(f: &mut Frame, area: Rect, state: &App) {
    let visible = usize::from(area.width.saturating_sub(15) / 12).max(1);
    let start = state.selected_order.saturating_add(1).saturating_sub(visible);
    let mut spans = vec![Span::raw(format!(
        "Order {}/{} ",
        state.selected_order,
        state.project.order.len()
    ))];
    for (index, pattern) in state.project.order.iter().enumerate().skip(start).take(visible) {
        spans.push(Span::styled(
            format!(
                " {}{index}:{} ",
                if matches!(state.playback_state, crate::state::PlaybackState::Playing)
                    && state.playback_mode == crate::state::PlaybackMode::Song
                    && index == state.playing_order
                {
                    ">"
                } else {
                    ""
                },
                pattern_label(*pattern)
            ),
            if index == state.selected_order {
                Style::default().bg(Color::Green).fg(Color::Black)
            } else {
                Style::default()
            },
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_buffer_line(f: &mut Frame, area: Rect, state: &App) {
    f.render_widget(
        Paragraph::new(Presentation(state).buffer_line(area.width))
            .style(Style::default().bg(Color::Black).fg(Color::Gray)),
        area,
    );
}

fn draw_pattern_editor(f: &mut Frame, area: Rect, state: &App) {
    const GUTTER: u16 = 5;
    const TRACK_WIDTH: u16 = 10;
    if area.is_empty() {
        return;
    }
    let pattern = &state.project.patterns[state.selected_pattern];
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
                format!(
                    "{:<10}",
                    format!("{} {}", track_number(track), state.project.instrument(track).label())
                ),
                Style::default().fg(Color::Green),
            ));
        }
        f.render_widget(
            Paragraph::new(Line::from(header)),
            Rect::new(area.x, area.y, area.width, 1),
        );
    }
    for (offset, row) in (first_row..pattern.num_rows).take(visible_rows).enumerate() {
        let playing = matches!(state.playback_state, crate::state::PlaybackState::Playing)
            && state.selected_pattern == state.playing_pattern
            && row == state.playback_row;
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
            format!("{}{}{} ", if playing { "▶" } else { " " }, row_label(row), boundary),
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
            let text = format_step(step);
            spans.push(Span::styled(text[..3].to_owned(), style));
            spans.push(Span::styled(text[3..].to_owned(), Style::default()));
            spans.push(Span::raw("  "));
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
    f.render_widget(Paragraph::new(Presentation(state).status_line(area.width)), area);
}

fn format_step(step: PatternStep) -> String {
    step.note.map_or_else(
        || " .      ".to_string(),
        |note| {
            format!(
                "{:<3}:{:>4}",
                format!("{}{}", note.pitch, note.octave),
                note.length_ticks.ticks()
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::{Note, NotePitch};
    use ratatui::backend::{Backend, TestBackend};

    #[test]
    fn instrument_keys_cycle_history_and_scroll_actual_assignment() {
        use crate::tracker::Instrument;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::default();
        let key = |app: &mut App, code, modifiers| crate::bindings::handle_key(KeyEvent::new(code, modifiers), app);
        app.cursor_row = 17;
        key(&mut app, KeyCode::F(4), KeyModifiers::NONE);
        assert_eq!(app.view, View::Instrument);
        assert!(!app.is_dirty());
        key(&mut app, KeyCode::Char('['), KeyModifiers::NONE);
        assert_eq!(app.project.instrument(0), Instrument::OpenHat);
        key(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert!(!app.is_dirty());
        key(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert_eq!(app.project.instrument(0), Instrument::OpenHat);
        for voice in Instrument::ALL {
            app.handle_command(Command::SetInstrument(voice));
            for (width, height) in [(100, 20), (40, 6), (1, 1), (0, 0)] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|f| render(f, &app)).unwrap();
                if width >= 40 {
                    assert!(screen(&terminal).contains(&format!("> {} (all patterns)", voice.label())));
                }
            }
        }
        for ch in ['v', 'a', 's', 'x'] {
            key(&mut app, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.cursor_row, 0);
        key(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);
        assert_eq!(app.cursor_track, 1);
        assert_eq!(app.project.instrument(1), Instrument::Pluck);
        key(&mut app, KeyCode::Char('?'), KeyModifiers::NONE);
        key(&mut app, KeyCode::F(3), KeyModifiers::NONE);
        assert_eq!(app.view, View::Instrument);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut app, KeyCode::Char(':'), KeyModifiers::NONE);
        key(&mut app, KeyCode::F(3), KeyModifiers::NONE);
        assert_eq!(app.view, View::Instrument);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut app, KeyCode::F(3), KeyModifiers::NONE);
        assert_eq!(app.view, View::Mixer);
        key(&mut app, KeyCode::F(2), KeyModifiers::NONE);
        assert_eq!(app.view, View::Arrangement);
        key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
        assert_eq!(app.view, View::Pattern);
    }

    #[test]
    fn voice_view_help_and_status_hide_pattern_commands_and_context() {
        for view in [View::Mixer, View::Instrument, View::Arrangement] {
            let app = App {
                view,
                octave: 9,
                ..App::default()
            };
            let text = help_lines(view, app.mode, 80)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(!text.contains("Insert C"));
            assert!(!text.contains("Lower notes"));
            assert!(!text.contains("Halve note length"));
            assert_eq!(text.contains("Audition voice"), view == View::Instrument);
            for key in ["F1", "F2", "F3", "F4"] {
                assert!(text.contains(key));
            }
            let status = presentation::Presentation(&app).status_line(120).to_string();
            assert!(!status.contains("Oct:"));
            assert!(!status.contains("Empty"));
            assert!(!status.contains("R0"));
        }
    }

    #[test]
    fn tone_rows_scroll_with_labels_and_view_specific_status() {
        let mut app = App::default();
        app.handle_command(Command::ShowMixer);
        for row in 2..crate::project::CHANNEL_ROWS {
            app.cursor_row = row;
            for (width, height) in [(80, 20), (24, 7), (12, 6)] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|f| render(f, &app)).unwrap();
                let text = screen(&terminal);
                assert!(
                    text.contains(crate::project::CHANNEL_LABELS[row]),
                    "{width}x{height}: {text}"
                );
            }
            let status = presentation::Presentation(&app).status_line(120).to_string();
            assert!(status.contains(["", "", "Lowshelf", "Highshelf", "HPF", "LPF"][row]));
        }
        let help = help_lines(app.view, app.mode, 120)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(help.contains("off"));
    }

    #[test]
    fn mixer_keys_edit_settings_without_hidden_note_edits() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::default();
        let key = |app: &mut App, code| crate::bindings::handle_key(KeyEvent::new(code, KeyModifiers::NONE), app);
        app.cursor_row = 17;
        key(&mut app, KeyCode::Char('2'));
        key(&mut app, KeyCode::F(3));
        assert_eq!(app.view, View::Mixer);
        assert_eq!(app.pending_count, None);
        assert!(!app.is_dirty());
        for ch in ['v', 'a', 's', 'x'] {
            key(&mut app, KeyCode::Char(ch));
        }
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.cursor_row, 0);
        key(&mut app, KeyCode::Char('l'));
        key(&mut app, KeyCode::Char(']'));
        assert_eq!(app.project.instrument(1), crate::tracker::Instrument::Pluck);
        assert_eq!(app.project.instrument(0), crate::tracker::Instrument::Pluck);
        key(&mut app, KeyCode::Char('u'));
        assert!(!app.is_dirty());
        for (width, height) in [(80, 12), (22, 6), (1, 1), (0, 0)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| render(f, &app)).unwrap();
            if width == 80 {
                assert!(screen(&terminal).contains("T2"));
                assert!(screen(&terminal).contains("Vol dB"));
                assert!(screen(&terminal).contains("Pan %"));
            }
        }
        key(&mut app, KeyCode::Char('?'));
        key(&mut app, KeyCode::F(1));
        assert_eq!(app.view, View::Mixer);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char(':'));
        key(&mut app, KeyCode::F(1));
        assert_eq!(app.view, View::Mixer);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::F(1));
        assert_eq!(app.view, View::Pattern);
    }

    fn screen_lines(terminal: &Terminal<TestBackend>, width: u16) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(ratatui::buffer::Cell::symbol).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

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
    fn file_prompt_dirty_path_and_errors_render_through_real_keys() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::default();
        app.handle_command(Command::IncreaseTempo);
        app.path = Some(std::path::PathBuf::from("my song.json"));
        let mut terminal = Terminal::new(TestBackend::new(100, 8)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        assert!(screen(&terminal).contains("* "));
        assert!(screen(&terminal).contains("my song.json"));
        for ch in ":w my next song.json".chars() {
            crate::bindings::handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut app);
        }
        terminal.draw(|frame| render(frame, &app)).unwrap();
        assert!(screen(&terminal).contains(":w my next song.json"));
        crate::bindings::handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &mut app);
        app.path = None;
        for ch in ":w".chars() {
            crate::bindings::handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut app);
        }
        crate::bindings::handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
        terminal.draw(|frame| render(frame, &app)).unwrap();
        assert!(screen(&terminal).contains("No file path"));
        let error = app.status.clone();
        for _ in 0..3 {
            terminal.draw(|frame| render(frame, &app)).unwrap();
            assert!(screen(&terminal).contains("No file path"));
            assert_eq!(app.status, error);
        }
        // A message never replaces active input, even if feedback arrives later.
        app.prompt = Some("w literal ? 界 path".into());
        terminal.draw(|frame| render(frame, &app)).unwrap();
        assert!(screen(&terminal).contains(":w literal ? 界"));
        assert!(!screen(&terminal).contains("No file path"));
        app.prompt = None;
        for (width, height) in [(1, 1), (12, 2), (40, 3)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| render(frame, &app)).unwrap();
        }
    }

    #[test]
    fn order_strip_scrolls_selection_and_playback_only_marks_the_playing_pattern() {
        let mut app = App::default();
        app.handle_command(Command::DuplicatePattern);
        app.handle_command(Command::AppendOrder);
        app.handle_command(Command::TogglePlayback);
        app.handle_command(Command::PreviousPattern);
        app.project.order = vec![0; 100];
        app.selected_order = 99;
        let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text = screen(&terminal);
        assert!(text.contains("[P0] >P1"));
        assert!(text.contains("Pattern Playing O0 P1 R00"));
        assert!(text.contains("99:P0"));
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 3)].symbol(), " ");
        assert!(buffer
            .content()
            .iter()
            .any(|cell| cell.symbol() == "9" && cell.bg == Color::Green));
        app.handle_command(Command::NextPattern);
        terminal.draw(|frame| render(frame, &app)).unwrap();
        assert_eq!(terminal.backend().buffer()[(0, 3)].symbol(), "▶");
        for (width, height) in [(0, 0), (1, 1), (12, 3), (30, 7)] {
            terminal.resize(Rect::new(0, 0, width, height)).unwrap();
            terminal.draw(|frame| render(frame, &app)).unwrap();
        }
    }

    #[test]
    fn buffer_and_order_cues_render_independently_without_color() {
        let mut app = App::default();
        app.handle_command(Command::DuplicatePattern);
        app.project.order = vec![0, 0, 1];
        app.selected_order = 1;
        app.playing_order = 0;
        app.playing_pattern = 0;
        app.playback_mode = crate::state::PlaybackMode::Song;
        app.playback_state = crate::state::PlaybackState::Playing;
        let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        assert!(screen(&terminal).contains(">P0 [P1]"));
        assert!(screen(&terminal).contains(">0:P0"));
        assert!(screen(&terminal).contains(" 1:P0"));
        let buffer = terminal.backend().buffer();
        assert!(buffer
            .content()
            .iter()
            .any(|cell| cell.symbol() == "1" && cell.bg == Color::Green));
        app.selected_pattern = 0;
        terminal.draw(|frame| render(frame, &app)).unwrap();
        assert!(screen(&terminal).contains(">[P0] P1"));
        assert_eq!(app.selected_order, 1);
        for (width, height) in [(6, 8), (24, 6), (40, 3), (0, 0), (80, 8)] {
            terminal.resize(Rect::new(0, 0, width, height)).unwrap();
            terminal.draw(|frame| render(frame, &app)).unwrap();
            if height >= 6 {
                assert!(screen(&terminal).contains(">[P0]"));
            }
        }
    }

    #[test]
    fn instrument_headers_fit_existing_columns_without_moving_highlights() {
        use crate::tracker::Instrument;
        for instrument in Instrument::ALL {
            let mut app = App::default();
            app.project.set_instrument(1, instrument);
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
            assert_eq!(buffer[(15, 3)].bg, Color::Green);
            assert_eq!(buffer[(15, 4)].bg, Color::DarkGray);
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
            app.project.patterns[app.selected_pattern]
                .set_step(
                    track,
                    0,
                    Some(Note {
                        length_ticks: crate::tracker::NoteLength::default(),
                        pitch,
                        octave: 4,
                    }),
                )
                .unwrap();
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
    fn added_fifth_track_scrolls_into_view_through_real_key() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::default();
        crate::bindings::handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL), &mut app);
        let mut terminal = Terminal::new(TestBackend::new(45, 9)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        assert!(screen(&terminal).contains("5 Pluck"));
        assert!(!screen(&terminal).contains("1 Pluck"));
        assert!(terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.bg == Color::Green));
    }

    #[test]
    fn narrow_viewport_follows_selected_track_and_last_row_not_playback() {
        let mut app = App {
            cursor_row: 63,
            cursor_track: 3,
            playback_state: crate::state::PlaybackState::Playing,
            ..App::default()
        };
        app.project.set_instrument(3, crate::tracker::Instrument::HiHat);
        let mut terminal = Terminal::new(TestBackend::new(15, 6)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        assert!(screen(&terminal).contains("4 HiHat"));
        assert!(screen(&terminal).contains("63"));
        assert!(!screen(&terminal).contains("? Help"));
        assert_eq!(terminal.backend().buffer()[(5, 4)].bg, Color::Green);
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
    fn help_sections_cover_every_registry_binding_and_alias() {
        for section in [None, Some(Mode::Normal), Some(Mode::Insert), Some(Mode::Visual)] {
            let groups = help_groups(View::Pattern, section);
            assert!(groups.windows(2).all(|pair| pair[0][0].category <= pair[1][0].category));
            for binding in DEFAULT_BINDINGS {
                let shared = [Mode::Normal, Mode::Insert, Mode::Visual]
                    .iter()
                    .all(|mode| binding.modes.contains(mode));
                let expected = binding.available_in_view(View::Pattern)
                    && section.map_or(shared, |mode| !shared && binding.modes.contains(&mode));
                assert_eq!(
                    groups
                        .iter()
                        .flatten()
                        .filter(|entry| entry.key == binding.key
                            && entry.modifiers == binding.modifiers
                            && entry.command == binding.command
                            && entry.modes == binding.modes)
                        .count(),
                    usize::from(expected)
                );
            }
        }
        let lines = help_lines(View::Pattern, Mode::Insert, 62);
        let text = lines.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
        let positions: Vec<_> = ["Shared", "Normal", "Insert (active)", "Visual"]
            .iter()
            .map(|label| text.find(label).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(text.contains("q/Q") && text.contains("1–9/0") && text.contains("0 only trailing"));
        assert!(text.contains("Ctrl-Space") && text.contains(":restart") && !text.contains("Null"));
        let active = lines.iter().find(|line| line.to_string() == "Insert (active)").unwrap();
        assert!(active.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn help_active_mode_uses_palette_bold_in_rendered_buffer() {
        for mode in [Mode::Normal, Mode::Insert, Mode::Visual] {
            let app = App {
                mode,
                help_visible: true,
                ..App::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(24, 6)).unwrap();
            terminal.draw(|f| render(f, &app)).unwrap();
            assert!(terminal.backend().buffer()[(0, 0)].modifier.contains(Modifier::BOLD));
            let lines = help_lines(View::Pattern, mode, 24);
            let index = lines
                .iter()
                .position(|line| line.to_string().ends_with("(active)"))
                .unwrap();
            app.help_scroll.set(
                Paragraph::new(lines[..index].to_vec())
                    .wrap(Wrap { trim: false })
                    .line_count(24),
            );
            terminal.draw(|f| render(f, &app)).unwrap();
            assert!(terminal.backend().buffer()[(0, 1)].modifier.contains(Modifier::BOLD));
            assert_eq!(terminal.backend().buffer()[(0, 1)].fg, Color::Reset);
        }
    }

    #[test]
    fn help_open_close_keeps_clipboard_dirty_revision_and_playback() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        for mode in [Mode::Normal, Mode::Insert, Mode::Visual] {
            let mut app = App::default();
            app.handle_command(Command::EnterInsert);
            app.handle_command(Command::InsertNote {
                pitch: NotePitch::C,
                octave_offset: 0,
            });
            app.handle_command(Command::EnterNormal);
            app.handle_command(Command::Copy);
            assert!(!app.clipboard.is_empty() && app.is_dirty());
            app.mode = mode;
            app.selection_anchor = Some((0, 0));
            app.pending_count = Some(12);
            app.handle_command(Command::TogglePlayback);
            let clipboard = app.clipboard.clone();
            let project = app.project.clone();
            let dirty = app.is_dirty();
            for ch in "?z :iv{}?".chars() {
                crate::bindings::handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut app);
            }
            assert!(!app.help_visible && !app.shutdown);
            assert_eq!(app.mode, mode);
            assert_eq!(app.selection_anchor, Some((0, 0)));
            assert_eq!(app.clipboard, clipboard);
            assert_eq!(app.project, project);
            assert_eq!(app.is_dirty(), dirty);
            assert!(app.prompt.is_none());
            assert_eq!(app.pending_count, None);
            assert!(matches!(app.playback_state, crate::state::PlaybackState::Playing));
        }
    }

    #[test]
    fn help_scrolls_every_wrapped_line_on_actual_small_terminals() {
        for (width, height) in [(80, 30), (32, 8), (24, 6)] {
            let mut app = App::default();
            app.handle_command(Command::ToggleHelp);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let inner_width = width.min(64) - if width >= 40 { 2 } else { 0 };
            let content_height = height.min(24) - if width >= 40 { 5 } else { 3 };
            let paragraph = Paragraph::new(help_lines(app.view, app.mode, inner_width)).wrap(Wrap { trim: false });
            let count = paragraph.line_count(inner_width);
            let mut reference = Terminal::new(TestBackend::new(inner_width, u16::try_from(count).unwrap())).unwrap();
            reference.draw(|f| f.render_widget(paragraph, f.area())).unwrap();
            let mut seen = Vec::new();
            for offset in 0..=count - usize::from(content_height) {
                terminal.draw(|frame| render(frame, &app)).unwrap();
                assert_eq!(app.help_scroll.get(), offset);
                let text = screen(&terminal);
                assert!(text.contains("?/Esc close") && text.contains("Up/Down scroll"));
                assert!(text.contains("Help · NORMAL"));
                let x = (width - width.min(64)) / 2 + u16::from(width >= 40);
                let y = (height - height.min(24)) / 2 + u16::from(width >= 40) + 1;
                for row in 0..content_height {
                    let actual: String = (0..inner_width)
                        .map(|col| terminal.backend().buffer()[(x + col, y + row)].symbol())
                        .collect();
                    let expected: String = (0..inner_width)
                        .map(|col| reference.backend().buffer()[(col, u16::try_from(offset).unwrap() + row)].symbol())
                        .collect();
                    assert_eq!(actual, expected, "offset {offset}, row {row}, {width}x{height}");
                    seen.push(offset + usize::from(row));
                }
                app.handle_command(Command::ScrollHelpDown);
            }
            assert!((0..count).all(|row| seen.contains(&row)));
            let end = app.help_scroll.get();
            for _ in 0..10000 {
                app.handle_command(Command::ScrollHelpDown);
            }
            assert_eq!(app.help_scroll.get(), end);
            terminal.resize(Rect::new(0, 0, 100, 40)).unwrap();
            terminal.draw(|frame| render(frame, &app)).unwrap();
            assert_eq!(app.help_scroll.get(), app.help_scroll_max.get());
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
        assert!(text.contains("VISUAL Count:12"));
        assert!(text.contains("P0R13T2"));
        assert!(text.contains("Sel:4r × 2t Empty"));
        let buffer = terminal.backend().buffer();
        // Cursor follows scrolling; selected cells take precedence over playback.
        assert_eq!(buffer[(15, 6)].bg, Color::Green);
        assert_eq!(buffer[(25, 6)].bg, Color::Blue);
        app.cursor_row = 15;
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(15, 6)].bg, Color::Green);
        assert_eq!(buffer[(25, 6)].bg, Color::Blue);
        assert_eq!(buffer[(0, 6)].symbol(), "▶");
        for (keys, mode) in [("", "NORMAL"), ("i", "INSERT"), ("v", "VISUAL")] {
            let mut app = App::default();
            for ch in keys.chars() {
                crate::bindings::handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut app);
            }
            for (width, height) in [(20, 3), (12, 2), (6, 1)] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|frame| render(frame, &app)).unwrap();
                if height >= 3 {
                    assert!(screen(&terminal).contains(mode));
                } else {
                    assert!(!screen(&terminal).contains("? Help"));
                }
            }
        }
    }

    #[test]
    fn gate_lengths_fit_ten_column_cells_and_selected_status() {
        let mut app = App::default();
        for (track, ticks) in [(0, 1), (1, 16), (2, 4096)] {
            app.project.patterns[0]
                .set_step(
                    track,
                    0,
                    Some(Note {
                        pitch: NotePitch::Cs,
                        octave: 4,
                        length_ticks: crate::tracker::NoteLength::try_from(ticks).unwrap(),
                    }),
                )
                .unwrap();
        }
        let mut terminal = Terminal::new(TestBackend::new(100, 1)).unwrap();
        terminal.draw(|f| draw_pattern_editor(f, f.area(), &app)).unwrap();
        let row = screen(&terminal);
        for (column, expected) in [(5, "C#4:   1"), (15, "C#4:  16"), (25, "C#4:4096"), (35, " .      ")] {
            assert_eq!(row.chars().skip(column).take(8).collect::<String>(), expected);
        }
        app.cursor_track = 2;
        terminal.draw(|f| draw_status_bar(f, f.area(), &app)).unwrap();
        assert!(screen(&terminal).contains("C#4 Len:4096t/256r"));
        app.cursor_track = 3;
        terminal.draw(|f| draw_status_bar(f, f.area(), &app)).unwrap();
        assert!(screen(&terminal).contains("Empty"));
    }

    #[test]
    fn insert_step_is_visible_in_status_and_registry_help() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::default();
        app.handle_command(Command::EnterInsert);
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        for step in [0, 1, 2, 4] {
            terminal.draw(|frame| render(frame, &app)).unwrap();
            assert!(screen(&terminal).contains(&format!("Step:{step}r")));
            crate::bindings::handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), &mut app);
        }
        app.handle_command(Command::ToggleHelp);
        let lines = help_lines(app.view, app.mode, 62);
        let index = lines
            .iter()
            .position(|line| line.to_string().contains("0/1/2/4"))
            .unwrap();
        let offset = Paragraph::new(lines[..index].to_vec())
            .wrap(Wrap { trim: false })
            .line_count(62);
        app.help_scroll.set(offset);
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text = screen(&terminal);
        assert!(text.contains("Tab") && text.contains("0/1/2/4"));
    }

    #[test]
    fn contextual_status_and_short_layout_keep_essentials() {
        for mode in [Mode::Normal, Mode::Insert, Mode::Visual] {
            let mut app = App { mode, ..App::default() };
            app.handle_command(Command::IncreaseTempo);
            for width in [24, 40, 80] {
                for height in 0..=10 {
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    terminal.draw(|frame| render(frame, &app)).unwrap();
                    let text = screen_lines(&terminal, width);
                    if height >= 3 {
                        assert!(text.contains(mode.label()), "{width}x{height}: {text}");
                        assert!(text.contains('*'));
                        assert!(text.contains("T1"));
                        assert!(text.contains("Stopped") || text.contains("P ■"));
                    }
                    if height >= 2 {
                        assert!(text.contains("00"), "missing editable row at {width}x{height}: {text}");
                    }
                    if height >= 4 {
                        assert_ne!(text.lines().last().unwrap().trim(), "? Help");
                    }
                }
            }
        }
    }

    #[test]
    fn long_literal_prompt_keeps_tail_and_terminal_cursor_without_wrapping() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .join(format!("{} 界e\u{301} 👩‍💻.json", "long path ".repeat(12)));
        let mut app = App::default();
        for ch in format!(":w {}", path.display()).chars() {
            crate::bindings::handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut app);
        }
        let original = app.prompt.clone();
        for width in [24, 40, 80] {
            for height in 1..=10 {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|frame| render(frame, &app)).unwrap();
                let text = screen_lines(&terminal, width);
                assert!(text.lines().last().unwrap().trim_end().ends_with("json"));
                assert!(text.lines().last().unwrap().contains("👩‍💻"));
                let (x, y) = terminal
                    .backend_mut()
                    .get_cursor_position()
                    .map(|p| (p.x, p.y))
                    .unwrap();
                assert_eq!(y, height - 1);
                assert!(x < width);
                assert_eq!(app.prompt, original);
                if height >= 3 {
                    assert!(text.contains("00"));
                }
            }
        }
        crate::bindings::handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut app);
        assert!(path.exists());
        assert!(app.status.contains(&path.display().to_string()));
        assert!(app.prompt.is_none());
    }

    #[test]
    fn arrangement_keys_preserve_editor_and_transport_and_block_hidden_edits() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
        let mut app = App::default();
        let press = |app: &mut App, code| {
            crate::bindings::handle_key(KeyEvent::new(code, KeyModifiers::NONE), app);
        };
        app.cursor_row = 17;
        app.handle_command(Command::TogglePlayback);
        press(&mut app, KeyCode::Char('2'));
        press(&mut app, KeyCode::F(2));
        assert_eq!(app.view, View::Arrangement);
        assert!(app.pending_count.is_none());
        assert!(!app.is_dirty());
        for code in [
            KeyCode::Char('i'),
            KeyCode::Char('v'),
            KeyCode::Char('j'),
            KeyCode::Char('x'),
        ] {
            press(&mut app, code);
        }
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.cursor_row, 0);
        assert!(!app.is_dirty());
        assert!(matches!(app.playback_state, crate::state::PlaybackState::Playing));
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::F(1));
        assert_eq!(app.view, View::Arrangement);
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char(':'));
        press(&mut app, KeyCode::F(1));
        assert_eq!(app.view, View::Arrangement);
        press(&mut app, KeyCode::Esc);
        let mut repeat = KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE);
        repeat.kind = KeyEventKind::Repeat;
        crate::bindings::handle_key(repeat, &mut app);
        assert_eq!(app.view, View::Arrangement);
        press(&mut app, KeyCode::F(1));
        assert_eq!(app.view, View::Pattern);
        assert_eq!(app.cursor_row, 17);
        assert!(!app.is_dirty());
    }

    #[test]
    fn arrangement_real_keys_edit_order_and_undo_without_switching_buffers() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::default();
        let key = |app: &mut App, code, modifiers| {
            crate::bindings::handle_key(KeyEvent::new(code, modifiers), app);
        };
        key(&mut app, KeyCode::F(2), KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('n'), KeyModifiers::CONTROL);
        assert_eq!(app.selected_pattern, 1);
        key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(app.project.order, [0, 1]);
        key(&mut app, KeyCode::Up, KeyModifiers::CONTROL);
        assert_eq!(app.project.order, [1, 0]);
        key(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(app.project.order, [0, 1]);
        key(&mut app, KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert_eq!(app.project.order, [1, 0]);
        key(&mut app, KeyCode::Right, KeyModifiers::CONTROL);
        assert_eq!(app.selected_order, 1);
        assert_eq!(app.selected_pattern, 1);
        key(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(app.project.order, [1]);
        key(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(app.project.order, [1, 0]);
        assert_eq!(app.view, View::Arrangement);
        key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(app.project.order, [0, 1]);
    }

    #[test]
    fn arrangement_scrolls_selected_entries_and_distinguishes_repeated_playback() {
        let mut app = App::default();
        app.handle_command(Command::ShowArrangement);
        for _ in 0..24 {
            app.handle_command(Command::AppendOrder);
        }
        app.selected_order = 23;
        app.playing_order = 22;
        app.playback_mode = crate::state::PlaybackMode::Song;
        app.playback_state = crate::state::PlaybackState::Playing;
        for (width, height) in [(80, 24), (24, 6), (20, 4)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| render(f, &app)).unwrap();
            let text = screen(&terminal);
            assert!(text.contains("]  O23 P0"), "{text}");
            if height > 4 {
                assert!(text.contains(" > O22 P0"), "{text}");
            }
        }
        app.handle_command(Command::RemoveOrder);
        assert_eq!(app.project.order.len(), 24);
        app.handle_command(Command::Undo);
        assert_eq!(app.project.order.len(), 25);
        app.handle_command(Command::Redo);
        assert_eq!(app.project.order.len(), 24);
        for width in 0..12 {
            for height in 0..5 {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|f| render(f, &app)).unwrap();
            }
        }
    }

    #[test]
    fn palette_status_segments_fill_width_and_keep_inline_help() {
        for (mode, color) in [
            (Mode::Normal, Color::Blue),
            (Mode::Insert, Color::Green),
            (Mode::Visual, Color::Cyan),
        ] {
            let mut app = App { mode, ..App::default() };
            app.path = Some("/tmp/song.json".into());
            app.handle_command(Command::IncreaseTempo);
            for width in [40, 80, 120] {
                let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
                terminal.draw(|f| draw_status_bar(f, f.area(), &app)).unwrap();
                let text = screen(&terminal);
                assert!(text.contains("song.json*"), "{text}");
                assert!(text.contains("? Help"), "{width}: {text}");
                assert!(text.contains("P0R00T1"));
                let buffer = terminal.backend().buffer();
                for x in 0..width {
                    let cell = &buffer[(x, 0)];
                    assert_eq!(
                        cell.bg,
                        if usize::from(x) < mode.label().len() {
                            color
                        } else {
                            Color::Black
                        }
                    );
                }
                assert!(buffer[(0, 0)].modifier.contains(Modifier::BOLD));
                assert_eq!(
                    buffer[(0, 0)].fg,
                    if mode == Mode::Normal {
                        Color::White
                    } else {
                        Color::Black
                    }
                );
                if mode != Mode::Insert {
                    assert!(!text.contains("Oct:"));
                }
            }
        }
    }

    #[test]
    fn idle_reclaims_grid_row_but_feedback_and_prompt_keep_their_row() {
        let mut app = App::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 6)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let idle = screen_lines(&terminal, 80);
        assert!(idle.lines().nth(4).unwrap().contains("02"));
        assert!(idle.lines().last().unwrap().contains("? Help"));
        app.status = "Save failed: permission denied".into();
        terminal.draw(|f| render(f, &app)).unwrap();
        let feedback = screen_lines(&terminal, 80);
        assert!(feedback.lines().last().unwrap().contains(&app.status));
        assert!(feedback.lines().nth(4).unwrap().contains("NORMAL"));
        assert!(feedback.lines().nth(3).unwrap().contains("01"));
        app.prompt = Some(":w song.json".into());
        terminal.draw(|f| render(f, &app)).unwrap();
        assert!(screen_lines(&terminal, 80)
            .lines()
            .last()
            .unwrap()
            .contains(":w song.json"));
    }

    #[test]
    fn buffer_tabs_use_contrast_and_keep_independent_playing_cues() {
        let mut app = App::default();
        app.handle_command(Command::NewPattern);
        app.playback_state = crate::state::PlaybackState::Playing;
        app.playing_pattern = 0;
        let mut terminal = Terminal::new(TestBackend::new(80, 1)).unwrap();
        terminal.draw(|f| draw_buffer_line(f, f.area(), &app)).unwrap();
        let text = screen(&terminal);
        assert!(text.contains(">P0"));
        let selected = u16::try_from(text.find("[P1]").unwrap()).unwrap();
        let playing = u16::try_from(text.find(">P0").unwrap()).unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(selected, 0)].bg, Color::DarkGray);
        assert!(buffer[(selected, 0)].modifier.contains(Modifier::BOLD));
        assert_eq!(buffer[(playing, 0)].fg, Color::Cyan);
        assert_eq!(buffer[(79, 0)].bg, Color::Black);
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
            length_ticks: crate::tracker::NoteLength::default(),
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
        assert!(screen.contains("[unsaved] * [P0]"));
        assert!(screen.contains("1 Pluck"));
        assert!(screen.contains("C#4"));
        assert!(screen.contains("BPM: 121.0"));
    }
}
