# 010 — Pattern entry and UI

Status: approved for implementation after PR #32 (stacked on #31).
Input contract implemented on `feat/chromatic-entry`; remaining slices pending.

Borrow the structure in [the tracker research](../.ideas/tui-trackers.md), not
Renoise's entire data model. That research documents sources, not hands-on testing.
Keep the existing F1–F4 views and view-local modes/cursors. This plan supersedes
the research's suggestions of permanent velocity/per-note instrument fields:
track-rs has shared track instruments and note pitch/duration, not those fields.

PR #32 already previews committed Pattern Insert notes for 200 ms plus release,
including no-op re-entry, through separate bounded preview slots. Feature 5 is
**playing without writing**, not another implementation of entry preview.

## 1. Chromatic Pattern Insert keyboard

Use the following unmodified QWERTY characters. `O` below means the displayed
entry octave, not the letter key. Valid octaves remain 0–9. Proposed boundary
change: reject an upper-row note at entry octave 9 instead of the current
`InsertNote` handler's `.min(9)` collapse; update its regression test explicitly.
Entry-octave controls themselves still clamp to 0–9.
This is a documented character layout, not physical-scancode portability.

| Semitone | Lower octave O | Upper octave O+1 |
| --- | --- | --- |
| C | z | q |
| C# | s | 2 |
| D | x | w |
| D# | d | 3 |
| E | c | e |
| F | v | r |
| F# | g | 5 |
| G | b | t |
| G# | h | 6 |
| A | n | y |
| A# | j | 7 |
| B | m | u |

Retain `,` as an alias for upper C for existing users. Do not extend the layout
past these two octaves yet. No shifted-letter note aliases and no Shift-hjkl
navigation: the upper note row already occupies those letters, and terminals
normalize Shift differently from other modifiers.

### Complete ownership and conflict audit

These overrides apply only to F1 Insert and the proposed F1 Audition mode.
Normal/Visual and F2–F4 keep their current controls unless explicitly noted.
Resolve ownership in binding metadata used by both input and help, not by an
undocumented early-return exception. Assert uniqueness per view/mode/key chord.

| Keys | F1 Insert / Audition ownership |
| --- | --- |
| Piano keys above, `,` | Notes only; never cut/copy/undo/quit/move/start a prefix |
| Arrow keys | Move one row/track, clamp at edges; no count parsing |
| h/j | G#/A#; not navigation |
| k/l | Unbound; use arrows, not partial hjkl navigation |
| g | F# immediately, not a pending jump prefix |
| a / s | `a` unbound; `s` C#. Existing octave transposition moves to Normal/Visual only |
| A / S | Unbound here; semitone transposition remains Normal/Visual only |
| 0–9 | 2/3/5/6/7 are notes; others unbound; no numeric counts here |
| q / Q | q upper C; Q unbound. Quit via Esc then q or :q, preserving dirty guard |
| x/d/y/u/v/i/p | Piano notes where mapped; i/p unbound. Editing/mode commands stay Normal/Visual |
| `-` / `+` | Existing tempo decrease/increase; not octave controls |
| PageUp / PageDown | Decrease/increase **entry** octave here; previous/next pattern stays Normal |
| `[` / `]` | Previous/next shared track instrument, as today; these are document edits |
| `{` / `}` | Insert: halve/double the cursor note duration as today. Audition: unbound |
| Tab | Cycle entry step 0/1/2/4 in Insert; unbound in Audition |
| Delete | Insert: clear cursor note. Audition: unbound |
| Esc | Return to F1 Normal; clear transient count/prefix/repeat state |
| Space | Existing play/stop; never a note |
| Ctrl-Space | Existing restart, including legacy NUL decoding; no new modifier dependency |
| F1–F4 | Existing view switching; clear pending input, preserve view-local context |
| ? | Help owns input; piano keys cannot leak through |
| : | Normal only. Esc then : opens prompt; prompt owns all text until closed |
| Enter / other keys | Unbound here unless listed; no implicit application fallthrough |

Normal/Visual retain counted hjkl and directional gh/gk/gj/gl, a/s and A/S
transposition, clipboard/history, pattern/order operations, and quit. All existing
Ctrl arrangement/history shortcuts remain Normal-only. Clear counts/prefixes when
entering either piano mode; `12` must never turn the next note into a count.
Help/prompt retain existing ownership rules (including guarded quit from help).
Don't introduce Ctrl-I/Tab, Ctrl-M/Enter, Ctrl-[/Esc aliases or depend on extended
keyboard protocols for essential actions.

