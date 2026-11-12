//! Device-free timing foundation. The editor and v3 files still use legacy rows.
//! These types deliberately do not change serialized tick units or playback yet.

use std::collections::BTreeMap;

pub const PPQ: u64 = 960;
pub const LEGACY_ROW_TICKS: u64 = 240;
pub const MAX_DURATION_TICKS: u32 = 61_440;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeError {
    InvalidMeter,
    UnsupportedGrid,
    InvalidDuration,
    InvalidOverride,
    OutsidePattern,
    Overflow,
}

/// Absolute or pattern-local position; traversal arithmetic must be checked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tick(pub u64);

impl Tick {
    /// # Errors
    /// Returns `Overflow` if the sum exceeds `u64`.
    pub fn checked_add(self, ticks: u64) -> Result<Self, TimeError> {
        self.0.checked_add(ticks).map(Self).ok_or(TimeError::Overflow)
    }

    /// # Errors
    /// Returns `Overflow` if the scaled row exceeds `u64`.
    pub fn from_legacy_row(row: u64) -> Result<Self, TimeError> {
        row.checked_mul(LEGACY_ROW_TICKS).map(Self).ok_or(TimeError::Overflow)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DurationTicks(u32);

impl DurationTicks {
    /// # Errors
    /// Rejects durations outside 1–61440 ticks.
    pub fn new(ticks: u32) -> Result<Self, TimeError> {
        if (1..=MAX_DURATION_TICKS).contains(&ticks) {
            Ok(Self(ticks))
        } else {
            Err(TimeError::InvalidDuration)
        }
    }

    /// # Errors
    /// Rejects legacy gates outside 1–4096 ticks.
    pub fn from_legacy(ticks: u16) -> Result<Self, TimeError> {
        if !(1..=4096).contains(&ticks) {
            return Err(TimeError::InvalidDuration);
        }
        Self::new(u32::from(ticks) * 15)
    }

    #[must_use]
    pub const fn ticks(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meter {
    numerator: u8,
    denominator: u8,
}

impl Meter {
    /// # Errors
    /// Rejects numerators outside 1–32 or denominators other than powers of two through 64.
    pub fn new(numerator: u8, denominator: u8) -> Result<Self, TimeError> {
        if !(1..=32).contains(&numerator) || !denominator.is_power_of_two() || denominator > 64 {
            return Err(TimeError::InvalidMeter);
        }
        Ok(Self { numerator, denominator })
    }

    #[must_use]
    pub const fn numerator(self) -> u8 {
        self.numerator
    }
    #[must_use]
    pub const fn denominator(self) -> u8 {
        self.denominator
    }
    #[must_use]
    pub fn unit_ticks(self) -> u64 {
        4 * PPQ / u64::from(self.denominator)
    }
    #[must_use]
    pub fn bar_ticks(self) -> u64 {
        u64::from(self.numerator) * self.unit_ticks()
    }

    #[must_use]
    pub fn bar_start(self, position: Tick) -> Tick {
        Tick(position.0 / self.bar_ticks() * self.bar_ticks())
    }
}

/// Explicit preset set, in divisions per quarter; no rounded subdivisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid(u16);

impl Grid {
    /// # Errors
    /// Rejects subdivisions outside the exact supported preset set.
    pub const fn new(divisions_per_quarter: u16) -> Result<Self, TimeError> {
        match divisions_per_quarter {
            1 | 2 | 3 | 4 | 5 | 6 | 8 | 12 | 16 | 24 | 32 | 48 | 64 => Ok(Self(divisions_per_quarter)),
            _ => Err(TimeError::UnsupportedGrid),
        }
    }

    #[must_use]
    pub const fn divisions_per_quarter(self) -> u16 {
        self.0
    }
    #[must_use]
    pub fn spacing(self) -> u64 {
        PPQ / u64::from(self.0)
    }
}

/// Pattern-local projection policy, not a second writable musical timeline.
///
/// Bar boundaries and the explicit end are always navigation/snap boundaries,
/// including when a subdivision does not divide a bar or the final bar is partial.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridMap {
    meter: Meter,
    end: Tick,
    default: Grid,
    overrides: BTreeMap<Tick, Grid>,
}

impl GridMap {
    pub(crate) const fn legacy(end: Tick) -> Self {
        Self {
            meter: Meter {
                numerator: 4,
                denominator: 4,
            },
            end,
            default: Grid(4),
            overrides: BTreeMap::new(),
        }
    }

    #[must_use]
    pub const fn meter(&self) -> Meter {
        self.meter
    }
    #[must_use]
    pub const fn end(&self) -> Tick {
        self.end
    }
    #[must_use]
    pub const fn default_grid(&self) -> Grid {
        self.default
    }
    pub fn overrides(&self) -> impl Iterator<Item = (Tick, Grid)> + '_ {
        self.overrides.iter().map(|(&tick, &grid)| (tick, grid))
    }

    pub(crate) fn resize(&mut self, end: Tick) {
        self.end = end;
        self.overrides.retain(|start, _| *start < end);
    }

    pub(crate) const fn set_default(&mut self, grid: Grid) {
        self.default = grid;
    }

    /// # Errors
    /// Rejects a meter change that would invalidate overrides unless explicitly confirmed.
    pub fn set_meter(&mut self, meter: Meter, clear: bool) -> Result<(), TimeError> {
        if self.overrides.keys().any(|tick| meter.bar_start(*tick) != *tick) {
            if !clear {
                return Err(TimeError::InvalidOverride);
            }
            self.overrides.retain(|tick, _| meter.bar_start(*tick) == *tick);
        }
        self.meter = meter;
        Ok(())
    }

    fn rows_before_bar(&self, bar: u64) -> u64 {
        let length = self.meter.bar_ticks();
        let base = length.div_ceil(self.default.spacing());
        let mut count = bar * base;
        for &grid in self.overrides.range(..Tick(bar * length)).map(|(_, grid)| grid) {
            count = count - base + length.div_ceil(grid.spacing());
        }
        count
    }

    #[must_use]
    pub fn row_count(&self) -> u64 {
        let bar = self.end.0 / self.meter.bar_ticks();
        let tail = self.end.0 % self.meter.bar_ticks();
        let spacing = self
            .overrides
            .get(&Tick(bar * self.meter.bar_ticks()))
            .copied()
            .unwrap_or(self.default)
            .spacing();
        self.rows_before_bar(bar) + tail.div_ceil(spacing)
    }

    /// Locate a visible row without allocating or walking the preceding grid points.
    #[must_use]
    pub fn row_tick(&self, row: u64) -> Option<Tick> {
        let count = self.row_count();
        if row == count {
            return Some(self.end);
        }
        if row > count {
            return None;
        }
        let mut lower = 0;
        let mut upper = self.end.0.div_ceil(self.meter.bar_ticks());
        while lower + 1 < upper {
            let middle = lower + (upper - lower) / 2;
            if self.rows_before_bar(middle) <= row {
                lower = middle;
            } else {
                upper = middle;
            }
        }
        let start = Tick(lower * self.meter.bar_ticks());
        let grid = self.grid_at(start).ok()?;
        start
            .checked_add((row - self.rows_before_bar(lower)) * grid.spacing())
            .ok()
    }

    /// Floor projection. The end maps to the row count, not an admissible start.
    #[must_use]
    pub fn row_at(&self, tick: Tick) -> Option<u64> {
        if tick > self.end {
            return None;
        }
        if tick == self.end {
            return Some(self.row_count());
        }
        let bar = tick.0 / self.meter.bar_ticks();
        Some(self.rows_before_bar(bar) + (tick.0 % self.meter.bar_ticks()) / self.grid_at(tick).ok()?.spacing())
    }

    /// # Errors
    /// Rejects an empty pattern.
    pub const fn new(meter: Meter, end: Tick, default: Grid) -> Result<Self, TimeError> {
        if end.0 == 0 {
            return Err(TimeError::OutsidePattern);
        }
        Ok(Self {
            meter,
            end,
            default,
            overrides: BTreeMap::new(),
        })
    }

    /// # Errors
    /// Rejects positions that are not bar starts inside the pattern.
    pub fn set_bar_grid(&mut self, start: Tick, grid: Grid) -> Result<(), TimeError> {
        if start >= self.end || self.meter.bar_start(start) != start {
            return Err(TimeError::InvalidOverride);
        }
        self.overrides.insert(start, grid);
        Ok(())
    }

    /// # Errors
    /// Rejects positions at or beyond the pattern end.
    pub fn grid_at(&self, position: Tick) -> Result<Grid, TimeError> {
        if position >= self.end {
            return Err(TimeError::OutsidePattern);
        }
        Ok(self
            .overrides
            .get(&self.meter.bar_start(position))
            .copied()
            .unwrap_or(self.default))
    }

    /// Inclusive surrounding points. An exact grid position returns itself twice.
    ///
    /// # Errors
    /// Rejects positions beyond the pattern end.
    pub fn neighbors(&self, position: Tick) -> Result<(Tick, Tick), TimeError> {
        if position == self.end {
            return Ok((position, position));
        }
        let spacing = self.grid_at(position)?.spacing();
        let start = self.meter.bar_start(position);
        let offset = position.0 - start.0;
        let lower = Tick(start.0 + offset / spacing * spacing);
        if lower == position {
            return Ok((lower, lower));
        }
        // Clip distances before addition so patterns near u64::MAX remain valid.
        let remaining = (self.end.0 - start.0).min(self.meter.bar_ticks());
        let upper_offset = (offset / spacing * spacing + spacing).min(remaining);
        Ok((lower, start.checked_add(upper_offset)?))
    }

    /// Ties go forward. The end is a boundary, not an admissible note start.
    ///
    /// # Errors
    /// Rejects positions beyond the pattern end.
    pub fn nearest(&self, position: Tick) -> Result<Tick, TimeError> {
        let (lower, upper) = self.neighbors(position)?;
        Ok(if position.0 - lower.0 < upper.0 - position.0 {
            lower
        } else {
            upper
        })
    }

    /// Project only a requested bucket. All exact positions survive coarse views;
    /// callers retain event identity and values rather than replacing this bucket.
    ///
    /// # Errors
    /// Rejects starts outside the pattern or not on the projected grid.
    pub fn bucket<'a, T>(
        &self,
        start: Tick,
        events: &'a [(Tick, T)],
    ) -> Result<impl Iterator<Item = &'a (Tick, T)>, TimeError> {
        if start >= self.end {
            return Err(TimeError::OutsidePattern);
        }
        let (lower, _) = self.neighbors(start)?;
        if lower != start {
            return Err(TimeError::UnsupportedGrid);
        }
        let next = self.neighbors(start.checked_add(1)?)?.1;
        Ok(events.iter().filter(move |(tick, _)| *tick >= start && *tick < next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meters_preserve_denominator_units() {
        for (n, d, length) in [(4, 4, 3840), (3, 4, 2880), (7, 8, 3360), (6, 8, 2880)] {
            let meter = Meter::new(n, d).unwrap();
            assert_eq!(meter.bar_ticks(), length);
            assert_eq!(meter.bar_start(Tick(length + 1)), Tick(length));
        }
        assert_ne!(
            Meter::new(3, 4).unwrap().unit_ticks(),
            Meter::new(6, 8).unwrap().unit_ticks()
        );
        for (n, d) in [(0, 4), (33, 4), (4, 0), (4, 3), (4, 128)] {
            assert_eq!(Meter::new(n, d), Err(TimeError::InvalidMeter));
        }
    }

    #[test]
    fn exact_subdivisions_and_legacy_fixtures() {
        assert_eq!(Grid::new(3).unwrap().spacing(), 320);
        assert_eq!(Grid::new(64).unwrap().spacing(), 15);
        assert_eq!(Grid::new(5).unwrap().spacing(), 192);
        for invalid in [0, 7, 11, 960] {
            assert!(Grid::new(invalid).is_err());
        }
        for rows in [1, 16, 64, 4096] {
            assert_eq!(Tick::from_legacy_row(rows).unwrap(), Tick(rows * 240));
        }
        for old in 1..=4096 {
            assert_eq!(DurationTicks::from_legacy(old).unwrap().ticks(), u32::from(old) * 15);
        }
        for invalid in [0, 4097, u16::MAX] {
            assert!(DurationTicks::from_legacy(invalid).is_err());
        }
        assert!(DurationTicks::new(0).is_err());
        assert!(DurationTicks::new(61441).is_err());
        assert!(Tick::from_legacy_row(u64::MAX).is_err());
        assert!(Tick(u64::MAX).checked_add(1).is_err());
    }

    #[test]
    fn mixed_grids_do_not_modify_or_merge_events() {
        let mut map = GridMap::new(Meter::new(3, 4).unwrap(), Tick(5760), Grid::new(3).unwrap()).unwrap();
        let events = [(Tick(0), 1), (Tick(15), 2), (Tick(160), 3), (Tick(2895), 4)];
        let original = events;
        for _ in 0..10 {
            map.set_bar_grid(Tick(2880), Grid::new(64).unwrap()).unwrap();
            assert_eq!(map.bucket(Tick(0), &events).unwrap().count(), 3);
            assert_eq!(map.bucket(Tick(2880), &events).unwrap().count(), 0);
            assert_eq!(map.bucket(Tick(2895), &events).unwrap().count(), 1);
            map.set_bar_grid(Tick(2880), Grid::new(3).unwrap()).unwrap();
            assert_eq!(map.bucket(Tick(2880), &events).unwrap().count(), 1);
        }
        assert_eq!(events, original);
        assert!(map.set_bar_grid(Tick(1), Grid::new(4).unwrap()).is_err());
        assert!(map.set_bar_grid(Tick(5760), Grid::new(4).unwrap()).is_err());
    }

    #[test]
    fn partial_bars_and_ties_forward() {
        let map = GridMap::new(Meter::new(7, 8).unwrap(), Tick(3400), Grid::new(3).unwrap()).unwrap();
        assert_eq!(map.nearest(Tick(160)), Ok(Tick(320)));
        assert_eq!(map.neighbors(Tick(3359)), Ok((Tick(3200), Tick(3360))));
        assert_eq!(map.neighbors(Tick(3380)), Ok((Tick(3360), Tick(3400))));
        assert_eq!(map.nearest(Tick(3380)), Ok(Tick(3400)));
        assert_eq!(map.nearest(Tick(3400)), Ok(Tick(3400)));
        assert!(map.nearest(Tick(3401)).is_err());
    }

    #[test]
    fn neighbors_bound_every_tick_and_do_not_overflow() {
        for divisions in [1, 3, 5, 64] {
            let map = GridMap::new(Meter::new(7, 8).unwrap(), Tick(7001), Grid::new(divisions).unwrap()).unwrap();
            for tick in 0..=7001 {
                let (lo, hi) = map.neighbors(Tick(tick)).unwrap();
                assert!(lo.0 <= tick && tick <= hi.0 && hi.0 <= 7001);
                assert_eq!(map.nearest(lo), Ok(lo));
                assert_eq!(map.nearest(hi), Ok(hi));
            }
        }
        let map = GridMap::new(Meter::new(32, 1).unwrap(), Tick(u64::MAX), Grid::new(3).unwrap()).unwrap();
        assert_eq!(map.neighbors(Tick(u64::MAX - 1)).unwrap().1, Tick(u64::MAX));
    }
}
