# Contextual status and buffer lines

## Current status — 2026-10-17

All three phases implemented: buffer identity, contextual status and responsive command/message layout.

Implementation is complete; full manual terminal/theme acceptance and listening
remain pending. User feedback was “so far looks good”, not a full sign-off.
Automated checkpoint results are recorded in [015](015-test-performance.md).
The implementation notes and test counts below are historical; README and the
binding registry describe current controls. MIDI and live recording remain deferred.

## Original scope and implementation record

Status: All three implementation phases and automated checks complete. Light/dark
readability review remains pending. No playback or data-editing behavior changes.

Replace the crowded title/status/footer with a pattern buffer line, the existing
song-order strip, and one contextual status line. Reserve the bottom command/message
line for prompts and feedback instead of repeating mode and file information.

## Layout

```text
beat1.prj *       [P0]  P1  P2                              # pattern buffers
Order 0/3         [0:P0]  1:P0  2:P1                       # song order
... pattern grid ...
NORMAL  R16 T2  C#4  512t/32r              120 BPM  Song ▶ O0 P0 R28
:w beat1.prj                                              # prompt or feedback
```

Labels and spacing are illustrative, not fixed strings. Use terminal-default and
palette colors, simple separators, and text cues that still work without color.
Keep grid space; don't add decorative borders or another panel.

## What each line owns

- **Buffer line:** project basename, project-level dirty marker, and available
  patterns. Active edit pattern is selected; playing pattern gets a separate cue.
  Patterns are buffers here, not separate files or multiple open projects.
- **Order strip:** sequence positions and their pattern references, with selected
  order entry distinct from the playing entry. Repeated patterns remain separate
  entries. Don't turn the buffer list into another arrangement list.
- **Status line:** editing mode, context under the cursor/selection, cursor
  coordinates, tempo, and compact transport information. Distinguish edit and
  playback positions; show Pattern/Song even while stopped.
- **Command/message line:** active `:` input, errors, save results, or a short
  `? Help` hint when idle. Full paths belong in messages/help, not repeated bars.

## Mode context

- **Insert:** show entry octave (`Oct 4`), edit step (`Step 1r`), and the current
  note's length when populated. Entry settings and the existing note's properties
  must be labeled separately: transposing a note doesn't change entry octave.
- **Normal:** show the cursor note and length, or an empty-cell marker. No global
  `Oct` or `Step` label: those are note-entry settings, not the selected note.
- **Visual:** show inclusive rectangle dimensions, e.g. `8r × 3t`. If pitch/length
  summaries are included, use actual selected notes: show a common value, range,
  or `mixed`, never the entry octave. Empty selections must be explicit.
- Show pending counts and `g` prefixes near the mode while they're active. Clear
  the cue with the input state; don't show a stale command after help or `Esc`.
- Label lengths in ticks/rows, not beats: `512t/32r`. Length means note-off; release
  rings afterward. Don't display an invented editable default-length setting.

## 1. Consolidate presentation state

Touches: `src/ui.rs`; extract a small UI module if it reduces the file's sprawl.

- [x] Build display fields from existing project/editor/transport state without
  duplicating document state or changing file format/history.
- [x] Share coordinate/unit formatting and pattern labels between lines. Preserve
  current zero-based row/pattern/order numbering; label track numbers consistently
  with grid headers. Test conversions instead of scattering `+ 1` operations.
- [x] Keep selection summaries bounded to the selected pattern/rectangle. Avoid
  scanning the whole project every frame; if aggregation is costly, cache by
  document revision, pattern, and selection endpoints.
- [x] Define explicit field priorities for each line. Measure rendered width,
  including Unicode, rather than slicing strings by byte length.

## 2. Pattern buffer line

- [x] Replace the app-name/title dump with basename, dirty marker, and pattern
  tabs derived from the project. Keep the active edit buffer visible as patterns
  are created, duplicated, switched, loaded, and undone.
