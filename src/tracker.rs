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
    fn from_note(start_tick: crate::musical_time::Tick, note: Note) -> Self {
        Self {
            start_tick,
            duration_ticks: crate::musical_time::DurationTicks::from_legacy(note.length_ticks.ticks())
                .expect("validated legacy gate"),
            pitch: note.pitch,
            octave: note.octave,
        }
    }

    fn row_note(self) -> Note {
        Note {
            length_ticks: NoteLength::try_from(
                u16::try_from(self.duration_ticks.ticks() / 15).expect("legacy gate fits"),
            )
            .expect("legacy-compatible stored gate"),
            pitch: self.pitch,
            octave: self.octave,
        }
    }
}

/// Sorted unique starts; no empty cells and no mutable dense mirror.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    events: Vec<PatternEvent>,
}

impl Track {
    #[must_use]
    pub fn events(&self) -> &[PatternEvent] {
        &self.events
    }

    fn search(&self, start: crate::musical_time::Tick) -> Result<usize, usize> {
        self.events.binary_search_by_key(&start, |event| event.start_tick)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    pub num_rows: usize,
    pub tracks: Vec<Track>,
}

impl Pattern {
    #[must_use]
    pub fn new(num_rows: usize, num_tracks: usize) -> Self {
        Self {
            num_rows,
            tracks: vec![Track::default(); num_tracks],
        }
    }

    #[must_use]
    pub fn get_step(&self, track_idx: usize, row_idx: usize) -> Option<PatternStep> {
        let track = self.tracks.get(track_idx)?;
        if row_idx >= self.num_rows {
            return None;
        }
        let start = crate::musical_time::Tick::from_legacy_row(row_idx as u64).ok()?;
        Some(PatternStep {
            note: track.search(start).ok().map(|index| track.events[index].row_note()),
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
        for track in &mut self.tracks {
            let keep = track.events.partition_point(|event| event.start_tick < end);
            track.events.truncate(keep);
        }
        self.num_rows = rows;
    }

    /// Replace the note at the specified track and row.
    ///
    /// # Errors
    /// Returns an error if the track or row index is outside the pattern.
    pub fn set_step(&mut self, track_idx: usize, row_idx: usize, new_note: Option<Note>) -> anyhow::Result<()> {
        if row_idx >= self.num_rows {
            return Err(anyhow!("Accessing inexistant row"));
        }
        let track = self
            .tracks
            .get_mut(track_idx)
            .ok_or_else(|| anyhow!("Accessing inexistant track"))?;
        let start = crate::musical_time::Tick::from_legacy_row(row_idx as u64)
            .map_err(|_| anyhow!("Pattern row exceeds tick range"))?;
        match (track.search(start), new_note) {
            (Ok(index), Some(note)) => track.events[index] = PatternEvent::from_note(start, note),
            (Ok(index), None) => {
                track.events.remove(index);
            }
            (Err(index), Some(note)) => track.events.insert(index, PatternEvent::from_note(start, note)),
            (Err(_), None) => {}
        }
        Ok(())
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
