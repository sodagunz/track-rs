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

`-`/`+` adjusts BPM by 1 (20–300). `f`/`r` lowers/raises the selected note
one semitone (C0–B9); empty cells stay empty. Note-entry octave is unchanged.

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
