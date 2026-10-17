//! Exact-event/value transactions; editor and transport state never enter the journal.
use std::collections::{BTreeMap, VecDeque};

use crate::{
    musical_time::Tick,
    project::{ChannelSettings, Project},
    tracker::{EventId, Instrument, Note, Pattern, PatternEvent, Track},
};

pub const MAX_TRANSACTIONS: usize = 256;
pub const MAX_MUTATIONS: usize = 65_536;

/// Identity of a document revision, independent of retained undo depth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Revision(u64);

#[derive(Clone, Debug)]
enum Mutation {
    Grid {
        pattern: usize,
        before: crate::musical_time::GridMap,
        after: crate::musical_time::GridMap,
    },
    Track {
        columns: Vec<Track>,
        instrument: Instrument,
    },
    Pattern {
        index: usize,
        value: Pattern,
        before: Option<Pattern>,
    },
    Order {
        before: Vec<usize>,
        after: Vec<usize>,
    },
    Event {
        pattern: usize,
        track: usize,
        start: Tick,
        before: Option<PatternEvent>,
        after: Option<PatternEvent>,
        before_id: Option<EventId>,
        after_id: Option<EventId>,
    },
    Instrument {
        track: usize,
        before: Instrument,
        after: Instrument,
    },
    Channel {
        track: usize,
        before: ChannelSettings,
        after: ChannelSettings,
    },
    Tempo {
        before: f64,
        after: f64,
    },
}

impl Mutation {
    fn weight(&self) -> usize {
        match self {
            Self::Grid { before, after, .. } => 1 + before.overrides().count() + after.overrides().count(),
            Self::Pattern { value, before, .. } => {
                value.storage_weight() + before.as_ref().map_or(0, Pattern::storage_weight)
            }
            Self::Track { columns, .. } => 1 + columns.iter().map(|track| 1 + track.events().len()).sum::<usize>(),
            Self::Order { before, after } => before.len() + after.len(),
            _ => 1,
        }
    }

    fn structural(&self) -> bool {
        matches!(self, Self::Pattern { .. } | Self::Order { .. } | Self::Track { .. })
            || matches!(self, Self::Grid { before, after, .. } if before.meter() != after.meter())
    }

    const fn key(&self) -> (u8, usize, usize, u64) {
        match *self {
            Self::Event {
                pattern, track, start, ..
            } => (1, pattern, track, start.0),
            Self::Instrument { track, .. } => (2, 0, track, 0),
            Self::Channel { track, .. } => (6, 0, track, 0),
            Self::Tempo { .. } => (3, 0, 0, 0),
            Self::Pattern { index, .. } => (0, index, 0, 0),
            Self::Order { .. } => (4, 0, 0, 0),
            Self::Track { .. } => (5, 0, 0, 0),
            Self::Grid { pattern, .. } => (7, pattern, 0, 0),
        }
    }

    #[allow(clippy::float_cmp)] // Exact document values determine no-ops.
    fn changed(&self) -> bool {
        match *self {
            Self::Grid {
                ref before, ref after, ..
            } => before != after,
            Self::Event {
                before,
                after,
                before_id,
                after_id,
                ..
            } => before != after || before_id != after_id,
            Self::Instrument { before, after, .. } => before != after,
            Self::Channel { before, after, .. } => before != after,
            Self::Tempo { before, after } => before != after,
            Self::Pattern { .. } | Self::Track { .. } => true,
            Self::Order { ref before, ref after } => before != after,
        }
    }

    fn merge(&mut self, next: Self) {
        match (self, next) {
            (Self::Grid { after, .. }, Self::Grid { after: value, .. }) => *after = value,
            (
                Self::Event { after, after_id, .. },
                Self::Event {
                    after: value,
                    after_id: id,
                    ..
                },
            ) => {
                *after = value;
                *after_id = id;
            }
            (Self::Instrument { after, .. }, Self::Instrument { after: value, .. }) => *after = value,
            (Self::Channel { after, .. }, Self::Channel { after: value, .. }) => *after = value,
            (Self::Tempo { after, .. }, Self::Tempo { after: value, .. }) => *after = value,
            (Self::Order { after, .. }, Self::Order { after: value, .. }) => *after = value,
            _ => unreachable!("same mutation key"),
        }
    }

