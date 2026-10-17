use std::fmt::Write as _;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::{
    state::{App, Mode, PlaybackMode, PlaybackState},
    tracker::{Note, NoteLength},
};

pub(super) fn row_label(row: usize) -> String {
    format!("{row:02}")
}

pub(super) const fn track_number(track: usize) -> usize {
    track + 1
}

pub(super) fn pattern_label(pattern: usize) -> String {
    format!("P{pattern}")
}

pub(super) fn order_label(order: usize) -> String {
    format!("O{order}")
}

pub(super) fn length_label(length: NoteLength) -> String {
    let ticks = length.ticks();
    let unit = NoteLength::TICKS_PER_ROW;
    let whole = ticks / unit;
    let remainder = ticks % unit;
    let rows = if remainder == 0 {
        whole.to_string()
    } else {
        // Sixteenths have an exact terminating four-digit decimal expansion.
        let fraction = format!("{:04}", u32::from(remainder) * 10_000 / u32::from(unit));
        format!("{whole}.{}", fraction.trim_end_matches('0'))
    };
    format!("{ticks}t/{rows}r")
}

fn pitch_label(note: Note) -> String {
    format!("{}{}", note.pitch, note.octave)
}

// A borrowed view, never a second document or editor state. Derive only the
// fields needed by the line being drawn (in particular, selection aggregation).
pub(super) struct Presentation<'a>(pub(super) &'a App);

