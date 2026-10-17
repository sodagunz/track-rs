//! Tick-event JSON with explicit version 1–3 migration. Decode and validate before installation.
//! The row editor adapter rejects timing it cannot yet preserve or play.
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

use anyhow::{bail, ensure, Context};
use serde::{Deserialize, Serialize};

use crate::{
    project::Project,
    tracker::{Instrument, Note, Pattern},
};

pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_TRACKS: usize = 128;
pub const MAX_PATTERNS: usize = 256;
pub const MAX_ROWS: usize = 4096;
pub const MAX_CELLS: usize = 1_000_000;
pub const MAX_ORDER: usize = 65_536;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyDocument<N = Note> {
    version: u32,
    bpm: f64,
    instruments: Vec<Instrument>,
    patterns: Vec<StoredPattern<N>>,
    order: Vec<usize>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    bpm: f64,
    instruments: Vec<Instrument>,
    channels: Vec<crate::project::ChannelSettings>,
    patterns: Vec<StoredPattern>,
    order: Vec<usize>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPattern<N = Note> {
    rows: usize,
    tracks: usize,
    cells: Vec<Vec<Option<N>>>,
}

impl Document {
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(self.version == 3, "Unsupported project version {}", self.version);
        ensure!(
            self.bpm.is_finite() && (20.0..=300.0).contains(&self.bpm),
            "BPM must be finite and between 20 and 300"
        );
        ensure!(
            (1..=MAX_TRACKS).contains(&self.instruments.len()),
            "Track count must be 1..={MAX_TRACKS}"
        );
        ensure!(
            (1..=MAX_PATTERNS).contains(&self.patterns.len()),
            "Pattern count must be 1..={MAX_PATTERNS}"
        );
        ensure!(
            (1..=MAX_ORDER).contains(&self.order.len()),
            "Order length must be 1..={MAX_ORDER}"
        );
        ensure!(
            self.order.iter().all(|&index| index < self.patterns.len()),
            "Invalid order reference"
        );
        ensure!(
            self.channels.len() == self.instruments.len() && self.channels.iter().all(|c| c.is_valid()),
            "Invalid channel settings or dimensions"
        );
        let mut total = 0_usize;
        for pattern in &self.patterns {
            ensure!((1..=MAX_ROWS).contains(&pattern.rows), "Rows must be 1..={MAX_ROWS}");
            ensure!(
                pattern.tracks == self.instruments.len() && pattern.cells.len() == pattern.tracks,
                "Inconsistent track dimensions"
            );
            total = total
                .checked_add(pattern.rows * pattern.tracks)
                .context("Cell count overflow")?;
            ensure!(total <= MAX_CELLS, "Project exceeds {MAX_CELLS} cells");
            for track in &pattern.cells {
                ensure!(track.len() == pattern.rows, "Inconsistent row dimensions");
                ensure!(
                    track.iter().flatten().all(|note| note.octave <= 9),
                    "Note octave must be 0..=9"
                );
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Header {
    version: u32,
}

fn neutral_channels(tracks: usize) -> anyhow::Result<Vec<crate::project::ChannelSettings>> {
    ensure!(
        (1..=MAX_TRACKS).contains(&tracks),
        "Track count must be 1..={MAX_TRACKS}"
    );
    Ok(vec![crate::project::ChannelSettings::default(); tracks])
}

/// Read at most 16 MiB, then validate all dimensions and values.
///
/// # Errors
/// Returns file, JSON, unsupported-version, or validation errors without changing a project.
pub fn load(path: &Path) -> anyhow::Result<Project> {
    let mut bytes = Vec::new();
    File::open(path)?.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_FILE_BYTES, "Project exceeds 16 MiB");
    let header: Header = serde_json::from_slice(&bytes)?;
    if !matches!(header.version, 1..=4) {
        bail!("Unsupported project version {}", header.version);
    }
    let document = if header.version == 4 {
        let stored: EventDocument = serde_json::from_slice(&bytes)?;
        stored.into_legacy()?
    } else if header.version == 1 {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct LegacyNote {
            pitch: crate::tracker::NotePitch,
            octave: u8,
        }
        let legacy: LegacyDocument<LegacyNote> = serde_json::from_slice(&bytes)?;
        Document {
            version: 3,
            channels: neutral_channels(legacy.instruments.len())?,
            bpm: legacy.bpm,
            instruments: legacy.instruments,
            order: legacy.order,
            patterns: legacy
                .patterns
                .into_iter()
                .map(|pattern| StoredPattern {
                    rows: pattern.rows,
                    tracks: pattern.tracks,
                    cells: pattern
                        .cells
                        .into_iter()
                        .map(|track| {
                            track
                                .into_iter()
                                .map(|note| {
                                    note.map(|note| Note {
                                        pitch: note.pitch,
                                        octave: note.octave,
                                        length_ticks: crate::tracker::NoteLength::default(),
                                    })
                                })
                                .collect()
                        })
                        .collect(),
                })
                .collect(),
        }
    } else if header.version == 2 {
        let legacy: LegacyDocument = serde_json::from_slice(&bytes)?;
        Document {
            version: 3,
            channels: neutral_channels(legacy.instruments.len())?,
            bpm: legacy.bpm,
            instruments: legacy.instruments,
            patterns: legacy.patterns,
            order: legacy.order,
        }
    } else {
        serde_json::from_slice(&bytes)?
    };
    document.validate()?;
    let patterns = document
        .patterns
        .into_iter()
        .map(|stored| {
            let mut pattern = Pattern::new(stored.rows, stored.tracks);
            for (target, source) in pattern.tracks.iter_mut().zip(stored.cells) {
                for (step, note) in target.iter_mut().zip(source) {
                    step.note = note;
                }
            }
            pattern
        })
        .collect();
    Ok(Project {
        channels: document.channels,
        bpm: document.bpm,
        instruments: document.instruments,
        patterns,
        order: document.order,
    })
}

/// Flush and sync a same-directory temporary file before atomically replacing the target.
///
/// # Errors
/// Returns validation, serialization, or filesystem errors. The target is not truncated.
pub fn save(project: &Project, path: &Path) -> anyhow::Result<()> {
    let document = Document {
        version: 3,
        channels: project.channels.clone(),
        bpm: project.bpm,
        instruments: project.instruments.clone(),
        patterns: project
            .patterns
            .iter()
            .map(|pattern| StoredPattern {
                rows: pattern.num_rows,
                tracks: pattern.tracks.len(),
                cells: pattern
                    .tracks
                    .iter()
                    .map(|track| track.iter().map(|step| step.note).collect())
                    .collect(),
            })
            .collect(),
        order: project.order.clone(),
    };
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(&EventDocument::from_legacy(document))?;
    ensure!(bytes.len() as u64 <= MAX_FILE_BYTES, "Project exceeds 16 MiB encoded");
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventDocument {
    version: u32,
    ppq: u64,
    bpm: f64,
    instruments: Vec<Instrument>,
    channels: Vec<crate::project::ChannelSettings>,
    patterns: Vec<EventPattern>,
    order: Vec<usize>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventPattern {
    end_tick: u64,
    meter: [u8; 2],
    tracks: Vec<Vec<StoredEvent>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEvent {
    start_tick: u64,
    duration_ticks: u32,
    pitch: crate::tracker::NotePitch,
    octave: u8,
}

impl EventDocument {
    fn from_legacy(document: Document) -> Self {
        Self {
            version: 4,
            ppq: crate::musical_time::PPQ,
            bpm: document.bpm,
            instruments: document.instruments,
            channels: document.channels,
            order: document.order,
            patterns: document
                .patterns
                .into_iter()
                .map(|pattern| EventPattern {
                    end_tick: pattern.rows as u64 * crate::musical_time::LEGACY_ROW_TICKS,
                    meter: [4, 4],
                    tracks: pattern
                        .cells
                        .into_iter()
                        .map(|track| {
                            track
                                .into_iter()
                                .enumerate()
                                .filter_map(|(row, note)| {
                                    note.map(|note| StoredEvent {
                                        start_tick: row as u64 * crate::musical_time::LEGACY_ROW_TICKS,
                                        duration_ticks: u32::from(note.length_ticks.ticks()) * 15,
                                        pitch: note.pitch,
                                        octave: note.octave,
                                    })
                                })
                                .collect()
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    // One writable timeline: this temporary adapter materializes rows only after checking
    // every event is exactly representable. Never round or drop unsupported positions.
    fn into_legacy(self) -> anyhow::Result<Document> {
        use crate::musical_time::{DurationTicks, LEGACY_ROW_TICKS, PPQ};
        ensure!(
            self.version == 4 && self.ppq == PPQ,
            "Unsupported project tick resolution"
        );
        ensure!(
            (1..=MAX_TRACKS).contains(&self.instruments.len()),
            "Invalid track count"
        );
        ensure!(
            (1..=MAX_PATTERNS).contains(&self.patterns.len()),
            "Invalid pattern count"
        );
        let mut total = 0usize;
        let mut patterns = Vec::with_capacity(self.patterns.len());
        for pattern in self.patterns {
            ensure!(
                pattern.meter == [4, 4],
                "Pattern meter requires the upcoming grid editor"
            );
            ensure!(
                pattern.end_tick > 0
                    && pattern.end_tick <= MAX_ROWS as u64 * LEGACY_ROW_TICKS
                    && pattern.end_tick % LEGACY_ROW_TICKS == 0,
                "Pattern end is not supported by the row editor"
            );
            let rows = usize::try_from(pattern.end_tick / LEGACY_ROW_TICKS)?;
            let tracks = pattern.tracks.len();
            ensure!(tracks == self.instruments.len(), "Inconsistent track dimensions");
            total = total
                .checked_add(rows.checked_mul(tracks).context("Cell count overflow")?)
                .context("Cell count overflow")?;
            ensure!(total <= MAX_CELLS, "Project exceeds {MAX_CELLS} compatibility cells");
            let mut cells = Vec::with_capacity(tracks);
            for events in pattern.tracks {
                ensure!(events.len() <= rows, "Too many events for the row editor");
                let mut previous = None;
                for event in &events {
                    ensure!(event.start_tick < pattern.end_tick, "Event starts outside pattern");
                    ensure!(
                        previous.is_none_or(|start| start < event.start_tick),
                        "Events must have unique ordered starts"
                    );
                    previous = Some(event.start_tick);
                    ensure!(
                        event.start_tick % LEGACY_ROW_TICKS == 0,
                        "Sub-row events require the upcoming tick scheduler"
                    );
                    ensure!(
                        DurationTicks::new(event.duration_ticks).is_ok(),
                        "Invalid event duration"
                    );
                    ensure!(
                        event.duration_ticks % 15 == 0,
                        "Gate resolution requires the upcoming tick scheduler"
                    );
                    ensure!(event.octave <= 9, "Note octave must be 0..=9");
                }
                let mut track = vec![None; rows];
                for event in events {
                    track[usize::try_from(event.start_tick / LEGACY_ROW_TICKS)?] = Some(Note {
                        length_ticks: crate::tracker::NoteLength::try_from(u16::try_from(event.duration_ticks / 15)?)
                            .map_err(anyhow::Error::msg)?,
                        pitch: event.pitch,
                        octave: event.octave,
                    });
                }
                cells.push(track);
            }
            patterns.push(StoredPattern { rows, tracks, cells });
        }
        Ok(Document {
            version: 3,
            bpm: self.bpm,
            instruments: self.instruments,
            channels: self.channels,
            patterns,
            order: self.order,
        })
    }
}
