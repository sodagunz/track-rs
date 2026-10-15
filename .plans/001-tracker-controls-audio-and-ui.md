# Tracker controls, audio, and UI

Status: In progress — typed commands, default bindings, and modal keybinding help implemented; transport, tempo, instruments, and grid improvements pending.

## Goal

Make the proof of concept easier to play and extend: explicit commands, discoverable controls, built-in sounds, adjustable tempo, clearer playback code, and a readable pattern grid.

## Current state

- `src/bindings.rs` resolves terminal events through `DEFAULT_BINDINGS` into typed commands handled by `App`.
- `src/state.rs` holds editor and playback state. BPM defaults to 120, with no tempo controls.
- `src/main.rs` mixes input, rendering, timing, row advancement, and note dispatch. Timing resets to the current instant on each tick, and the calculated sleep is disabled.
- `src/audio.rs` uses one hardcoded pluck voice, a fixed note duration, and a fixed effects graph for every track.
- `src/ui.rs` renders a plain table without row numbers or musical grouping. Playback highlighting can overwrite the cursor highlight.

## Scope and decisions

- Keep existing controls where practical. Choose new bindings without collisions and document them in the same registry used by input handling.
- Use Rust enums for commands and instruments. Keep terminal key types out of application command handling.
- Treat each row as a sixteenth note: four rows per beat, with stronger grouping every sixteen rows. This makes the current timing assumption explicit.
- Start with one instrument per track, selected on the current track. Notes continue to carry pitch and octave; instrument changes affect that track's notes.
- Proposed controls: `?` toggles help, `Esc` closes it, `-`/`+` change BPM by one, and `[`/`]` cycle the current track's instrument. Confirm these during implementation against terminal key events.
- Proposed BPM range: 20–300, clamped at the command boundary. Tempo changes take effect at the next row boundary without restarting the pattern.
- Defer TOML/DSL configuration, sample loading, instrument parameter editing, song arrangement, and a general effects editor.

## Implementation plan

### 1. Introduce application commands and a binding registry

Files: `src/bindings.rs`, `src/state.rs`, a new command module, and module declarations in `src/lib.rs`/`src/main.rs` as needed.

- [ ] Define a comprehensive `Command` enum covering existing and planned actions: quit, toggle playback, move cursor in four directions, insert note, delete note, change octave, toggle/dismiss help, change tempo, and select/cycle instrument.
- [x] Give commands typed payloads where needed, such as pitch plus an octave offset for note entry. Resolve the current octave in `App::handle_command`, including clamping the upper-C shortcut safely.
- [x] Add `App::handle_command(Command)` as the entry point for user actions; retain focused state methods behind it.
- [x] Define default bindings with keys/modifiers, command, description, and help category. Resolve key events through this registry instead of duplicating a match table and help text.
- [x] Handle key press/repeat/release deliberately, with repeats allowed for movement and adjustments but not toggles. Normalize Shift on printable character events for future `?` and `+` controls.

Acceptance criteria:

- Existing movement, note entry, deletion, octave, playback, and quit controls work through commands.
- Application commands can be tested without creating terminal events.
- Every supported user action has an enum representation, and all default bindings have help metadata.

### 2. Add the keybinding help overlay

Files: `src/state.rs`, `src/ui.rs`, `src/bindings.rs`.

- [x] Track help visibility and render a centered overlay with registry categories for currently supported controls. Instrument controls remain deferred to step 5.
- [x] Generate its contents from the binding registry, including aliases and a visible close instruction.
- [x] Make help modal for editing controls; permit closing, scrolling, and quitting while keeping playback running.
- [x] Support small terminals with scrolling or a compact layout and add a persistent `? Help` hint.

Acceptance criteria:

- `?` opens and closes help; `Esc` closes it without quitting.
- Every currently supported default binding is discoverable, including aliases. Tempo and instrument bindings will appear when implemented in steps 4 and 5.
- Opening help does not interrupt playback or accidentally edit the pattern.

### 3. Separate transport timing from audio rendering

Files: `src/main.rs`, `src/audio.rs`, `src/state.rs`, and a new transport module.

- [ ] Extract a transport that owns playback deadlines and row advancement, with explicit BPM and rows-per-beat settings.
- [ ] Make start, stop, restart, row wrapping, and tempo-change behavior explicit. Start at row zero immediately and stop active voices when stopping playback.
- [ ] Advance deadlines from the previous scheduled time to avoid accumulating drift. Define bounded handling for missed deadlines so a delayed UI frame cannot trigger an unbounded burst of stale notes.
- [ ] Separate row-to-note-event collection from voice construction and output-device setup. Include track instrument and musical duration in note events.
- [ ] Keep rendering work outside the audio callback. Use the existing sequencer/backend boundary where sufficient; add message passing only where needed for scheduling and cancellation.
- [ ] Reduce the main loop to input dispatch, transport updates, note-event submission, rendering, and waiting until the next event/deadline. Restore an actual wait to eliminate busy spinning.
- [ ] Replace fixed note durations and implicit effects choices with named defaults; provide a clear dry signal path for the new sounds.

Acceptance criteria:

