//! Explicit onset snapping. Planning is read-only; blocked plans never enter history.
use std::{collections::BTreeSet, fmt::Write as _, ops::Range};

use crate::{
    history::Edit,
    musical_time::Tick,
    tracker::{EventId, Pattern, PatternEvent},
};

struct Move {
    track: usize,
    before: PatternEvent,
    destination: Tick,
    id: EventId,
}

pub struct Quantization {
    moves: Vec<Move>,
    selected: usize,
    collisions: usize,
    end_hits: usize,
    first_collision: Option<(usize, Tick)>,
}

impl Quantization {
    pub(crate) fn plan(pattern: &Pattern, tracks: Range<usize>, range: Range<Tick>) -> Self {
        let mut plan = Self {
            moves: Vec::new(),
            selected: 0,
            collisions: 0,
            end_hits: 0,
            first_collision: None,
        };
        for track in tracks {
            let mut destinations = BTreeSet::new();
            for &before in pattern.tracks[track].events_in(range.clone()) {
                plan.selected += 1;
                let destination = pattern.grid().nearest(before.start_tick).expect("event inside grid");
                if destination == pattern.length_ticks() {
                    plan.end_hits += 1;
                    continue;
                }
                if !destinations.insert(destination)
                    || (!range.contains(&destination) && pattern.event_at(track, destination).is_some())
                {
                    plan.collisions += 1;
                    plan.first_collision.get_or_insert((track, destination));
                }
                if destination != before.start_tick {
                    plan.moves.push(Move {
                        track,
                        before,
                        destination,
                        id: pattern.tracks[track]
                            .event_id(before.start_tick)
                            .expect("event has identity"),
                    });
                }
            }
        }
        plan
    }

    pub(crate) fn summary(&self) -> String {
        let mut text = format!(
            "Quantize: {}/{} events move; {} collisions; {} end hits",
            self.moves.len(),
            self.selected,
            self.collisions,
            self.end_hits
        );
        if let Some((track, tick)) = self.first_collision {
            let _ = write!(text, " (first T{} @ {}t)", track + 1, tick.0);
        }
        text
    }

    pub(crate) fn apply(self, edit: &mut Edit<'_>, pattern: usize) -> anyhow::Result<usize> {
        anyhow::ensure!(
            self.collisions == 0 && self.end_hits == 0,
            "{}; unchanged: refine grid/selection or edit exact events",
            self.summary()
        );
        let target = edit
            .pattern(pattern)
            .ok_or_else(|| anyhow::anyhow!("No edited pattern"))?;
        for movement in &self.moves {
            anyhow::ensure!(
                target.event_at(movement.track, movement.before.start_tick) == Some(movement.before)
                    && target.tracks[movement.track].event_id(movement.before.start_tick) == Some(movement.id),
                "Quantization target changed"
            );
        }
        // Remove every old onset before inserting destinations: chains of moves must not
        // overwrite each other or temporarily exceed event budgets.
        for movement in self.moves.iter().rev() {
            edit.set_event(pattern, movement.track, movement.before.start_tick, None);
        }
        for movement in &self.moves {
            edit.insert_relocated_event(
                pattern,
                movement.track,
                PatternEvent {
                    start_tick: movement.destination,
                    ..movement.before
                },
                movement.id,
            );
        }
        Ok(self.moves.len())
    }
}
