# track-rs

### Old school audio tracker for the terminal

This project is mostly intended as a fidget and a tribute to retro digital
audio. Sorry Mario, your blazingly fast modern tracker is in another repo.

### AI Disclosure

After letting an initial PoC gather dust for about a year, I came back to it and
felt like I was doing myself a disservice by attempting to manually tackle
everything when I rarely got time for hobby projects. Hence most development
is now AI assisted, and I'll only manually tackle stuff I find interesting.

There's still a human on the driver seat, at least for the time being.

### Controls

`?` opens help; `?` or `Esc` closes it. `Up`/`Down` scrolls help, `q` or `Q`
quits. While help is open, editing and playback toggles are blocked, but playback
keeps running.

Start in Normal mode. `i` enters Insert; `v` starts an inclusive rectangular
Visual selection and cancels it when pressed again. `Esc` returns to Normal
without changing notes. `hjkl` moves in every mode. In Normal/Visual, prefix
movement with a count (`16j`, `2l`); counts cap at 9999 and movement clamps at
pattern edges. `0` is only a trailing digit. Other sequences discard the count
without editing; help also clears counts but preserves mode and selection.

In Insert, `zxcvbnm,` enters C D E F G A B and the next C; `a`/`s` changes the
entry octave and Delete clears a note. `f`/`r` lowers/raises the selected note
one semitone (C0–B9); empty cells stay empty. Note-entry octave is unchanged.
Tab cycles the Insert edit step through 0/1/2/4 rows (default 0), shown in status.
Valid note entry advances by that step, clamped at the last row—even re-entering
the same note, which creates no undo entry. Other edits don't advance.
Movement and adjustments can repeat; note entry and mode switches are press-only.

Normal `x` or Delete clears one cell; `d` copies then clears, `y` copies, and `p`
pastes at the cursor. Visual `d`/`y` acts on the inclusive rectangle; Visual `x`
or Delete clears it without copying. All four finish Visual mode, keep the
cursor in place, and clear the selection. `v`/`Esc` cancels without editing or
changing the clipboard. Operators are press-only and don't accept counts.

The internal clipboard snapshots notes and empty cells, not instruments or BPM.
Paste overwrites empty cells too, clips at pattern edges, and never wraps or
resizes. Cut, rectangle clear, and paste each undo as one edit; the clipboard
survives undo/redo. An unset clipboard does nothing.

In Normal, `u` undoes an edit and `Ctrl-r` redoes it (press-only, no counts).
Notes, transposition, track instruments, and BPM are undoable; navigation, modes,
entry octave, and playback aren't. Undo/redo doesn't restart playback. New edits
discard redo; unchanged edits don't. History retains up to 256 transactions and
65,536 changed values across undo and redo, evicting oldest transactions first.
An edit larger than that budget still applies but clears retained history.

Space toggles playback and `-`/`+` adjusts BPM by 1 (20–300) in every mode.

`[`/`]` cycles the selected track’s instrument: Pluck (default), Sine, Square, Saw,
Kick, Snare, HiHat. Percussion ignores pitch; new presets play dry.

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
