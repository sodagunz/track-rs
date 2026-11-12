# 017 — Quantized live recording

Status: planned, 2026-10-17. Depends on [019](019-musical-time-and-grids.md)'s event timeline and [016](016-metronome-and-record-arming.md); shares its input contract with [018](018-midi-input.md). Proposed behavior, not current controls.

## Modes and eligibility

| Context | Unarmed | Armed |
| --- | --- | --- |
| Stopped, Pattern Insert | Existing cursor step-write + preview | Same step-write + preview |
| Stopped, Pattern Audition | Preview only | Preview only |
| Playing, Pattern Insert | Preview only; no cursor write | Quantized write + immediate preview when eligible |
| Playing, Pattern Audition | Preview only | Preview only, even when armed |
| Normal, Visual, other views, help or prompt | No terminal musical input | No terminal musical input |
| Count-in, Insert/Audition | Preview only | Preview only |

This deliberately changes today's playing Insert behavior: unarmed input no longer edits the cursor cell. Stopped Insert remains an explicit editor action, not live recording. Audition retains its strong NO WRITE promise. Explain this distinction in help/status/README rather than silently reinterpreting modes.

First slice records **Pattern playback only**, onto one pinned track in the playing pattern. Starting armed captures that target; arming during eligible playback captures the current Pattern route and selected track. Require F1 Insert displaying the playing pattern to write. Moving the cursor/track or switching edit patterns never silently retargets a take; display the pinned target. Leaving eligibility closes the take and suspends capture; returning resumes a new take on the same valid target. Switching playback route stops capture. Song recording, repeated-order pattern duplication, overdub-to-new-tracks and automatic chord allocation are deferred; armed Song mode must visibly say capture unavailable.

## Timing and event policy

- Use input arrival timestamps in the transport's monotonic clock domain. Terminal timestamps mean receipt, not physical key-down; device buffering and UI delay remain limitations. MIDI will normalize its source timestamps through 018.
- Quantize to the nearest supported musical-grid boundary, ties forward, using 019's per-bar overrides. Default is four divisions per quarter (sixteenths), not a fixed storage row. Grid changes during capture activate at the next local bar boundary, or loop start after a partial final bar. Swing and latency compensation remain deferred. Ignore edit step for live capture.
- Extend the transport interface with an immutable timing snapshot: run generation, absolute unwrapped tick/cycle, adjacent scheduled musical boundaries and tempo segment. Do not quantize from `App.playback_row`, UI redraw time, or an independently recomputed BPM clock. Tempo changes retain the already scheduled boundary.
- Quantize on an unwrapped tick timeline, then map into the pinned pattern; a hit near the final boundary can belong to tick zero of the next loop. Never clamp that hit to the last displayed row. Reject timestamps before this run/count-in completion, from an old generation, or older than the snapshot's previous admitted musical boundary at ingestion; report dropped/late input rather than silently placing it elsewhere. Process admitted events in timestamp order, with arrival order breaking ties.
- Last admitted event wins when multiple notes address the same track and exact start tick; different starts in one projected row survive. Replace pitch **and gate** using the current New length (019's migrated units and limits); unlike step entry, live replacement doesn't preserve an old gate. Empty/nonempty cells follow the same rule. Unchanged cells are history no-ops.
- Terminal recording uses fixed New length, not held-key duration: release events are unreliable. Preserve Press-only note filtering and reject reported Repeat/Release. Legacy terminals may still retrigger held keys; document that limit.
- Preview immediately through the existing separate bounded preview voices. Never trigger both a new song voice and a preview for the same input. The stored event plays on the next eligible scheduler visit; already-dispatched events aren't retroactively replayed. This is quantized storage, not sample-accurate monitoring or lookahead playback.

## History and lifecycle

Use existing value-diff history, with original cell values retained once per take. One undo transaction per pattern pass; close on wrap, stop/restart, disarm, eligibility loss, target invalidation or structural edit. A recording armed mid-pass starts a partial take. No-op takes create no history. Do not keep a mutable history group open forever across loops or unrelated edits; respect retention limits. Keep dirty state accurate while capturing and flush before save. Undo/redo during capture stops capture/playback and closes the take before applying history; it must not merge into a mixer adjustment group.

Stop leaves arming on; load disarms as specified in 016. Ordinary navigation doesn't move playback or the pinned track. Structural edits retain their existing stop/cancel semantics. Counts and Normal motions must never become musical input merely because REC is armed.

## Implementation slices and seam

1. Device-free quantizer and explicit timestamp/run contract, tested at the transport interface.
2. A recording module consumes normalized note input plus transport/session context and returns preview/write decisions. Keep eligibility, cell collisions and take closure here rather than duplicating them in terminal and MIDI handlers.
3. Integrate terminal input, history, status and commands with 016. Add MIDI as the second adapter in 018; don't build a generic plugin framework first.

Touch `src/transport.rs`, `src/state.rs`, `src/history.rs`, `src/main.rs`, input bindings and presentation. 019 first migrates row-addressed storage, history/clipboard and project files to exact event positions and new duration units. Reuse that migration; recording needs no velocity field or additional schema change.

## Acceptance

Use synthetic timestamps, not sleeps: before/on/after boundaries and ties; shortest/longest patterns; multiple loops; count-in; tempo changes; stale/out-of-order events; blocked input; run cancellation; pinned target versus moving edit cursor; existing-cell replacement; collisions; save/dirty state; undo/redo and history limits. Verify every table row through actual commands/terminal events, including Audition's no-write promise and unarmed playing Insert's new behavior. Offline tests must retain song/preview isolation, bounded voices and channel mute/solo routing.

Run focused tests per slice and a full CI checkpoint when integrated. Manual acceptance: play along unarmed, arm and record after count-in, hear the next pass, disarm without stopping, then undo each pass. Check perceived timing on real output separately; don't claim hardware latency compensation.
