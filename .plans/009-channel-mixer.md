# 009 — Channel mixer

Status: foundation and volume/pan submitted as PR #31, stacked on #30.
Tone controls implemented on `feat/channel-tone`, ready for stacked review.
Bulk Visual edits and bounded adjustment undo are implemented on
`feat/mixer-bulk-next`, stacked on PR #36. Meters remain deferred.

Build a usable gain/pan mixer first, then add the requested tone controls. No
parametric EQ, routing graph, sends, automation, or custom effects in this plan.
Low/high shelves are EQ already; they aren't being replaced with instrument tone
controls.

## Interaction

Columns are project tracks; rows are controls. Keep channel headers and parameter
labels visible while scrolling. Each cell shows its value and unit, not just a
knob drawing. Highlight the selected cell and channel with terminal palette colors.
Keep the selected control visible on small terminals; don't shrink values to noise.

Modes belong to the view, not to the whole application. Keep each view's mode and
cursor when switching panels. Pattern Insert/Visual state must not turn into mixer
input or be modified by mixer navigation. Clear pending counts/prefixes on switching;
preserve a pattern selection for returning to F1. F1–F4 switch views in every mode,
except while a prompt or help owns input.

### Mixer Normal

- `h`/`l`: previous/next channel; `j`/`k`: next/previous control.
- Counts work: `3l`, `2j`. Clamp at edges, don't wrap.
- `gh`/`gl`: first/last channel; `gk`/`gj`: first/last control.
- `i`: enter mixer Insert (adjustment) mode on the selected cell.
- `v`: rectangular Visual selection, using the same inclusive geometry as patterns.
- `u`/Ctrl-r: undo/redo mixer edits without stopping playback.

### Mixer Insert

- `h`/`l`: decrease/increase the selected value; `j`/`k`: select the next/previous
  control on that channel. Arrow keys follow the same mapping.
- Counts multiply the normal increment; `10l` is ten increments, not ten channels.
- Enter opens a numeric prompt for the selected control, with its unit shown.
  Accept finite in-range numbers; reject invalid input without changing the project.
- Esc leaves a numeric prompt first, then returns to mixer Normal.

### Mixer Visual

- `hjkl`, counts, and `g` motions extend the rectangle.
- `a`/`s`: decrease/increase every selected control by its own increment, atomically.
  This mirrors pattern transposition without borrowing its octave/semitone meanings.
- `d`/`x`: reset selected controls to their neutral defaults, atomically.
- Esc clears selection and returns to mixer Normal. No note clipboard operations.

Normal `d`/`x` resets the current cell. Backspace in Insert does the same.
No shifted coarse-adjustment keys initially: counts already provide coarse control.
A held adjustment key forms one undo gesture; close it on release, navigation,
mode/view change, prompt, or another command. Keep explicit counted edits atomic.

Space/Ctrl-Space, tempo, help, project commands, and quit remain application-level.
View bindings resolve before unrelated pattern bindings; `a`, `s`, `d`, and `i`
must never reach hidden pattern edits. Registry-derived help lists only commands
available in the active view, grouped by its modes. Status shows e.g.
`MIXER INSERT | T2 Pan R25`, rather than a hidden pattern note or entry octave.

## Channel controls

All settings belong to a project track, shared by every pattern and order entry.
New tracks and older project files start neutral.

| Control | Range | Default | Increment |
| --- | --- | --- | --- |
| Volume | −60 to +6 dB, plus −∞ | 0 dB | 1 dB |
| Pan | L100 through centre to R100 | Centre | 1 percentage point |
| Low shelf | −12 to +12 dB at 200 Hz | 0 dB | 1 dB |
| High shelf | −12 to +12 dB at 4 kHz | 0 dB | 1 dB |
| HPF | Off; 20 Hz–20 kHz | Off | One semitone in frequency |
| LPF | Off; 20 Hz–20 kHz | Off | One semitone in frequency |

