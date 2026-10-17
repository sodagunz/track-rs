use crate::{
    clipboard::EventClipboard,
    command::Command,
    history::{Edit, History, Revision},
    musical_time::Tick,
    project::Project,
    tracker::Note,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Pattern,
    Arrangement,
    Mixer,
    Instrument,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ViewState {
    mode: Mode,
    row: usize,
    track: usize,
    anchor: Option<(usize, usize)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Audition,
    Visual,
}

impl Mode {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Audition => "AUDITION",
            Self::Visual => "VISUAL",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaybackMode {
    #[default]
    Pattern,
    Song,
}

#[derive(Clone, Copy, Debug)]
pub enum PlaybackState {
    Stopped,
    Playing,
}

#[derive(Clone, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Input repeat suppression, help, shutdown, and transport stop flags are independent."
)]
pub struct App {
    pub theme: crate::theme::Theme,
    pub meters: crate::meter::MeterDisplay,
    pub view: View,
    pub(crate) view_states: [ViewState; 4],
    pub(crate) audition: Option<crate::transport::NoteEvent>,
    pub(crate) audition_active: bool,
    pub(crate) history: History,
    pub(crate) clipboard: EventClipboard,
    /// Session-only click preference; not a document or record-arm flag.
    pub metronome: bool,
    pub edit_step: usize,
    pub entry_length: crate::tracker::NoteLength,
    pub(crate) entry_duration: Option<crate::musical_time::DurationTicks>,
    pub mode: Mode,
    pub selection_anchor: Option<(usize, usize)>,
    pub pending_count: Option<u16>,
    pub suppressed_repeat: bool,
    pub(crate) mixer_gesture: Option<crate::bindings::MixerGesture>,
    pub pending_prefix: Option<char>,
    pub project: Project,
    pub selected_pattern: usize,
    pub selected_order: usize,
    pub playback_mode: PlaybackMode,
    pub playing_pattern: usize,
    pub playing_order: usize,
    pub prompt: Option<String>,
    pub status: String,
    pub path: Option<std::path::PathBuf>,
    pub(crate) saved_revision: Revision,
    pub(crate) stop_requested: bool,
    pub cursor_row: usize,
    pub(crate) inspected_tick: Option<Tick>,
    pub cursor_track: usize,
    /// UI mirror of the transport row; the runtime transport owns advancement.
    pub playback_row: usize,
    /// Exact local tick of the last transport boundary, independent of view density.
    pub playback_tick: Tick,
    pub(crate) playback_start_tick: Option<Tick>,
    pub playback_state: PlaybackState,
    pub octave: u8,
    pub shutdown: bool,
    pub help_visible: bool,
    /// Rendered-line offset and last measured viewport limit for the help overlay.
    pub help_scroll: std::cell::Cell<usize>,
    pub(crate) help_scroll_max: std::cell::Cell<usize>,
}

pub enum CursorOffset {
    Neg(usize),
    Pos(usize),
    None,
}

impl CursorOffset {
    const fn from_delta(value: i32) -> Self {
        if value < 0 {
            Self::Neg(value.unsigned_abs() as usize)
        } else {
            Self::Pos(value.unsigned_abs() as usize)
        }
    }

    const fn apply_to(&self, position: usize) -> usize {
        match self {
            Self::Pos(delta) => position.saturating_add(*delta),
            Self::Neg(delta) => position.saturating_sub(*delta),
            Self::None => position,
        }
    }
}

impl Default for App {
    fn default() -> Self {
        let default_rows = 64;
        let default_tracks = 4;
        Self {
            theme: crate::theme::Theme::default(),
            meters: crate::meter::MeterDisplay::default(),
            view: View::Pattern,
            view_states: [ViewState::default(); 4],
            audition: None,
            audition_active: false,
            history: History::default(),
            clipboard: EventClipboard::default(),
            metronome: false,
            edit_step: 0,
            entry_length: crate::tracker::NoteLength::default(),
            entry_duration: None,
            mode: Mode::Normal,
            selection_anchor: None,
            pending_count: None,
            pending_prefix: None,
            suppressed_repeat: false,
            mixer_gesture: None,
            project: Project::new(default_rows, default_tracks),
            selected_pattern: 0,
            selected_order: 0,
            playback_mode: PlaybackMode::Pattern,
            playing_pattern: 0,
            playing_order: 0,
            prompt: None,
            status: String::new(),
            path: None,
            saved_revision: Revision::default(),
            stop_requested: false,
            cursor_row: 0,
            inspected_tick: None,
            cursor_track: 0,
            octave: 4,
            playback_row: 0,
            playback_tick: Tick(0),
            playback_start_tick: None,
            playback_state: PlaybackState::Stopped,
            shutdown: false,
            help_visible: false,
            help_scroll: std::cell::Cell::new(0),
            help_scroll_max: std::cell::Cell::new(0),
        }
    }
}

