# Tracks, sustained sounds, and playback

## Current status — 2026-10-17

All five phases implemented, including track creation, cursor playback, explicit note lengths, sustained presets and mode-first help.

Implementation is complete and merged. The user provisionally accepts the current
build’s general manual UI/audio gate for now (2026-10-17); the UI still feels
clunky. This does not certify every specialized historical check below.
See the [canonical backlog](README.md) for acceptance scope and next priorities,
and [015](015-test-performance.md) for the automated checkpoint. Original scope,
pending checks and test counts below are historical, not current blockers.

## Original scope and implementation record

Status: Phase 1 implemented and validated on `feat/add-tracks`, based on develop
`f0a7ea7` after the earlier stacks merged. Phase 2 implemented on
`feat/cursor-playback`, stacked on `feat/add-tracks` (PR #17). Phase 3 implemented
on `feat/note-lengths`, stacked on `feat/cursor-playback` (PR #18). Phase 4's
implementation and automated checks are complete on `feat/sustained-presets`, stacked
on `feat/note-lengths` (PR #19); listening is pending. Phase 5's implementation
and automated checks are complete on `feat/mode-first-help`, stacked on
`feat/sustained-presets` (PR #20); subjective UI review is pending.

One phase per PR. Preserve the existing modal editor, history, and project
files. No mixer, arbitrary synth editor, or track removal in this pass.

## 1. Add tracks

Touches: project model, edit history, commands/bindings, viewport, file tests.

- [x] Normal Ctrl-t appends one track to the project, not just the edited pattern.
  Add an empty column of the right row count to every pattern and one shared
  Pluck assignment. Select the new track and scroll it into view.
- [x] Keep at least one track and enforce the existing 128-track/total-cell limits
  before allocation. Reject the whole operation on failure; no partial columns.
- [x] Record one structural transaction, including all affected patterns. Undo
  removes the added column; redo restores its notes/instrument, not a fresh column.
  Account for column snapshots in the history budget; retain dirty revision rules.
- [x] Structural edits and their undo/redo cancel playback and its pending deadlines.
  Clamp cursor and cancel Visual selection after shape changes. Clipboard remains
  an independent rectangle; pasting still clips to current dimensions.
- [x] Test adding beyond four tracks across different pattern lengths, scrolling,
  limits/no-ops, atomic undo/redo, clipboard behavior, and save/load round trips.

Evidence: 75 nextest tests pass, including real-key dispatch, variable-length
columns, mixed structural transactions, limits, history budgets, clipboard,
round trips, song events, deadline cancellation, and fifth-track viewport rendering.
The full CI gate passes (cargo-deny reports existing duplicate-dependency warnings).
A 45×12 pseudo-terminal with ALSA null output passed Ctrl-t, note entry on track
five, save, Song start/stop, help, reload, and clean quit.

- [ ] Listening check: hear the saved fifth track in song playback. Null output
  checks the runtime path, not audible output; subjective UI review is also pending.

## 2. Play from cursor and restart from beginning

Touches: transport, runtime orchestration, audio output, state, input metadata.

- [x] Keep Stopped/Playing. Space stops a playing transport; from Stopped it starts
  at the current cursor. No frozen-voice pause/resume checkpoint.
- [x] In Pattern mode, Space starts the edited pattern at the cursor row. In Song
  mode, start at the selected order entry and cursor row, clamped to that entry's
  pattern length. The selected order entry determines the pattern even if the
  editor currently shows another one; show the playing target clearly.
- [x] Ctrl-Space always restarts and plays: row zero of the edited pattern in
  Pattern mode, or order zero/row zero in Song mode. Cancel old voices/deadlines
  first, even if already playing.
- [x] Decode Ctrl-Space as delivered by supported terminals, including legacy NUL
  where applicable. Test real Crossterm events; ignore releases/repeated toggles.
  Add Normal `:restart` as a fallback for terminals that intercept Ctrl-Space.
- [x] Emit the starting row immediately, then advance from anchored deadlines.
  Don't replay earlier notes whose gates overlap the cursor: start with the notes
  on the chosen row. Pin the route for this playback run; moving the edit cursor
  or switching the edited pattern doesn't seek or retarget active playback.
- [x] Space stop, file load, structural edits/history, route changes, and quit
  cancel voices and effect tails. Already-buffered device samples may finish;
  don't promise zero output latency. A later Space start uses the new cursor,
  not the old playback position or time elapsed while stopped.
- [x] Playback controls never dirty the document or enter edit history. Keep the
  existing next-row tempo-change behavior. Help/prompt input must not accidentally
  trigger playback; show stopped/playing position separately from the edit cursor.
- [x] Test nonzero cursor starts, song entry selection, repeated patterns,
  variable lengths, row clamping, stop/move/start, and Ctrl-Space while playing or
  stopped. Render through cancellation to prove old notes don't leak into a new
  start. Exercise both shortcuts through actual input.

Done: Space auditions from the cursor; Ctrl-Space plays from the mode's beginning.

Evidence: 80 nextest tests pass (75 baseline + 5 new), including offline callback
cancellation through runtime orchestration. Full CI gate and null-audio example
pass; cargo-deny retains existing duplicate-dependency warnings. A 100×24 PTY
passed Space at row 16, stop/move/start, actual Ctrl-Space NUL while stopped and
playing, `:restart`, and clean quit with ALSA null output. Listening and subjective
UI checks remain pending.

## 3. Editable note length

Touches: cell model, clipboard/history, file format, events, scheduling, UI.

- [x] Store note length as integer musical ticks: 16 ticks per row, default 16,
  range 1–4096 (1/16 row to 256 rows). Keep this separate from Insert's edit step,
  which controls cursor movement rather than sound duration.
- [x] Insert `{`/`}` halves/doubles the selected note's length, clamped at the
  limits. An empty cell is a no-op. New notes default to one row; replacing a
  pitch in a populated cell preserves its length. Show selected length in status
  and a compact length indicator without breaking note-cell alignment.
- [x] Cut/copy/paste/duplicate/save include length; clearing removes the whole
  note. Extend the edit interface and history to capture complete cell changes,
  not just pitch/octave. Each length adjustment and bulk paste is atomic.
- [x] Resolve ticks to seconds at the trigger's BPM. Already-triggered notes keep
  that resolved duration if tempo changes; future notes use the new tempo.
  Stopping cancels active gates. No infinite gates yet.
- [x] Allow gates across row, pattern, and song-loop boundaries. Give each track
  monophonic voice ownership: its next note releases the old voice with a short
  de-click fade instead of accumulating arbitrarily many sustained voices.
  Different tracks remain polyphonic; bound releasing voices too.
- [x] Introduce project format v2. Read v1 with one-row defaults; always write v2.
  Preserve v1 fixtures and add a v2 fixture. Reject zero/out-of-range ticks and
  unknown versions before replacing live data; retain atomic-save/dirty guards.
- [x] Test shortest/longest lengths, replacement/transpose, clipboard empties and
  clipped pastes, undo/redo, v1 import/v2 round trip, and gates crossing patterns.
  Verify onset/release timing at several BPMs/sample rates and bounded retriggers.

Done: note gates are editable independently of cursor step. Existing preset
identity/decay is unchanged; new sustained presets remain phase 4.

Evidence: 91 nextest tests pass (80 baseline + 11 new), including real-key length
edits, whole-note clipboard/history, strict v2 validation, and the unchanged v1
fixture. All 91 tests also pass without ignored demos or development-shell files.
Offline gates cover 1/16/4096 ticks at 50/125/750 ms row
periods and 8,000/44,100/48,000 Hz; tests cover retrigger fades, bounded releasing
voices, track polyphony, tempo changes, pattern/song-loop crossings, and runtime
restart cancellation. Full CI gate and null-audio example pass; cargo-deny retains
existing duplicate-dependency warnings. A 100×24 PTY passed length editing, v2
save/reload, real v1 demo import with one-row defaults, Space/NUL restart, help,
and clean quit using ALSA null output.

- [ ] Listening and subjective UI review: null output and offline timing checks
  do not verify audible clicks, preset identity, or terminal scanability.

## 4. Sustained presets and open hi-hat

Touches: instrument definitions, voice factory, preset selection, audio tests.

- [x] Add Pad, Drone, Sustain, and OpenHat alongside the existing presets. Keep
  Pluck and existing instrument names/defaults intact. OpenHat is noise-based and
  ignores pitch like the other percussion; the new pitched voices follow notes.
- [x] Pad: softly detuned oscillators with a gentle attack/release. Drone: a low,
  slowly moving sustained tone. Sustain: a plain stable pitched voice with a short
  attack/release. Keep these small fixed presets, not a parameter editor.
- [x] OpenHat uses a slower-decaying filtered noise envelope than HiHat. Note-off
  starts its release; longer lengths let more of its natural decay ring.
- [x] Give presets their own attack/release defaults rather than one global fade.
  Scale attack for short gates; long pitched gates hold instead of decaying to
  silence early. Retain short percussion attacks and measured level checks.
- [x] Note length ends at note-off; the preset's full release follows afterward.
  This supersedes the original inside-gate release rule after the reported
  512-tick Pad/Drone early-fade bug.
  Retrigger, stop, restart, and structural cancellation must all honor per-track
  voice ownership without resurrecting old notes or leaving tails running.
- [x] Add seeded offline tests for finite/non-silent output, held-note energy,
  onset/release continuity, open/closed-hat decay difference, mixed-level headroom,
  and no voice growth under repeated triggering. Test loaded v2 instrument names.
- [ ] Do a listening pass for sound identity, bass audibility, relative levels,
  clicks, and long holds. Automated RMS/peak checks don't replace that pass.

Implementation and automation done; listening not reviewed. Choose a pad or drone,
hold it across a bar, and mix it with an open hat.

Evidence: 96 nextest tests pass (91 baseline + 5 new), with no skipped tests,
in a fresh tracked-files-only build without ignored demos or development-shell
files. Production `OutputCommand`/`GatedVoice` tests cover deterministic holds,
pitch, onset/release continuity, and held energy at 20/120/300 BPM and
8,000/44,100/48,000 Hz. Hat comparisons and short gates use 44,100/48,000 Hz
(the hats retain the existing 7 kHz filter). Normal four-track mixes and repeated
triggers stay below clipping; this is not a limiter or a 128-track headroom claim.
Both envelope paths share preset fade settings. Existing percussion attack-level
checks and v1 fixtures remain intact; all instrument names cycle and round-trip
in v2. Full CI checks and the unchanged null-audio example pass; cargo-deny retains
existing duplicate-dependency warnings. Listening remains unchecked above.

## 5. Glanceable, mode-first help

Touches: binding metadata and help rendering; no second handwritten key table.

- [x] Group by applicability first: Shared, Normal, Insert, Visual. Keep shared
  bindings once, rather than duplicating them in every mode. Emphasize the active
  mode; category ordering within each section is secondary.
- [x] Use aligned key/action columns, short section headings, and consistent
  spacing. Merge aliases and digit ranges where they share meaning. Show modifiers
  explicitly; don't repeat category/mode labels on every binding row.
- [x] Use terminal palette/default colors. Keep close/scroll hints visible. Adapt
  to narrow screens with wrapped descriptions and section-aware scrolling, so all
  controls remain reachable even when headings consume space.
- [x] Preserve editing mode, selection, and playback state while help is open;
  clear pending counts as before. Closing help must not edit, start, or stop playback.
- [x] Test every registered binding appears in the right section with no missing
  aliases, stable mode-first ordering, active-mode emphasis, wrapping, scrolling,
  and small terminals.
- [ ] Subjective UI review: check a real terminal for scanability.

Evidence: 98 nextest tests pass (96 baseline, two help tests replaced and two
added), with no skipped tests. Actual TestBackend renders cover every wrapped
content row at 80×30, 32×8, and 24×6, active-mode bold styling, held-Down bounds,
resize clamping, and overlay focus. Full CI checks and the null-audio example
pass; cargo-deny retains existing duplicate-dependency warnings. An ALSA-null
PTY passed 24×6 help, 450 Down events, Up, resize to 80×24, Insert help,
close, and clean quit. This is runtime evidence, not subjective UI review.

Done: find a Normal editing key or Insert note-length key without reading the
whole help screen.

## Validation

Run the full existing CI gate for each phase. Add device-free regression tests
through real command dispatch and the actual audio/transport interfaces. Use a
pseudo-terminal with null audio for add-track, cursor-start/restart, length editing,
help, and save/load smoke tests. Leave listening and subjective UI checks marked
pending unless someone actually performs them.