Insert keeps current write, duration, undo and step semantics: one accepted note
press writes once, previews once, then advances; same-note re-entry previews
without adding history. Ignore reported Repeat/Release events for note entry.
Legacy terminals may send repeated Press events for held keys; do not promise
reliable hold suppression without protocol support. Movement repeats may work.

Acceptance: real-key tests for all 24 notes and comma, accidentals across octave
boundaries, minimum/maximum octave rejection, duration preservation, no-op entry,
step 0/1/2/4 and end-row clamp, undo/redo, held-event guards, every collision above,
help/prompt isolation, and unchanged F2–F4/Normal/Visual behavior. Check pitches
with melodic voices; drum voices intentionally ignore pitch.

## 2. Stable aligned pattern fields

Keep equal-width track columns with fixed pitch and duration positions. Reserve
space for empty cells too; accidentals, two-digit octaves, short/fractional lengths
and maximum lengths must not shift neighboring fields. Derive widths from actual
supported ranges, not today's default pattern. Use explicit units in the header
or detail, not an ambiguous number that might mean velocity.

Focus may reveal secondary detail in the existing contextual status area (exact
ticks/rows, voice name), but must not expand the selected column and move other
tracks. Horizontal scrolling keeps the selected track/header visible. Preserve
row numbers. At narrow widths show fewer whole tracks; for widths too small for
one, clip safely while keeping cursor identity in status. No per-note instrument,
velocity, effects, subcolumn editing, new persistence fields or format migration.

Acceptance: TestBackend assertions for populated/empty/accidental notes, min/max
lengths and octaves, stable x coordinates when focus changes, many tracks and
horizontal scrolling, widths 80/40/24/12/0 and heights 0–10. Assert selected note
and track remain identifiable when optional detail drops.

## 3. Beat/bar landmarks and independent position cues

Use `src/transport.rs::DEFAULT_ROWS_PER_BEAT` (currently 4), which the app's
transport uses: beat landmarks every 4 rows, bars every 16. Do not confuse 16
length ticks per row with beats. Four beats per bar is a display convention,
not a new song time-signature field. Keep UI and transport tied to that constant
rather than duplicating a magic row interval.

Keep a fixed gutter: row number plus a beat/bar character (`.` beat, `|` bar,
blank otherwise; bar wins). Separate gutter positions mark cursor (`>`) and
playback (`*`), so coincident positions remain legible. Visual selection gets an
inclusive outline/edge marker and emphasis, not color alone; preserve landmark
and position markers at rectangle intersections. Use terminal defaults/palette,
underline/bold/reverse where useful, and no fixed RGB backgrounds.

Only mark playback in the grid when its pattern matches the edited pattern.
Offscreen/different-pattern playback stays in transport status; never steal the
edit viewport. Selection and cursor remain independent of playback.

Acceptance: beat/bar boundaries before/after vertical scrolling, coincident
cursor/selection/playback, offscreen playback, repeated order entries, stopped
transport, empty patterns, narrow gutters, and monochrome buffer assertions.
Manually check light/dark themes and a no-color-style rendering; snapshots alone
do not establish readability.

## 4. One compact context strip

Evolve `src/ui/presentation.rs`'s existing status line; don't add another fixed
header/footer. Keep buffer identity/dirty state and order selection in their
existing bars, and command/error text on its existing line. Do not repeat those
fields in a new panel. Give the entire width remaining after editor context to
transport rather than boxing transport into a small fixed allocation.

- F1 Insert: explicit WRITE, focused track instrument, entry octave, `New:1r`
  (current fixed new-note length), step, cursor coordinates; existing cursor
  duration is separately labeled `Cell`, not presented as an entry setting.
- F1 Audition: explicit NO WRITE, focused instrument, entry octave, bounded
  preview duration; don't imply step advance or cursor-length editing.
- F1 Normal: cursor pitch/duration and focused instrument; no entry-settings dump.
- F1 Visual: inclusive dimensions and existing common/mixed note summaries.
- F2–F4: retain view-specific order/channel/voice context, not hidden F1 settings.

