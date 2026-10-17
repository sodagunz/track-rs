# Arrangement view

## Current status — 2026-10-17

Arrangement view implemented. Plan 009 extends F1–F4 switching to every view-local mode, not just Normal.

Implementation is complete and merged. The user provisionally accepts the current
build’s general manual UI/audio gate for now (2026-10-17); the UI still feels
clunky. This does not certify every specialized historical check below.
See the [canonical backlog](README.md) for acceptance scope and next priorities,
and [015](015-test-performance.md) for the automated checkpoint. Original scope,
pending checks and test counts below are historical, not current blockers.

## Original scope and implementation record

A bounded slice of `.ideas/ui.md`: F1/F2 panel switching, not a panel framework.

- Normal F2 opens a vertically scrolling song order; F1 returns to the grid.
- Keep selected order, edited pattern, and playing entry independent. Show pattern
  row counts and distinct selection/playback markers, including repeated entries.
- Reuse existing Normal order/pattern controls, undo/redo, prompt, and transport.
  Block hidden note edits and cursor motions in the arrangement view.
- Switching panels clears pending input, not playback, document history, or cursor.
  Help and prompt retain input priority. No format changes or dependencies.
- Test actual F keys, overlay isolation, scrolling/resizing, independent playback
  cues, existing arrangement edits/history, and no dirtying from view changes.

Implemented on `feat/arrangement-view`, stacked on `feat/contextual-status-line`.

Validation: `cargo fmt --all --check`, strict all-target Clippy, actionlint,
and all-target nextest pass (123 tests, none skipped). Three new tests cover
real-key view/arrangement controls, overlay isolation, hidden-edit blocking,
playback preservation, order undo/redo, repeated-entry cues, and tiny viewports.
No interactive terminal or audible-output check was performed for this slice.

Deferred: F3 mixer, F4 instrument editor, custom themes, and subjective UI review.