    fn apply(&self, project: &mut Project, forward: bool) {
        match *self {
            Self::Grid {
                pattern,
                ref before,
                ref after,
            } => project.patterns[pattern].set_grid(if forward { after } else { before }.clone()),
            Self::Track {
                ref columns,
                instrument,
                ..
            } => {
                for (pattern, column) in project.patterns.iter_mut().zip(columns) {
                    if forward {
                        pattern.tracks.push(column.clone());
                    } else {
                        pattern.tracks.pop();
                    }
                }
                if forward {
                    project.instruments.push(instrument);
                    project.channels.push(ChannelSettings::default());
                } else {
                    project.instruments.pop();
                    project.channels.pop();
                }
            }
            Self::Event {
                pattern,
                track,
                start,
                before,
                after,
                before_id,
                after_id,
            } => {
                project.patterns[pattern]
                    .replace_event(track, start, if forward { after } else { before })
                    .expect("history event target exists");
                if let Some(id) = if forward { after_id } else { before_id } {
                    project.patterns[pattern].tracks[track].restore_id(start, id);
                }
            }
            Self::Instrument { track, before, after } => {
                project.set_instrument(track, if forward { after } else { before });
            }
            Self::Channel { track, before, after } => project.channels[track] = if forward { after } else { before },
            Self::Tempo { before, after } => project.bpm = if forward { after } else { before },
            Self::Pattern {
                index,
                ref value,
                ref before,
            } => {
                if let Some(before) = before {
                    project.patterns[index].clone_from(if forward { value } else { before });
                } else if forward {
                    project.patterns.insert(index, value.clone());
                } else {
                    project.patterns.remove(index);
                }
            }
            Self::Order { ref before, ref after } => project.order.clone_from(if forward { after } else { before }),
        }
    }
}

type EventOrigin = (Option<PatternEvent>, Option<EventId>);

/// One atomic edit. Row commands adapt to exact tick-event patches.
/// Invalid targets and unchanged values are ignored before recording.
pub struct Edit<'a> {
    project: &'a mut Project,
    changes: Vec<Mutation>,
    event_origins: BTreeMap<(usize, usize, u64), EventOrigin>,
    event_count: usize,
}

