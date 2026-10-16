# Tracks, sustained sounds, and playback

Status: Phase 1 implemented and validated on `feat/add-tracks`, based on develop
`f0a7ea7` after the earlier stacks merged. Phase 2 implemented on
`feat/cursor-playback`, stacked on `feat/add-tracks` (PR #17). Phases 3–5 pending.

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

- [ ] Store note length as integer musical ticks: 16 ticks per row, default 16,
  range 1–4096 (1/16 row to 256 rows). Keep this separate from Insert's edit step,
  which controls cursor movement rather than sound duration.
- [ ] Insert `{`/`}` halves/doubles the selected note's length, clamped at the
  limits. An empty cell is a no-op. New notes default to one row; replacing a
  pitch in a populated cell preserves its length. Show selected length in status
  and a compact length indicator without breaking note-cell alignment.
- [ ] Cut/copy/paste/duplicate/save include length; clearing removes the whole
  note. Extend the edit interface and history to capture complete cell changes,
  not just pitch/octave. Each length adjustment and bulk paste is atomic.
- [ ] Resolve ticks to seconds at the trigger's BPM. Already-triggered notes keep
  that resolved duration if tempo changes; future notes use the new tempo.
  Stopping cancels active gates. No infinite gates yet.
- [ ] Allow gates across row, pattern, and song-loop boundaries. Give each track
  monophonic voice ownership: its next note releases the old voice with a short
  de-click fade instead of accumulating arbitrarily many sustained voices.
  Different tracks remain polyphonic; bound releasing voices too.
- [ ] Introduce project format v2. Read v1 with one-row defaults; always write v2.
  Preserve v1 fixtures and add a v2 fixture. Reject zero/out-of-range ticks and
  unknown versions before replacing live data; retain atomic-save/dirty guards.
- [ ] Test shortest/longest lengths, replacement/transpose, clipboard empties and
  clipped pastes, undo/redo, v1 import/v2 round trip, and gates crossing patterns.
  Verify onset/release timing at several BPMs/sample rates and bounded retriggers.

Done: the same pitched preset can make a short stab or a held multi-bar note.

## 4. Sustained presets and open hi-hat

Touches: instrument definitions, voice factory, preset selection, audio tests.

- [ ] Add Pad, Drone, Sustain, and OpenHat alongside the existing presets. Keep
  Pluck and existing instrument names/defaults intact. OpenHat is noise-based and
  ignores pitch like the other percussion; the new pitched voices follow notes.
- [ ] Pad: softly detuned oscillators with a gentle attack/release. Drone: a low,
  slowly moving sustained tone. Sustain: a plain stable pitched voice with a short
  attack/release. Keep these small fixed presets, not a parameter editor.
- [ ] OpenHat uses a slower-decaying filtered noise envelope than HiHat. The gate
  still bounds the sound; use longer note lengths to let the open tail ring.
- [ ] Give presets their own attack/release defaults rather than one global fade.
  Scale fades for short gates; long pitched gates hold instead of decaying to
  silence early. Retain short percussion attacks and measured level checks.
- [ ] Budget release inside the note's total gate length for predictable timing.
  Retrigger, stop, restart, and structural cancellation must all honor per-track
  voice ownership without resurrecting old notes or leaving tails running.
- [ ] Add seeded offline tests for finite/non-silent output, held-note energy,
  onset/release continuity, open/closed-hat decay difference, mixed-level headroom,
  and no voice growth under repeated triggering. Test loaded v2 instrument names.
- [ ] Do a listening pass for sound identity, bass audibility, relative levels,
  clicks, and long holds. Automated RMS/peak checks don't replace that pass.

Done: choose a pad or drone, hold it across a bar, and mix it with an open hat.

## 5. Glanceable, mode-first help

Touches: binding metadata and help rendering; no second handwritten key table.

- [ ] Group by applicability first: Shared, Normal, Insert, Visual. Keep shared
  bindings once, rather than duplicating them in every mode. Emphasize the active
  mode; category ordering within each section is secondary.
- [ ] Use aligned key/action columns, short section headings, and consistent
  spacing. Merge aliases and digit ranges where they share meaning. Show modifiers
  explicitly; don't repeat category/mode labels on every binding row.
- [ ] Use terminal palette/default colors. Keep close/scroll hints visible. Adapt
  to narrow screens with wrapped descriptions and section-aware scrolling, so all
  controls remain reachable even when headings consume space.
- [ ] Preserve editing mode, selection, and playback state while help is open;
  clear pending counts as before. Closing help must not edit, start, or stop playback.
- [ ] Test every registered binding appears in the right section with no missing
  aliases, stable mode-first ordering, active-mode emphasis, wrapping, scrolling,
  and small terminals. Check a real terminal for scanability.

Done: find a Normal editing key or Insert note-length key without reading the
whole help screen.

## Validation

Run the full existing CI gate for each phase. Add device-free regression tests
through real command dispatch and the actual audio/transport interfaces. Use a
pseudo-terminal with null audio for add-track, cursor-start/restart, length editing,
help, and save/load smoke tests. Leave listening and subjective UI checks marked
pending unless someone actually performs them.