impl Presentation<'_> {
    pub(super) fn mode(&self) -> String {
        let app = self.0;
        let mut text = app.mode.label().to_owned();
        if let Some(count) = app.pending_count {
            let _ = write!(text, " Count:{count}");
        }
        if let Some(prefix) = app.pending_prefix {
            let _ = write!(text, " {prefix}");
        }
        text
    }

    pub(super) fn cursor(&self) -> String {
        let app = self.0;
        format!(
            "{} R{} T{}",
            pattern_label(app.selected_pattern),
            row_label(app.cursor_row),
            track_number(app.cursor_track)
        )
    }

    pub(super) fn transport(&self) -> String {
        let app = self.0;
        let mode = match app.playback_mode {
            PlaybackMode::Pattern => "Pattern",
            PlaybackMode::Song => "Song",
        };
        match app.playback_state {
            PlaybackState::Stopped => format!("{mode} Stopped"),
            PlaybackState::Playing => format!(
                "{mode} Playing {} {} R{}",
                order_label(app.playing_order),
                pattern_label(app.playing_pattern),
                row_label(app.playback_row)
            ),
        }
    }

    pub(super) fn context(&self) -> String {
        let app = self.0;
        if app.mode == Mode::Visual {
            return self.selection();
        }
        let note = app.project.patterns[app.selected_pattern]
            .get_step(app.cursor_track, app.cursor_row)
            .and_then(|step| step.note);
        let cell = note.map_or_else(
            || "Empty".to_owned(),
            |note| format!("{} Len:{}", pitch_label(note), length_label(note.length_ticks)),
        );
        if app.mode == Mode::Insert {
            format!("Oct:{} Step:{}r {cell}", app.octave, app.edit_step)
        } else {
            cell
        }
    }

    fn selection(&self) -> String {
        let app = self.0;
        let (anchor_row, anchor_track) = app.selection_anchor.unwrap_or((app.cursor_row, app.cursor_track));
        let rows = anchor_row.min(app.cursor_row)..=anchor_row.max(app.cursor_row);
        let tracks = anchor_track.min(app.cursor_track)..=anchor_track.max(app.cursor_track);
        let dimensions = format!("{}r × {}t", rows.clone().count(), tracks.clone().count());
        let pattern = &app.project.patterns[app.selected_pattern];
        let mut first: Option<Note> = None;
        let mut mixed_pitch = false;
        let mut mixed_length = false;
        for track in tracks {
            for row in rows.clone() {
                let Some(note) = pattern.get_step(track, row).and_then(|step| step.note) else {
                    continue;
                };
                if let Some(initial) = first {
                    mixed_pitch |= (initial.pitch, initial.octave) != (note.pitch, note.octave);
                    mixed_length |= initial.length_ticks != note.length_ticks;
                } else {
                    first = Some(note);
                }
                if mixed_pitch && mixed_length {
                    return format!("Sel:{dimensions} Pitch:mixed Len:mixed");
                }
            }
        }
        first.map_or_else(
            || format!("Sel:{dimensions} Empty"),
            |note| {
                format!(
                    "Sel:{dimensions} Pitch:{} Len:{}",
                    if mixed_pitch {
                        "mixed".to_owned()
                    } else {
                        pitch_label(note)
                    },
                    if mixed_length {
                        "mixed".to_owned()
                    } else {
                        length_label(note.length_ticks)
                    }
                )
            },
        )
    }

    pub(super) fn status_line(&self, width: u16) -> Line<'static> {
        let width = usize::from(width);
        let app = self.0;
        let base = Style::default().bg(Color::Black).fg(Color::Gray);

        let dirty = if app.is_dirty() { "*" } else { "" };
        let cursor = self.cursor().replace(' ', "");
        let compact = format!(
            "{} {}",
            if app.playback_mode == PlaybackMode::Song {
                "S"
            } else {
                "P"
            },
            if matches!(app.playback_state, PlaybackState::Playing) {
                ">"
            } else {
                "■"
            }
        );
        let mut mode = self.mode();
        let minimum = Line::from(format!("{mode} …{dirty} {cursor} {compact}")).width();
        if minimum > width {
            app.mode.label().clone_into(&mut mode);
        }
        if Line::from(format!("{mode} …{dirty} {cursor} {compact}")).width() > width {
            app.mode.label()[..1].clone_into(&mut mode);
        }
        let mut right = compact;
        let context = self.context();
        let help = format!("{} Help", super::command_keys(crate::command::Command::ToggleHelp));
        let fixed = Line::from(format!("{mode} [unsaved]{dirty} {cursor} ")).width()
            + if width >= 60 {
                Line::from(context.as_str()).width() + 3
            } else {
                Line::from(help.as_str()).width() + 3
            };
        let positioned = format!(
            "{} O{} P{} R{}",
            right,
            app.playing_order,
            app.playing_pattern,
            row_label(app.playback_row)
        );
        let minimum = Line::from(format!("{mode} …{dirty} {cursor} ")).width();
        for (candidate, reserved, enabled) in [
            (self.transport(), fixed, true),
            (
                positioned,
                minimum,
                matches!(app.playback_state, PlaybackState::Playing),
            ),
        ] {
            if enabled && reserved + Line::from(candidate.as_str()).width() <= width {
                right = candidate;
                break;
            }
        }
        let name_budget = width.saturating_sub(Line::from(format!("{mode} {dirty} {cursor} {right}")).width());
        let name = elide(&self.basename(), name_budget.min(18));
        let mut spans = vec![
            Span::styled(mode, self.mode_style()),
            Span::raw(" "),
            Span::styled(
                format!("{name}{dirty}"),
                base.fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(cursor, base.fg(Color::White)),
        ];

        // Spend spare columns on whole fields; essentials never depend on context size.
        let bpm = format!("BPM: {:.1}", app.project.bpm);
        let mut fields = [(context, false, 4), (help, true, 4), (bpm, true, 14)];
        if width < 60 {
            fields.swap(0, 1);
        }
        for (field, on_right, reserve) in fields {
            let used = Line::from(spans.clone()).width() + Line::from(right.as_str()).width();
            if used + Line::from(field.as_str()).width() + reserve <= width {
                if on_right {
                    right = format!("{field} | {right}");
                } else {
                    spans.push(Span::styled(format!(" | {field}"), base));
                }
            }
        }
        let gap = width.saturating_sub(Line::from(spans.clone()).width() + Line::from(right.as_str()).width());
        spans.extend(self.transport_bar(gap, base));
        spans.push(Span::styled(right, base.fg(Color::White)));
        Line::from(spans).style(base)
    }

    fn transport_bar(&self, width: usize, base: Style) -> Vec<Span<'static>> {
        if width < 5 {
            return vec![Span::raw(" ".repeat(width))];
        }
        let app = self.0;
        let rows = |index: usize| app.project.patterns.get(index).map_or(0, |pattern| pattern.num_rows);
        let (position, total) = match app.playback_mode {
            PlaybackMode::Pattern => (app.playback_row, rows(app.playing_pattern)),
            PlaybackMode::Song => {
                let before: usize = app
                    .project
                    .order
                    .iter()
                    .take(app.playing_order)
                    .map(|&index| rows(index))
                    .sum();
                let total = app.project.order.iter().map(|&index| rows(index)).sum();
                (before.saturating_add(app.playback_row), total)
            }
        };
        let cells = width - 2;
        let head = position.min(total.saturating_sub(1)) * (cells - 1) / total.saturating_sub(1).max(1);
        let cue = if matches!(app.playback_state, PlaybackState::Playing) {
            "▶"
        } else {
            "Ⅱ"
        };
        vec![
            Span::raw(" "),
            Span::styled("┃".repeat(head), base.fg(Color::Cyan)),
            Span::styled(cue, base.fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("─".repeat(cells - head - 1), base.fg(Color::DarkGray)),
            Span::raw(" ".repeat(width - cells - 1)),
        ]
    }

    fn mode_style(&self) -> Style {
        let (foreground, background) = match self.0.mode {
            Mode::Normal => (Color::White, Color::Blue),
            Mode::Insert => (Color::Black, Color::Green),
            Mode::Visual => (Color::Black, Color::Cyan),
        };
        Style::default()
            .fg(foreground)
            .bg(background)
            .add_modifier(Modifier::BOLD)
    }

    fn basename(&self) -> String {
        self.0
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map_or_else(|| "[unsaved]".to_owned(), |name| name.to_string_lossy().into_owned())
    }

    pub(super) fn buffer_line(&self, width: u16) -> Line<'static> {
        let app = self.0;
        let width = usize::from(width);
        let dirty = if app.is_dirty() { "*" } else { "" };
        let tab = |index| {
            let label = pattern_label(index);
            let label = if index == app.selected_pattern {
                format!("[{label}]")
            } else {
                label
            };
            if matches!(app.playback_state, PlaybackState::Playing) && index == app.playing_pattern {
                format!(">{label}")
            } else {
                label
            }
        };
        let active = tab(app.selected_pattern);
        let essential = active.len() + dirty.len() + usize::from(!dirty.is_empty());
        if essential > width {
            return Line::from(if dirty.is_empty() {
                active
            } else {
                format!("* {active}")
            });
        }
        // Grow a contiguous window around the edit buffer, never the playhead.
        // Overflow markers are optional only when the essential fields barely fit.
        let mut start = app.selected_pattern;
        let mut end = start + 1;
        let tabs = |start: usize, end: usize| {
            let mut fields = Vec::new();
            if start > 0 {
                fields.push("←".to_owned());
            }
            fields.extend((start..end).map(tab));
            if end < app.project.patterns.len() {
                fields.push("→".to_owned());
            }
            fields.join(" ")
        };
        let budget = width - dirty.len() - usize::from(!dirty.is_empty());
        let basename = app
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map_or_else(|| "[unsaved]".to_owned(), |name| name.to_string_lossy().into_owned());
        // Reserve the active tab and overflow cues before the filename; only
        // then spend spare columns on neighboring buffers.
        let basename = elide(
            &basename,
            budget.saturating_sub(Line::from(tabs(start, end)).width() + 1),
        );
        let budget = budget - Line::from(basename.as_str()).width() - usize::from(!basename.is_empty());
        loop {
            if start > 0 && Line::from(tabs(start - 1, end)).width() <= budget {
                start -= 1;
            } else if end < app.project.patterns.len() && Line::from(tabs(start, end + 1)).width() <= budget {
                end += 1;
            } else {
                break;
            }
        }
        let window = tabs(start, end);
        let window = if Line::from(window.as_str()).width() <= budget {
            window
        } else {
            active
        };

        let base = Style::default().bg(Color::Black).fg(Color::Gray);
        let mut spans = Vec::new();
        for field in [basename.as_str(), dirty, window.as_str()]
            .into_iter()
            .filter(|field| !field.is_empty())
        {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            for (index, token) in field.split(' ').enumerate() {
                if index > 0 {
                    spans.push(Span::raw(" "));
                }
                let style = if token.contains('[') && token.contains(&pattern_label(app.selected_pattern)) {
                    base.bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)
                } else if token.starts_with('>') {
                    base.fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    base.add_modifier(Modifier::DIM)
                };
                spans.push(Span::styled(token.to_owned(), style));
            }
        }
        Line::from(spans).style(base)
    }
}