impl Edit<'_> {
    pub(crate) fn set_grid(&mut self, pattern: usize, after: crate::musical_time::GridMap) {
        let Some(target) = self.project.patterns.get_mut(pattern) else {
            return;
        };
        if target.grid() != &after && target.length_ticks() == after.end() {
            let before = target.grid().clone();
            target.set_grid(after.clone());
            self.changes.push(Mutation::Grid { pattern, before, after });
        }
    }

    pub fn set_channel(&mut self, track: usize, after: ChannelSettings) {
        if let Some(&before) = self.project.channels.get(track) {
            if after.is_valid() && before != after {
                self.changes.push(Mutation::Channel { track, before, after });
                self.project.channels[track] = after;
            }
        }
    }

    /// Append a shared Pluck track after checking all allocation limits.
    pub fn append_track(&mut self) {
        if !self.project.can_append_track() {
            return;
        }
        let columns: Vec<Track> = self.project.patterns.iter().map(|_| Track::default()).collect();
        let change = Mutation::Track {
            columns,
            instrument: Instrument::Pluck,
        };
        change.apply(self.project, true);
        self.event_origins.clear();
        self.changes.push(change);
    }

    /// Append sparse patterns only after checking event and structural limits.
    pub fn append_pattern(&mut self, source: usize, duplicate: bool) {
        use crate::project_file::{MAX_EVENTS, MAX_PATTERNS, MAX_PATTERN_TICKS, MAX_TRACKS};
        let Some(pattern) = self.project.patterns.get(source) else {
            return;
        };
        let tracks = self.project.instruments.len();
        if self.project.patterns.len() >= MAX_PATTERNS
            || !(1..=MAX_PATTERN_TICKS).contains(&pattern.length_ticks().0)
            || !(1..=MAX_TRACKS).contains(&tracks)
            || (duplicate && self.event_count.saturating_add(pattern.event_count()) > MAX_EVENTS)
        {
            return;
        }
        let value = if duplicate {
            pattern.duplicate()
        } else {
            let mut empty = Pattern::new(1, tracks);
            empty.resize_ticks(pattern.length_ticks());
            empty
        };
        self.event_count += value.event_count();
        let index = self.project.patterns.len();
        self.project.patterns.push(value.clone());
        self.event_origins.clear();
        self.changes.push(Mutation::Pattern {
            index,
            value,
            before: None,
        });
    }

    /// Resize one pattern after validating limits and destructive truncation.
    ///
    /// # Errors
    /// Returns an error without editing if rows, allocation or note protection fail.
    pub fn resize_pattern(&mut self, index: usize, rows: usize, force: bool) -> anyhow::Result<()> {
        use crate::project_file::MAX_ROWS;
        anyhow::ensure!((1..=MAX_ROWS).contains(&rows), "Rows must be 1..={MAX_ROWS}");
        self.set_pattern_end(index, Tick(rows as u64 * crate::musical_time::LEGACY_ROW_TICKS), force)
            .map_err(|error| anyhow::anyhow!("{error}; use :resize! {rows} to confirm truncation"))
    }

    /// Change the exact end, protecting onsets and bar overrides, not note gates.
    ///
    /// # Errors
    /// Rejects invalid lengths, allocation limits or unconfirmed truncation without mutation.
    pub fn set_pattern_end(&mut self, index: usize, end: Tick, force: bool) -> anyhow::Result<()> {
        use crate::project_file::MAX_PATTERN_TICKS;
        anyhow::ensure!(
            (1..=MAX_PATTERN_TICKS).contains(&end.0),
            "End must be 1..={MAX_PATTERN_TICKS} ticks"
        );
        let pattern = self
            .project
            .patterns
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("No edited pattern"))?;
        if end == pattern.length_ticks() {
            return Ok(());
        }
        let dropped = pattern
            .tracks
            .iter()
            .map(|track| track.events_in(end..pattern.length_ticks()).len())
            .sum::<usize>();
        let overrides = pattern.grid().overrides().filter(|(start, _)| *start >= end).count();
        anyhow::ensure!(
            force || (dropped == 0 && overrides == 0),
            "Would drop {dropped} note onsets and {overrides} bar overrides; use :end! {}t",
            end.0
        );
        let mut value = pattern.clone();
        value.resize_ticks(end);
        self.event_count -= dropped;
        let before = Some(std::mem::replace(&mut self.project.patterns[index], value.clone()));
        self.event_origins.clear();
        self.changes.push(Mutation::Pattern { index, value, before });
        Ok(())
    }

    /// Replace a validated order as one weighted history patch.
    pub fn set_order(&mut self, after: Vec<usize>) {
        if after.is_empty()
            || after.len() > crate::project_file::MAX_ORDER
            || after.iter().any(|&index| index >= self.project.patterns.len())
            || after == self.project.order
        {
            return;
        }
        let before = std::mem::replace(&mut self.project.order, after.clone());
        self.changes.push(Mutation::Order { before, after });
    }

    pub fn set_cell(&mut self, pattern: usize, track: usize, row: usize, after: Option<Note>) {
        let Some(start) = self
            .project
            .patterns
            .get(pattern)
            .and_then(|pattern| pattern.row_tick(row))
            .filter(|tick| *tick < self.project.patterns[pattern].length_ticks())
        else {
            return;
        };
        self.set_event(
            pattern,
            track,
            start,
            after.map(|note| PatternEvent::from_note(start, note)),
        );
    }

    /// Replace one exact position; invalid targets and unsupported timing are no-ops.
    pub fn set_event(&mut self, pattern: usize, track: usize, start: Tick, after: Option<PatternEvent>) {
        let _ = self.try_set_event(pattern, track, start, after);
    }

    pub(crate) fn try_set_event(
        &mut self,
        pattern: usize,
        track: usize,
        start: Tick,
        after: Option<PatternEvent>,
    ) -> anyhow::Result<()> {
        let target = self
            .project
            .patterns
            .get_mut(pattern)
            .ok_or_else(|| anyhow::anyhow!("No edited pattern"))?;
        let before_id = target.tracks.get(track).and_then(|track| track.event_id(start));
        anyhow::ensure!(
            !(after.is_some() && before_id.is_none() && self.event_count >= crate::project_file::MAX_EVENTS),
            "Project event limit reached"
        );
        let before = target.replace_event(track, start, after)?;
        self.event_count = self.event_count + usize::from(after.is_some()) - usize::from(before.is_some());
        if before != after {
            // Normalize value round-trips before a later structural edit takes a snapshot.
            let original = *self
                .event_origins
                .entry((pattern, track, start.0))
                .or_insert((before, before_id));
            if let (value, Some(id)) = original {
                if value == after {
                    target.tracks[track].restore_id(start, id);
                }
            }
            self.changes.push(Mutation::Event {
                pattern,
                track,
                start,
                before,
                after,
                before_id,
                after_id: target.tracks[track].event_id(start),
            });
        }
        Ok(())
    }

    pub(crate) fn insert_relocated_event(&mut self, pattern: usize, track: usize, event: PatternEvent, id: EventId) {
        debug_assert!(self.project.patterns[pattern]
            .event_at(track, event.start_tick)
            .is_none());
        self.try_set_event(pattern, track, event.start_tick, Some(event))
            .expect("preflighted relocation fits");
        self.project.patterns[pattern].tracks[track].restore_id(event.start_tick, id);
        // Value normalization may have restored the old destination handle. Relocation
        // deliberately carries the source handle, even when pitch/gate values coincide.
        if let Some(Mutation::Event { after_id, .. }) = self.changes.last_mut() {
            *after_id = Some(id);
        }
    }

    pub(crate) fn check_range_replacement(
        &self,
        pattern: usize,
        first_track: usize,
        range: std::ops::Range<Tick>,
        counts: &[usize],
    ) -> anyhow::Result<()> {
        use crate::project_file::{MAX_EVENTS, MAX_TRACK_EVENTS};
        let target = &self.project.patterns[pattern];
        let mut total = self.event_count;
        for (offset, &count) in counts.iter().enumerate() {
            let track = &target.tracks[first_track + offset];
            let removed = track.events_in(range.clone()).len();
            anyhow::ensure!(
                track.events().len() - removed + count <= MAX_TRACK_EVENTS,
                "Track exceeds {MAX_TRACK_EVENTS} events"
            );
            total = total - removed + count;
        }
        anyhow::ensure!(total <= MAX_EVENTS, "Project exceeds {MAX_EVENTS} events");
        Ok(())
    }

    pub(crate) fn pattern(&self, pattern: usize) -> Option<&Pattern> {
        self.project.patterns.get(pattern)
    }

    pub(crate) fn clear_range(&mut self, pattern: usize, track: usize, range: std::ops::Range<Tick>) {
        let Some(events) = self.project.patterns.get(pattern).and_then(|p| p.tracks.get(track)) else {
            return;
        };
        let starts: Vec<Tick> = events.events_in(range).iter().map(|event| event.start_tick).collect();
        for start in starts {
            self.set_event(pattern, track, start, None);
        }
    }

    pub fn set_instrument(&mut self, track: usize, after: Instrument) {
        let before = self.project.instrument(track);
        if track < self.project.instruments.len() && before != after {
            self.changes.push(Mutation::Instrument { track, before, after });
            self.project.set_instrument(track, after);
        }
    }

    #[allow(clippy::float_cmp)] // Exact document equality, not approximate DSP comparison.
    pub fn set_tempo(&mut self, after: f64) {
        if after.is_finite() && (20.0..=300.0).contains(&after) && self.project.bpm != after {
            self.changes.push(Mutation::Tempo {
                before: self.project.bpm,
                after,
            });
            self.project.bpm = after;
        }
    }
}

