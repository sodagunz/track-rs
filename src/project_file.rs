//! Tick-event JSON with explicit version 1–3 migration. Decode and validate before installation.
//! The row editor adapter rejects timing it cannot yet expose for editing.
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
        validate_metadata(
            self.bpm,
            &self.instruments,
            &self.channels,
            self.patterns.len(),
            &self.order,
        )?;
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
    if header.version == 4 {
        let stored: EventDocument = serde_json::from_slice(&bytes)?;
        return stored.into_project();
    }
    let document = if header.version == 1 {
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
            for (track, source) in stored.cells.into_iter().enumerate() {
                for (row, note) in source.into_iter().enumerate() {
                    if note.is_some() {
                        pattern.set_step(track, row, note)?;
                    }
                }
            }
            Ok(pattern)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
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
    let document = EventDocument::from_project(project)?;
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
    #[serde(default = "straight_grid")]
    grid: u16,
    #[serde(default)]
    bar_grids: Vec<[u64; 2]>,
    tracks: Vec<Vec<StoredEvent>>,
}

const fn straight_grid() -> u16 {
    4
}

impl EventPattern {
    fn grid_map(&self) -> anyhow::Result<crate::musical_time::GridMap> {
        use crate::musical_time::{Grid, GridMap, Meter, Tick};
        let meter = Meter::new(self.meter[0], self.meter[1]).map_err(|_| anyhow::anyhow!("Invalid pattern meter"))?;
        let grid = Grid::new(self.grid).map_err(|_| anyhow::anyhow!("Unsupported grid"))?;
        let mut map =
            GridMap::new(meter, Tick(self.end_tick), grid).map_err(|_| anyhow::anyhow!("Invalid pattern end"))?;
        let mut previous = None;
        for &[tick, divisions] in &self.bar_grids {
            ensure!(
                previous.is_none_or(|value| value < tick),
                "Bar overrides must have unique ordered starts"
            );
            previous = Some(tick);
            let grid = Grid::new(u16::try_from(divisions)?).map_err(|_| anyhow::anyhow!("Unsupported bar grid"))?;
            map.set_bar_grid(Tick(tick), grid)
                .map_err(|_| anyhow::anyhow!("Invalid bar grid start"))?;
        }
        Ok(map)
    }
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
    fn from_project(project: &Project) -> anyhow::Result<Self> {
        ensure!(
            project
                .patterns
                .iter()
                .all(|pattern| (1..=MAX_ROWS).contains(&pattern.num_rows())),
            "Rows must be 1..={MAX_ROWS}"
        );
        Ok(Self {
            version: 4,
            ppq: crate::musical_time::PPQ,
            bpm: project.bpm,
            instruments: project.instruments.clone(),
            channels: project.channels.clone(),
            order: project.order.clone(),
            patterns: project
                .patterns
                .iter()
                .map(|pattern| EventPattern {
                    end_tick: pattern.length_ticks().0,
                    meter: [pattern.grid().meter().numerator(), pattern.grid().meter().denominator()],
                    grid: pattern.grid().default_grid().divisions_per_quarter(),
                    bar_grids: pattern
                        .grid()
                        .overrides()
                        .map(|(tick, grid)| [tick.0, u64::from(grid.divisions_per_quarter())])
                        .collect(),
                    tracks: pattern
                        .tracks
                        .iter()
                        .map(|track| {
                            track
                                .events()
                                .iter()
                                .map(|event| StoredEvent {
                                    start_tick: event.start_tick.0,
                                    duration_ticks: event.duration_ticks.ticks(),
                                    pitch: event.pitch,
                                    octave: event.octave,
                                })
                                .collect()
                        })
                        .collect(),
                })
                .collect(),
        })
    }

    fn validate(&self) -> anyhow::Result<()> {
        use crate::musical_time::{DurationTicks, LEGACY_ROW_TICKS, PPQ};
        ensure!(
            self.version == 4 && self.ppq == PPQ,
            "Unsupported project tick resolution"
        );
        validate_metadata(
            self.bpm,
            &self.instruments,
            &self.channels,
            self.patterns.len(),
            &self.order,
        )?;
        let mut total = 0usize;
        for pattern in &self.patterns {
            pattern.grid_map()?;
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
            for events in &pattern.tracks {
                ensure!(events.len() <= MAX_CELLS, "Too many events");
                let mut previous = None;
                for event in events {
                    ensure!(event.start_tick < pattern.end_tick, "Event starts outside pattern");
                    ensure!(
                        previous.is_none_or(|start| start < event.start_tick),
                        "Events must have unique ordered starts"
                    );
                    previous = Some(event.start_tick);

                    ensure!(
                        DurationTicks::new(event.duration_ticks).is_ok(),
                        "Invalid event duration"
                    );

                    ensure!(event.octave <= 9, "Note octave must be 0..=9");
                }
            }
        }
        Ok(())
    }

    fn into_project(self) -> anyhow::Result<Project> {
        self.validate()?;
        let mut patterns = Vec::with_capacity(self.patterns.len());
        for stored in self.patterns {
            let rows = usize::try_from(stored.end_tick / crate::musical_time::LEGACY_ROW_TICKS)?;
            let mut pattern = Pattern::new(rows, stored.tracks.len());
            pattern.set_grid(stored.grid_map()?);
            for (track, events) in stored.tracks.into_iter().enumerate() {
                for event in events {
                    let start = crate::musical_time::Tick(event.start_tick);
                    pattern.replace_event(
                        track,
                        start,
                        Some(crate::tracker::PatternEvent {
                            start_tick: start,
                            duration_ticks: crate::musical_time::DurationTicks::new(event.duration_ticks)
                                .map_err(|_| anyhow::anyhow!("Invalid duration"))?,
                            pitch: event.pitch,
                            octave: event.octave,
                        }),
                    )?;
                }
            }
            patterns.push(pattern);
        }
        Ok(Project {
            bpm: self.bpm,
            instruments: self.instruments,
            channels: self.channels,
            patterns,
            order: self.order,
        })
    }
}

fn validate_metadata(
    bpm: f64,
    instruments: &[Instrument],
    channels: &[crate::project::ChannelSettings],
    patterns: usize,
    order: &[usize],
) -> anyhow::Result<()> {
    ensure!(
        bpm.is_finite() && (20.0..=300.0).contains(&bpm),
        "BPM must be finite and between 20 and 300"
    );
    ensure!(
        (1..=MAX_TRACKS).contains(&instruments.len()),
        "Track count must be 1..={MAX_TRACKS}"
    );
    ensure!(
        (1..=MAX_PATTERNS).contains(&patterns),
        "Pattern count must be 1..={MAX_PATTERNS}"
    );
    ensure!(
        (1..=MAX_ORDER).contains(&order.len()),
        "Order length must be 1..={MAX_ORDER}"
    );
    ensure!(order.iter().all(|&index| index < patterns), "Invalid order reference");
    ensure!(
        channels.len() == instruments.len() && channels.iter().all(|c| c.is_valid()),
        "Invalid channel settings or dimensions"
    );
    Ok(())
}
