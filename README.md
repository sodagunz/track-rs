# track-rs

### Old school audio tracker for the terminal

This project is mostly intended as a fidget and a tribute to retro digital
audio. Sorry Mario, your blazingly fast modern tracker is in another repo.

### Controls

Start in Normal mode. `i` enters Insert, `v` selects a rectangle, `Esc` returns
to Normal. `hjkl` moves in every mode, with counts like `16j`.
`gh`/`gj`/`gk`/`gl` jumps to first track/first row/last row/last track.
`?` shows the bindings for each mode. Help blocks editing, not playback.

In every mode, `a`/`s` lowers/raises notes one octave; `A`/`S` does so one
semitone. Visual transposes the inclusive rectangle without leaving selection;
otherwise only the cursor cell changes.
In Insert, `zxcvbnm,` enters notes. Tab cycles entry steps of 0/1/2/4 rows.
`{`/`}` halves/doubles note length (16 ticks per row); new notes last one row.
Length sets note-off; the instrument's release rings afterward.

Normal `y` copies, `d` cuts, `p` pastes, and `x`/Delete clears. Visual `y`/`d`/`x`
acts on the rectangle and returns to Normal. Paste includes empty cells and clips
at pattern edges. `u` undoes; Ctrl-r redoes. Bulk edits undo together.

Space stops playback, or starts from the cursor: the edited pattern in Pattern
mode, the selected order entry in Song mode (row clamped to its pattern).
Ctrl-Space restarts from the beginning; Normal `:restart` is the fallback.
`-`/`+` adjusts BPM (20–300), and `[`/`]` changes the track's instrument.
Pad, Drone, and Sustain hold pitched notes; OpenHat rings longer than HiHat. Percussion ignores note pitch.

### Patterns and order

Normal `F2` opens the scrolling arrangement view; `F1` returns to the pattern
editor. The view keeps the selected order entry visible: `]` marks selection,
`>` marks the playing Song entry. Repeated patterns have separate entries.
Panel switching doesn't stop playback or change the document. Help and `:` keep
input priority. Note edits and grid motions are blocked while the grid is hidden.
F3/F4 panels aren't implemented.

In Normal mode (in either view):

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

The JSON format is experimental: saves use v2; v1 files load with one-row notes. Unsupported versions are rejected;
[format limits](src/project_file.rs) keep malformed files from allocating huge projects.

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