- Timing and event collection can be exercised without a terminal or output device.
- Start/stop/restart and pattern wrapping have predictable behavior; stop silences voices.
- Playback retains its timing across UI redraw delays, and idle operation does not busy spin.

### 4. Add tempo controls

Files: command/binding modules, `src/state.rs`, transport module, `src/ui.rs`.

- [ ] Implement bounded BPM adjustments through commands and default bindings.
- [ ] Apply tempo changes at the next row boundary while retaining the playback position.
- [ ] Keep the current BPM visible in the status bar and include controls in help.

Acceptance criteria:

- Tempo changes work while stopped and playing, cannot produce invalid intervals, and do not restart playback.
- At 120 BPM and four rows per beat, rows are scheduled 125 ms apart.

### 5. Add built-in instruments

Files: `src/tracker.rs`, `src/audio.rs`, `src/state.rs`, command/binding modules, `src/ui.rs`.

- [ ] Define an `Instrument` enum with sine, square, saw, kick, snare, and hi-hat presets.
- [ ] Extend the track model with an instrument assignment and provide commands to cycle/select it. Show the assignment in each track header.
- [ ] Extract voice construction into a factory keyed by instrument. Use note frequency for pitched oscillators and short attack/release envelopes to avoid clicks.
- [ ] Synthesize a primitive kick with a descending pitch envelope, snare with noise plus a tonal component, and closed hi-hat with filtered noise and short decay.
- [ ] Define how pitch affects percussion, use conservative preset gains, and keep duration/envelope choices explicit.
- [ ] Decide whether to retain the current pluck as an additional preset; ensure all six requested sounds are available regardless.

Acceptance criteria:

- All six requested sounds are selectable and audible through ordinary pattern playback, without external assets.
- Tracks can use different instruments, and their assignments are visible.
- Offline rendering produces finite, non-silent output for each preset, with envelopes that end cleanly. A listening pass checks sound identity, relative levels, clicks, and mixed playback.

### 6. Improve pattern readability

Files: `src/ui.rs` and any editor viewport state needed in `src/state.rs`.

- [ ] Add row numbers and consistent note-cell widths/alignment.
- [ ] Mark every four-row group with a subtle divider or background change and every sixteen-row group with a stronger separator. Anchor grouping to absolute pattern rows.
- [ ] Give cursor, playback row, and musical grouping a defined visual priority so the selected cell remains clear during playback.
- [ ] Keep the cursor visible when navigating beyond the viewport and account for separators in scrolling/layout calculations.
- [ ] Fit track/instrument headers, BPM, octave, playback status, and help hint into a coherent layout, including small terminal sizes.

Acceptance criteria:

- Four-row and sixteen-row boundaries are visually distinct, including after scrolling.
- All 64 default rows are reachable and visible through navigation.
- Cursor and playback positions remain distinguishable when they overlap.

## Progress

- Implemented commands for all existing actions, a documented default binding registry, terminal event resolution, and state dispatch. Future commands will be added alongside their behavior.
- Added tests for key resolution, modifiers, repeat/release handling, unique bindings, movement bounds, note insertion/deletion, octave bounds, playback, and quit.
- Implemented registry-generated modal help: `?` toggles, `Esc` dismisses without quitting, Up/Down scroll a bounded binding index, and a persistent `? Help` hint remains outside the overlay. Categories and all aliases (including `Q`) come from the registry. Compact borderless rendering wraps entries on small screens; physically tiny viewports degrade by clipping without panicking.
- Help blocks editor and playback-toggle commands while allowing quit, close, and scrolling. Existing playback advancement is unchanged and tested while help is open. Help toggles do not repeat; scrolling does.
- Added TestBackend coverage for every binding through scrolling at 80×30, 32×8, and 24×6, plus tiny/empty viewport safety. `cargo fmt`, `cargo fmt --check`, `cargo test` (11 tests), and `cargo clippy --all-targets` passed; repository lint warnings remain. No interactive terminal/audio pass performed.

## Order and validation

Implement in the order above: commands → help → transport/audio structure → tempo → instruments → UI polish. Keep each phase independently reviewable.

- Use focused unit tests for binding resolution, command behavior, tempo bounds, track instrument selection, and deterministic transport deadlines with a controllable clock.
- Extend existing device-free audio tests for each preset and stop behavior; use deterministic noise seeds where needed.
- Extend Ratatui `TestBackend` coverage for help content, four/sixteen-row grouping, scrolling, and overlapping cursor/playback highlights.
- Run `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets` and resolve failures attributable to the changes.
- Finish with a manual terminal/audio pass: enter a short pattern with melodic and percussion tracks, change BPM during playback, open/close help, navigate all rows, stop/restart, and resize the terminal.

## Completion checklist

- [ ] All user actions pass through typed commands.
- [x] `?` shows the complete current binding list.
- [ ] Sine, square, saw, kick, snare, and hi-hat are usable.
- [ ] Tempo is adjustable from the keyboard and visible.
- [ ] Transport, note scheduling, voice construction, and UI orchestration have clear responsibilities.
- [ ] Pattern rows have clear four-row and sixteen-row grouping.
