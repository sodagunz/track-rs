# 019 — Musical time, meter and local grids

Status: in progress, 2026-10-17. Implementation order and recommended design approved by “Let's start implementing in order.” Foundation before [016](016-metronome-and-record-arming.md), [017](017-quantized-live-recording.md) and recording integration in [018](018-midi-input.md). Device-free foundation and v4 tick-event persistence adapter implemented; no new dependency. Runtime notes, history patches and clipboard snapshots use tick events/ranges; UI entry and playback still use the straight-row compatibility interfaces. [Primary-source research](../.ideas/musical-time.md).

## Outcome and vocabulary

Write non-4/4 patterns, exact triplets and 256th notes; use a triplet grid in one bar and a fine straight grid in another without retiming existing notes. “Bar” (compass/measure) is musical structure, not a pattern or a fixed number of rows.

| Concept | Proposed meaning |
| --- | --- |
| Musical position | Integer ticks at fixed 960 ticks per quarter note (PPQ), independent of UI rows |
| Meter | Numerator/denominator; bar length = numerator × 4 × PPQ / denominator |
| Pattern length | Explicit end tick, independent of meter and view density; partial bars allowed |
| Grid | Editing/display divisions per quarter: straight or supported tuplet subdivision; optional per-bar override |
| Quantization | Explicit snapping of new input or a selected event range to a chosen grid; never an automatic consequence of viewing it |
| Edit step / duration | Cursor movement / note gate; independent of each other and of display density |
| Tempo | Quarter notes per minute; mapping to elapsed time, not meter or note value |

960 represents quarter triplets (320 ticks), eighth triplets (160), 256ths (15), dotted eighths (720) and fifths of a quarter (192) exactly. Start with a tested preset set; do not promise arbitrary tuplets, arbitrary denominators or an unbounded LCM. Reject unsupported non-integral subdivisions rather than silently rounding. Widen durations to validated `u32`; use checked `u64` traversal arithmetic and explicit limits. Meter denominators initially supported powers of two through 64; numerator bounded and validated (proposed 1–32).

A 6/8 bar contains six eighth-note units, not six quarters. Click grouping is separate: initially denominator-unit clicks with first-unit accent, optionally grouped 3+3 for dotted-quarter pulses. BPM remains quarter-based; label this explicitly instead of implying dotted-quarter BPM.

## Actual starting point

Evidence is merged `origin/develop` at `fb27cea`, not the older checked-out feature branch:

- `src/tracker.rs`: `Pattern.num_rows`, `Track = Vec<PatternStep>`, one optional `Note` per cell. `NoteLength(u16)` has 16 ticks/row, range 1–4096.
- `src/transport.rs`: `DEFAULT_ROWS_PER_BEAT = 4`, row deadlines, row collection and Song traversal by summed rows; gate conversion scales row duration. Current effective PPQ = 64.
- `src/project_file.rs`: version 3 dense-cell JSON, versions 1/2 migration; limits include 4096 rows, one million cells and 16 MiB encoded size.
- `src/state.rs`, `src/history.rs`, clipboard and `src/ui/presentation.rs`: row-addressed editing, value diffs, numeric New length/step and duration labels must migrate together. `src/ui.rs` derives beat landmarks from rows/beat. `src/meter.rs` is audio level metering, not musical meter.

One legacy tick already gives a 256th **gate**, but starts only occur every sixteenth. Neither a PPQ increase nor a per-cell delay permits multiple starts in one old cell.

## Options and recommendation

1. **Bounded LPB extension:** add pattern-local rows/quarter, use 12 for mixed straight/triplet rhythms or 64 for 256ths, and resample dense data on resolution changes. Smaller initial diff, but mixed resolution needs common grids, migration/collision rules and potentially huge matrices. Per-bar LPB changes also retime following rows unless positions are independently stored. Useful only as an explicitly limited intermediate experiment, not the promised solution.
2. **Sparse canonical events + projected grid (recommended):** sorted per-track events with stable identity, `start_tick`, pitch/octave and `duration_ticks`; explicit pattern end. Grid rows are a view. This is a larger coordinated migration, but directly supports multiple starts in a former row and lossless grid switching. No plugin framework, general automation engine or arbitrary rational-time library.