Volume below −60 dB becomes −∞; increasing from −∞ returns to −60 dB.
For filters, decreasing below 20 Hz turns HPF off; increasing above 20 kHz turns
LPF off. Increasing HPF from Off starts at 20 Hz; decreasing LPF from Off starts
at 20 kHz. Movement away from the active range while Off does nothing. Numeric
entry accepts `off` for filters and `-inf` for volume. Display cutoff values in Hz
or kHz without changing their stored value. Use fixed, non-resonant filter shapes;
no exposed Q or resonance. HPF and LPF remain independent and may overlap.

Pan is stereo balance: centre preserves the existing stereo signal, endpoints
silence the opposite side. Don't fold chorus to mono or boost the surviving side.
Label this in help. Neutral settings must preserve today's output level and width.

Use 12 dB/octave HPF/LPF and fixed gentle shelves. Clamp effective frequencies
below Nyquist for the actual output sample rate, including shelf frequencies;
retain the requested setting in the document. Off means bypass, not an extreme
cutoff. Gain/pan and bypass transitions must be smoothed; filter sweeps must stay
stable. Don't claim click-free changes until measured and listened to.

Boost can overload the master sum. Show a clip indicator when metering lands;
don't silently add a limiter or change global gain. Initially document that boosts
can clip and test finite output even when it exceeds the device range.

## Implementation seams

- `src/project.rs`: validated channel settings beside shared instrument assignments.
  Keep one settings value per track. No DSP memory in the document.
- `src/history.rs`: channel-setting mutations through existing document transactions.
  Include track append/undo/redo and dirty tracking. Parameter edits aren't structural
  edits and must not cancel playback.
- `src/project_file.rs`: version 3 with explicit v1/v2 migration to neutral settings;
  preserve existing v1 note migration. Validate lengths, finite values, and ranges.
- `src/audio.rs`: one persistent channel processor after each track's active/releasing
  voice sum and before the master sum. Filters and smoothers survive retriggers and
  process release tails. Use the same track path for instrument audition.
- Deliver changed targets through the existing output-command seam. Coalesce unchanged
  settings on the control side; don't rebuild voices, allocate filter graphs, or send
  complete project snapshots per sample. Keep processing state on the audio side.
- `src/main.rs`: synchronize load, undo/redo, track creation, and edits—not just key
  handlers. Reset old DSP memory on project replacement/transport cancellation without
  losing current targets. Install settings before new note triggers.
- `src/bindings.rs`, `src/state.rs`: explicit view-local command resolution, mode and
  cursor state. Keep count/prefix, prompt/help priority and repeat handling centralized.
- `src/ui.rs`, `src/ui/presentation.rs`: channel grid and view-specific status/help.
  Audit the legacy sequencer/example path so any bypass of channel processing is explicit.

## Stack and acceptance

1. **View-local interaction foundation.** Separate view modes/cursors and binding
   contexts; make F1–F4 available across modes. Tests cover real keys, counts,
   prefixes, switching with a Visual selection, help/prompt priority, and no hidden
   edits. Update README and help together.
2. **Working volume/pan mixer.** Add settings, v3 migration, history, live channel
   processing and the F3 grid. Implement Normal/Insert, numeric entry and reset.
   Verify held notes and tails respond without retriggering, track isolation,
   centre/endpoints, neutral equivalence, smoothing at several sample rates,
   append/history/load synchronization, and project round trips. This PR must
   actually change audio; don't ship another control-only shell.
3. **Tone controls.** Add shelves and HPF/LPF to that same processor and grid.
   Verify frequency response, bypass neutrality, cutoff limits at low sample rates,
   rapid sweeps, silence/tails, and stable finite output. Keep the first usable
   mixer if this slice takes longer; don't block gain/pan on filters.
4. **Bulk adjustment and polish.** Visual rectangles, per-parameter increments,
   grouped repeat undo, scrolling and narrow-terminal tests. Add per-channel stereo
   peak meters and a master clip indicator only after the controls work; meters
   must not allocate or lock the callback and aren't saved or undoable.

