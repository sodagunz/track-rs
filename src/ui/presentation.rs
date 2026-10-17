use std::fmt::Write as _;

use ratatui::text::Line;

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

// Fields arrive in priority order. Omit whole optional fields rather than slicing
// UTF-8 or confusing byte length with terminal columns; the widget clips essentials.
pub(super) fn prioritized_line(fields: impl IntoIterator<Item = String>, width: u16) -> Line<'static> {
    let mut line = Line::default();
    for field in fields {
        let candidate = if line.spans.is_empty() {
            Line::from(field)
        } else {
            Line::from(format!("{line} {field}"))
        };
        if line.spans.is_empty() || candidate.width() <= usize::from(width) {
            line = candidate;
        }
    }
    line
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
        // Preserve mode/input, edit coordinates and transport before optional
        // note/selection context and tempo. No layout or prompt ownership here.
        prioritized_line(
            [
                self.mode(),
                self.cursor(),
                self.transport(),
                self.context(),
                format!("BPM: {:.1}", self.0.project.bpm),
            ],
            width,
        )
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

        Line::from(
            [basename.as_str(), dirty, window.as_str()]
                .into_iter()
                .filter(|field| !field.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

// Keep zero-width combining marks with their preceding character. Width comes
// from Ratatui, so CJK characters consume columns rather than UTF-8 bytes.
fn elide(text: &str, width: usize) -> String {
    if Line::from(text).width() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let columns = Line::from(ch.to_string()).width();
        if used + columns > width - 1 {
            break;
        }
        result.push(ch);
        used += columns;
    }
    result.push('…');
    result
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
    fn priority_uses_rendered_unicode_columns_and_keeps_whole_fields() {
        for (width, expected) in [(9, "界 e\u{301} tail"), (8, "界 e\u{301}"), (2, "界"), (0, "界")] {
            let line = prioritized_line(["界", "e\u{301}", "tail"].map(str::to_owned), width);
            assert_eq!(line.to_string(), expected);
        }
        let line = prioritized_line(["界".to_owned(), "optional".to_owned(), "x".to_owned()], 4);
        assert_eq!(line.to_string(), "界 x");
        assert_eq!(line.width(), 4);
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
        assert!(Presentation(&app)
            .status_line(100)
            .to_string()
            .contains("P2 R00 T1 Song Playing O3 P1 R16"));
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
