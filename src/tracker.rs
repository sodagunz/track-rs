use anyhow::anyhow;
use fundsp::math::midi_hz;
use serde::{Deserialize, Serialize};

/// Built-in voices. Percussion ignores note pitch and uses fixed drum tuning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Instrument {
    #[default]
    Pluck,
    Sine,
    Square,
    Saw,
    Kick,
    Snare,
    HiHat,
    Pad,
    Drone,
    Sustain,
    OpenHat,
}

impl Instrument {
    pub const ALL: [Self; 11] = [
        Self::Pluck,
        Self::Sine,
        Self::Square,
        Self::Saw,
        Self::Kick,
        Self::Snare,
        Self::HiHat,
        Self::Pad,
        Self::Drone,
        Self::Sustain,
        Self::OpenHat,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pluck => "Pluck",
            Self::Sine => "Sine",
            Self::Square => "Square",
            Self::Saw => "Saw",
            Self::Kick => "Kick",
            Self::Snare => "Snare",
            Self::HiHat => "HiHat",
            Self::Pad => "Pad",
            Self::Drone => "Drone",
            Self::Sustain => "Sustain",
            Self::OpenHat => "OpenHat",
        }
    }

    #[must_use]
    pub fn cycle(self, forward: bool) -> Self {
        let index = Self::ALL.iter().position(|instrument| *instrument == self).unwrap_or(0);
        Self::ALL[(index + if forward { 1 } else { Self::ALL.len() - 1 }) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotePitch {
    C,
    #[serde(rename = "C#")]
    Cs,
    D,
    #[serde(rename = "D#")]
    Ds,
    E,
    F,
    #[serde(rename = "F#")]
    Fs,
    G,
    #[serde(rename = "G#")]
    Gs,
    A,
    #[serde(rename = "A#")]
    As,
    B,
}

impl NotePitch {
    const fn semitone(self) -> i32 {
        match self {
            Self::C => 0,
            Self::Cs => 1,
            Self::D => 2,
            Self::Ds => 3,
            Self::E => 4,
            Self::F => 5,
            Self::Fs => 6,
            Self::G => 7,
            Self::Gs => 8,
            Self::A => 9,
            Self::As => 10,
            Self::B => 11,
        }
    }
}

impl std::fmt::Display for NotePitch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::C => "C",
            Self::Cs => "C#",
            Self::D => "D",
            Self::Ds => "D#",
            Self::E => "E",
            Self::F => "F",
            Self::Fs => "F#",
            Self::G => "G",
            Self::Gs => "G#",
            Self::A => "A",
            Self::As => "A#",
            Self::B => "B",
        })
    }
}

/// Validated musical gate: sixteen ticks per row, one through 4096 ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct NoteLength(u16);

impl NoteLength {
    pub const TICKS_PER_ROW: u16 = 16;

    #[must_use]
    pub const fn ticks(self) -> u16 {
        self.0
    }

    #[must_use]
    pub fn adjusted(self, double: bool) -> Self {
        Self(if double {
            self.0.saturating_mul(2).min(4096)
        } else {
            (self.0 / 2).max(1)
        })
    }
}

impl Default for NoteLength {
    fn default() -> Self {
        Self(Self::TICKS_PER_ROW)
    }
}

impl TryFrom<u16> for NoteLength {
    type Error = &'static str;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if (1..=4096).contains(&value) {
            Ok(Self(value))
        } else {
            Err("Note length must be 1..=4096 ticks")
        }
    }
}

impl From<NoteLength> for u16 {
    fn from(value: NoteLength) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub length_ticks: NoteLength,
    pub pitch: NotePitch,
    pub octave: u8,
}

impl Note {
    /// Transpose chromatically, clamping to C0 through B9.
    #[must_use]
    pub fn transpose(self, semitones: i8) -> Self {
        let index = (i32::from(self.octave) * 12 + self.pitch.semitone() + i32::from(semitones)).clamp(0, 119);
        let pitch = match index % 12 {
            0 => NotePitch::C,
            1 => NotePitch::Cs,
            2 => NotePitch::D,
            3 => NotePitch::Ds,
            4 => NotePitch::E,
            5 => NotePitch::F,
            6 => NotePitch::Fs,
            7 => NotePitch::G,
            8 => NotePitch::Gs,
            9 => NotePitch::A,
            10 => NotePitch::As,
            _ => NotePitch::B,
        };
        Self {
            length_ticks: self.length_ticks,
            pitch,
            octave: u8::try_from(index / 12).unwrap_or(9),
        }
    }

