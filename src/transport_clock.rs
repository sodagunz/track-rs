//! Anchored tick deadlines. The exact binary BPM value is reduced to an integer ratio.
use std::time::{Duration, Instant};

use crate::musical_time::PPQ;

#[derive(Clone, Copy, Debug)]
pub(super) struct TickClock {
    pub(super) time: Instant,
    pub(super) tick: u64,
    bpm: f64,
    numerator: u128,
    denominator: u128,
}

impl TickClock {
    pub(super) fn new(time: Instant, tick: u64, bpm: f64) -> Option<Self> {
        if !bpm.is_finite() || !(20.0..=300.0).contains(&bpm) {
            return None;
        }
        let bits = bpm.to_bits();
        let significand = u128::from((bits & ((1u64 << 52) - 1)) | (1u64 << 52));
        let exponent = u32::try_from((bits >> 52) & 0x7ff).ok()?;
        let numerator = 60_000_000_000u128.checked_shl(1075 - exponent)?;
        let denominator = significand.checked_mul(u128::from(PPQ))?;
        let mut a = numerator;
        let mut b = denominator;
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Some(Self {
            time,
            tick,
            bpm,
            numerator: numerator / a,
            denominator: denominator / a,
        })
    }

    pub(super) const fn same_tempo(self, bpm: f64) -> bool {
        self.bpm.to_bits() == bpm.to_bits()
    }

    pub(super) fn duration(self, ticks: u64) -> Option<Duration> {
        let nanos = u128::from(ticks)
            .checked_mul(self.numerator)?
            .checked_add(self.denominator / 2)?
            / self.denominator;
        Some(Duration::new(
            u64::try_from(nanos / 1_000_000_000).ok()?,
            u32::try_from(nanos % 1_000_000_000).ok()?,
        ))
    }

    pub(super) fn deadline(self, tick: u64) -> Option<Instant> {
        self.time.checked_add(self.duration(tick.checked_sub(self.tick)?)?)
    }

    pub(super) fn position(self, now: Instant) -> Option<u64> {
        let elapsed = now.checked_duration_since(self.time)?.as_nanos();
        // Invert nearest-nanosecond rounding: include exactly the ticks whose deadlines passed.
        let scaled = elapsed
            .checked_add(1)?
            .checked_mul(self.denominator)?
            .checked_sub(1 + self.denominator / 2)?;
        self.tick.checked_add(u64::try_from(scaled / self.numerator).ok()?)
    }
}

/// One occurrence in the unwrapped route; pattern ends never pad to rows or bars.
pub(super) struct Position {
    pub(super) pattern: usize,
    pub(super) order: Option<usize>,
    pub(super) local: u64,
    pub(super) prefix: u64,
}

pub(super) fn locate(
    project: &crate::project::Project,
    route: super::PlaybackRoute,
    absolute: u64,
) -> Option<Position> {
    match route {
        super::PlaybackRoute::Pattern(pattern) => {
            let length = project.patterns.get(pattern)?.length_ticks().0;
            if length == 0 {
                return None;
            }
            Some(Position {
                pattern,
                order: None,
                local: absolute % length,
                prefix: absolute / length * length,
            })
        }
        super::PlaybackRoute::Song => {
            let cycle = project.order.iter().try_fold(0u64, |sum, &index| {
                sum.checked_add(project.patterns.get(index)?.length_ticks().0)
            })?;
            if cycle == 0 {
                return None;
            }
            let mut local = absolute % cycle;
            let mut prefix = absolute / cycle * cycle;
            for (order, &pattern) in project.order.iter().enumerate() {
                let length = project.patterns.get(pattern)?.length_ticks().0;
                if local < length {
                    return Some(Position {
                        pattern,
                        order: Some(order),
                        local,
                        prefix,
                    });
                }
                local -= length;
                prefix = prefix.checked_add(length)?;
            }
            None
        }
    }
}

impl Position {
    pub(super) fn previous_boundary(&self, project: &crate::project::Project, spacing: u64) -> u64 {
        let pattern = &project.patterns[self.pattern];
        let mut previous = self.local / spacing * spacing;
        for track in &pattern.tracks {
            let index = track.events().partition_point(|event| event.start_tick.0 <= self.local);
            if let Some(event) = index.checked_sub(1).map(|index| &track.events()[index]) {
                previous = previous.max(event.start_tick.0);
            }
        }
        self.prefix + previous
    }

    pub(super) fn next_boundary(&self, project: &crate::project::Project, spacing: u64) -> Option<u64> {
        let pattern = &project.patterns[self.pattern];
        let mut next = ((self.local / spacing + 1) * spacing).min(pattern.length_ticks().0);
        for track in &pattern.tracks {
            let index = track.events().partition_point(|event| event.start_tick.0 <= self.local);
            if let Some(event) = track.events().get(index) {
                next = next.min(event.start_tick.0);
            }
        }
        self.prefix.checked_add(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_offsets_and_long_runs_use_one_anchor_not_rounded_period_addition() {
        let now = Instant::now();
        for bpm in [20.0, 121.0, 137.25, 300.0] {
            let clock = TickClock::new(now, 17, bpm).unwrap();
            for delta in [1, 15, 160, 240, 320, 960, 960 * 1_000_000] {
                let deadline = clock.deadline(17 + delta).unwrap();
                assert_eq!(clock.position(deadline), Some(17 + delta));
                assert_eq!(
                    clock.position(deadline.checked_sub(Duration::from_nanos(1)).unwrap()),
                    Some(16 + delta)
                );
                let expected = Duration::from_secs_f64(f64::from(u32::try_from(delta).unwrap()) * 60.0 / bpm / 960.0);
                assert!(clock.duration(delta).unwrap().abs_diff(expected) <= Duration::from_nanos(1));
            }
        }
    }

    #[test]
    fn tempo_segments_keep_the_committed_boundary_and_reanchor_future_ticks() {
        let now = Instant::now();
        let old = TickClock::new(now, 0, 120.0).unwrap();
        let committed = old.deadline(160).unwrap();
        let new = TickClock::new(committed, 160, 60.0).unwrap();
        assert_eq!(new.deadline(160), Some(committed));
        assert_eq!(new.deadline(175), Some(committed + Duration::from_micros(15_625)));
        assert!(TickClock::new(now, 0, f64::NAN).is_none());
        assert!(new.deadline(159).is_none());
        assert_eq!(old.position(now), Some(0));
    }
}
