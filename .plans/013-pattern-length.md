# Pattern length

Add `:resize N` / `:resize! N` in Pattern and Arrangement Normal mode.
Resize the edited pattern across all tracks; repeated order entries still refer to
that same pattern. Keep gate lengths unchanged, including gates crossing its end.

Reject non-decimal integers, rows outside 1..=4096, and projects exceeding
1,000,000 cells before allocating. Refuse to trim notes without `!`; trimming
empty cells is fine. Equal lengths don't dirty the document or stop playback.

Use a replacement in the existing Pattern mutation: one atomic undo restores
all dropped notes. Count both snapshots against history's retention budget.
Actual shape changes stop playback/cancel voices, invalidate selections and clamp
current and saved cursors. No schema, audio, dependency or theme changes.

## Checks

- [x] Implement command, history and structural cleanup.
- [x] Real prompt keys: input validation, force, undo/redo, dirty state, views.
- [x] Repeated order entries, file roundtrip and allocation limits.
- [x] Mixed history barriers, snapshot retention budget and transport cancellation.
- [x] Format, strict all-target clippy, full nextest (183 passed; 177 baseline
  plus six new tests) and documentation tests (no doctests).
- [ ] Manual terminal interaction/listening (pending; no audio changes).