    #[must_use]
    pub fn freq(self) -> f64 {
        // freq = 440.0 * 2^((midi_note - 69) / 12)
        let pitch_class = self.pitch.semitone();

        let midi_note = (i32::from(self.octave) * 12) + pitch_class + 12;

        midi_hz(f64::from(midi_note))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PatternStep {
    pub note: Option<Note>,
}

/// Canonical stored note. Row cells are temporary projections of these values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatternEvent {
    pub start_tick: crate::musical_time::Tick,
    pub duration_ticks: crate::musical_time::DurationTicks,
    pub pitch: NotePitch,
    pub octave: u8,
}

impl PatternEvent {
    pub(crate) fn from_note(start_tick: crate::musical_time::Tick, note: Note) -> Self {
        Self {
            start_tick,
            duration_ticks: crate::musical_time::DurationTicks::from_legacy(note.length_ticks.ticks())
                .expect("validated legacy gate"),
            pitch: note.pitch,
            octave: note.octave,
        }
    }

    pub(crate) fn row_note(self) -> Option<Note> {
        if !self.duration_ticks.ticks().is_multiple_of(15) {
            return None;
        }
        Some(Note {
            length_ticks: NoteLength::try_from(u16::try_from(self.duration_ticks.ticks() / 15).ok()?).ok()?,
            pitch: self.pitch,
            octave: self.octave,
        })
    }

    pub(crate) fn transposed(self, semitones: i8) -> Self {
        let note = Note {
            pitch: self.pitch,
            octave: self.octave,
            length_ticks: NoteLength::default(),
        }
        .transpose(semitones);
        Self {
            pitch: note.pitch,
            octave: note.octave,
            ..self
        }
    }
}

/// Process-local event identity. Snapshots preserve it; copies and file loads renew it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EventId(u64);

impl EventId {
    fn fresh() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self(
            NEXT.try_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |id| id.checked_add(1),
            )
            .expect("event identity space exhausted"),
        )
    }
}

/// Sorted unique starts; identities are metadata, not a second timeline.
#[derive(Clone, Debug, Default)]
pub struct Track {
    events: Vec<PatternEvent>,
    identities: Vec<EventId>,
}

// Document equality compares music, not process-local handles.
impl PartialEq for Track {
    fn eq(&self, other: &Self) -> bool {
        self.events == other.events
    }
}

impl Eq for Track {}

impl Track {
    #[must_use]
    pub fn event_id(&self, start: crate::musical_time::Tick) -> Option<EventId> {
        self.search(start).ok().map(|index| self.identities[index])
    }

    #[must_use]
    pub fn event_by_id(&self, id: EventId) -> Option<&PatternEvent> {
        self.identities
            .iter()
            .position(|value| *value == id)
            .map(|index| &self.events[index])
    }

    pub(crate) fn restore_id(&mut self, start: crate::musical_time::Tick, id: EventId) {
        let index = self.search(start).expect("identity target exists");
        self.identities[index] = id;
    }

    #[must_use]
    pub fn events(&self) -> &[PatternEvent] {
        &self.events
    }

    #[must_use]
    pub fn events_in(&self, range: std::ops::Range<crate::musical_time::Tick>) -> &[PatternEvent] {
        let first = self.events.partition_point(|event| event.start_tick < range.start);
        let last = self.events.partition_point(|event| event.start_tick < range.end);
        &self.events[first..last.max(first)]
    }

    fn search(&self, start: crate::musical_time::Tick) -> Result<usize, usize> {
        self.events.binary_search_by_key(&start, |event| event.start_tick)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    grid: crate::musical_time::GridMap,
    pub tracks: Vec<Track>,
}

impl Pattern {
    /// # Panics
    /// Panics if the row count cannot be represented as ticks.
    #[must_use]
    pub fn new(num_rows: usize, num_tracks: usize) -> Self {
        Self {
            grid: crate::musical_time::GridMap::legacy(
                crate::musical_time::Tick::from_legacy_row(num_rows as u64).expect("pattern rows fit ticks"),
            ),
            tracks: vec![Track::default(); num_tracks],
        }
    }