Introduce a small musical-time interface owning checked conversions, meter/bar lookup and grid neighbors. Pattern/event operations own insertion, exact-position collisions, range edits and bounds; transport owns clock mapping and scheduling. UI/input ask these interfaces rather than reimplementing timing arithmetic. Prototype conversion and projection with fixtures before committing to a full rewrite; keep each slice reviewable and the old grid usable until projection is ready.

## Storage, editing and compatibility

- Next project-format version stores fixed PPQ, pattern end, pattern-local meter and ordered events. One canonical timeline only; don't keep writable dense and sparse copies in sync. Preserve project BPM, order references/repeats, track instruments, channel volume/pan/tone, mute/solo semantics and note pitch/gates. No new velocity field is required.
- Load v1/v2 using their existing defaults, then convert v3 cells: start = row × 240; end = rows × 240; duration = old ticks × 15. Default row gate becomes 240; old maximum 4096 becomes 61440. `u16` happens to fit that maximum, but is too restrictive for the new general duration contract: use `u32`, checked conversions and an explicit musically equivalent minimum/maximum policy (initial 1–61440 new ticks). Versioned files must never reinterpret old numeric ticks at the new scale. Keep atomic validated load/save and reject unsupported versions without mutating the current project. Saving upgrades; no lossy legacy export initially.
- Empty legacy cells are absence of events, not note-off commands. The inspected schema has no explicit rest/note-off event. Preserve gate-based releases and current overlap/retrigger behavior through fixtures; don't invent or infer note-offs from blank rows. Clip-on-pattern-end versus sustained tails must match existing playback before extending it.
- History and clipboard store exact event values, stable IDs and relative tick ranges, not projected row numbers. Paste translates positions without implicit re-quantization. Preserve step-entry gate-retention behavior; live replacement uses New length as in 017. Stop/close takes before structural edits, resize, undo or redo; preserve dirty/no-op semantics and bounded history.
- Initial collision rule: at most one event per track at an exact start tick; intentional insertion there replaces that event. Different ticks are never merged merely because they share a visible row. Range deletion/cut acts on an explicit tick interval and includes off-grid events, with a visible count/confirmation when not individually shown. Single-cell replacement never erases an aggregated bucket.
- Numeric New length cannot silently retain its old tick meaning. Migrate stored/session defaults and document new units; offer musical duration presets. Edit step counts grid moves, not gate ticks; changing grid never stretches notes or changes pattern length. Explicit quantize is undoable and previews collisions; require confirmation or cancel rather than discard events silently.

## Meter and local-grid policy

First delivery has one meter per pattern, anchored at local tick zero; shared/repeated patterns reuse the same meter and grid map. Each Song occurrence offsets that local timeline by preceding pattern lengths. Pattern boundaries reset local bar numbering/accent phase; partial final bars remain partial, with no padding. Show partial bars so this reset isn't mistaken for a continuous song-wide meter. A continuous song meter map/pickups is a later separate decision.

Store a default grid and optional overrides keyed by local bar start tick, not a project-global LPB. Overrides affect presentation/entry defaults only; existing events and audio remain identical. Grid commands apply immediately to editing; a pending live quantization-grid change takes effect at the next local bar boundary, displayed as pending. A partial final bar rolls pending changes to the next pattern-loop start. Meter/end edits are structural: stop playback/count-in/capture, validate and commit atomically. Changing meter keeps absolute ticks/end unchanged; clear affected bar overrides only with an explicit preview/confirmation. Within-pattern meter segments can follow later, at explicit bar boundaries, with the same tick anchoring; not needed for the initial outcome.

Render only the viewport; don't allocate a finest-resolution matrix. Show bar/unit/subdivision and grid labels, off-grid markers and event counts; allow next/previous-event navigation and exact tick inspection. Coarse views may aggregate visually, never lose hidden events. Dense grids scroll/zoom locally instead of expanding every bar to thousands of rows.

## Scheduling and recording seam

Schedule event starts and gate ends at tick precision, not only at visible row boundaries. Use anchored monotonic tempo segments with integer/remainder-based conversion to deadlines or samples; don't repeatedly add rounded float row periods. Advance to the next actual event/click/end boundary, not 960 busy ticks each quarter. Preserve cancellation, pinned routes, bounded late-wake skipping and preview/song isolation. Audio callback work stays bounded, with no allocations, sorting or UI work; build bounded batches outside it and measure high-density dispatch before promising sample-accurate output.