impl App {
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.revision() != self.saved_revision
    }

    fn request_audition(&mut self) {
        if self.view == View::Instrument && matches!(self.playback_state, PlaybackState::Stopped) {
            self.audition_active = true;
            self.audition = Some(crate::transport::NoteEvent {
                track: self.cursor_track,
                instrument: self.project.instrument(self.cursor_track),
                note: Note {
                    pitch: crate::tracker::NotePitch::C,
                    octave: self.octave,
                    length_ticks: crate::tracker::NoteLength::default(),
                },
                beats: 0.0,
                duration: crate::audio::DEFAULT_NOTE_DURATION,
            });
        }
    }

    /// Consume a bounded preview once; transport transitions clear stale requests.
    pub const fn take_audition(&mut self) -> Option<crate::transport::NoteEvent> {
        let event = self.audition.take();
        if self.shutdown {
            return None;
        }
        event
    }

    /// Consume a stop transition even if a caller restarted playback before the next tick.
    pub fn take_stop_request(&mut self) -> bool {
        std::mem::take(&mut self.stop_requested)
    }

    pub(crate) fn execute_prompt(&mut self, text: &str) {
        self.status.clear();
        let result = if self.view == View::Mixer && self.mode == Mode::Insert {
            self.enter_mixer_value(text)
        } else {
            self.execute_file_command(text)
        };
        if let Err(error) = result {
            self.status = format!("{error:#}");
        }
    }

    fn execute_file_command(&mut self, text: &str) -> anyhow::Result<()> {
        let text = text.trim_start();
        let (command, remainder) = text
            .find(char::is_whitespace)
            .map_or((text, ""), |index| (&text[..index], text[index..].trim_start()));
        anyhow::ensure!(
            self.mode != Mode::Visual || matches!(command, "quantize" | "quantize?"),
            "Visual prompt supports :quantize or :quantize? only"
        );
        match command {
            "quantize" | "quantize?" => self.quantize(command == "quantize?", remainder.trim())?,
            "metronome" => {
                anyhow::ensure!(self.mode == Mode::Normal, "Metronome requires Normal mode");
                self.metronome = match remainder.trim() {
                    "on" => true,
                    "off" => false,
                    "toggle" => !self.metronome,
                    _ => anyhow::bail!("Expected :metronome on|off|toggle"),
                };
                self.status = if self.metronome {
                    "Click on (playing transport only)"
                } else {
                    "Click off"
                }
                .into();
            }
            "w" | "wq" => {
                let path = if remainder.is_empty() {
                    self.path
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("No file path; use :w <path>"))?
                } else {
                    std::path::PathBuf::from(remainder)
                };
                crate::project_file::save(&self.project, &path)?;
                self.path = Some(path);
                self.saved_revision = self.revision();
                self.status = format!("Saved {}", self.path.as_ref().unwrap().display());
                if command == "wq" {
                    self.shutdown = true;
                }
            }
            "e" | "e!" => {
                anyhow::ensure!(!remainder.is_empty(), "Expected :e[!] <path>");
                anyhow::ensure!(
                    command == "e!" || !self.is_dirty(),
                    "Unsaved changes; use :w or :e! <path>"
                );
                let path = std::path::PathBuf::from(remainder);
                let project = crate::project_file::load(&path)?;
                self.project = project;
                self.audition = None;
                self.audition_active = false;
                self.path = Some(path);
                self.selected_pattern = 0;
                self.selected_order = 0;
                self.playing_pattern = 0;
                self.playing_order = 0;
                self.history = History::default();
                self.saved_revision = self.revision();
                self.clipboard = EventClipboard::default();
                self.view_states = [ViewState::default(); 4];
                self.selection_anchor = None;
                self.cursor_row = 0;
                self.cursor_track = 0;
                self.pending_count = None;
                self.mode = Mode::Normal;
                self.playback_state = PlaybackState::Stopped;
                self.playback_row = 0;
                self.playback_tick = Tick(0);
                self.stop_requested = true;
                self.status = format!("Loaded {}", self.path.as_ref().unwrap().display());
            }
            "meter" | "meter!" | "grid" | "next-event" | "prev-event" | "paste!" => {
                self.execute_time_command(command, remainder.trim())?;
            }
            "step" | "length" | "duration" => self.set_entry_setting(command, remainder.trim())?,
            "end" | "end!" | "resize" | "resize!" => self.execute_end_command(command, remainder.trim())?,
            "restart" => {
                anyhow::ensure!(remainder.is_empty(), "Expected :restart");
                self.restart_playback();
            }
            "q" | "q!" => {
                anyhow::ensure!(remainder.is_empty(), "Expected :q or :q!");
                if command == "q!" {
                    self.shutdown = true;
                } else {
                    self.shutdown();
                }
            }
            _ => anyhow::bail!(
                "Unknown command; use :w [path], :e[!] <path>, :q[!], :wq, :restart, :resize[!] <rows>, :step <rows>, \
                 :length <integer>t/r, :duration <ticks>t, :end[!] <ticks>t"
            ),
        }
        Ok(())
    }

    fn quantize(&mut self, preview: bool, input: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.view == View::Pattern && matches!(self.mode, Mode::Normal | Mode::Visual) && input.is_empty(),
            "Expected :quantize[?] in Pattern Normal/Visual mode"
        );
        let pattern = self.selected_pattern;
        let target = &self.project.patterns[pattern];
        let (first_row, last_row, first_track, last_track) = self.rectangle();
        let range = if self.mode == Mode::Visual {
            target.row_tick(first_row).unwrap()..target.row_tick(last_row + 1).unwrap()
        } else {
            let tick = self.cursor_tick();
            tick..Tick(tick.0 + 1)
        };
        let plan = crate::quantize::Quantization::plan(target, first_track..last_track + 1, range);
        let summary = plan.summary();
        if preview {
            self.status = summary;
            return Ok(());
        }
        let tick = self.project.patterns[pattern]
            .grid()
            .nearest(self.cursor_tick())
            .expect("cursor inside grid");
        let mut result = Ok(0);
        self.edit(|edit| result = plan.apply(edit, pattern));
        let moved = result?;
        if moved > 0 {
            self.stop_playback();
            self.remap_cursor(tick);
            self.finish_selection();
        }
        self.status = summary;
        Ok(())
    }

    fn execute_end_command(&mut self, command: &str, remainder: &str) -> anyhow::Result<()> {
        match command {
            "end" | "end!" => {
                anyhow::ensure!(
                    self.mode == Mode::Normal && matches!(self.view, View::Pattern | View::Arrangement),
                    "End requires Pattern or Arrangement Normal mode"
                );
                let input = remainder
                    .trim()
                    .strip_suffix('t')
                    .ok_or_else(|| anyhow::anyhow!("Expected :end[!] <integer>t (960 PPQ)"))?;
                anyhow::ensure!(
                    !input.is_empty() && input.bytes().all(|byte| byte.is_ascii_digit()),
                    "Expected :end[!] <integer>t"
                );
                let end = Tick(input.parse()?);
                let index = self.selected_pattern;
                let before = self.project.patterns[index].length_ticks();
                let mut result = Ok(());
                self.edit(|edit| result = edit.set_pattern_end(index, end, command == "end!"));
                result?;
                self.status = if before == end {
                    format!("Pattern {index} already ends at {}t", end.0)
                } else {
                    format!("Pattern {index} end: {}t → {}t", before.0, end.0)
                };
            }
            "resize" | "resize!" => {
                anyhow::ensure!(
                    self.mode == Mode::Normal && matches!(self.view, View::Pattern | View::Arrangement),
                    "Resize requires Pattern or Arrangement Normal mode"
                );
                let input = remainder.trim();
                anyhow::ensure!(
                    !input.is_empty() && input.bytes().all(|b| b.is_ascii_digit()),
                    "Expected :resize[!] <rows> (integer)"
                );
                let rows = input
                    .parse::<usize>()
                    .map_err(|_| anyhow::anyhow!("Rows must be 1..={}", crate::project_file::MAX_ROWS))?;
                let index = self.selected_pattern;
                let before_tick = self.project.patterns[index].length_ticks();
                let before = self.project.patterns[index].num_rows();
                let mut result = Ok(());
                self.edit(|edit| result = edit.resize_pattern(index, rows, command == "resize!"));
                result?;
                self.status = if Some(before_tick) == Tick::from_legacy_row(rows as u64).ok() {
                    format!("Pattern {index} already has {rows} rows")
                } else {
                    format!("Pattern {index}: {before} → {rows} rows")
                };
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn execute_time_command(&mut self, command: &str, remainder: &str) -> anyhow::Result<()> {
        match command {
            "meter" | "meter!" | "grid" => self.set_projection(command, remainder.trim())?,
            "next-event" | "prev-event" => {
                anyhow::ensure!(
                    remainder.is_empty() && self.view == View::Pattern,
                    "Event navigation requires Pattern view"
                );
                self.navigate_event(command == "next-event");
            }
            "paste!" => {
                anyhow::ensure!(
                    remainder.is_empty() && self.view == View::Pattern && self.mode == Mode::Normal,
                    "Expected :paste! in Pattern Normal mode"
                );
                self.paste_events(true);
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn set_projection(&mut self, command: &str, input: &str) -> anyhow::Result<()> {
        use crate::musical_time::{Grid, Meter};
        anyhow::ensure!(
            self.view == View::Pattern && self.mode == Mode::Normal,
            "Meter/grid requires Pattern Normal mode"
        );
        let index = self.selected_pattern;
        let tick = self.cursor_tick();
        let mut grid = self.project.patterns[index].grid().clone();
        if command.starts_with("meter") {
            let (n, d) = input
                .split_once('/')
                .ok_or_else(|| anyhow::anyhow!("Expected :meter[!] N/D"))?;
            let meter = Meter::new(n.parse()?, d.parse()?)
                .map_err(|_| anyhow::anyhow!("Meter: numerator 1–32, power-of-two denominator 1–64"))?;
            grid.set_meter(meter, command == "meter!")
                .map_err(|_| anyhow::anyhow!("Meter would clear incompatible bar overrides; use :meter! {input}"))?;
        } else {
            let words = input.split_whitespace().collect::<Vec<_>>();
            anyhow::ensure!(
                (1..=2).contains(&words.len()) && (words.len() == 1 || words[1] == "bar"),
                "Expected :grid <divisions/quarter> [bar]"
            );
            let value = Grid::new(words[0].parse()?)
                .map_err(|_| anyhow::anyhow!("Supported divisions/quarter: 1,2,3,4,5,6,8,12,16,24,32,48,64"))?;
            if words.len() == 2 {
                grid.set_bar_grid(grid.meter().bar_start(tick), value)
                    .map_err(|_| anyhow::anyhow!("Bar lies outside pattern"))?;
            } else {
                grid.set_default(value);
            }
        }
        if self.project.patterns[index].grid() != &grid {
            self.edit(|edit| edit.set_grid(index, grid));
            self.remap_cursor(tick);
            if index == self.playing_pattern {
                self.playback_row = self.project.patterns[index].row_at(self.playback_tick);
            }
        }
        self.status = format!("{}; starts, gates and end unchanged", self.grid_context());
        Ok(())
    }

    #[must_use]
    pub fn cursor_tick(&self) -> Tick {
        let pattern = &self.project.patterns[self.selected_pattern];
        self.inspected_tick
            .filter(|tick| *tick < pattern.length_ticks() && pattern.row_at(*tick) == self.cursor_row)
            .or_else(|| pattern.row_tick(self.cursor_row))
            .unwrap_or(Tick(0))
    }

    fn remap_cursor(&mut self, tick: Tick) {
        let pattern = &self.project.patterns[self.selected_pattern];
        let tick = Tick(tick.0.min(pattern.length_ticks().0.saturating_sub(1)));
        let row = pattern.row_at(tick);
        if self.view == View::Pattern {
            self.cursor_row = row;
        } else {
            self.view_states[View::Pattern as usize].row = row;
        }
        self.inspected_tick = (pattern.row_tick(row) != Some(tick)).then_some(tick);
        self.selection_anchor = None;
        self.view_states[View::Pattern as usize].anchor = None;
    }

    pub(crate) fn grid_context(&self) -> String {
        let pattern = &self.project.patterns[self.selected_pattern];
        let tick = self.cursor_tick();
        let meter = pattern.grid().meter();
        let bar = tick.0 / meter.bar_ticks() + 1;
        let unit = tick.0 % meter.bar_ticks() / meter.unit_ticks() + 1;
        let partial = !pattern.length_ticks().0.is_multiple_of(meter.bar_ticks());
        format!(
            "{}/{} B{bar}.{unit}+{} Grid:{}/q Tick:{}{}",
            meter.numerator(),
            meter.denominator(),
            tick.0 % meter.unit_ticks(),
            pattern
                .grid()
                .grid_at(tick)
                .map_or(4, crate::musical_time::Grid::divisions_per_quarter),
            tick.0,
            if partial { " Partial" } else { "" }
        )
    }

    fn navigate_event(&mut self, forward: bool) {
        let tick = self.cursor_tick();
        let pattern = &self.project.patterns[self.selected_pattern];
        let events = pattern.tracks[self.cursor_track].events();
        let event = if forward {
            events.iter().find(|event| event.start_tick > tick)
        } else {
            events.iter().rev().find(|event| event.start_tick < tick)
        };
        if let Some(event) = event {
            self.remap_cursor(event.start_tick);
        }
        self.status = self.grid_context();
    }

    fn set_entry_setting(&mut self, command: &str, input: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.view == View::Pattern && self.mode == Mode::Normal,
            "Entry settings require Pattern Normal mode"
        );
        if command == "duration" {
            let number = input
                .strip_suffix('t')
                .ok_or_else(|| anyhow::anyhow!("Expected :duration <integer>t (960 ticks per quarter)"))?;
            anyhow::ensure!(
                !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()),
                "Expected integer ticks"
            );
            let ticks: u32 = number.parse()?;
            self.entry_duration = Some(
                crate::musical_time::DurationTicks::new(ticks)
                    .map_err(|_| anyhow::anyhow!("Duration must be 1..=61440 ticks"))?,
            );
            self.status = format!("New note duration: {ticks} PPQ ticks");
        } else if command == "step" {
            let rows = Self::entry_integer(input)?;
            anyhow::ensure!(
                usize::from(rows) <= crate::project_file::MAX_ROWS,
                "Step must be 0..=4096 rows"
            );
            self.edit_step = usize::from(rows);
            self.status = format!("Entry step: {rows} rows");
        } else {
            let (number, multiplier) = if let Some(number) = input.strip_suffix('t') {
                (number, 1)
            } else if let Some(number) = input.strip_suffix('r') {
                (number, crate::tracker::NoteLength::TICKS_PER_ROW)
            } else {
                anyhow::bail!("Expected :length <integer>t or <integer>r (16 ticks per row)");
            };
            let ticks = Self::entry_integer(number)?
                .checked_mul(multiplier)
                .ok_or_else(|| anyhow::anyhow!("Length must be 1..=4096 ticks"))?;
            self.entry_length = crate::tracker::NoteLength::try_from(ticks).map_err(anyhow::Error::msg)?;
            self.entry_duration = None;
            self.status = format!("New note length: {ticks} ticks");
        }
        Ok(())
    }

    /// Whether a cell lies in the inclusive Visual rectangle.
    #[must_use]
    pub fn is_selected(&self, row: usize, track: usize) -> bool {
        self.selection_anchor.is_some_and(|(anchor_row, anchor_track)| {
            (anchor_row.min(self.cursor_row)..=anchor_row.max(self.cursor_row)).contains(&row)
                && (anchor_track.min(self.cursor_track)..=anchor_track.max(self.cursor_track)).contains(&track)
        })
    }

    fn prepare_command(&mut self, command: Command) {
        if matches!(
            command,
            Command::EnterNormal
                | Command::ToggleHelp
                | Command::ShowPattern
                | Command::ShowArrangement
                | Command::ShowMixer
                | Command::ShowInstrument
        ) {
            self.audition = None;
        }
        self.pending_prefix = None;

        if !self.help_visible
            || matches!(
                command,
                Command::ToggleHelp
                    | Command::DismissHelp
                    | Command::ScrollHelpUp
                    | Command::ScrollHelpDown
                    | Command::Quit
            )
        {
            self.status.clear();
        }
    }

    fn show_view(&mut self, command: Command) {
        self.view_states[self.view as usize] = ViewState {
            mode: self.mode,
            row: self.cursor_row,
            track: self.cursor_track,
            anchor: self.selection_anchor,
        };
        self.view = match command {
            Command::ShowArrangement => View::Arrangement,
            Command::ShowMixer => View::Mixer,
            Command::ShowInstrument => View::Instrument,
            _ => View::Pattern,
        };
        let saved = self.view_states[self.view as usize];
        self.mode = saved.mode;
        self.cursor_row = saved.row.min(if self.view == View::Mixer {
            crate::project::CHANNEL_ROWS - 1
        } else {
            self.project.patterns[self.selected_pattern].view_rows() - 1
        });
        self.cursor_track = saved.track.min(self.project.instruments().len() - 1);
        self.selection_anchor = saved.anchor;
        self.pending_count = None;
        self.pending_prefix = None;
        self.suppressed_repeat = true;
    }

    /// Apply an editor action without depending on terminal key events.
    pub fn handle_command(&mut self, command: Command) {
        self.mixer_gesture = None;
        self.prepare_command(command);
        match command {
            Command::ToggleHelp => {
                self.pending_count = None;
                self.help_visible = !self.help_visible;
                self.help_scroll.set(0);
            }
            Command::DismissHelp => {
                self.pending_count = None;
                self.help_visible = false;
            }
            Command::ScrollHelpUp | Command::ScrollHelpDown => self.scroll_help(command == Command::ScrollHelpDown),
            Command::Quit => self.shutdown(),
            Command::CountDigit(_) => {}
            _ if self.help_visible => {}

            Command::ShowPattern | Command::ShowArrangement | Command::ShowMixer | Command::ShowInstrument => {
                self.show_view(command);
            }
            _ if !command.available_in_view(self.view) => {}
            _ if self.view == View::Mixer && self.handle_mixer(command) => {}
            Command::MixerValue
            | Command::MixerReset
            | Command::MixerAdjust(_)
            | Command::MixerMute
            | Command::MixerSolo => {}
            Command::AuditionInstrument => self.request_audition(),
            Command::PreviousPattern
            | Command::NextPattern
            | Command::NewPattern
            | Command::DuplicatePattern
            | Command::PreviousOrder
            | Command::NextOrder
            | Command::AppendOrder
            | Command::RemoveOrder
            | Command::OrderEarlier
            | Command::OrderLater
            | Command::ToggleSong
            | Command::Undo
            | Command::Redo => self.handle_arrangement(command),
            Command::AddTrack => self.add_track(),
            Command::OpenPrompt => self.open_prompt(),
            Command::EnterInsert => self.enter_insert(),
            Command::EnterAudition => self.enter_audition(),
            Command::EnterNormal => {
                self.mode = Mode::Normal;
                self.selection_anchor = None;
                self.pending_count = None;
            }
            Command::ToggleVisual => {
                if self.mode == Mode::Visual {
                    self.mode = Mode::Normal;
                    self.selection_anchor = None;
                } else {
                    self.mode = Mode::Visual;
                    self.selection_anchor = Some((self.cursor_row, self.cursor_track));
                }
            }
            Command::MoveCursor { rows, tracks } => {
                self.offset_cursor(&CursorOffset::from_delta(rows), &CursorOffset::from_delta(tracks));
            }
            Command::FirstTrack | Command::FirstRow | Command::LastRow | Command::LastTrack => self.jump(command),
            Command::TogglePlayback => self.toggle_playback(),
            Command::RestartPlayback => self.restart_playback(),
            Command::MoveCursorUp => self.offset_cursor(&CursorOffset::Neg(1), &CursorOffset::None),
            Command::MoveCursorDown => self.offset_cursor(&CursorOffset::Pos(1), &CursorOffset::None),
            Command::MoveCursorLeft => self.offset_cursor(&CursorOffset::None, &CursorOffset::Neg(1)),
            Command::MoveCursorRight => self.offset_cursor(&CursorOffset::None, &CursorOffset::Pos(1)),
            Command::InsertNote { pitch, octave_offset } => self.enter_pitch(pitch, octave_offset),
            Command::HalveLength | Command::DoubleLength => self.adjust_length(command == Command::DoubleLength),
            Command::CycleEditStep => self.cycle_edit_step(),
            Command::Copy | Command::Cut => self.copy_or_cut(command == Command::Cut),
            Command::Paste => self.paste(),
            Command::SetInstrument(instrument) => {
                let track = self.cursor_track;
                self.edit(|edit| edit.set_instrument(track, instrument));
            }
            Command::PreviousInstrument | Command::NextInstrument => {
                let instrument = self
                    .project
                    .instrument(self.cursor_track)
                    .cycle(command == Command::NextInstrument);
                let track = self.cursor_track;
                self.edit(|edit| edit.set_instrument(track, instrument));
            }
            Command::DecreaseTempo | Command::IncreaseTempo => {
                let bpm =
                    (self.project.bpm + if command == Command::IncreaseTempo { 1.0 } else { -1.0 }).clamp(20.0, 300.0);
                self.edit(|edit| edit.set_tempo(bpm));
            }
            Command::LowerNote | Command::RaiseNote => {
                self.transpose_notes(if command == Command::RaiseNote { 1 } else { -1 });
            }
            Command::LowerOctave | Command::RaiseOctave => {
                self.transpose_notes(if command == Command::RaiseOctave { 12 } else { -12 });
            }
            Command::DeleteNote => self.clear_selection(),
            Command::DecreaseOctave => self.octave = self.octave.saturating_sub(1).min(9),
            Command::IncreaseOctave => self.octave = self.octave.saturating_add(1).min(9),
        }
    }

    fn handle_mixer(&mut self, command: Command) -> bool {
        match command {
            Command::MixerMute | Command::MixerSolo => {
                if self.mode == Mode::Normal {
                    let track = self.cursor_track;
                    let mut setting = self.project.channels()[track];
                    if command == Command::MixerMute {
                        setting.muted = !setting.muted;
                    } else {
                        setting.solo = !setting.solo;
                    }
                    self.edit(|edit| edit.set_channel(track, setting));
                }
            }
            Command::MixerValue => {
                self.prompt = Some(String::new());
            }
            Command::MixerReset => self.edit_mixer_rectangle(None),
            Command::MixerAdjust(steps) if self.mode == Mode::Visual => self.edit_mixer_rectangle(Some(steps)),
            Command::LowerOctave | Command::RaiseOctave => {
                if self.mode == Mode::Visual {
                    self.edit_mixer_rectangle(Some(if command == Command::RaiseOctave { 1 } else { -1 }));
                }
            }
            Command::MoveCursor { rows, tracks } if self.mode == Mode::Insert => {
                if tracks != 0 {
                    let track = self.cursor_track;
                    let setting = self.project.channels()[track].adjusted(self.cursor_row, tracks);
                    self.edit(|edit| edit.set_channel(track, setting));
                }
                self.offset_cursor(&CursorOffset::from_delta(rows), &CursorOffset::None);
            }
            Command::MoveCursorLeft | Command::MoveCursorRight if self.mode == Mode::Insert => {
                return self.handle_mixer(Command::MoveCursor {
                    rows: 0,
                    tracks: if command == Command::MoveCursorLeft { -1 } else { 1 },
                });
            }
            _ => return false,
        }
        true
    }

    fn edit_mixer_rectangle(&mut self, steps: Option<i32>) {
        let (first_row, last_row, first_track, last_track) = self.rectangle();
        let values: Vec<_> = (first_track..=last_track)
            .map(|track| {
                let mut setting = self.project.channels()[track];
                for row in first_row..=last_row {
                    if let Some(steps) = steps {
                        setting = setting.adjusted(row, steps);
                    } else {
                        match row {
                            0 => setting.volume_db = Some(0.0),
                            1 => setting.pan = 0.0,
                            2 => setting.low_shelf_db = 0.0,
                            3 => setting.high_shelf_db = 0.0,
                            4 => setting.hpf_hz = None,
                            _ => setting.lpf_hz = None,
                        }
                    }
                }
                (track, setting)
            })
            .collect();
        self.edit(|edit| {
            for (track, setting) in values {
                edit.set_channel(track, setting);
            }
        });
    }

    fn enter_mixer_value(&mut self, text: &str) -> anyhow::Result<()> {
        let track = self.cursor_track;
        let mut setting = self.project.channels()[track];
        let text = text.trim();
        if self.cursor_row == 0 && text == "-inf" {
            setting.volume_db = None;
        } else if self.cursor_row >= 4 && text == "off" {
            if self.cursor_row == 4 {
                setting.hpf_hz = None;
            } else {
                setting.lpf_hz = None;
            }
        } else {
            let value: f32 = text
                .parse()
                .map_err(|_| anyhow::anyhow!("Expected a number, -inf for volume, or off for filters"))?;
            match self.cursor_row {
                0 => setting.volume_db = Some(value),
                1 => setting.pan = value,
                2 => setting.low_shelf_db = value,
                3 => setting.high_shelf_db = value,
                4 => setting.hpf_hz = Some(value),
                _ => setting.lpf_hz = Some(value),
            }
        }
        anyhow::ensure!(
            setting.is_valid(),
            "Expected finite volume -60..+6 dB/-inf, pan -100..100, shelf -12..12 dB, or cutoff 20..20000 Hz/off"
        );
        self.edit(|edit| edit.set_channel(track, setting));
        Ok(())
    }

    const fn enter_audition(&mut self) {
        self.mode = Mode::Audition;
        self.pending_count = None;
        self.pending_prefix = None;
        self.suppressed_repeat = true;
        self.audition = None;
    }

    const fn enter_insert(&mut self) {
        self.mode = Mode::Insert;
        self.pending_count = None;
        self.suppressed_repeat = true;
        self.selection_anchor = None;
    }

    fn jump(&mut self, command: Command) {
        self.inspected_tick = None;
        let pattern = &self.project.patterns[self.selected_pattern];
        match command {
            Command::FirstTrack => self.cursor_track = 0,
            Command::FirstRow => self.cursor_row = 0,
            Command::LastRow => {
                self.cursor_row = if self.view == View::Mixer {
                    crate::project::CHANNEL_ROWS - 1
                } else {
                    pattern.view_rows().saturating_sub(1)
                }
            }
            Command::LastTrack => self.cursor_track = pattern.tracks.len().saturating_sub(1),
            _ => unreachable!(),
        }
    }

    fn add_track(&mut self) {
        if self.project.can_append_track() {
            self.edit(|edit| {
                edit.append_track();
            });
            self.cursor_track = self.project.instruments().len() - 1;
        } else {
            self.status = "Track limit reached (128 tracks)".into();
        }
    }

    fn transpose_notes(&mut self, semitones: i8) {
        let (first_row, last_row, first_track, last_track) = self.rectangle();
        let pattern = self.selected_pattern;
        let source = &self.project.patterns[pattern];
        let start = if self.mode == Mode::Visual {
            source.row_tick(first_row).unwrap()
        } else {
            self.cursor_tick()
        };
        let end = if self.mode == Mode::Visual {
            source.row_tick(last_row + 1).unwrap()
        } else {
            Tick(start.0 + 1)
        };
        let mut notes = Vec::new();
        for (track, values) in source.tracks.iter().enumerate().take(last_track + 1).skip(first_track) {
            for event in values.events_in(start..end) {
                let note = event.transposed(semitones);
                notes.push((
                    track,
                    crate::tracker::PatternEvent {
                        pitch: note.pitch,
                        octave: note.octave,
                        ..*event
                    },
                ));
            }
        }
        if !notes.is_empty() {
            self.edit(|edit| {
                for (track, event) in notes {
                    edit.set_event(pattern, track, event.start_tick, Some(event));
                }
            });
        }
    }

    fn scroll_help(&self, down: bool) {
        if self.help_visible {
            let current = self.help_scroll.get().min(self.help_scroll_max.get());
            self.help_scroll.set(if down {
                current.saturating_add(1).min(self.help_scroll_max.get())
            } else {
                current.saturating_sub(1)
            });
        }
    }

    fn handle_arrangement(&mut self, command: Command) {
        match command {
            Command::PreviousPattern => self.switch_pattern(self.selected_pattern.saturating_sub(1)),
            Command::NextPattern => {
                self.switch_pattern((self.selected_pattern + 1).min(self.project.patterns.len() - 1));
            }
            Command::NewPattern | Command::DuplicatePattern => {
                let source = self.selected_pattern;
                let count = self.project.patterns.len();
                self.edit(|edit| edit.append_pattern(source, command == Command::DuplicatePattern));
                if self.project.patterns.len() > count {
                    self.switch_pattern(count);
                } else {
                    self.status = "Pattern/cell limit reached".into();
                }
            }
            Command::PreviousOrder => self.selected_order = self.selected_order.saturating_sub(1),
            Command::NextOrder => self.selected_order = (self.selected_order + 1).min(self.project.order.len() - 1),
            Command::AppendOrder | Command::RemoveOrder | Command::OrderEarlier | Command::OrderLater => {
                self.arrange(command);
            }
            Command::ToggleSong => {
                self.stop_playback();
                self.playback_mode = if self.playback_mode == PlaybackMode::Song {
                    PlaybackMode::Pattern
                } else {
                    PlaybackMode::Song
                };
            }
            Command::Undo | Command::Redo => {
                let redo = command == Command::Redo;
                let tick = self.cursor_tick();
                let grid = self.project.patterns[self.selected_pattern].grid().clone();
                let structural = self.history.structural_next(redo);
                if structural {
                    self.stop_playback();
                }
                if redo {
                    self.history.redo(&mut self.project);
                } else {
                    self.history.undo(&mut self.project);
                }
                if self
                    .project
                    .patterns
                    .get(self.selected_pattern)
                    .is_some_and(|pattern| pattern.grid() != &grid)
                {
                    self.remap_cursor(tick);
                }
                self.selected_pattern = self.selected_pattern.min(self.project.patterns.len() - 1);
                self.offset_cursor(&CursorOffset::None, &CursorOffset::None);
                if structural {
                    self.invalidate_view_selections();
                    self.selected_order = self.selected_order.min(self.project.order.len() - 1);
                    self.playing_pattern = self.playing_pattern.min(self.project.patterns.len() - 1);
                    self.playing_order = self.playing_order.min(self.project.order.len() - 1);
                    self.finish_selection();
                    self.switch_pattern(self.selected_pattern.min(self.project.patterns.len() - 1));
                }
            }
            _ => unreachable!("arrangement command"),
        }
    }

    fn switch_pattern(&mut self, index: usize) {
        let pattern_state = &mut self.view_states[View::Pattern as usize];
        pattern_state.anchor = None;
        if pattern_state.mode == Mode::Visual {
            pattern_state.mode = Mode::Normal;
        }
        self.inspected_tick = None;
        self.selected_pattern = index;
        self.selection_anchor = None;
        self.pending_count = None;
        self.offset_cursor(&CursorOffset::None, &CursorOffset::None);
    }

    const fn stop_playback(&mut self) {
        self.audition = None;
        self.audition_active = false;
        self.playback_state = PlaybackState::Stopped;
        self.stop_requested = true;
    }

    fn arrange(&mut self, command: Command) {
        let index = self.selected_order;
        // Reject no-ops and limits before allocating an order snapshot or stopping.
        match command {
            Command::AppendOrder if self.project.order.len() >= crate::project_file::MAX_ORDER => {
                self.status = "Order limit reached".into();
                return;
            }
            Command::RemoveOrder if self.project.order.len() == 1 => return,
            Command::OrderEarlier if index == 0 || self.project.order[index] == self.project.order[index - 1] => return,
            Command::OrderLater
                if index + 1 == self.project.order.len()
                    || self.project.order[index] == self.project.order[index + 1] =>
            {
                return
            }
            _ => {}
        }
        self.stop_playback();
        let mut order = self.project.order.clone();
        match command {
            Command::AppendOrder => {
                order.push(self.selected_pattern);
                self.selected_order = order.len() - 1;
            }
            Command::RemoveOrder => {
                order.remove(index);
                self.selected_order = index.min(order.len() - 1);
            }
            Command::OrderEarlier => {
                order.swap(index, index - 1);
                self.selected_order -= 1;
            }
            Command::OrderLater => {
                order.swap(index, index + 1);
                self.selected_order += 1;
            }
            _ => unreachable!(),
        }
        self.edit(|edit| edit.set_order(order));
    }

    fn entry_integer(input: &str) -> anyhow::Result<u16> {
        anyhow::ensure!(
            !input.is_empty() && input.bytes().all(|byte| byte.is_ascii_digit()),
            "Expected an unsigned decimal integer"
        );
        input.parse().map_err(|_| anyhow::anyhow!("Entry value is too large"))
    }

    fn open_prompt(&mut self) {
        self.pending_count = None;
        self.prompt = Some(String::new());
        self.status.clear();
    }

    const fn cycle_edit_step(&mut self) {
        self.edit_step = match self.edit_step {
            0 => 1,
            1 => 2,
            2 => 4,
            _ => 0,
        };
    }

    fn adjust_length(&mut self, double: bool) {
        if let Some(mut event) =
            self.project.patterns[self.selected_pattern].event_at(self.cursor_track, self.cursor_tick())
        {
            let ticks = event.row_note().map_or_else(
                || {
                    if double {
                        event.duration_ticks.ticks().saturating_mul(2).min(61440)
                    } else {
                        (event.duration_ticks.ticks() / 2).max(1)
                    }
                },
                |note| u32::from(note.length_ticks.adjusted(double).ticks()) * 15,
            );
            event.duration_ticks = crate::musical_time::DurationTicks::new(ticks).expect("bounded duration");
            let pattern = self.selected_pattern;
            let track = self.cursor_track;
            self.edit(|edit| edit.set_event(pattern, track, event.start_tick, Some(event)));
        }
    }

    fn enter_pitch(&mut self, pitch: crate::tracker::NotePitch, octave_offset: u8) {
        if let Some(octave) = self.octave.checked_add(octave_offset).filter(|octave| *octave <= 9) {
            let note = Note {
                length_ticks: self.entry_length,
                pitch,
                octave,
            };
            if self.mode == Mode::Audition {
                self.preview_note(note);
            } else {
                self.enter_note(note);
            }
        }
    }

    fn preview_note(&mut self, note: Note) {
        self.audition_active = true;
        self.audition = Some(crate::transport::NoteEvent {
            track: self.cursor_track,
            instrument: self.project.instrument(self.cursor_track),
            note,
            beats: 0.0,
            duration: crate::audio::DEFAULT_NOTE_DURATION,
        });
    }

    fn enter_note(&mut self, note: Note) {
        let pattern = self.selected_pattern;
        let track = self.cursor_track;
        let tick = self.cursor_tick();
        if self.cursor_row < self.project.patterns[pattern].view_rows()
            && track < self.project.patterns[pattern].tracks.len()
        {
            let mut event = crate::tracker::PatternEvent::from_note(tick, note);
            event.duration_ticks = self.project.patterns[pattern]
                .event_at(track, tick)
                .map(|previous| previous.duration_ticks)
                .or(self.entry_duration)
                .unwrap_or(event.duration_ticks);
            let mut result = Ok(());
            self.edit(|edit| result = edit.try_set_event(pattern, track, tick, Some(event)));
            if let Err(error) = result {
                self.status = error.to_string();
                return;
            }
            if self.mode == Mode::Insert && self.view == View::Pattern {
                self.preview_note(note);
            }
            if self.mode == Mode::Insert {
                self.offset_cursor(&CursorOffset::Pos(self.edit_step), &CursorOffset::None);
            }
        }
    }

    fn copy_or_cut(&mut self, cut: bool) {
        self.copy_rectangle();
        if cut {
            self.clear_rectangle();
        }
        self.finish_selection();
    }

    fn rectangle(&self) -> (usize, usize, usize, usize) {
        let (row, track) = if self.mode == Mode::Visual {
            self.selection_anchor.unwrap_or((self.cursor_row, self.cursor_track))
        } else {
            (self.cursor_row, self.cursor_track)
        };
        (
            row.min(self.cursor_row),
            row.max(self.cursor_row).min(if self.view == View::Mixer {
                crate::project::CHANNEL_ROWS - 1
            } else {
                self.project.patterns[self.selected_pattern]
                    .view_rows()
                    .saturating_sub(1)
            }),
            track.min(self.cursor_track),
            track.max(self.cursor_track).min(
                self.project.patterns[self.selected_pattern]
                    .tracks
                    .len()
                    .saturating_sub(1),
            ),
        )
    }

    fn copy_rectangle(&mut self) {
        let (first_row, last_row, first_track, last_track) = self.rectangle();
        let pattern = &self.project.patterns[self.selected_pattern];
        let start = if self.mode == Mode::Visual {
            pattern.row_tick(first_row).unwrap()
        } else {
            self.cursor_tick()
        };
        let end = pattern.row_tick(last_row + 1).unwrap();
        self.clipboard = EventClipboard::capture(
            &self.project.patterns[self.selected_pattern],
            first_track..last_track + 1,
            start..end,
        );
        if self.mode != Mode::Visual {
            for track in &mut self.clipboard.tracks {
                track.retain(|event| event.start_tick == Tick(0));
            }
        }
        let count = self.clipboard.tracks.iter().map(Vec::len).sum::<usize>();
        self.status = format!("Copied {count} events, {} PPQ ticks", self.clipboard.span_ticks);
    }

    fn clear_rectangle(&mut self) {
        let (first_row, last_row, first_track, last_track) = self.rectangle();
        let pattern = self.selected_pattern;
        let source = &self.project.patterns[pattern];
        if self.mode != Mode::Visual {
            let tick = self.cursor_tick();
            self.edit(|edit| edit.set_event(pattern, first_track, tick, None));
            return;
        }
        let start = source.row_tick(first_row).unwrap();
        let end = source.row_tick(last_row + 1).unwrap();
        let count = source.tracks[first_track..=last_track]
            .iter()
            .map(|track| track.events_in(start..end).len())
            .sum::<usize>();
        self.edit(|edit| {
            for track in first_track..=last_track {
                edit.clear_range(pattern, track, start..end);
            }
        });
        self.status = format!(
            "Cleared {count} events (including hidden) in ticks {}..{}",
            start.0, end.0
        );
    }

    fn clear_selection(&mut self) {
        self.clear_rectangle();
        self.finish_selection();
    }

    fn invalidate_view_selections(&mut self) {
        let pattern = &self.project.patterns[self.selected_pattern.min(self.project.patterns.len() - 1)];
        for (index, saved) in self.view_states.iter_mut().enumerate() {
            saved.row = saved.row.min(if index == View::Mixer as usize {
                crate::project::CHANNEL_ROWS - 1
            } else {
                pattern.view_rows() - 1
            });
            saved.track = saved.track.min(pattern.tracks.len() - 1);
            saved.anchor = None;
            if saved.mode == Mode::Visual {
                saved.mode = Mode::Normal;
            }
        }
    }

    fn finish_selection(&mut self) {
        if self.mode == Mode::Visual {
            self.mode = Mode::Normal;
        }
        self.selection_anchor = None;
    }

    fn paste(&mut self) {
        self.paste_events(false);
    }

    fn paste_events(&mut self, force: bool) {
        let track = self.cursor_track;
        let start = self.cursor_tick();
        let target = &self.project.patterns[self.selected_pattern];
        let end = Tick(
            start
                .0
                .saturating_add(self.clipboard.span_ticks)
                .min(target.length_ticks().0),
        );
        let hidden = target
            .tracks
            .iter()
            .skip(track)
            .take(self.clipboard.tracks.len())
            .flat_map(|track| track.events_in(start..end))
            .filter(|event| {
                event.start_tick != start
                    && target
                        .grid()
                        .neighbors(event.start_tick)
                        .is_ok_and(|(lo, _)| lo != event.start_tick)
            })
            .count();
        if hidden > 0 && !force {
            self.status = format!("Paste would replace {hidden} hidden events; use :paste! to confirm");
            return;
        }
        let clipboard = self.clipboard.clone();
        let pattern = self.selected_pattern;
        let mut result = Ok(());
        self.edit(|edit| result = clipboard.paste(edit, pattern, track, start));
        if let Err(error) = result {
            self.status = error.to_string();
        }
    }

    pub fn offset_cursor(&mut self, dr: &CursorOffset, dt: &CursorOffset) {
        if !matches!(dr, CursorOffset::None | CursorOffset::Pos(0) | CursorOffset::Neg(0))
            || !matches!(dt, CursorOffset::None | CursorOffset::Pos(0) | CursorOffset::Neg(0))
        {
            self.inspected_tick = None;
        }
        let max_rows = if self.view == View::Mixer {
            crate::project::CHANNEL_ROWS
        } else {
            self.project.patterns[self.selected_pattern].view_rows()
        };
        let max_tracks = self.project.patterns[self.selected_pattern].tracks.len();

        self.cursor_row = dr.apply_to(self.cursor_row).clamp(0, max_rows.saturating_sub(1));
        self.cursor_track = dt.apply_to(self.cursor_track).clamp(0, max_tracks.saturating_sub(1));
    }

    pub fn shutdown(&mut self) {
        if self.is_dirty() {
            self.status = "Unsaved changes; use :w or :q!".into();
        } else {
            self.shutdown = true;
        }
    }

    pub fn toggle_playback(&mut self) {
        match self.playback_state {
            PlaybackState::Stopped => {
                self.begin_playback(false);
            }
            PlaybackState::Playing => {
                self.stop_playback();
            }
        }
    }

    pub fn restart_playback(&mut self) {
        self.stop_playback();
        self.begin_playback(true);
    }

    fn begin_playback(&mut self, beginning: bool) {
        self.audition = None;
        self.stop_requested |= std::mem::take(&mut self.audition_active);
        self.playing_order = if beginning || self.playback_mode == PlaybackMode::Pattern {
            0
        } else {
            self.selected_order
        }
        .min(self.project.order.len().saturating_sub(1));
        self.playing_pattern = if self.playback_mode == PlaybackMode::Song {
            let Some(&pattern) = self.project.order.get(self.playing_order) else {
                return;
            };
            pattern
        } else {
            self.selected_pattern
        };
        let Some(pattern) = self.project.patterns.get(self.playing_pattern) else {
            return;
        };
        let row = if self.view == View::Pattern {
            self.cursor_row
        } else {
            self.view_states[View::Pattern as usize].row
        };
        self.playback_row = if beginning { 0 } else { row }.min(pattern.view_rows().saturating_sub(1));
        self.playback_start_tick = Some(if beginning {
            Tick(0)
        } else if self.view == View::Pattern && self.playing_pattern == self.selected_pattern {
            self.cursor_tick()
        } else {
            pattern.row_tick(self.playback_row).unwrap_or(Tick(0))
        });
        self.playback_tick = self.playback_start_tick.unwrap_or(Tick(0));
        self.playback_state = PlaybackState::Playing;
    }

    /// Apply one document transaction, including future multi-cell operations.
    pub fn edit(&mut self, operation: impl FnOnce(&mut Edit<'_>)) {
        if self.history.edit(&mut self.project, operation) {
            self.invalidate_view_selections();
            self.stop_playback();
            self.selected_order = self.selected_order.min(self.project.order.len() - 1);
            self.finish_selection();
            self.pending_count = None;
            self.offset_cursor(&CursorOffset::None, &CursorOffset::None);
        }
    }

    /// Stable identity for future saved-revision comparisons, even after eviction.
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.history.revision()
    }

    /// Replace the note at the cursor; invalid positions are ignored.
    pub fn insert_note_under_cursor(&mut self, note: Option<Note>) {
        if self.cursor_row >= self.project.patterns[self.selected_pattern].view_rows() {
            return;
        }
        let track = self.cursor_track;
        let tick = self.cursor_tick();
        let pattern = self.selected_pattern;
        self.edit(|edit| {
            edit.set_event(
                pattern,
                track,
                tick,
                note.map(|note| crate::tracker::PatternEvent::from_note(tick, note)),
            );
        });
    }

    /// Legacy standalone editor helper; the runtime uses `Transport` instead.
    pub fn advance_playback_row(&mut self) {
        if matches!(self.playback_state, PlaybackState::Playing)
            && self.project.patterns[self.playing_pattern].view_rows() > 0
        {
            self.playback_row = (self.playback_row + 1) % self.project.patterns[self.playing_pattern].view_rows();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::NotePitch;

    #[test]
    fn instruments_wrap_isolate_selected_track_and_obey_help() {
        use crate::tracker::Instrument;
        let mut app = App {
            cursor_track: 2,
            cursor_row: 7,
            playback_row: 3,
            playback_state: PlaybackState::Playing,
            ..App::default()
        };
        app.handle_command(Command::PreviousInstrument);
        assert_eq!(app.project.instrument(2), Instrument::OpenHat);
        app.handle_command(Command::NextInstrument);
        assert_eq!(app.project.instrument(2), Instrument::Pluck);
        for expected in Instrument::ALL
            .into_iter()
            .cycle()
            .skip(1)
            .take(Instrument::ALL.len() * 2)
        {
            app.handle_command(Command::NextInstrument);
            assert_eq!(app.project.instrument(2), expected);
            assert_eq!(app.project.instrument(0), Instrument::Pluck);
            assert_eq!((app.cursor_track, app.cursor_row, app.playback_row), (2, 7, 3));
            assert!(matches!(app.playback_state, PlaybackState::Playing));
        }
        app.handle_command(Command::SetInstrument(Instrument::Snare));
        app.handle_command(Command::ToggleHelp);
        for command in [
            Command::PreviousInstrument,
            Command::NextInstrument,
            Command::SetInstrument(Instrument::Sine),
        ] {
            app.handle_command(command);
            assert_eq!(app.project.instrument(2), Instrument::Snare);
        }
        app.project.instruments.push(Instrument::Pluck);
        assert_eq!(app.project.instrument(4), Instrument::Pluck);
        app.project.set_instrument(4, Instrument::Kick);
        assert_eq!(app.project.instrument(4), Instrument::Kick);
        app.project.set_instrument(usize::MAX, Instrument::Kick);
        assert_eq!(app.project.instrument(usize::MAX), Instrument::Pluck);
    }

    #[test]
    fn tempo_bounds_preserve_editor_and_playback_state() {
        for playing in [false, true] {
            let mut app = App::default();
            if playing {
                app.handle_command(Command::TogglePlayback);
            }
            app.playback_row = 17;
            app.cursor_row = 5;
            for _ in 0..400 {
                app.handle_command(Command::DecreaseTempo);
            }
            assert_eq!(app.project.bpm, 20.0);
            app.handle_command(Command::IncreaseTempo);
            assert_eq!(app.project.bpm, 21.0);
            for _ in 0..400 {
                app.handle_command(Command::IncreaseTempo);
            }
            assert_eq!(app.project.bpm, 300.0);
            app.handle_command(Command::DecreaseTempo);
            assert_eq!(app.project.bpm, 299.0);
            assert_eq!(matches!(app.playback_state, PlaybackState::Playing), playing);
            assert_eq!(
                (app.playback_row, app.cursor_row, app.cursor_track, app.octave),
                (17, 5, 0, 4)
            );
        }
    }

    #[test]
    fn semitone_commands_cover_every_pitch_octave_and_limit() {
        use NotePitch::{As, Cs, Ds, Fs, Gs, A, B, C, D, E, F, G};
        let pitches = [C, Cs, D, Ds, E, F, Fs, G, Gs, A, As, B];
        let mut app = App {
            mode: Mode::Insert,
            cursor_row: 3,
            cursor_track: 2,
            ..App::default()
        };
        for command in [Command::LowerNote, Command::RaiseNote] {
            app.handle_command(command);
            assert_eq!(
                app.project.patterns[app.selected_pattern].get_step(2, 3).unwrap().note,
                None
            );
            for index in 0..120_usize {
                let note = Note {
                    length_ticks: crate::tracker::NoteLength::default(),
                    pitch: pitches[index % 12],
                    octave: u8::try_from(index / 12).unwrap(),
                };
                app.insert_note_under_cursor(Some(note));
                app.handle_command(command);
                let expected = if command == Command::RaiseNote {
                    (index + 1).min(119)
                } else {
                    index.saturating_sub(1)
                };
                assert_eq!(
                    app.project.patterns[app.selected_pattern].get_step(2, 3).unwrap().note,
                    Some(Note {
                        length_ticks: crate::tracker::NoteLength::default(),
                        pitch: pitches[expected % 12],
                        octave: u8::try_from(expected / 12).unwrap()
                    })
                );
                assert_eq!(app.octave, 4);
                assert_eq!(
                    app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note,
                    None
                );
            }
            app.insert_note_under_cursor(None);
        }
    }

    #[test]
    fn help_is_modal_bounded_and_does_not_pause_playback() {
        let mut app = App {
            mode: Mode::Insert,
            ..App::default()
        };
        app.handle_command(Command::DismissHelp);
        assert!(!app.shutdown);
        app.handle_command(Command::TogglePlayback);
        app.handle_command(Command::InsertNote {
            pitch: NotePitch::C,
            octave_offset: 0,
        });
        let note = app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note;
        app.handle_command(Command::ToggleHelp);
        for binding in crate::bindings::DEFAULT_BINDINGS {
            if !matches!(
                binding.command,
                Command::Quit
                    | Command::ToggleHelp
                    | Command::DismissHelp
                    | Command::ScrollHelpUp
                    | Command::ScrollHelpDown
            ) {
                app.handle_command(binding.command);
                assert_eq!(app.project.bpm, 120.0, "{:?}", binding.command);
                assert_eq!((app.cursor_row, app.cursor_track, app.octave), (0, 0, 4));
                assert_eq!(
                    app.project.patterns[app.selected_pattern].get_step(0, 0).unwrap().note,
                    note
                );
                assert!(matches!(app.playback_state, PlaybackState::Playing));
            }
        }
        app.advance_playback_row();
        assert_eq!(app.playback_row, 1);
        for _ in 0..100 {
            app.handle_command(Command::ScrollHelpDown);
        }
        assert_eq!(app.help_scroll.get(), app.help_scroll_max.get());
        for _ in 0..100 {
            app.handle_command(Command::ScrollHelpUp);
        }
        assert_eq!(app.help_scroll.get(), 0);
        app.handle_command(Command::ToggleHelp);
        assert!(!app.help_visible);
        app.handle_command(Command::ToggleHelp);
        app.handle_command(Command::DismissHelp);
        assert!(!app.help_visible);
        app.handle_command(Command::ToggleHelp);
        app.handle_command(Command::Quit);
        assert!(!app.shutdown);
        assert!(app.status.contains("Unsaved changes"));
    }

    #[test]
    fn commands_move_edit_and_delete_at_the_selected_cell() {
        let mut app = App {
            mode: Mode::Insert,
            ..App::default()
        };
        app.handle_command(Command::MoveCursorDown);
        app.handle_command(Command::MoveCursorRight);
        app.handle_command(Command::InsertNote {
            pitch: NotePitch::D,
            octave_offset: 1,
        });
        assert_eq!(
            app.project.patterns[app.selected_pattern].get_step(1, 1).unwrap().note,
            Some(Note {
                length_ticks: crate::tracker::NoteLength::default(),
                pitch: NotePitch::D,
                octave: 5
            })
        );
        app.handle_command(Command::DeleteNote);
        assert_eq!(
            app.project.patterns[app.selected_pattern].get_step(1, 1).unwrap().note,
            None
        );
        app.handle_command(Command::MoveCursorUp);
        app.handle_command(Command::MoveCursorLeft);
        app.handle_command(Command::MoveCursorUp);
        app.handle_command(Command::MoveCursorLeft);
        assert_eq!((app.cursor_row, app.cursor_track), (0, 0));
        for _ in 0..100 {
            app.handle_command(Command::MoveCursorDown);
            app.handle_command(Command::MoveCursorRight);
        }
        assert_eq!((app.cursor_row, app.cursor_track), (63, 3));
    }

    #[test]
    fn octave_commands_and_upper_c_stay_in_range() {
        let mut app = App {
            mode: Mode::Insert,
            ..App::default()
        };
        for _ in 0..20 {
            app.handle_command(Command::IncreaseOctave);
        }
        assert_eq!(app.octave, 9);
        app.handle_command(Command::InsertNote {
            pitch: NotePitch::C,
            octave_offset: 1,
        });
        assert!(app.project.patterns[app.selected_pattern]
            .get_step(0, 0)
            .unwrap()
            .note
            .is_none());
        for _ in 0..20 {
            app.handle_command(Command::DecreaseOctave);
        }
        assert_eq!(app.octave, 0);
    }

    #[test]
    fn playback_commands_start_at_cursor_restart_at_zero_and_quit() {
        let mut app = App::default();
        app.handle_command(Command::TogglePlayback);
        assert!(matches!(app.playback_state, PlaybackState::Playing));
        app.advance_playback_row();
        app.handle_command(Command::TogglePlayback);
        assert!(matches!(app.playback_state, PlaybackState::Stopped));
        app.cursor_row = 7;
        app.handle_command(Command::TogglePlayback);
        assert_eq!(app.playback_row, 7);
        app.handle_command(Command::RestartPlayback);
        assert_eq!(app.playback_row, 0);
        app.handle_command(Command::Quit);
        assert!(app.shutdown);
    }
}