    /// Straight sixteenth-note view, rounded up for a partial final row.
    ///
    /// # Panics
    /// Panics if the projected count cannot fit `usize` on this platform.
    #[must_use]
    pub fn num_rows(&self) -> usize {
        usize::try_from(self.length_ticks().0.div_ceil(crate::musical_time::LEGACY_ROW_TICKS))
            .expect("pattern end fits rows")
    }

    #[must_use]
    pub const fn length_ticks(&self) -> crate::musical_time::Tick {
        self.grid.end()
    }

    #[must_use]
    pub const fn grid(&self) -> &crate::musical_time::GridMap {
        &self.grid
    }

    pub(crate) fn set_grid(&mut self, grid: crate::musical_time::GridMap) {
        self.grid = grid;
    }

    #[must_use]
    pub fn view_rows(&self) -> usize {
        usize::try_from(self.grid.row_count()).unwrap_or(usize::MAX)
    }

    #[must_use]
    pub fn row_tick(&self, row: usize) -> Option<crate::musical_time::Tick> {
        self.grid.row_tick(row as u64)
    }

    #[must_use]
    pub fn row_at(&self, tick: crate::musical_time::Tick) -> usize {
        self.grid
            .row_at(tick)
            .and_then(|row| usize::try_from(row).ok())
            .unwrap_or(0)
    }

    #[must_use]
    pub fn event_at(&self, track: usize, tick: crate::musical_time::Tick) -> Option<PatternEvent> {
        let track = self.tracks.get(track)?;
        track.search(tick).ok().map(|index| track.events[index])
    }

    /// Legacy note-value adapter. Returns `None` for gates not representable in legacy units;
    /// use `event_at` for exact canonical values.
    #[must_use]
    pub fn view_step(&self, track: usize, row: usize) -> Option<PatternStep> {
        let tick = self.row_tick(row).filter(|tick| *tick < self.length_ticks())?;
        self.tracks.get(track)?;
        Some(PatternStep {
            note: match self.event_at(track, tick) {
                Some(event) => Some(event.row_note()?),
                None => None,
            },
        })
    }

    #[must_use]
    pub fn hidden_in_row(&self, track: usize, row: usize) -> usize {
        let (Some(start), Some(end)) = (self.row_tick(row), self.row_tick(row + 1)) else {
            return 0;
        };
        self.tracks.get(track).map_or(0, |track| {
            track
                .events_in(start..end)
                .iter()
                .filter(|event| event.start_tick != start)
                .count()
        })
    }

    /// Legacy straight-row adapter. Returns `None` for gates not representable in legacy units;
    /// use `event_at` for exact canonical values.
    #[must_use]
    pub fn get_step(&self, track_idx: usize, row_idx: usize) -> Option<PatternStep> {
        let track = self.tracks.get(track_idx)?;
        if row_idx >= self.num_rows() {
            return None;
        }
        let start = crate::musical_time::Tick::from_legacy_row(row_idx as u64).ok()?;
        Some(PatternStep {
            note: match track.search(start) {
                Ok(index) => Some(track.events[index].row_note()?),
                Err(_) => None,
            },
        })
    }

    pub(crate) fn has_notes_from_row(&self, row: usize) -> bool {
        let start = crate::musical_time::Tick::from_legacy_row(row as u64).expect("pattern row fits ticks");
        self.tracks
            .iter()
            .any(|track| track.events.last().is_some_and(|event| event.start_tick >= start))
    }

    pub(crate) fn resize_rows(&mut self, rows: usize) {
        let end = crate::musical_time::Tick::from_legacy_row(rows as u64).expect("pattern row fits ticks");
        self.resize_ticks(end);
    }

    pub(crate) fn resize_ticks(&mut self, end: crate::musical_time::Tick) {
        for track in &mut self.tracks {
            let keep = track.events.partition_point(|event| event.start_tick < end);
            track.events.truncate(keep);
            track.identities.truncate(keep);
        }
        self.grid.resize(end);
    }

    pub(crate) fn duplicate(&self) -> Self {
        let mut copy = self.clone();
        for track in &mut copy.tracks {
            for id in &mut track.identities {
                *id = EventId::fresh();
            }
        }
        copy
    }

