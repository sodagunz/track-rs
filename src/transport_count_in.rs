//! One full destination bar on the transport clock, separate from song ticks.
use std::time::{Duration, Instant};

use super::{click, clock, ClickEvent};
use crate::musical_time::{Meter, Tick};

pub(super) struct CountIn {
    clock: clock::TickClock,
    next: u64,
    interval: u64,
    end: u64,
    pattern: usize,
}

pub(super) struct Update {
    pub(super) click: Option<ClickEvent>,
    pub(super) complete: Option<Instant>,
}

impl CountIn {
    pub(super) fn new(now: Instant, bpm: f64, meter: Meter, pattern: usize) -> Option<Self> {
        let clock = clock::TickClock::new(now, 0, bpm)?;
        clock.deadline(meter.bar_ticks())?;
        Some(Self {
            clock,
            next: 0,
            interval: click::click_ticks(meter),
            end: meter.bar_ticks(),
            pattern,
        })
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.clock.deadline(self.next)
    }

    pub(super) const fn beat(&self) -> (u64, u64) {
        (self.next / self.interval, self.end / self.interval)
    }

    pub(super) fn update(&mut self, now: Instant, bpm: f64) -> Option<Update> {
        let boundary = self.deadline()?;
        if now < boundary {
            return Some(Update {
                click: None,
                complete: None,
            });
        }
        // The pending beat keeps its deadline. New tempo governs intervals after it.
        if !self.clock.same_tempo(bpm) {
            self.clock = clock::TickClock::new(boundary, self.next, bpm)?;
        }
        let end = self.clock.deadline(self.end)?;
        if now >= end {
            return Some(Update {
                click: None,
                complete: Some(end),
            });
        }
        let tick = self.clock.position(now)? / self.interval * self.interval;
        let scheduled_at = self.clock.deadline(tick)?;
        self.next = tick.checked_add(self.interval)?;
        let click = (now.saturating_duration_since(scheduled_at) <= Duration::from_millis(50)).then_some(ClickEvent {
            accent: tick == 0,
            pattern: self.pattern,
            order: None,
            tick: Tick(tick),
            absolute_tick: tick,
            scheduled_at,
        });
        Some(Update { click, complete: None })
    }
}