Expose immutable run generation, absolute unwrapped tick/cycle, tempo segments and scheduled musical boundaries. 017/018 map high-resolution input timestamps through that active clock, then choose nearest grid point (ties forward), not wall-time/BPM rounding or redraw rows. Quantize across loop boundaries before mapping back into a pattern. Grid candidates on either side use the applicable bar override; simultaneous events use deterministic arrival ordering. Preserve stale-event rejection, pinned-target takes, immediate fixed-gate preview, Audition NO WRITE and unarmed preview-only contracts. Exact-tick collisions use 017's last-admitted-event rule, not last-visible-row wins.

016 count-in uses one full bar of the destination's meter/grouping, even when starting inside that bar; count-in does not move the project playhead. Clicks, accents and count text derive from meter, not hardcoded four. Existing tempo-change scheduling rules remain explicit; don't reschedule an already committed boundary.

## Delivery slices

1. Device-free musical-time types, supported subdivisions, meter/grid lookup, migration fixtures and projection prototype. Approve format/limits and pattern-local meter policy before integration; no dependency until a measured need.
2. Canonical events + versioned persistence + history/clipboard/editor compatibility on the familiar straight grid. Keep old projects sounding and editing identically.
3. Tick scheduler and Song traversal; offline density/latency tests and cancellation regressions. Do not release a format capable of events the player silently skips.
4. Non-4/4 pattern meter, per-bar grids, off-grid navigation and explicit snapping UI. Deliver mixed grids before calling 019 complete.
5. Integrate 016/017 and then 018. Preview-only MIDI adapter work may proceed independently; recording waits for this shared timing seam.

## Implementation checkpoint

- Added `src/musical_time.rs`: checked tick arithmetic, validated 1–61440-tick durations, exact legacy row/gate conversions, bounded meter and subdivision presets.
- Added pattern-local bar overrides, surrounding grid points and ties-forward snapping, including partial bars and explicit pattern-end boundaries. End is a navigation boundary, never an admissible note start.
- Added a device-free bucket projection prototype: multiple exact positions remain separate under coarse views. It scans supplied events; this is not yet a production viewport index or editor integration.
- Deterministic fixtures cover legacy gate values, mixed-grid projection, invalid input, overflow and grid-neighbor invariants. Full v1/v2/v3 project migration fixtures remain part of the persistence slice.
- Validation: all-target locked nextest passed (195 tests, zero skipped); strict all-target/all-feature Clippy, formatting, doctests (zero cases) and whitespace checks passed. No hardware/listening check ran for this device-free slice.
- No file version, note storage, command, playback behavior or New length unit changed. Slice 1 is underway; canonical event storage/persistence/history/clipboard compatibility comes next, before metronome or recording work. Mixed-grid UI and tick scheduling are not delivered yet.

### Persistence compatibility checkpoint

- Saves now upgrade to v4: fixed PPQ 960, explicit pattern end, pattern-local 4/4 meter and ordered per-track tick events. v1/v2/v3 loads retain their defaults; starts scale by 240 and legacy gate ticks by 15. Empty cells produce no events. Channels, instruments, order/repeats and gates crossing the pattern end round-trip unchanged.
- This is a bounded intermediate adapter, not completion of slice 2. The only writable in-memory timeline remains dense rows. v4 loading explicitly rejects sub-row starts, gates not divisible by 15, other meters and non-row-aligned ends rather than loading music the player would skip. Existing row/cell budgets remain compatibility limits alongside the encoded-byte limit.
- The UI New length units remain unchanged and are documented separately from file ticks. Saving upgrades the format; older app versions cannot open v4. Atomic validated loading/saving remains in place.
- Added exact migration fixtures across all three legacy versions, invalid event/order/resolution checks and forced-load atomicity coverage. All-target locked nextest: 198 passed, zero skipped; strict all-target/all-feature Clippy, formatting, doctests (zero cases) and whitespace checks passed. No hardware/listening check ran.
- Next: replace dense runtime storage and row-addressed history/clipboard with stable exact events, then tick scheduling and projected editing. Stable event identity, local grid persistence, sparse event budgets and general-duration entry are not delivered by this adapter. 019 remains in progress; do not start 016 yet.