    /// Replace the note at the specified track and row.
    ///
    /// # Errors
    /// Returns an error if the track/row is outside the pattern or the note octave is invalid.
    pub fn set_step(&mut self, track_idx: usize, row_idx: usize, new_note: Option<Note>) -> anyhow::Result<()> {
        if row_idx >= self.num_rows() {
            return Err(anyhow!("Accessing inexistant row"));
        }
        let start = crate::musical_time::Tick::from_legacy_row(row_idx as u64)
            .map_err(|_| anyhow!("Pattern row exceeds tick range"))?;
        self.replace_event(
            track_idx,
            start,
            new_note.map(|note| PatternEvent::from_note(start, note)),
        )?;
        Ok(())
    }

    // Keep the player/editor contract centralized until they support arbitrary ticks.
    pub(crate) fn validate_event(
        &self,
        start: crate::musical_time::Tick,
        event: Option<PatternEvent>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(start < self.length_ticks(), "Event starts outside pattern");

        if let Some(event) = event {
            anyhow::ensure!(
                event.start_tick == start,
                "Event start does not match replacement position"
            );

            anyhow::ensure!(event.octave <= 9, "Note octave must be 0..=9");
        }
        Ok(())
    }

    pub(crate) fn replace_event(
        &mut self,
        track_idx: usize,
        start: crate::musical_time::Tick,
        event: Option<PatternEvent>,
    ) -> anyhow::Result<Option<PatternEvent>> {
        self.validate_event(start, event)?;
        let track = self
            .tracks
            .get_mut(track_idx)
            .ok_or_else(|| anyhow!("Accessing inexistant track"))?;
        let before = match (track.search(start), event) {
            (Ok(index), Some(event)) => Some(std::mem::replace(&mut track.events[index], event)),
            (Ok(index), None) => {
                track.identities.remove(index);
                Some(track.events.remove(index))
            }
            (Err(index), Some(event)) => {
                track.events.insert(index, event);
                track.identities.insert(index, EventId::fresh());
                None
            }
            (Err(_), None) => None,
        };
        Ok(before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::musical_time::Tick;

    fn note(ticks: u16) -> Note {
        Note {
            pitch: NotePitch::Fs,
            octave: 5,
            length_ticks: NoteLength::try_from(ticks).unwrap(),
        }
    }

    // Canonical fixtures bypass admission guards deliberately: file/editor admission stays
    // restricted until the projected UI can expose every event and exact duration.
    fn tick_fixture(end: u64, starts: &[u64], duration: u32) -> Pattern {
        let mut pattern = Pattern::new(1, 1);
        pattern.resize_ticks(Tick(end));
        for &start in starts {
            pattern.tracks[0].events.push(PatternEvent {
                start_tick: Tick(start),
                duration_ticks: crate::musical_time::DurationTicks::new(duration).unwrap(),
                pitch: NotePitch::C,
                octave: 4,
            });
            pattern.tracks[0].identities.push(EventId::fresh());
        }
        pattern
    }

    #[test]
    fn tick_scheduler_dispatches_subrow_starts_once_and_uses_exact_gates() {
        use crate::{
            project::Project,
            transport::{collect_tick, PlaybackRoute, Transport},
        };
        use std::time::{Duration, Instant};
        let mut project = Project::new(1, 1);
        project.bpm = 121.0;
        project.patterns[0] = tick_fixture(641, &[0, 15, 160, 320, 640], 1);
        let now = Instant::now();
        let mut transport = Transport::default();
        let mut starts = Vec::new();
        let mut at = now;
        let mut generation = None;
        loop {
            let event = transport
                .update_project(at, true, &project, PlaybackRoute::Pattern(0), 4)
                .unwrap();
            assert_eq!(*generation.get_or_insert(event.generation), event.generation);
            let notes = collect_tick(&project.patterns[0], project.instruments(), event);
            if !notes.is_empty() {
                starts.push(event.absolute_tick);
            }
            assert!(notes.iter().all(|note| note.duration == Duration::from_nanos(516_529)));
            assert!(transport
                .update_project(at, true, &project, PlaybackRoute::Pattern(0), 4)
                .is_none());
            at = transport.deadline().unwrap();
            if event.absolute_tick == 641 {
                break;
            }
        }
        assert_eq!(starts, [0, 15, 160, 320, 640, 641]);
        transport.stop();
        assert!(
            transport
                .update_project(now, true, &project, PlaybackRoute::Pattern(0), 4)
                .unwrap()
                .generation
                > generation.unwrap()
        );
    }

    #[test]
    fn subrow_tempo_edits_keep_the_next_deadline_and_do_not_retime_held_gates() {
        use crate::{
            project::Project,
            transport::{collect_tick, PlaybackRoute, Transport},
        };
        use std::time::{Duration, Instant};
        let mut project = Project::new(1, 1);
        project.patterns[0] = tick_fixture(641, &[0, 160, 320], 61_440);
        let now = Instant::now();
        let mut transport = Transport::default();
        let first = transport
            .update_project(now, true, &project, PlaybackRoute::Pattern(0), 4)
            .unwrap();
        let held = collect_tick(&project.patterns[0], project.instruments(), first)[0];
        let committed = transport.deadline().unwrap();
        assert_eq!(committed, now + Duration::from_nanos(83_333_333));
        project.bpm = 60.0;
        assert!(transport
            .update_project(
                committed.checked_sub(Duration::from_nanos(1)).unwrap(),
                true,
                &project,
                PlaybackRoute::Pattern(0),
                4
            )
            .is_none());
        assert_eq!(transport.deadline(), Some(committed));
        let next = transport
            .update_project(committed, true, &project, PlaybackRoute::Pattern(0), 4)
            .unwrap();
        assert_eq!(next.tick, Tick(160));
        assert_eq!(held.duration, Duration::from_secs(32));
        assert_eq!(
            collect_tick(&project.patterns[0], project.instruments(), next)[0].duration,
            Duration::from_secs(64)
        );
        assert_eq!(transport.deadline(), Some(committed + Duration::from_nanos(83_333_333)));
    }

    #[test]
    fn tick_song_traversal_never_pads_partial_patterns_and_bounds_huge_skips() {
        use crate::{
            project::Project,
            transport::{PlaybackRoute, Transport},
        };
        use std::time::{Duration, Instant};
        let mut project = Project::new(1, 1);
        project.patterns = vec![tick_fixture(241, &[240], 15), tick_fixture(17, &[0, 15], 15)];
        project.order = vec![0, 1, 0];
        let mut transport = Transport::default();
        let now = Instant::now();
        let mut at = now;
        let mut seen = Vec::new();
        loop {
            let event = transport
                .update_project(at, true, &project, PlaybackRoute::Song, 4)
                .unwrap();
            seen.push((event.absolute_tick, event.order.unwrap(), event.tick.0));
            at = transport.deadline().unwrap();
            if event.absolute_tick == 499 {
                break;
            }
        }
        assert_eq!(
            seen,
            [
                (0, 0, 0),
                (240, 0, 240),
                (241, 1, 0),
                (256, 1, 15),
                (258, 2, 0),
                (498, 2, 240),
                (499, 0, 0)
            ]
        );
        let event = transport
            .update_project(
                now + Duration::from_secs(1_000_000),
                true,
                &project,
                PlaybackRoute::Song,
                4,
            )
            .unwrap();
        assert!(event.absolute_tick > 1_000_000_000);
        assert!(transport.deadline().unwrap() > now + Duration::from_secs(1_000_000));
    }

    #[test]
    fn production_tick_dispatch_catches_up_dense_notes_in_deterministic_track_order() {
        use crate::{
            state::{App, PlaybackState},
            transport::Transport,
        };
        use std::time::{Duration, Instant};
        let mut app = App::default();
        app.project.bpm = 300.0;
        app.project.patterns[0] = tick_fixture(960, &(0..960).collect::<Vec<_>>(), 15);
        let second = app.project.patterns[0].duplicate().tracks.remove(0);
        app.project.patterns[0].tracks.push(second);
        app.playback_state = PlaybackState::Playing;
        let now = Instant::now();
        let mut transport = Transport::default();
        let initial = transport.update_app(now, &mut app, || panic!("new run"));
        assert_eq!(initial.iter().map(|note| note.track).collect::<Vec<_>>(), [0, 1]);
        let notes = transport.update_app(now + Duration::from_millis(40), &mut app, || panic!("active run"));
        assert_eq!(notes.len(), 192 * 2);
        assert!(notes
            .as_chunks::<2>()
            .0
            .iter()
            .all(|pair| pair[0].track == 0 && pair[1].track == 1));
        assert!(transport
            .update_app(now + Duration::from_millis(40), &mut app, || panic!("active run"))
            .is_empty());
        let late = transport.update_app(now + Duration::from_secs(1_000_000), &mut app, || panic!("active run"));
        assert!(late.len() <= 2);
        assert!(transport.deadline().unwrap() > now + Duration::from_secs(1_000_000));
    }

    #[test]
    fn explicit_end_projects_partial_rows_without_stretching_gates() {
        let mut pattern = Pattern::new(4, 1);
        pattern.set_step(0, 0, Some(note(4096))).unwrap();
        pattern.set_step(0, 1, Some(note(1))).unwrap();
        let id = pattern.tracks[0].event_id(Tick(240));
        pattern.resize_ticks(Tick(241));
        assert_eq!(pattern.length_ticks(), Tick(241));
        assert_eq!(pattern.num_rows(), 2);
        assert_eq!(pattern.get_step(0, 1).unwrap().note, Some(note(1)));
        assert!(pattern.get_step(0, 2).is_none());
        assert_eq!(pattern.tracks[0].event_id(Tick(240)), id);
        pattern.resize_ticks(Tick(240));
        assert_eq!(pattern.num_rows(), 1);
        assert!(pattern.tracks[0].event_by_id(id.unwrap()).is_none());
        assert_eq!(pattern.tracks[0].events()[0].duration_ticks.ticks(), 61_440);
        pattern.resize_ticks(Tick(1));
        assert_eq!(pattern.num_rows(), 1);
        assert_eq!(pattern.length_ticks(), Tick(1));
        pattern.resize_rows(4);
        assert_eq!(pattern.length_ticks(), Tick(960));
        assert_eq!(pattern.tracks[0].events().len(), 1);
    }

    #[test]
    fn sparse_storage_projects_empty_rows_without_allocating_cells() {
        let mut pattern = Pattern::new(4096, 128);
        assert!(pattern.tracks.iter().all(|track| track.events().is_empty()));
        assert_eq!(pattern.get_step(127, 4095), Some(PatternStep::default()));
        assert_eq!(pattern.get_step(128, 0), None);
        assert_eq!(pattern.get_step(0, 4096), None);
        pattern.set_step(127, 4095, Some(note(4096))).unwrap();
        let events = pattern.tracks[127].events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].start_tick, Tick(4095 * 240));
        assert_eq!(events[0].duration_ticks.ticks(), 61_440);
        let mut projected = pattern.get_step(127, 4095).unwrap();
        projected.note.as_mut().unwrap().octave = 1;
        assert_eq!(pattern.get_step(127, 4095).unwrap().note, Some(note(4096)));
        pattern.set_step(127, 4095, None).unwrap();
        assert_eq!(pattern.tracks[127].events(), []);
    }