### Foundation progress (2026-10-16)

- Views save and restore their own mode, cursor and selection. F1–F4 work from
  Pattern Normal/Insert/Visual, including pending counts/prefixes; help and prompts
  retain priority. Playback starts use the preserved pattern cursor.
- Command availability is shared by resolution, dispatch and registry-derived help.
  F3/F4 keep working voice edits and counted horizontal navigation without hidden
  pattern edits. Status reports the active panel/voice rather than hidden notes.
- Structural edits/history invalidate saved selections; restored cursors clamp to
  current dimensions. Loading resets saved view state.
- Deferred to the working gain/pan slice: mixer control rows, Insert adjustment,
  numeric prompts and resets. Visual bulk edits remain slice 4. No inert controls
  or new audio behavior are exposed by this foundation.
- Validation: formatting check, strict all-target Clippy and all-target nextest
  pass (132 tests, none skipped). Regression coverage includes real keys, saved
  Insert/Visual state, independent cursors, count/prefix clearing, help/prompt
  priority, hidden-edit prevention, structural undo and view-specific help/status.
- Listening and real-terminal readability/navigation review remain pending.

### Volume/pan progress (2026-10-16)

- Per-track validated volume/pan targets, neutral new tracks, value-diff history,
  no-op/dirty handling and v3 persistence are implemented. Strict v1/v2 decoders
  migrate to neutral settings; v1 note-length migration is preserved.
- F3 has only working Volume/Pan rows, horizontally scrolling channels, selected
  cell/channel cues and parameter labels. Normal navigates; Insert adjusts with
  counts/arrows, numeric prompts and resets. Numeric errors leave the document
  and history untouched. View-local state and hidden-note protection are retained.
- Persistent channel gain processors run after active/retrigger-release sums,
  before the master sum. Stereo balance preserves centre width and doesn't boost
  endpoints. Linear 10 ms gain ramps affect held notes, tails and F4 audition.
  Stop/replacement resets ramp memory to current targets, without losing settings.
- The main loop synchronizes changed targets before transport/audition triggers,
  including history, appended tracks and load. Unchanged targets send nothing.
  No voice rebuilding, filter graphs or mixer allocations were added to the callback.
- The legacy `AudioEngine::play`/sequencer example deliberately has no track mixer;
  project playback and F4 audition both use the processed `play_event` path.
- Validation: strict all-target Clippy and all-target nextest pass (139 tests,
  none skipped); warning-as-error docs/doctests and the ALSA-null `just_a_note`
  example pass. Coverage includes real keys, transactional numeric rejection,
  v3 round trips/v2 migration, append/history/load target ordering, track isolation,
  held notes/release tails, balance endpoints, neutral processing, finite boosted
  output, and ramp duration at 8/44.1/48/96 kHz.
- Remaining: tone controls (slice 3); mixer Visual/bulk edits, grouped held-key
  undo and meters (slice 4). Mixer Visual isn't exposed. Counted adjustments are
  atomic; repeat events currently produce separate undo entries. Small-terminal
  rendering has automated coverage, but listening and real-terminal interaction
  checks remain pending. Boost can clip; no limiter or click-free claim is added.

### Tone progress (2026-10-16)

- F3 adds working 200 Hz/4 kHz shelves (−12..+12 dB) and independent
  Butterworth HPF/LPF (12 dB/octave, Off or 20..20000 Hz). Semitone keys follow
  the directional Off boundaries above; numeric entry accepts Hz/`off` and dB.
  Reset, counts, undo/redo, view-local status/help and vertical scrolling cover all
  six rows. Invalid numeric entries leave history and document unchanged.
- Fixed-size stereo TPT state-variable filters persist after the summed voices,
  before gain/pan. Control-side targets clamp effective frequencies, including
  shelf warping, below actual Nyquist (0.45 × sample rate); requested values stay
  in the project. Positive filter parameters and bypass weights interpolate over
  10 ms. Neutral processing has an exact dry fast path; fully bypassed filter
  memory is cleared. Retriggers preserve active filter memory; stop/replacement
  clears it while preserving targets. No callback graph building or allocations,
  master limiter, new dependencies, or unrelated voice changes.
