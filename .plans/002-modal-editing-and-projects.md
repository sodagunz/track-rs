# Modal editing and projects

Status: Phases 1–4 implemented; phase 5 planned. Builds on `feat/pattern-grid` (#9).

Make writing and reusing a pattern comfortable, then make it safe to keep. Each
phase gets its own branch/stacked PR. Don't build the mixer or a full Vim clone.

## Decisions

- Start in **Normal** mode. `i` enters Insert; `v` enters Visual. `Esc` returns to
  Normal, clears pending input, and cancels a selection without changing notes.
- Keep `hjkl` navigation in every mode. Normal/Visual accept counts: `16j` moves
  16 rows; `2l` moves two tracks. Clamp at pattern edges. With four rows per beat,
  16 rows are one 4/4 bar, not four bars.
- Insert keeps the current piano keys (`zxcvbnm,`), octave (`a`/`s`), semitone
  (`f`/`r`), and Delete controls. Normal `x` clears one cell. `d` cuts the current
  cell or Visual selection; `y` copies it; `p` pastes at the cursor.
- Visual selects an inclusive rectangle between its anchor and cursor, in either
  direction. `v`/`Esc` cancels; `y`/`d` finishes the action and returns to Normal.
- Space, tempo, instrument cycling, and help remain available in all three modes.
  Help takes input priority: closing it preserves the editing mode and selection,
  but discards any pending count. Playback keeps running.
- `q`/`Q` still requests quit; add unsaved-change protection when saving lands.
- Keep the internal clipboard separate from the OS clipboard. No operator-motion
  grammar (`d16j`, `dd`), registers, macros, text-style line selections, or counts
  on edit commands yet. Invalid count sequences clear the count without editing.

## 1. Modes and counted movement

Touches: `src/bindings.rs`, `src/command.rs`, `src/state.rs`, `src/ui.rs`, input
call sites in `src/main.rs`.

- [x] Add an input resolver with mode, modifiers, and pending-count handling. It
  emits terminal-independent commands; the editor owns mode/selection state.
  Replace the old stateless dispatch path rather than maintaining two resolvers.
- [x] Extend the binding registry with mode/modifier metadata. Generate help from
  it, including counts; don't keep a separate help key table. Defer `Ctrl-r` help
  until redo is implemented in phase 2.
- [x] Add Normal/Insert/Visual transitions, counted movement, and a visible mode,
  count, and selection cue. Keep the cursor visible; cursor styling wins over
  selection, with playback still identifiable in the gutter.
- [x] Bound counts before arithmetic; dispatch a counted move once, not N times.
  `0` is only a trailing count digit for now. Unknown keys and `Esc` reset counts.
  Ignore releases; repeat movement/adjustments, not mode switches or operators.
- [x] Test actual key sequences: `i v` inserts F, Normal `v` selects, `16j`, `2l`,
  reversed selections, overflow counts, edge clamping, help interruption, and
  press/repeat/release behavior. Verify the mode in small-terminal renders.

Done: navigation and selection work without unintended note entry. Clear/cut,
copy/paste, and history follow below; don't expose unimplemented help actions.

Phase 1 validation: formatting, strict all-target Clippy, 35 nextest tests,
0 doctests, warning-free docs, actionlint, cargo-deny, and the null-audio example
passed. Cargo-deny reports duplicate-version warnings. Real-key and headless UI
tests cover modes/counts/selection/help/resizing; no interactive terminal or
audible playback pass was performed. Counts saturate at 9999.

## 2. Undoable edits

Touches: editor state/commands; extract an edit-history module if it keeps the
mutation rules in one place.

- [x] Route document changes through one edit interface: note entry/replacement,
  clear, transpose, instrument assignment, and BPM. Record before/after values;
  keep cursor, mode, selection, clipboard, entry octave, and playback out of history.
- [x] Normal `u` undoes; `Ctrl-r` redoes. Each dispatched edit is one transaction;
  later bulk operations are one transaction too. No-op edits create no entries.
- [x] New edits after undo discard redo. Bound history with a documented budget;
  retain enough information for dirty tracking even when old entries are evicted.
- [x] Undo/redo during playback affects future row events without restarting the
  transport. Don't record transport updates or generated audio.
- [x] Test every edit's undo/redo, limits, no-ops, divergent history, and playback.

Done: all supported document edits can be undone without rewinding playback.

Phase 2 uses value diffs through `App::edit`, including the public note-entry
helper. A transaction can contain multiple cell/instrument/BPM writes; repeated
writes coalesce and net no-ops preserve redo. History retains at most 256
transactions and 65,536 changed values across undo/redo. Oldest transactions are
evicted whole; an oversized edit applies but clears retained history. Revision
tokens are monotonic within the history, never reused after divergence/eviction;
undo/redo restores the original tokens for future saved-revision comparisons.
Raw pattern setters remain available for document construction, not editor edits.

Phase 2 validation: formatting, strict all-target Clippy, 41 nextest tests,
0 doctests, warning-free docs, actionlint, cargo-deny, and the null-audio example
passed. Cargo-deny reports duplicate-version warnings. Key-dispatch and headless
UI tests cover history modes/modifiers/counts/help; device-free transport tests
cover future note events and unchanged deadlines. No interactive terminal or
audible playback pass was performed. No phase 3–5 behavior is implemented.

## 3. Selection, clipboard, and edit step

- [x] Implement Normal `x`, `d`, `y`, `p`, and Visual `d`/`y`. Delete remains a
  non-cutting clear alias; `d` copies then clears. Clearing never shifts rows.
- [x] Copy a deep snapshot of the selected rectangle's cells, including empty
  cells, not track instruments or tempo. A single-cell action is a 1×1 rectangle.
- [x] Paste overwrites, including copied empty cells, starting at the cursor.
  Clip at pattern edges; don't wrap, add tracks, or resize the pattern. Empty
  clipboard is a no-op. Undo restores the entire affected region at once.
- [x] Clear selection after cut/copy; keep cursor position. Clipboard survives
  undo/redo. Selection and bulk edits must not change neighboring cells.
- [x] Add an Insert-mode edit step of 0/1/2/4 rows, default 0 (current behavior).
  Choose unused bindings and show the value in status/help. Only successful note
  entry advances; clamp at the last row. No advance on transpose, clear, or paste.
- [x] Test reversed/multi-track selections, empty cells, overlapping copy/paste,
  clipped pastes, atomic undo, and each edit step. Render selection after scrolling.

Done: a short drum phrase can be copied across rows/tracks without re-entering it.

Phase 3: Visual `x`/Delete clears the rectangle without copying, then returns to
Normal and clears the selection, just like Visual cut/copy. Cursor stays put.
Insert Tab cycles edit step 0/1/2/4 (default 0). Valid note entry advances even
when the same note is already present; that no-op preserves revision and redo.
Transpose, clear, and paste never advance. Clipboard snapshots are independent
of history, instruments, and BPM. Bulk edits use the existing `App::edit` patch
interface as one transaction.

Phase 3 validation: formatting, strict all-target Clippy, 46 nextest tests,
all-target cargo tests, 0 doctests, warning-free docs, actionlint, cargo-deny,
and the null-audio example passed. Cargo-deny reports duplicate-version warnings.
Key-dispatch tests cover clipboard rectangles, overlap, empty-cell overwrite,
edge clipping, atomic history, cancellation, counts/help/modifiers/repeats, and
all edit steps. Headless UI tests cover step status/help and scrolled selection.
No interactive terminal or audible playback pass was performed. Phases 4–5
remain unimplemented.

## 4. Minimal save/load

Touches: a small project model, a file-format module, editor state, and a minimal
command prompt. This is persistence, not an interchange format.

- [x] Separate document data from editor/runtime state. Introduce a project with
  BPM, track instruments, patterns, and a playback order, initially one pattern
  and `[0]`. Keep track assignments shared across patterns; avoid serializing
  parallel public vectors whose lengths can disagree.
- [x] Use versioned JSON with explicit pitch/instrument names and row/track
  dimensions. Version 1 includes the pattern list/order even before arrangement
  editing. Add serialization dependencies here, not in earlier phases.
- [x] Add Normal `:` with only `w [path]`, `e[!] <path>`, `q`, `q!`, and `wq`.
  Parse the path as the remainder so spaces work; no shell expansion/evaluation.
  `Esc` cancels; prompt input never dispatches note/editor shortcuts.
- [x] Save through a temporary file beside the destination, flush it, and replace
  the destination atomically where supported. Surface errors; never silently
  truncate an existing file or mark a failed save as clean.
- [x] Validate version, finite/in-range BPM, dimensions, pitches, instruments,
  order references, and documented allocation/file-size limits before replacing
  the live project. Reject unsupported versions clearly; no migration engine yet.
- [x] Track dirty state against the saved revision, including undo/redo. Refuse
  ordinary quit/load while dirty; `q!`/`e!` explicitly discards changes. Successful load
  stops audio and resets history, clipboard, selection, and cursor. Failed load
  leaves the existing project and runtime untouched.
- [x] Test round trips, a checked-in v1 fixture, unknown versions, malformed and
  oversized files, save failures preserving the old file, and dirty-state flows.

Done: compose, save, restart, load, and hear the same pattern. No MIDI, autosave,
plugin state, or compatibility promise for unpublished experimental formats.

Phase 4: `Project` owns BPM, shared instruments, patterns, and order; `App`
keeps the selected pattern index and editor/runtime state. Cell history patches
include a pattern index; no pattern/order editing or song playback is exposed.
The v1 fixture includes multiple patterns of different lengths and repeated order
entries. JSON uses named instruments/pitches and explicit row/track dimensions.
Limits are 16 MiB raw/encoded, 128 tracks, 256 patterns, 4096 rows per pattern,
1,000,000 total cells, and 65,536 order entries. Invalid files never replace the
live project. Saves use a same-directory `NamedTempFile`, flush, sync, and persist;
failed writes preserve the saved checkpoint and path. Revision tokens remain
independent of retained history. A load requests a runtime stop independently of
the playback-state mirror, then resets editor/history state only after validation.

Phase 4 validation: actionlint, formatting, strict all-target Clippy, 60 nextest
tests, all-target cargo tests, 0 doctests, warning-free docs, cargo-deny, and the
null-audio example passed using `.devenv/profile/bin` tools. Cargo-deny reports
duplicate-version warnings only; no policy exceptions or advisory suppressions
were added. Tests cover fixture/multipattern round trips, corruption and limits,
failed persist to a directory, preservation of prior bytes, dirty undo/redo and
divergence/eviction, failed-load isolation, runtime stop requests, literal prompt
keystrokes with spaces/shell metacharacters, non-Unicode remembered paths, and
headless prompt/status rendering. No interactive terminal or audible playback
pass was performed. Phase 5 remains unimplemented.

## 5. Multiple patterns and a playback order

- [ ] Add create/duplicate/switch pattern actions and a basic order-list editor.
  Start in the pattern panel; defer a full F2 arrangement view. Choose and document
  bindings alongside implementation, without stealing Insert piano keys.
- [ ] Order entries reference existing patterns and can repeat them. Allow append,
  remove, and reorder entries. Prevent an empty order and invalid references;
  defer deleting patterns to avoid reference-remapping rules in this pass.
- [ ] Keep edited pattern separate from playing pattern. Offer pattern-loop and
  song-order playback; show both positions. Song start begins at order 0/row 0;
  completing the last entry loops the order. Stop/restart keeps existing semantics.
- [ ] Extend the device-free transport without drift or catch-up bursts across
  patterns of different lengths. Editing the order stops playback before applying
  the change; undo/redo of those edits does too. Pattern switching alone doesn't.
- [ ] Include project/arrangement edits in history and persistence. Test repeated
  entries, different lengths, delayed updates across boundaries, undo/redo,
  save/load, and editing one pattern while another plays.

Done: arrange an A/A/B loop, save it, reload it, and play the same sequence.

## Validation and deferred work

For each phase, run actionlint, formatting, cargo-deny, strict all-target Clippy,
nextest, doctests, docs, and the headless example as defined in CI. Test through
real key dispatch and edit/transport interfaces, not only private helpers.

Finish with a terminal/audio pass: modes and counts, Visual copy/cut/paste, undo,
save/reload, order playback, help, and resizing. Record what was actually checked.

Later: add/remove tracks, custom themes, dedicated F-key panels, richer `:`
commands, MIDI/audio export, autosave, and broader Vim motions. Keep their ideas
in `.ideas/ui.md`; don't make them prerequisites for these phases.