On narrow screens retain mode/write state, cursor identity, dirty indication
where its buffer bar is absent, and compact playing/stopped Pattern/Song state.
Then add instrument, octave and entry length/step when relevant; drop verbose
cell detail/help/BPM before essential cues. Use rendered widths/graphemes.
Transport uses remaining columns for full order/pattern/row information; no
unconditional half-width split. Preserve short-screen prompt priority from 004.

Acceptance: all views/modes, long voice/project names, dirty state, mixed/empty
selections, pending Normal commands, help/prompt restoration, simultaneous edit
and playback positions, resizing 80/40/24/12 columns and 0–10 rows. Assert each
field has one owner and no overflow or stale mode/entry labels.

## 5. Non-writing audition mode

Proposed entry: F1 Normal `Enter` enters Audition; Esc leaves it. F4 Normal Enter
keeps its current stopped-only C audition. Name the state explicitly rather than
pretending it is Insert with writes silently disabled. Reuse feature 1's note
layout, arrow movement and entry-octave controls. Do not overload Normal z or
add a global piano mode to the mixer/arrangement.

Each accepted note press uses the current track instrument and channel settings,
200 ms gate plus the instrument release, whether transport is playing or stopped.
No note/length changes, cursor step, revision, dirty flag, selection, clipboard,
undo entry, or playhead changes. Explicit instrument selection with brackets still
edits the shared track assignment; distinguish that command from playing notes.

Reuse PR #32's `preview_event`, separate from song voices. Keep the existing fixed
active/releasing preview slots per track; rapid notes may replace preview tails,
never transport voices. This is bounded retriggered audition, not arbitrary chord
polyphony. Transport start/stop/restart, successful load and shutdown cancel
previews using existing lifecycle behavior. Clear pending audition requests when
leaving Audition/opening help/changing view so stale input cannot sound later;
already sounding notes expire naturally unless a transport transition clears them.

Terminal release events are not generally available. Baseline audition is timed,
not sustain-until-key-up; ignore reported Repeat/Release just as entry does. A
legacy repeated Press may retrigger, but voices remain bounded. Do not wait for
an absent release event or promise reliable chords/rollover/velocity. MIDI input,
velocity, key-held gates and extended-protocol optimization are deferred.

Acceptance: same pitches as Insert, empty/populated cells unchanged, no revision
or history changes from playing, no cursor advance, instrument/octave capture,
preview once/no stale replay, help/prompt guards, mode/view restoration, rapid
retrigger bound, finite audio/tail expiration, playback voice/playhead continuity,
mixer application and stop/load/shutdown cleanup. Failed loads retain existing
state according to current load semantics. Test both pitched and drum voices.

## Delivery slices and validation

Each slice is separately reviewable, stacked only after its parent is ready.
The user approved all proposed bindings, including PageUp/Down and Normal Enter.

1. **Input contract:** feature 1 plus metadata/help/README and exhaustive real-key
   collision tests. Keep existing writing preview. PageUp/Down and removal of Insert counts/transposition are approved.
2. **Grid clarity:** feature 2, then feature 3 with deterministic buffer tests;
   split into two PRs if either diff is substantial. No input or file changes.
3. **Context:** feature 4 against the existing presentation seam and short-screen
   tests; verify remaining-width transport layout instead of adding another bar.
4. **Play without writing:** feature 5 reuses the settled input contract/audio
   seam; Normal Enter as the mode entry is approved. Add state,
   real-key and offline audio lifecycle tests, not another renderer.

Run focused tests first, then `cargo fmt --all -- --check`, strict all-target
Clippy, and `.devenv/profile/bin/cargo-nextest nextest run --all-targets --locked
--status-level fail --final-status-level fail` for each slice. Run doctests,
warning-as-error docs and the ALSA-null `just_a_note` example for audio/API changes.
Run `git diff --check`. Record actual counts; the shipping PR #32 baseline is
150 passing tests, not a required final count. Finish with real-terminal key,
resize/light-dark checks and listening during playback; null audio isn't listening.

Plan 009 retains its own remaining mixer Visual/bulk editing, grouped held-key
undo, mute/solo/meters follow-ups. This plan neither implements nor reprioritizes
those, custom synthesis, routing, recording, MIDI, or a new project format.