- Version 3 stores tone targets. Missing new fields in earlier v3 saves default
  neutral, preserving the submitted gain/pan format. Existing v1/v2 migration is
  unchanged; malformed/non-finite/out-of-range targets remain rejected.
- Automated coverage: shelf/cutoff response at 8/44.1/48/96 kHz; neutral identity,
  stereo separation, 10 ms transitions, held voices/retrigger/filter tails,
  replacement reset, and rapid finite sweeps at 1/8/44.1/48/96 kHz. Real-key tests
  cover counts, directional Off boundaries, numeric rejection, reset/history and
  saved cursors; persistence covers tone round trips and earlier-v3 defaults;
  terminal buffers cover selected tone labels at small sizes.
- Validation: all-target nextest passes (146 tests, none skipped), along with
  formatting, strict Clippy, warning-as-error docs/doctests and ALSA-null example.
  An initial run hit an outdated two-row assertion and timed out while draining
  another test; after updating the assertion and adding the neutral fast path,
  the full suite passed. Manual listening and real-terminal checks are pending.
  Boosts can clip; smoothing is not a click-free claim. Slice 4 remains deferred.
- Final shipping checks also cover the directional `gk`/`gj` correction and
  immediate note-entry previews in separate bounded slots (no song-voice theft).
  Formatting, strict Clippy, all-target nextest (150 passed, none skipped),
  warning-as-error docs, doctests and the ALSA-null `just_a_note` example pass.
  The null sink checks setup, not audible output.

### Bulk editing and undo progress (2026-10-16)

- Mixer Normal `v` enters view-local Visual. Counted hjkl and gh/gl/gk/gj extend
  inclusive rectangles across all six controls, independently of pattern height.
  Visual a/s applies each control's own increments and directional Off/range
  rules; d/x resets selected controls to neutral. Each rectangle/count is one
  transaction; unchanged values add no history. Esc clears the mixer rectangle;
  switching views preserves it and the separate pattern selection.
- Insert h/l/arrows and Visual a/s group successive same-key adjustments. Reported
  release, any other key (including counts/prefixes), command, help, prompt or view
  change ends the gesture. Without releases, grouping is deliberately heuristic:
  at most 400 ms between changes and two seconds from the first change. Quick
  separate taps can group; long holds split. No keyboard protocol is assumed or
  enabled. Counted edits remain separate atomic transactions.
- Channel-only history joins retain original before-values and newest revisions,
  guarded against crossing unrelated edits. Undo/redo, dirty state, clamp no-ops,
  bulk reset, one-row patterns, saved pattern selection, append/structural undo,
  load reset, help ownership and narrow/zero-size buffers have automated coverage.
  Visual highlighting uses reverse/underline, not RGB or color alone.
- Validation: formatting, strict all-target Clippy and all-target nextest pass
  (161 tests, none skipped). Initial runs exposed obsolete inert-v assertions and
  an overly wide status expectation; these were updated to the implemented input
  and existing compact-status contracts. One 120-second run timed out draining
  the sustained-audio test; the full run passed with a longer bound.
- No audio, persistence format, dependencies, mute/solo or metering changes.
  Real-terminal releases/repeats, resizing, light/dark/no-color readability and
  listening remain pending. Per-channel stereo meters/master clip indicator are
  suitable follow-up work, not part of this batch.

Each slice gets a short plan update, focused tests, formatting, strict Clippy and
all-target nextest before committing/pushing a stacked PR. Run docs and the null
output example when audio interfaces change. A null sink checks setup, not sound.
Finish with manual listening for pan, gain sweeps, filter sweeps and release tails,
and a real-terminal check of navigation, value readability and mode cues. Record
those checks as pending until somebody actually performs them.