    #[test]
    fn sparse_replacements_and_deletions_match_dense_reference() {
        let mut pattern = Pattern::new(64, 3);
        let mut reference = vec![vec![None; 64]; 3];
        let mut random = 17u32;
        for operation in 0..2048 {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let track = (random as usize / 64) % 3;
            let row = random as usize % 64;
            let value = if operation % 3 == 0 {
                None
            } else {
                Some(note(if operation % 2 == 0 { 1 } else { 4096 }))
            };
            pattern.set_step(track, row, value).unwrap();
            reference[track][row] = value;
            for (track, cells) in reference.iter().enumerate() {
                for (row, value) in cells.iter().enumerate() {
                    assert_eq!(pattern.get_step(track, row).unwrap().note, *value);
                }
                let events = pattern.tracks[track].events();
                assert_eq!(events.len(), cells.iter().flatten().count());
                assert!(events.windows(2).all(|pair| pair[0].start_tick < pair[1].start_tick));
            }
        }
        let before = pattern.clone();
        assert!(pattern.set_step(3, 0, Some(note(16))).is_err());
        assert!(pattern.set_step(0, 64, Some(note(16))).is_err());
        assert_eq!(pattern, before);
    }

    #[test]
    fn sparse_resize_preserves_gates_and_only_truncates_starts() {
        let mut pattern = Pattern::new(64, 2);
        for (row, ticks) in [(3, 4096), (4, 1), (63, 16)] {
            pattern.set_step(0, row, Some(note(ticks))).unwrap();
        }
        assert!(pattern.has_notes_from_row(4));
        assert!(!pattern.has_notes_from_row(64));
        pattern.resize_rows(4);
        assert_eq!(pattern.tracks[0].events().len(), 1);
        assert_eq!(pattern.get_step(0, 3).unwrap().note, Some(note(4096)));
        pattern.resize_rows(128);
        assert_eq!(pattern.get_step(0, 4).unwrap().note, None);
        assert_eq!(pattern.tracks[1].events(), []);
    }
}