### Sparse runtime checkpoint

- Replaced dense runtime tracks with private sorted event vectors: exact tick start, validated `u32` tick duration, pitch and octave. Empty rows allocate no cells; row reads return projected values, not mutable references. Row replacement/deletion uses exact-position binary search.
- v4 loads and saves now traverse sparse events directly, without a dense intermediate document. Legacy files still decode their dense input before conversion. All previous timing/meter guards remain; this does not admit off-row music the player cannot schedule.
- Track append, duplication, resize and structural history snapshots use sparse storage. Shrinking examines event starts, not gates; long gates and undo restoration keep their previous behavior. Conservative row/cell allocation and history budgets remain until the exact-event editing migration.
- Added dense-reference operation checks, empty-pattern storage/projection checks and start-only truncation tests. Full locked all-target nextest: 201 passed, zero skipped; strict Clippy, formatting, doctests and whitespace checks passed. No hardware/listening check ran.
- Still outstanding: tick-addressed history/clipboard, stable event identity, explicit tick-based runtime pattern length, event budgets, tick scheduler and mixed-grid editor. This checkpoint replaces the runtime-storage limitation above, not the remaining 019 acceptance criteria.

### Tick history and clipboard checkpoint

- History value patches now store exact `PatternEvent` before/after values keyed by tick position. The row edit interface translates once into the event interface; mixed writes coalesce without losing no-op, redo or revision behavior. Structural history barriers and conservative bounded retention remain unchanged.
- Clipboard snapshots own relative tick events plus an explicit tick span and track extent. Empty space remains part of paste without allocating empty cells. Cut/clear traverse starts in a half-open tick interval; paste preflights translated events, clips starts at the destination end and leaves gate durations untouched. Overlapping paste uses an owned snapshot and remains one transaction.
- Runtime event replacement validates bounds, exact-position consistency and the current legacy timing contract before mutation. Unsupported sub-row starts/gates are still rejected; no new music becomes loadable before scheduling/editor support. No implicit quantization was added.
- Added exact-event history/no-op/redo checks, half-open deletion and sparse clipboard extent/translation/clipping/atomicity tests. Full locked all-target nextest: 206 passed, zero skipped; strict all-target/all-feature Clippy, formatting, doctests and whitespace checks passed. No hardware/listening check ran.
- Still outstanding: stable event IDs, explicit tick-based runtime pattern length, event budgets, general duration entry, tick scheduler, local meter/grid persistence and projected editing. Clipboard/history no longer store projected row addresses, but this is not completion of slice 2 or 019.

## Acceptance

Deterministic tests, no sleeps or required audio hardware:

- At PPQ 960: 4/4 = 3840 ticks (legacy 16 rows), 3/4 = 2880, 7/8 = 3360, 6/8 = 2880; 256th = 15; quarter triplet = 320. Distinguish 3/4 from 6/8 accents/grouping despite equal length.
- One pattern with a 3/4 bar on a triplet grid and another on 256ths; switch either grid repeatedly and compare exact events/audio schedules. Multiple starts inside one legacy row survive save/load, clipboard, undo and coarse projections.
- Boundary and property tests: grid neighbors/ties forward, partial bars, nearest points across different grids, start/end bounds, unsupported fractions, repeated Song occurrences and non-bar-aligned pattern transitions.
- Every legacy format migrates deterministically; old row starts, gate values (including 1 and 4096), notes, order, instrument/channel values round-trip through the new format without musical changes. Validate overflow, event ordering/duplicates, duration limits, corrupted maps and atomic failed loads. Budget events/tracks/patterns/encoded bytes, not hypothetical finest-grid cell counts.
- Fixed clocks verify tempo changes, sub-row starts/gates, long-run drift/remainders, late wakes, stop/restart and stale input. Count-in for 3/4, 7/8 and grouped 6/8; recording wrap/ties/grid rollover; preview/no-write rules; bounded event batches and no callback allocation.
- UI tests cover narrow terminals, hidden-event warnings, dense scrolling, explicit quantization collisions and exact-position replacement. Run focused tests then existing full CI; separately listen to triplets/short notes/click alignment and report hardware actually checked.
