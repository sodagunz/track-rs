# track-rs

### Old school audio tracker for the terminal

This project is mostly intended as a fidget and a tribute to retro digital
audio. Sorry Mario, your blazingly fast modern tracker is in another repo.

### Controls

Start in Normal mode. `i` enters Insert, `v` selects a rectangle, `Esc` returns
to Normal. Normal/Visual use `hjkl`, with counts like `16j`.
`gh`/`gk`/`gj`/`gl` jumps to first track/first row/last row/last track.
`?` shows the bindings for each mode. Help blocks editing, not playback.

In Normal/Visual, `a`/`s` lowers/raises notes one octave; `A`/`S` does so one
semitone. Visual transposes the inclusive rectangle without leaving selection;
otherwise only the cursor cell changes.
In Pattern Insert, `zsxdcvgbhnjm` plays C through B chromatically;
`q2w3er5t6y7u` plays the octave above. `,` also plays upper C.
Arrows navigate; PageUp/PageDown lowers/raises entry octave (0–9). Upper notes
at octave 9 are rejected. These are QWERTY characters, not physical key positions.
Esc returns to Normal for counts, transposition, undo, clipboard, prompts or quit.
Each note enters once and immediately auditions the selected track's
instrument for 200 ms plus release, including when re-entering the same note.
Reported Repeat/Release events are ignored for notes; legacy terminals may report
held keys as repeated Press events and retrigger. Previews use the track's mixer settings and
separate bounded voices, so entry during playback doesn't cut off song notes or
move the playhead. Tab cycles entry steps of 0/1/2/4 rows.
`{`/`}` halves/doubles note length (16 ticks per row); new notes last one row.
Length sets note-off; the instrument's release rings afterward.

F1 Normal Enter enters Audition: the same piano keys play **without writing**,
including during playback. Arrows, PageUp/PageDown and `[`/`]` still work;
brackets deliberately edit the shared instrument assignment. Notes don't move
the cursor or add undo history. Esc returns to Normal. The strip says WRITE in
Insert and NO WRITE in Audition (`I:W`/`A:N` when narrow). Audition has a 200 ms
gate plus release, not key-held sustain; rapid notes replace bounded preview
tails, not song voices. Terminal key release, chords and velocity aren't promised.

Normal `y` copies, `d` cuts, `p` pastes, and `x`/Delete clears. Visual `y`/`d`/`x`
acts on the rectangle and returns to Normal. Paste includes empty cells and clips
at pattern edges. `u` undoes; Ctrl-r redoes. Bulk edits undo together.

Space stops playback, or starts from the cursor: the edited pattern in Pattern
mode, the selected order entry in Song mode (row clamped to its pattern).
Ctrl-Space restarts from the beginning; Normal `:restart` is the fallback.
`-`/`+` adjusts BPM (20–300), and `[`/`]` changes the track's instrument.
Pad, Drone, and Sustain hold pitched notes; OpenHat rings longer than HiHat. Percussion ignores note pitch.

### Views

F1–F4 switch views in every mode, except while help or a prompt owns input.
The controls below apply in each panel's Normal mode.

| Key | View | Controls |
| --- | --- | --- |
| F1 | Pattern editor | Note editing described above |
| F2 | Arrangement | Order controls below; scrolls with selection |
| F3 | Channel mixer | Volume, stereo balance, shelves and filters; controls below |
| F4 | Built-in instruments | Same track/voice controls; Enter auditions while stopped |

In F3 Normal, `h`/`l` selects a channel and `j`/`k` selects a control.
`i` enters adjustment mode: `h`/`l` decreases/increases the value, while `j`/`k`
changes parameter. Arrow keys do the same. Counts work in both modes (`10l`
adjusts ten steps in Insert). `gh`/`gl` selects first/last channel; `gk`/`gj`
selects first/last parameter. Enter in Insert opens numeric entry; Esc cancels the
prompt first, then returns to Normal. Normal `d`/`x` or Insert Backspace resets
the selected cell. Normal `u`/Ctrl-r undoes/redoes without stopping playback.

Volume is −60..+6 dB in 1 dB steps, plus `-inf` for silence; neutral is 0 dB.
Pan is −100 (L100)..+100 (R100), in one-point steps. Centre preserves the existing
stereo signal; endpoints silence the opposite side without boosting or folding
to mono. Numeric entry rejects non-finite/out-of-range values without an edit.
Settings are shared across patterns and affect held notes, release tails and F4
audition, with a 10 ms gain ramp. Boosts can clip; there is no limiter. Listening
checks are still pending, so smoothing isn't a claim of click-free output.