#[derive(Clone, Debug)]
struct Transaction {
    changes: Box<[Mutation]>,
    before: Revision,
    after: Revision,
}

impl Transaction {
    fn weight(&self) -> usize {
        self.changes.iter().map(Mutation::weight).sum()
    }
}

#[derive(Clone, Debug, Default)]
pub struct History {
    entries: VecDeque<Transaction>,
    position: usize,
    mutations: usize,
    revision: Revision,
    next_revision: u64,
}

impl History {
    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Record an edit; return whether existing dimensions or the order changed, even if the
    /// transaction exceeds the retention budget. Callers must cancel playback
    /// before the next runtime update when this returns true.
    ///
    /// # Panics
    /// Panics if the operation panics or the revision counter exhausts u64.
    pub fn edit(&mut self, project: &mut Project, operation: impl FnOnce(&mut Edit<'_>)) -> bool {
        let event_count = project.event_count();
        let mut edit = Edit {
            event_count,
            project,
            changes: Vec::new(),
            event_origins: BTreeMap::new(),
        };
        operation(&mut edit);
        // Structural changes are barriers: cells and patterns may only exist on
        // one side of them. Coalesce value writes without reordering operations.
        let mut combined = BTreeMap::<_, usize>::new();
        let mut changes: Vec<Mutation> = Vec::new();
        for change in edit.changes {
            if matches!(change, Mutation::Pattern { .. } | Mutation::Track { .. }) {
                combined.clear();
                changes.push(change);
            } else if let Some(&index) = combined.get(&change.key()) {
                changes[index].merge(change);
            } else {
                combined.insert(change.key(), changes.len());
                changes.push(change);
            }
        }
        changes.retain(Mutation::changed);
        if changes.is_empty() {
            return false;
        }
        let shape_or_order_changed = changes.iter().any(|change| {
            matches!(
                change,
                Mutation::Order { .. } | Mutation::Track { .. } | Mutation::Pattern { before: Some(_), .. }
            ) || matches!(change, Mutation::Grid { before, after, .. } if before.meter() != after.meter())
        });
        while self.entries.len() > self.position {
            self.mutations -= self.entries.pop_back().expect("redo entry").weight();
        }
        self.next_revision = self.next_revision.checked_add(1).expect("revision space exhausted");
        let after = Revision(self.next_revision);
        self.mutations += changes.iter().map(Mutation::weight).sum::<usize>();
        self.entries.push_back(Transaction {
            changes: changes.into_boxed_slice(),
            before: self.revision,
            after,
        });
        self.revision = after;
        while self.entries.len() > MAX_TRANSACTIONS || self.mutations > MAX_MUTATIONS {
            self.mutations -= self.entries.pop_front().expect("over budget").weight();
        }
        self.position = self.entries.len();
        shape_or_order_changed
    }

    /// Join adjacent channel-only edits, retaining the first before-values and
    /// the newest revision. The revision guard prevents crossing unrelated edits.
    pub(crate) fn join_channel_edits(&mut self, previous: Revision) {
        if self.position != self.entries.len() || self.position < 2 {
            return;
        }
        let last = &self.entries[self.position - 1];
        let prior = &self.entries[self.position - 2];
        if last.before != previous
            || prior.after != previous
            || !last
                .changes
                .iter()
                .chain(prior.changes.iter())
                .all(|m| matches!(m, Mutation::Channel { .. }))
        {
            return;
        }
        let last = self.entries.pop_back().expect("last edit");
        let prior = self.entries.back_mut().expect("previous edit");
        self.mutations -= last.weight() + prior.weight();
        let mut changes = prior.changes.to_vec();
        for change in last.changes {
            if let Some(existing) = changes.iter_mut().find(|m| m.key() == change.key()) {
                existing.merge(change);
            } else {
                changes.push(change);
            }
        }
        prior.changes = changes.into_boxed_slice();
        prior.after = last.after;
        self.mutations += prior.weight();
        self.position -= 1;
    }

    /// Merge only contiguous writes from one recording pass on its pinned lane.
    pub(crate) fn join_record_edits(&mut self, previous: Revision, pattern: usize, track: usize) -> bool {
        if self.position != self.entries.len() || self.position < 2 {
            return true;
        }
        let last = &self.entries[self.position - 1];
        let prior = &self.entries[self.position - 2];
        if last.before != previous
            || prior.after != previous
            || !last.changes.iter().chain(prior.changes.iter()).all(
                |change| matches!(change, Mutation::Event { pattern: p, track: t, .. } if *p == pattern && *t == track),
            )
        {
            return true;
        }
        let last = self.entries.pop_back().expect("last recording edit");
        let prior = self.entries.back_mut().expect("previous recording edit");
        self.mutations -= last.weight() + prior.weight();
        let mut changes = BTreeMap::new();
        for change in prior.changes.iter().cloned().chain(last.changes) {
            changes
                .entry(change.key())
                .and_modify(|existing: &mut Mutation| existing.merge(change.clone()))
                .or_insert(change);
        }
        prior.changes = changes.into_values().filter(Mutation::changed).collect();
        prior.after = last.after;
        self.mutations += prior.weight();
        self.position -= 1;
        if prior.changes.is_empty() {
            self.revision = prior.before;
            self.entries.pop_back();
            self.position -= 1;
            return false;
        }
        true
    }

    #[must_use]
    pub fn structural_next(&self, redo: bool) -> bool {
        let index = if redo {
            Some(self.position)
        } else {
            self.position.checked_sub(1)
        };
        index
            .and_then(|i| self.entries.get(i))
            .is_some_and(|entry| entry.changes.iter().any(Mutation::structural))
    }

    pub fn undo(&mut self, project: &mut Project) {
        if self.position == 0 {
            return;
        }
        self.position -= 1;
        let entry = &self.entries[self.position];
        for change in entry.changes.iter().rev() {
            change.apply(project, false);
        }
        self.revision = entry.before;
    }

    pub fn redo(&mut self, project: &mut Project) {
        let Some(entry) = self.entries.get(self.position) else {
            return;
        };
        for change in &entry.changes {
            change.apply(project, true);
        }
        self.revision = entry.after;
        self.position += 1;
    }
}
