# Custom themes

## Current status — 2026-10-17

Custom themes and the alternating-row shading follow-up (PR #42) implemented. Both are included in this continuation.

Implementation is complete and merged. The user provisionally accepts the current
build’s general manual UI/audio gate for now (2026-10-17); the UI still feels
clunky. This does not certify every specialized historical check below.
See the [canonical backlog](README.md) for acceptance scope and next priorities,
and [015](015-test-performance.md) for the automated checkpoint. Original scope,
pending checks and test counts below are historical, not current blockers.

## Original scope and implementation record

Keep terminal-palette defaults; custom RGB is explicit opt-in. Store presentation
settings on App, outside project serialization/history. No audio changes or deps.

- [x] Add strict partial JSON theme parsing, pure path discovery/loading, and
  `--theme PATH`; validate before audio/terminal startup.
- [x] Centralize semantic styles across panels, status/buffer strips, help and
  prompts. Keep a small role set, with shared highlight foreground.
- [x] Test defaults/partial/invalid files, discovery priority and Unicode paths;
  exercise themed rendering across modes/views and narrow/full-width layouts.
- [x] Document discovery and a minimal example; run fmt, strict clippy, full
  nextest, warning-denied docs and doctests.
- [x] Commit and open stacked PR against `feat/mixer-followup-next`.

Validation: 177/177 nextest tests passed (170 inherited + 7 new), fmt check,
strict all-target/all-feature clippy, warning-denied docs and doctests (0),
and example test target (0 tests). Startup subprocess tests cover invalid and
missing explicit themes before audio/terminal initialization. No workflow or
audio implementation changes; actionlint/null-audio reruns not needed.

Manual terminal palette/truecolor appearance and interactive listening checks
remain pending; buffer tests do not establish readability or terminal support.

Correction: the default grid inherited unshaded rows from the earlier pattern
landmarks change (91a057c), not from custom themes. The row-shading follow-up
adds a semantic alternate background and rendered-cell regression coverage.

Row-shading follow-up validation: 4 theme unit tests, 52 UI/presentation tests,
2 theme-startup tests, fmt check, strict all-target/all-feature clippy and
`git diff --check` passed. The rendered regression was first observed failing
without the row style. Full suite/nextest deliberately not run for this focused
fix. Manual light/dark terminal contrast checks remain pending.
