//! Value-diff transactions; editor and transport state never enter the journal.
use std::collections::{BTreeMap, VecDeque};

use crate::{
    project::Project,
    tracker::{Instrument, Note, Pattern},
};

pub const MAX_TRANSACTIONS: usize = 256;
pub const MAX_MUTATIONS: usize = 65_536;

/// Identity of a document revision, independent of retained undo depth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Revision(u64);

#[derive(Clone, Debug)]
enum Mutation {
    Pattern {
        index: usize,
        value: Pattern,
    },
    Order {
        before: Vec<usize>,
        after: Vec<usize>,
    },
    Cell {
        pattern: usize,
        track: usize,
        row: usize,
        before: Option<Note>,
        after: Option<Note>,
    },
    Instrument {
        track: usize,
        before: Instrument,
        after: Instrument,
    },
    Tempo {
        before: f64,
        after: f64,
    },
}

impl Mutation {
    const fn weight(&self) -> usize {
        match self {
            Self::Pattern { value, .. } => value.num_rows * value.tracks.len(),
            Self::Order { before, after } => before.len() + after.len(),
            _ => 1,
        }
    }

    const fn structural(&self) -> bool {
        matches!(self, Self::Pattern { .. } | Self::Order { .. })
    }

    const fn key(&self) -> (u8, usize, usize, usize) {
        match *self {
            Self::Cell {
                pattern, track, row, ..
            } => (1, pattern, track, row),
            Self::Instrument { track, .. } => (2, 0, track, 0),
            Self::Tempo { .. } => (3, 0, 0, 0),
            Self::Pattern { index, .. } => (0, index, 0, 0),
            Self::Order { .. } => (4, 0, 0, 0),
        }
    }

    #[allow(clippy::float_cmp)] // Exact document values determine no-ops.
    fn changed(&self) -> bool {
        match *self {
            Self::Cell { before, after, .. } => before != after,
            Self::Instrument { before, after, .. } => before != after,
            Self::Tempo { before, after } => before != after,
            Self::Pattern { .. } => true,
            Self::Order { ref before, ref after } => before != after,
        }
    }

    fn merge(&mut self, next: Self) {
        match (self, next) {
            (Self::Cell { after, .. }, Self::Cell { after: value, .. }) => *after = value,
            (Self::Instrument { after, .. }, Self::Instrument { after: value, .. }) => *after = value,
            (Self::Tempo { after, .. }, Self::Tempo { after: value, .. }) => *after = value,
            (Self::Order { after, .. }, Self::Order { after: value, .. }) => *after = value,
            _ => unreachable!("same mutation key"),
        }
    }

    fn apply(&self, project: &mut Project, forward: bool) {
        match *self {
            Self::Cell {
                pattern,
                track,
                row,
                before,
                after,
            } => {
                project.patterns[pattern]
                    .set_step(track, row, if forward { after } else { before })
                    .expect("history cell exists");
            }
            Self::Instrument { track, before, after } => {
                project.set_instrument(track, if forward { after } else { before });
            }
            Self::Tempo { before, after } => project.bpm = if forward { after } else { before },
            Self::Pattern { index, ref value } => {
                if forward {
                    project.patterns.insert(index, value.clone());
                } else {
                    project.patterns.remove(index);
                }
            }
            Self::Order { ref before, ref after } => project.order.clone_from(if forward { after } else { before }),
        }
    }
}

/// One atomic edit. Add any number of cells for a future rectangle operation.
/// Invalid targets and unchanged values are ignored before recording.
pub struct Edit<'a> {
    project: &'a mut Project,
    changes: Vec<Mutation>,
}

impl Edit<'_> {
    /// Append only after checking the same allocation limits as version 1 files.
    pub fn append_pattern(&mut self, source: usize, duplicate: bool) {
        use crate::project_file::{MAX_CELLS, MAX_PATTERNS, MAX_ROWS, MAX_TRACKS};
        let Some(pattern) = self.project.patterns.get(source) else {
            return;
        };
        let rows = pattern.num_rows;
        let tracks = self.project.instruments.len();
        let total: usize = self.project.patterns.iter().map(|p| p.num_rows * p.tracks.len()).sum();
        if self.project.patterns.len() >= MAX_PATTERNS
            || !(1..=MAX_ROWS).contains(&rows)
            || !(1..=MAX_TRACKS).contains(&tracks)
            || total.saturating_add(rows * tracks) > MAX_CELLS
        {
            return;
        }
        let value = if duplicate {
            pattern.clone()
        } else {
            Pattern::new(rows, tracks)
        };
        let index = self.project.patterns.len();
        self.project.patterns.push(value.clone());
        self.changes.push(Mutation::Pattern { index, value });
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
        let Some(step) = self.project.patterns.get(pattern).and_then(|p| p.get_step(track, row)) else {
            return;
        };
        if step.note != after {
            self.changes.push(Mutation::Cell {
                pattern,
                track,
                row,
                before: step.note,
                after,
            });
            // The immutable lookup above validated the same target.
            let _ = self.project.patterns[pattern].set_step(track, row, after);
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

    /// Record an edit; return whether the order actually changed, even if the
    /// transaction exceeds the retention budget. Callers must cancel playback
    /// before the next runtime update when this returns true.
    ///
    /// # Panics
    /// Panics if the operation panics or the revision counter exhausts u64.
    pub fn edit(&mut self, project: &mut Project, operation: impl FnOnce(&mut Edit<'_>)) -> bool {
        let mut edit = Edit {
            project,
            changes: Vec::new(),
        };
        operation(&mut edit);
        let mut combined = BTreeMap::<_, Mutation>::new();
        for change in edit.changes {
            match combined.entry(change.key()) {
                std::collections::btree_map::Entry::Occupied(mut entry) => entry.get_mut().merge(change),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(change);
                }
            }
        }
        let changes: Vec<_> = combined.into_values().filter(Mutation::changed).collect();
        if changes.is_empty() {
            return false;
        }
        let order_changed = changes.iter().any(|change| matches!(change, Mutation::Order { .. }));
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
        order_changed
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
