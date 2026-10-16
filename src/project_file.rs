//! Version 2 JSON with strict version 1 migration. Decode and validate completely before
//! installation.
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
struct Document<N = Note> {
    version: u32,
    bpm: f64,
    instruments: Vec<Instrument>,
    patterns: Vec<StoredPattern<N>>,
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
        ensure!(self.version == 2, "Unsupported project version {}", self.version);
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

/// Read at most 16 MiB, then validate all dimensions and values.
///
/// # Errors
/// Returns file, JSON, unsupported-version, or validation errors without changing a project.
pub fn load(path: &Path) -> anyhow::Result<Project> {
    let mut bytes = Vec::new();
    File::open(path)?.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_FILE_BYTES, "Project exceeds 16 MiB");
    let header: Header = serde_json::from_slice(&bytes)?;
    if !matches!(header.version, 1 | 2) {
        bail!("Unsupported project version {}", header.version);
    }
    let document = if header.version == 1 {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct LegacyNote {
            pitch: crate::tracker::NotePitch,
            octave: u8,
        }
        let legacy: Document<LegacyNote> = serde_json::from_slice(&bytes)?;
        Document {
            version: 2,
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
        version: 2,
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
    let bytes = serde_json::to_vec_pretty(&document)?;
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
