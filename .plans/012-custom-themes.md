# Custom themes

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