// Use Ratatui's grapheme iterator for combining marks, flags and ZWJ emoji.
fn elide(text: &str, width: usize) -> String {
    if Line::from(text).width() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    let span = Span::raw(text);
    for grapheme in span.styled_graphemes(Style::default()) {
        let columns = Line::from(grapheme.symbol).width();
        if used + columns > width - 1 {
            break;
        }
        result.push_str(grapheme.symbol);
        used += columns;
    }
    result.push('…');
    result
}

// Input is append/backspace-only. Reserve a terminal cell for the insertion
// cursor and show a grapheme-aligned suffix without changing the literal input.
pub(super) fn prompt_view(text: &str, width: u16) -> (String, u16) {
    let full = format!(":{text}");
    let budget = usize::from(width.saturating_sub(1));
    if Line::from(full.as_str()).width() <= budget {
        let cursor = u16::try_from(Line::from(full.as_str()).width()).unwrap_or(0);
        return (full, cursor);
    }
    let span = Span::raw(text);
    let graphemes: Vec<_> = span.styled_graphemes(Style::default()).collect();
    let mut used = usize::from(budget > 0);
    let mut start = graphemes.len();
    for (index, grapheme) in graphemes.iter().enumerate().rev() {
        let columns = Line::from(grapheme.symbol).width();
        if used + columns > budget {
            break;
        }
        used += columns;
        start = index;
    }
    let mut visible = if budget > 0 { "…".to_owned() } else { String::new() };
    for grapheme in &graphemes[start..] {
        visible.push_str(grapheme.symbol);
    }
    (visible, u16::try_from(used).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        command::Command,
        tracker::{NotePitch, Pattern},
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn run_prompt(app: &mut App, text: &str) {
        app.handle_command(Command::OpenPrompt);
        for ch in text.chars() {
            crate::bindings::handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), app);
        }
        crate::bindings::handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), app);
    }

    #[test]
    fn buffer_basename_unicode_and_essential_priorities() {
        let mut app = App::default();
        assert_eq!(Presentation(&app).buffer_line(80).to_string(), "[unsaved] [P0]");
        app.path = Some("/hidden/界e\u{301}界song.json".into());
        assert_eq!(
            Presentation(&app).buffer_line(80).to_string(),
            "界e\u{301}界song.json [P0]"
        );
        assert_eq!(elide("界e\u{301}界song.json", 4), "界e\u{301}…");
        assert_eq!(elide("界e\u{301}", 2), "…");
        app.handle_command(Command::IncreaseTempo);
        for width in 0..40 {
            let line = Presentation(&app).buffer_line(width);
            if width >= 6 {
                assert!(line.to_string().contains("[P0]"));
                assert!(line.width() <= usize::from(width));
            }
            assert!(line.to_string().contains('*'));
            assert!(!line.to_string().contains("hidden"));
        }
        assert_eq!(Presentation(&app).buffer_line(6).to_string(), "* [P0]");
    }

    #[test]
    fn all_256_buffers_and_playhead_are_independent_across_resizes() {
        let mut app = App::default();
        app.project.patterns = vec![Pattern::new(1, 1); 256];
        app.playback_state = PlaybackState::Playing;
        app.playing_pattern = 0;
        for active in 0..256 {
            app.selected_pattern = active;
            let wide = Presentation(&app).buffer_line(2000).to_string();
            for pattern in 0..256 {
                assert!(wide.split_whitespace().any(|field| field == format!("P{pattern}")
                    || field == format!("[P{pattern}]")
                    || (pattern == 0 && (field == ">P0" || field == ">[P0]"))));
            }
            for width in [8, 12, 24, 40, 80] {
                let line = Presentation(&app).buffer_line(width);
                let text = line.to_string();
                assert!(text.contains(&format!("[P{active}]")), "{width}: {text}");
                assert!(line.width() <= usize::from(width));
                if active == 0 {
                    assert!(text.contains(">[P0]"));
                }
                if width >= 12 {
                    let visible: Vec<usize> = text
                        .split_whitespace()
                        .filter_map(|field| field.trim_matches(['>', '[', ']']).strip_prefix('P')?.parse().ok())
                        .collect();
                    assert_eq!(text.contains('←'), visible[0] > 0);
                    assert_eq!(text.contains('→'), *visible.last().unwrap() < 255);
                }
            }
            assert_eq!(app.selected_pattern, active);
            assert!(Presentation(&app).transport().contains("P0"));
        }
        app.playback_state = PlaybackState::Stopped;
        assert!(!Presentation(&app).buffer_line(80).to_string().contains('>'));
    }

    #[test]
    fn buffer_creation_history_save_and_load_use_current_document() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("界e\u{301}.json");
        let mut app = App::default();
        app.handle_command(Command::NewPattern);
        assert!(Presentation(&app).buffer_line(80).to_string().contains("* P0 [P1]"));
        run_prompt(&mut app, &format!("w {}", path.display()));
        assert!(!app.is_dirty());
        app.handle_command(Command::IncreaseTempo);
        assert!(Presentation(&app).buffer_line(80).to_string().contains('*'));
        app.handle_command(Command::Undo);
        assert!(!Presentation(&app).buffer_line(80).to_string().contains('*'));
        app.handle_command(Command::Redo);
        assert!(Presentation(&app).buffer_line(80).to_string().contains('*'));
        app.handle_command(Command::DuplicatePattern);
        assert!(Presentation(&app).buffer_line(12).to_string().contains("[P2]"));
        app.handle_command(Command::Undo);
        assert!(Presentation(&app).buffer_line(12).to_string().contains("[P1]"));
        run_prompt(&mut app, &format!("e! {}", path.display()));
        assert!(!app.is_dirty());
        assert_eq!(app.project.patterns.len(), 2);
        assert!(Presentation(&app)
            .buffer_line(12)
            .to_string()
            .contains(&format!("[P{}]", app.selected_pattern)));
    }

    fn note(ticks: u16) -> Note {
        Note {
            pitch: NotePitch::C,
            octave: 4,
            length_ticks: NoteLength::try_from(ticks).unwrap(),
        }
    }

    #[test]
    fn transport_bar_scales_pattern_and_song_positions() {
        let mut app = App::default();
        app.project.patterns[0].num_rows = 4;
        app.project.patterns.push(app.project.patterns[0].clone());
        app.project.patterns[1].num_rows = 8;
        app.project.order = vec![0, 1, 0];
        app.playback_state = PlaybackState::Playing;
        let bar = |app: &App, width| Line::from(Presentation(app).transport_bar(width, Style::default())).to_string();
        assert_eq!(bar(&app, 9), " ▶────── ");
        app.playback_row = 3;
        assert_eq!(bar(&app, 9), " ┃┃┃┃┃┃▶ ");
        app.playback_mode = PlaybackMode::Song;
        app.playing_order = 1;
        app.playing_pattern = 1;
        app.playback_row = 4;
        assert_eq!(bar(&app, 9), " ┃┃┃▶─── ");
        app.playing_order = 2;
        app.playing_pattern = 0;
        app.playback_row = 3;
        assert_eq!(bar(&app, 9), " ┃┃┃┃┃┃▶ ");
        app.playback_state = PlaybackState::Stopped;
        assert_eq!(bar(&app, 9), " ┃┃┃┃┃┃Ⅱ ");
        for width in 0..80 {
            assert_eq!(
                Line::from(Presentation(&app).transport_bar(width, Style::default())).width(),
                width
            );
        }
        app.project.order.clear();
        assert_eq!(bar(&app, 9), " Ⅱ────── ");
    }

    #[test]
    fn wide_transport_fills_remaining_status_width() {
        let mut app = App::default();
        for width in [40, 80, 160, 240] {
            let bar = Line::from(Presentation(&app).transport_bar(width, Style::default()));
            assert_eq!(bar.width(), width);
            assert_eq!(bar.to_string(), format!(" Ⅱ{} ", "─".repeat(width - 3)));
            let status = Presentation(&app).status_line(u16::try_from(width).unwrap());
            assert_eq!(status.width(), width);
            if width >= 160 {
                assert!(status.to_string().contains(&"─".repeat(31)));
            }
        }
        app.playback_row = app.project.patterns[0].num_rows - 1;
        let bar = Line::from(Presentation(&app).transport_bar(100, Style::default()));
        assert_eq!(bar.to_string(), format!(" {}Ⅱ ", "┃".repeat(97)));
    }

    #[test]
    fn exact_tick_fractions_cover_the_entire_valid_range() {
        for ticks in 1..=4096 {
            let label = length_label(NoteLength::try_from(ticks).unwrap());
            let (actual_ticks, rows) = label.split_once("t/").unwrap();
            assert_eq!(actual_ticks.parse::<u16>().unwrap(), ticks);
            assert_eq!(
                rows.trim_end_matches('r').parse::<f64>().unwrap(),
                f64::from(ticks) / 16.0
            );
        }
        assert_eq!(length_label(note(1).length_ticks), "1t/0.0625r");
        assert_eq!(length_label(note(4096).length_ticks), "4096t/256r");
    }

    #[test]
    fn contexts_follow_actual_notes_not_entry_octave_after_transpose() {
        let mut app = App::default();
        assert_eq!(Presentation(&app).context(), "Empty");
        app.insert_note_under_cursor(Some(note(1)));
        app.handle_command(Command::RaiseNote);
        assert_eq!(app.octave, 4);
        assert_eq!(Presentation(&app).context(), "C#4 Len:1t/0.0625r");
        app.octave = 7;
        app.handle_command(Command::EnterInsert);
        assert_eq!(Presentation(&app).context(), "Oct:7 Step:0r C#4 Len:1t/0.0625r");
        app.mode = Mode::Normal;
        assert!(!Presentation(&app).context().contains("Oct"));
        assert!(!Presentation(&app).context().contains("Step"));
    }

    #[test]
    fn visual_summary_is_inclusive_and_only_uses_selected_edit_pattern_rectangle() {
        let mut app = App::default();
        app.project.patterns.push(Pattern::new(64, 4));
        app.selected_pattern = 1;
        app.playing_pattern = 0;
        app.mode = Mode::Visual;
        app.selection_anchor = Some((2, 1));
        app.cursor_row = 1;
        app.cursor_track = 0;
        app.project.patterns[0].set_step(0, 1, Some(note(4096))).unwrap();
        app.project.patterns[1].set_step(3, 1, Some(note(4096))).unwrap();
        assert_eq!(Presentation(&app).context(), "Sel:2r × 2t Empty");
        app.project.patterns[1].set_step(0, 1, Some(note(1))).unwrap();
        assert_eq!(Presentation(&app).context(), "Sel:2r × 2t Pitch:C4 Len:1t/0.0625r");
        let mut different = note(16);
        different.octave = 5;
        app.project.patterns[1].set_step(1, 2, Some(different)).unwrap();
        assert_eq!(Presentation(&app).context(), "Sel:2r × 2t Pitch:mixed Len:mixed");
        different.length_ticks = note(1).length_ticks;
        app.project.patterns[1].set_step(1, 2, Some(different)).unwrap();
        assert_eq!(Presentation(&app).context(), "Sel:2r × 2t Pitch:mixed Len:1t/0.0625r");
    }

    #[test]
    fn coordinates_and_transport_keep_edit_and_play_positions_independent() {
        let mut app = App {
            selected_pattern: 2,
            playing_pattern: 1,
            playing_order: 3,
            playback_row: 16,
            playback_mode: PlaybackMode::Song,
            ..App::default()
        };
        app.project.patterns = vec![Pattern::new(64, 4); 3];
        assert_eq!(Presentation(&app).cursor(), "P2 R00 T1");
        assert_eq!(track_number(0), 1);
        assert_eq!(row_label(16), "16");
        assert_eq!(pattern_label(0), "P0");
        assert_eq!(order_label(0), "O0");
        assert_eq!(Presentation(&app).transport(), "Song Stopped");
        app.playback_state = PlaybackState::Playing;
        assert_eq!(Presentation(&app).transport(), "Song Playing O3 P1 R16");
        let status = Presentation(&app).status_line(100).to_string();
        assert!(status.contains("P2R00T1"));
        assert!(status.ends_with("Song Playing O3 P1 R16"));
    }

    #[test]
    fn status_prioritizes_dirty_cursor_and_right_aligned_transport() {
        let mut app = App::default();
        app.handle_command(Command::IncreaseTempo);
        app.playback_mode = PlaybackMode::Song;
        app.playback_state = PlaybackState::Playing;
        app.playing_order = 3;
        app.playing_pattern = 7;
        app.playback_row = 19;
        for mode in [Mode::Normal, Mode::Insert, Mode::Visual] {
            app.mode = mode;
            for width in [24, 40, 80] {
                let line = Presentation(&app).status_line(width);
                let text = line.to_string();
                assert_eq!(line.width(), usize::from(width));
                assert!(text.starts_with(mode.label()));
                assert!(text.contains('*'));
                assert!(text.contains("T1"));
                if width == 24 {
                    assert!(text.ends_with("S >"));
                } else {
                    assert!(text.ends_with("O3 P7 R19"), "{width}: {text}");
                }
            }
        }
        app.playback_state = PlaybackState::Stopped;
        let text = Presentation(&app).status_line(80).to_string();
        assert!(text.ends_with("Song Stopped"));
        assert!(!text.contains("O3"));
        assert!(!text.contains("P7"));
        assert!(!text.contains("R19"));
    }

    #[test]
    fn prompt_and_basename_clipping_preserve_whole_graphemes() {
        for text in ["👩‍💻", "🇦🇷", "e\u{301}", "界"] {
            assert_eq!(elide(&format!("{text}tail"), 1), "…");
            for width in 0..=12 {
                let input = format!("long prefix {text}");
                let (visible, cursor) = prompt_view(&input, width);
                assert!(Line::from(visible.as_str()).width() <= usize::from(width.saturating_sub(1)));
                assert_eq!(usize::from(cursor), Line::from(visible.as_str()).width());
                if visible.len() > '…'.len_utf8() && visible.starts_with('…') {
                    assert!(visible.ends_with(text));
                }
            }
        }
    }

    #[test]
    fn pending_labels_follow_real_input_and_clear_on_escape_and_help() {
        let mut app = App::default();
        for ch in "12g".chars() {
            crate::bindings::handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut app);
        }
        assert_eq!(Presentation(&app).mode(), "NORMAL Count:12 g");
        crate::bindings::handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &mut app);
        assert_eq!(Presentation(&app).mode(), "NORMAL");
        for ch in "3g?".chars() {
            crate::bindings::handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), &mut app);
        }
        assert!(app.help_visible);
        assert_eq!(Presentation(&app).mode(), "NORMAL");
    }
}