Low/high shelves provide −12..+12 dB in 1 dB steps at 200 Hz/4 kHz.
HPF and LPF are independent, non-resonant 12 dB/octave filters: Off or
20..20000 Hz, adjusted in semitone steps. From Off, increase HPF to start at
20 Hz; decrease LPF to start at 20000 Hz. Decreasing HPF below 20 Hz or
increasing LPF above 20000 Hz turns it off; moving the other way while Off does
nothing. Numeric entry uses Hz or `off`. Effective frequencies, including shelves,
are capped below the output's Nyquist frequency; saved settings retain the
requested values. Tone targets and bypass blend over 10 ms, before gain/pan.
Neutral shelves and Off filters preserve the original signal.

Mixer Normal `v` starts a rectangle; `hjkl`, counts and `g` motions extend it.
Visual `a`/`s` decrease/increase each selected control by its own increment;
`d`/`x` reset to neutral. Esc clears the selection. Each bulk/count edit is atomic.
Repeated adjustment keys share one undo entry until release, another key/command,
or a 400 ms idle gap, with a maximum two-second group. Legacy terminals don't
report releases reliably: quick separate taps may group, and long holds split.
Mute/solo and meters aren't implemented.
Voice assignments in F1/F4 apply across all patterns; `u`/Ctrl-r undoes/redoes them.
Custom synth parameters aren't implemented. In F4, Enter auditions C in the current
entry octave for 200 ms plus release, only while playback is stopped. Holding
Enter doesn't repeat; percussion keeps its fixed tuning. Audition doesn't edit
notes or mark the project dirty. Starting playback cancels the preview.

Panel switching doesn't stop playback or change the document. Each view keeps
its own cursor and mode; returning to F1 restores Insert or a Visual rectangle.
Switching clears pending counts and `g` prefixes. Structural edits still cancel
selections. Space starts from the preserved pattern row, not another panel's cursor.
Help lists only the active view's commands. Mixer keys never edit hidden notes;
F4 supports counted `h`/`l` and `gh`/`gl` track navigation.

### Patterns and order

Pattern cells show pitch and gate length in ticks at fixed positions (16 ticks
per row). The gutter marks the edit row `>`, playing row `*`, beats `.`, and bars
`|` (four rows per beat, four beats per bar). Selection has a text edge as well
as emphasis; playback never scrolls the edit cursor out of view.

In the arrangement, `]` marks selection and `>` marks the playing Song entry.
Repeated patterns have separate entries.

In Normal mode (in the pattern or arrangement view):

- PageUp/PageDown switches patterns; Ctrl-n creates one, Ctrl-d duplicates it.
- Ctrl-t adds a track to every pattern and selects it.
- Ctrl-Left/Right selects an order entry. Ctrl-a appends the edited pattern,
  Ctrl-x removes an entry, and Ctrl-Up/Down reorders it.
- `O` toggles Pattern/Song playback, stopping audio; Space starts it again.

Pattern playback loops the pattern selected at start; Song loops the order.
Moving the cursor or switching the edited pattern doesn't retarget playback.
Starting midway doesn't replay earlier notes. Stop/restart cancels voices and tails;
already-buffered device samples may finish. Order edits and
undo/redo of structural edits stop audio. The order must keep at least one entry.

For A/A/B: write A, Ctrl-a, Ctrl-d, edit B, Ctrl-a, then `O` and Space.

### Project files

In Normal mode, `:w my song.json` saves, `:w` saves again, and `:e my song.json`
loads. Paths can contain spaces; they're literal, without quotes or shell expansion.
Esc cancels the prompt. `:wq` saves and quits.

`q`/`:q` and `:e` refuse to discard unsaved edits. Use `:q!` or `:e! path` to force
it. A failed load leaves your project alone; a successful load stops playback and
clears undo history. The footer marks unsaved changes with `*`.

The JSON format is experimental: saves use v3; v1/v2 files load with neutral mixer
settings, and v1 notes receive one-row lengths. Earlier v3 saves without tone
fields load with neutral shelves and Off filters. Unsupported versions are rejected;
[format limits](src/project_file.rs) keep malformed files from allocating huge projects.

The legacy `AudioEngine::play`/sequencer example path has no project track and
intentionally bypasses the mixer. Transport uses `play_event`; note entry and F4
use `preview_event` with the same channel settings.

### Dev

With `devenv`, `direnv`, and `nix` installed:

```bash
cd track-rs
direnv allow
cargo run
```

The shell provides nightly Rust and the [CI tools](.github/workflows/ci.yml).
Without it, Linux builds need ALSA and udev development libraries plus
`pkg-config` (`libasound2-dev libudev-dev pkg-config` on Ubuntu).

To run the note example without audio hardware (as CI does):

```sh
ALSA_CONFIG_PATH="$PWD/.github/ci/alsa.conf" cargo run --locked --example just_a_note
```

It exits after a second. This checks stream setup, not how it sounds.

### AI Disclosure

After letting an initial PoC gather dust for about a year, I came back to it and
felt like I was doing myself a disservice by attempting to manually tackle
everything when I rarely got time for hobby projects. Hence most development
is now AI assisted, and I'll only manually tackle stuff I find interesting.

There's still a human on the driver seat, at least for the time being.