- [x] Show left/right overflow cues and scroll the tab window. Distinguish playing
  from editing even when both refer to one pattern; never auto-switch the editor
  to follow playback. Indicate offscreen playback in status rather than stealing
  the selected buffer's viewport.
- [x] Keep buffer and order selection independent. Reuse current navigation;
  don't add mouse behavior, buffer closing, pattern renaming, or project tabs.
- [x] Test many patterns, long/Unicode basenames, unsaved projects, dirty undo/redo,
  simultaneous edit/play cues, and short viewports.

## 3. Contextual status, messages, and resizing

- [x] Replace duplicated footer/status fields with the mode-specific context
  above. Align editing information left and transport right when space allows.
  Stopped transport must not pretend a stale playing position is active.
- [x] Keep `:` input isolated from editor keys and give its text a horizontal
  viewport so the insertion point and end of a long path stay visible. Preserve
  literal path semantics. Show errors without overwriting prompt input.
- [x] Keep errors/results until the next user action or explicit dismissal, not
  until the next redraw; successful actions replace old errors. Don't clear a
  failure before the user can read it. Avoid timers unless genuinely needed.
- [x] On narrow screens, drop optional context before mode, cursor, dirty state,
  or transport state. Shorten the basename with an overflow cue. Don't let a long
  message push all useful status offscreen.
- [x] On short screens, prioritize the command line while typing, then a usable
  grid and compact mode/transport cues; collapse buffer/order lines before leaving
  no editable rows. Tiny/empty terminals must render safely.
- [x] Test all modes, mixed/empty Visual selections, changed entry octave versus
  transposed notes, length units, pending commands, help restore, dirty save/load,
  message replacement, long command input, and 80/40/24-column layouts.

## Acceptance

Run the full CI gate and Ratatui TestBackend coverage. Exercise the real app in a
pseudo-terminal: switch many patterns, edit while another plays, enter a long
save path, fail a load, open/close help, and resize. Finally check readability in
light and dark terminal themes. Record what was actually checked; automated
snapshots don't establish that the layout feels good.

No custom theme system, F-key panel framework, or multi-project buffers here.

Phase 1 passed 108 tests; phase 2 passed 112. Their stacked PRs are #23 and #24.

## Phase 3 evidence (2026-10-16)

- Status keeps mode/pending input, edit coordinates, dirty marker and transport
  before note details/BPM; transport is right-aligned. Narrow transport uses
  `P`/`S` and `▶`/`■`, with playing coordinates when they fit.
- Feedback lasts until an accepted command (including navigation at a boundary),
  changed count, accepted `g` prefix, or prompt text change. Releases, suppressed
  repeats, unknown keys and invalid chords don't dismiss it. Prompt `Esc` cancels
  and explicitly dismisses; Enter replaces feedback with the new result/error.
  Redraws don't mutate feedback. Prompt input owns its line ahead of messages.
- Prompt and basename clipping use Ratatui's grapheme iterator; no new dependency.
  The append-only prompt reserves a cell for the real terminal insertion cursor.
- Below four rows, drop idle feedback/hint before the grid. While typing, reserve
  the prompt first; keep a two-row grid whenever three rows are available. Status
  appears when a two-row grid still fits; buffer/order need six/seven rows.
- TestBackend covers 80/40/24 columns, heights 0–10, modes, dirty state, prompt
  tail/cursor, message persistence, and literal temporary Unicode paths. Existing
  tests cover selection summaries, tick fractions, checkpoints and independent
  playing/order/edit cues.
- Real app PTY with null ALSA: 19 patterns, switch/edit during playback, help
  open/close, literal long Unicode save verified on disk, failed load, resizing
  through 80/40/24 columns down to three rows, clean exit.
- CI checks: formatting, Clippy with `-D warnings`, cargo-deny (existing duplicate
  dependency warnings), actionlint, all-target nextest, doc tests, documentation
  with warnings denied, and the null-audio example. Final test count recorded in
  the completion report.
- Not checked: subjective readability in light/dark themes or audible output.
