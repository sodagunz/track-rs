//! A click calendar on the transport's active tempo segment, never a second timer.
use std::time::{Duration, Instant};

use super::{clock, ClickEvent, MetronomeEvent, PlaybackRoute};

/// Compound meters group three denominator units; simple and irregular meters
/// retain denominator clicks. This changes the click only, never the edit grid.
pub(super) fn click_ticks(meter: crate::musical_time::Meter) -> u64 {
    let compound = meter.denominator() >= 8 && meter.numerator() >= 6 && meter.numerator().is_multiple_of(3);
    meter.unit_ticks() * if compound { 3 } else { 1 }
}

#[derive(Default)]
pub(super) struct Metronome {
    enabled: bool,
    next: Option<u64>,
    pending: Option<MetronomeEvent>,
}

impl Metronome {
    pub(super) const fn clear(&mut self) {
        self.next = None;
        self.pending = None;
    }

    pub(super) fn deadline(&self, clock: Option<clock::TickClock>) -> Option<Instant> {
        clock?.deadline(self.next?)
    }

    pub(super) const fn take_event(&mut self) -> Option<MetronomeEvent> {
        self.pending.take()
    }

    pub(super) fn update(
        &mut self,
        now: Instant,
        enabled: bool,
        clock: Option<clock::TickClock>,
        project: &crate::project::Project,
        route: PlaybackRoute,
    ) {
        self.pending = None;
        if self.enabled && !enabled {
            self.pending = Some(MetronomeEvent::Stop);
        }
        self.enabled = enabled;
        let Some(clock) = clock.filter(|_| enabled) else {
            self.next = None;
            return;
        };
        let Some(absolute) = clock.position(now) else {
            return;
        };
        let Some(position) = clock::locate(project, route, absolute) else {
            return;
        };
        let meter = project.patterns[position.pattern].grid().meter();
        let interval = click_ticks(meter);
        let local = position.local / interval * interval;
        let tick = position.prefix + local;
        let scheduled = clock.deadline(tick);
        let due = self
            .next
            .map_or_else(|| scheduled == Some(now), |next| next <= absolute);
        if due {
            // One latest fresh click at most, even if a delayed wake crossed many units/loops.
            if let Some(scheduled_at) =
                scheduled.filter(|at| now.saturating_duration_since(*at) <= Duration::from_millis(50))
            {
                self.pending = Some(MetronomeEvent::Click(ClickEvent {
                    accent: local.is_multiple_of(meter.bar_ticks()),
                    pattern: position.pattern,
                    order: position.order,
                    tick: crate::musical_time::Tick(local),
                    absolute_tick: tick,
                    scheduled_at,
                }));
            }
        }
        if due || self.next.is_none() {
            let end = project.patterns[position.pattern].length_ticks().0;
            self.next = position
                .prefix
                .checked_add(((position.local / interval + 1) * interval).min(end));
        }
    }
}
