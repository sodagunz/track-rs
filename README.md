# track-rs

### Old school audio tracker for the terminal

This project is mostly intended as a fidget and a tribute to retro digital
audio. Sorry Mario, your blazingly fast modern tracker is in another repo.

### Plans

The [canonical backlog](.plans/README.md) records current acceptance and delivery
order: musical time and grids, metronome/count-in and arming, quantized live
recording, then MIDI input. Musical time/grid editing, metronome/count-in,
arming, fixed-gate Pattern recording and optional MIDI input are implemented.

### Synth DSP preview

Plan 020's three-oscillator synth has a standalone, device-free WAV example:

```sh
cargo run --example synth_render -- /tmp/track-synth.wav
```

Use a new output filename. It renders a held note, a smoothed filter edit and
release; edit `examples/synth_render.rs` to try other patches. The synth isn't
wired into F4 or project files yet. See [020](.plans/020-three-oscillator-synth.md#dsp-checkpoint--2026-10-18)
for DSP choices, headroom and remaining work. Triangle isn't band-limited;
resonance can clip, and automated checks don't replace listening.

### MIDI input

Start disconnected; no MIDI device opens automatically. In Normal mode:

- `:midi list` starts discovery, then opens a read-only input browser (zero-based
  indices). Any intervening key, including Esc, cancels presentation and keeps
  the previous snapshot.
  Left/Right or h/l selects a port; Up/Down or j/k scrolls its wrapped name.
  PgUp/PgDn scrolls ten lines; Esc closes. Enter does not connect.
- `:midi connect 0` connects the listed port; list again if it changes.
- `:midi channel omni` accepts all channels (default); use `1`–`16` to filter.
- `:midi disconnect` cancels MIDI previews, closes the take and disarms recording.

These settings are session-only. Duplicate port names are rejected rather than
silently choosing one. The browser shows the selected index/count and marks the
connected port in that snapshot. Navigation doesn't rescan; close and run
`:midi list` again to refresh. Snapshots above 4,096 ports are rejected without
replacing the previous list. Control characters in names display as escapes;
wide glyphs use Unicode escapes in a one-column viewport. Stored identities are
unchanged. Connection failures appear in status and preserve an existing
connection.
Discovery, connect and backend cleanup run on one lazy worker, never the UI or
audio callback. Only one request is pending; busy requests are rejected without
changing existing input. The UI reports failure after five seconds and disables
further backend requests for this session; restart to retry. This is a UI deadline,
not a hard OS timeout: midir calls cannot be interrupted, and no replacement
threads are spawned. Disconnect immediately detaches queued/callback input,
silences MIDI voices and disarms; physical connection cleanup may wait for a
blocked backend call. Shutdown doesn't join a stalled worker.

In F1 Pattern Insert, stopped notes step-enter; playing notes preview unless
Pattern recording is armed. F1 Audition previews without writes, even if armed.
Normal/Visual, other views, help, the MIDI browser and prompts suppress new
note-ons. MIDI 60 is C4; pitches below MIDI 12 are rejected. Capture uses callback receipt time, not the
backend's microsecond clock, and the same quantizer, pinned target and grouped
undo as terminal recording. At most 64 packets are drained per loop (128 queued).
Overflow or observable callback-producer closure discards queued input, cancels
MIDI previews, closes the take and disarms with a warning; reconnect explicitly.
Song playback and terminal previews are not stopped.

Notes use fixed dynamics (velocity magnitude is ignored). Armed live MIDI capture
starts with provisional New length, then replaces its gate on release. Onset and
end snap to the nearest active local grid (ties forward), with a one-tick minimum.
Duration uses unwrapped transport ticks across tempo changes and pattern loops,
not elapsed seconds multiplied by the release BPM. Gates may cross the pattern
end; holds beyond 61,440 ticks clamp with a status warning. Stopped step entry and
terminal recording keep fixed New length. MIDI monitoring holds the gate until
Note Off/zero-velocity Note On, then plays its release tail. One-shot instruments
can decay naturally while held. Terminal previews still use a 200 ms gate.
Chords are last-note-on wins, not polyphonic entry; an older key's release never
stops its replacement or resumes an older note. Repeated Note On for the same
channel/pitch replaces the gate; the first matching Note Off releases it (MIDI
has no per-press identity). MIDI release never steals terminal or song voices.
Stop, disarm and terminal focus/target changes close captured holds at the last
transport position. Load discards old holds; disconnect, reconnect and overflow
leave provisional onsets intact but discard pending duration updates. Changed or
removed events aren't overwritten or resurrected by late releases. Duration edits
join the onset take only while that take is still the latest history transaction;
otherwise they are separate undo steps. Focus/target/run changes clear MIDI
previews and discard pending old input.
Monitoring uses bounded main-loop input batches, not sample-accurate scheduling.
Sustain pedal, CC (including all-notes-off), pitch bend, MIDI clock/output and
auto-reconnect aren't implemented. There is no reliable backend hot-unplug
notification: midir may retain the callback producer after physical unplug,
so channel-closure cleanup does not guarantee unplug detection. If a release
is lost while that producer remains alive, a held tone can keep ringing. Use
`:midi disconnect` before reconnecting; it cancels all MIDI-owned voices.

Linux builds use `midir 0.11.1` with ALSA; install `libasound2-dev`, `libudev-dev`
and `pkg-config` (the existing CI audio dependencies). Validation uses device-free
input tests and offline audio, not a physical MIDI controller. Other operating
systems and real-device discovery/recording have not been tested here.

### Themes

Defaults use your terminal's foreground/background and ANSI palette, not RGB.
Optionally create `$XDG_CONFIG_HOME/track-rs/theme.json`; when XDG_CONFIG_HOME is
unset or empty, use `$HOME/.config/track-rs/theme.json`. If neither is available,
there's no discovered file. A missing discovered file is harmless.

```json
{
  "accent": "magenta",
  "cursor": "light-blue"
}
```

`cargo run -- --theme /path/to/theme.json` overrides discovery entirely. An
explicit missing file, unreadable file, unknown role or invalid color is an error
before audio or terminal startup. Omitted roles keep their defaults; `{}` uses
all defaults. Themes load at startup, not on save or reload of a project.

Roles: `foreground`, `background`, `muted`, `accent`, `highlight_foreground`,
`cursor`, `selection`, `playback`, `warning`, `normal`, `insert`, `visual`,
`audition`, `row_alternate`. Cursor/selection/mode roles set backgrounds and share
`highlight_foreground` (black by default). Selection also uses an underline.
Pattern rows alternate between `background` (terminal default) and
`row_alternate` (`dark-gray`); playback uses `gray` with `highlight_foreground`.
Cursor and selection take priority over playback, then row shading. Beat/bar
markers remain visible without color. To change or disable the alternate shade:

```json
{"row_alternate": "dark-gray"}
```

Use the same color as `background` to disable shading. ANSI gray shades depend
on your terminal palette; check contrast on both light and dark backgrounds.
All views, help, prompts and buffer/status strips share these settings.

Colors are lowercase ANSI names: `black`, `red`, `green`, `yellow`, `blue`,
`magenta`, `cyan`, `gray`, `dark-gray`, `light-red`, `light-green`, `light-yellow`,
`light-blue`, `light-magenta`, `light-cyan`, `white`; `default` inherits the
terminal color. `indexed:0` through `indexed:255` select palette entries.
Explicit `#RRGGBB` strings are also accepted for custom truecolor themes; they
bypass the terminal palette and need terminal support. Choose highlight colors
and foregrounds together for your terminal. Themes aren't stored in projects
and don't affect editing, undo or audio.

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
While stopped, each note step-enters once and immediately auditions the selected
track's instrument for 200 ms plus release, including when re-entering the same
note. **During playback, unarmed Insert now previews only**; it no longer edits
the cursor cell. Arm explicitly for live capture (below).
Reported Repeat/Release events are ignored for notes; legacy terminals may report
held keys as repeated Press events and retrigger. Previews use the track's mixer settings and
separate bounded voices, so entry during playback doesn't cut off song notes or
move the playhead. Tab cycles entry steps of 0/1/2/4 rows (an arbitrary step
returns to zero). Rows here mean grid moves, not note-gate units. In F1 Normal, `:step 3` sets an exact advance in rows, from
0–4096; zero stays on the same cell. `:length 4r` or `:length 64t` sets the
length for new notes in empty cells (16 ticks per row, 1–4096 ticks). Length
requires an integer with a lowercase `r` or `t` suffix; bare numbers are rejected.
Counts don't multiply these commands. Insert and Audition must first Esc to
Normal; other views reject them. Both settings are session-only: no project or
undo changes, and switching views or loading projects retains them.
The Insert strip separates `New` length and `Step` from cursor `Cell` detail.
`{`/`}` halves/doubles only the cursor note's length; replacing its pitch preserves
that length. New notes default to one row until `:length` changes it.
Length sets note-off; the instrument's release rings afterward.

For exact musical gates, F1 Normal `:duration 320t` sets quarter-triplet length
at 960 ticks per quarter (range 1–61440). This session-only New setting replaces
`:length` until either command is used again; stopped step-entry pitch replacement
retains the existing gate. Live capture replaces both pitch and gate. Non-legacy cell gates use `@` instead of `:` and status labels
use `ppqt` to distinguish canonical ticks from legacy `:length` ticks. `{`/`}`
retain legacy adjustment for representable gates; other gates halve (rounding
odd ticks down, minimum one) or double, capped at 61440 ticks.

F1 Normal Enter enters Audition: the same piano keys play **without writing**,
including during playback. Arrows, PageUp/PageDown and `[`/`]` still work;
brackets deliberately edit the shared instrument assignment. Notes don't move
the cursor or add undo history. Esc returns to Normal. The strip says WRITE for
stopped Insert, PREVIEW for unarmed playing Insert, and NO WRITE in Audition
(`I:W`/`I:P`/`A:N` when narrow). Audition has a 200 ms
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

### Metronome

In any view's Normal mode, `:metronome on|off|toggle` controls the playback click.
It defaults off, never runs while stopped, and doesn't change the project or undo
history. The preference survives loads; stop/restart/load cancels the click tail.
Status shows `Click:on` or compact `CLK`. Turning it off cancels only the click,
not song or preview voices. No extra keyboard shortcut steals piano keys.

Clicks follow the transport's anchored clock, not redraws. Each pattern clicks
denominator units in simple/irregular meters and groups of three units in
compound meters (numerator at least six and divisible by three, denominator
at least eight). 3/4 has three quarter clicks; 7/8 seven eighth clicks;
6/8 two dotted-quarter clicks (3+3), 9/8 three, and 12/8 four. Local bar starts
are accented. Grouping is automatic, not configurable. Song occurrences reset that local phase, including partial
bars, without padding. Grid density doesn't change the click. Late wakes skip
stale clicks rather than replaying a burst.

The separate ten-millisecond fixed-gain voice bypasses channel mute/solo/tone and
joins the master sum before clip telemetry. It is not a project track or note.
This is soft-deadline dispatch, not sample-accurate output; click level, physical
alignment and device latency still need listening checks. There is no limiter.

### Pattern recording

1. In F1 Normal, choose the pattern, track and start cursor. Set a fixed New gate
   with `:duration 320t` or `:length 1r` if needed.
2. `:record on` arms; `:countin 1` requests one full destination-meter bar.
3. Space starts at the cursor; Ctrl-Space or `:restart` starts at the beginning.
   Enter Insert with `i` and play. Count-in input previews only and isn't queued.
4. Esc then `:record off` disarms without stopping playback. `u` undoes each take.

Normal commands work in every view: `:record on|off|toggle`, `:countin 0|1`
(bars). Defaults are unarmed and zero count-in. The count-in setting applies to
the next start; disarming is how to cancel a running count. Settings aren't saved
or undoable.
Stop retains arming; successful load disarms but retains count-in/metronome
preferences. Disarming during count-in stops it. Count-in clicks sound even with
`:metronome off`; song playback and unarmed starts never count in.

Recording is **Pattern-only, one pinned track**. Starting armed pins the playing
pattern and selected track; arming during playback pins the current playing
Pattern, not a different edit pattern. Only F1 Insert displaying that pattern
can capture. Cursor/track movement doesn't retarget the take. Leaving Insert,
changing views/patterns, help or prompts suspends capture and closes the take;
returning to eligibility starts a new take on the same target. Song mode says
capture unavailable. Audition always previews without writing.

`REC ARMED` means intent, `COUNT` shows beats in its own full-bar progress,
and `REC P… T…` means eligible capture. Narrow status keeps COUNT/REC/ARM cues
and the pinned target. Hits snap to the nearest local grid (default sixteenths),
ties forward, wrapping a final-boundary hit to the next loop's tick zero.
Per-bar grids apply; edits to the capture grid take effect at the next bar or
partial-pattern loop. Edit step is ignored. Last hit at an exact lane/tick wins,
replacing pitch and the fixed New gate; other off-grid events survive.

Monitoring uses the separate preview voice immediately. A newly captured future
occurrence is skipped by song dispatch to avoid hearing the same hit twice;
the stored note joins playback on the following pass (including provisional MIDI
gates if a key is still held). Terminal gates use New length, not unreliable key releases. Reported Repeat/Release is ignored; old terminals
may still retrigger held keys as Press events. No chord allocation or velocity.

Each pass is one undo transaction; stop/restart, disarm, save, eligibility loss
and unrelated edits close it. Undo/redo while armed and playing stops transport
before applying history. No-op hits don't dirty the project. Capacity errors
leave storage/history unchanged and report feedback, while still previewing.
Timing uses terminal receipt timestamps and transport tempo segments; stale or
out-of-order hits are dropped with feedback. This is quantized storage with
soft-deadline monitoring, not sample-accurate input or latency compensation.

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

Mixer Normal `m` toggles the cursor track's mute; `M` toggles solo. Flags apply
across patterns and to audition, and are saved and undoable. Mute wins over solo;
when any track is soloed, only unmuted solo tracks are audible. Headers show `M`,
`S`, and `-` for inaudible tracks. The post-tone gain ramps over 10 ms while voices
keep advancing, so unmute reveals an ongoing sustain without replaying notes.
Parameter resets leave these flags alone. Meters measure the gated output.

Mixer Normal `v` starts a rectangle; `hjkl`, counts and `g` motions extend it.
Visual `a`/`s` decrease/increase each selected control by its own increment;
`d`/`x` reset to neutral. Esc clears the selection. Each bulk/count edit is atomic.
Repeated adjustment keys share one undo entry until release, another key/command,
or a 400 ms idle gap, with a maximum two-second group. Legacy terminals don't
report releases reliably: quick separate taps may group, and long holds split.
F3 shows stereo channel peaks after tone/gain/pan, including previews. Meters
fall at 24 dB/s; `!` marks a channel above unity. `MASTER CLIP` holds for two
seconds when the summed output exceeds unity; `MASTER ERR/CLIP` also flags
non-finite audio. Stop/restart and successful load clear the meters and hold.
Short panels hide meters before editable cells. This is telemetry, not a limiter;
boosts and summed channels can still clip.
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

Pattern rows project a musical grid; changing the grid never retimes notes or
changes the pattern end. In F1 Normal, `:meter 3/4` sets pattern-local meter,
`:grid 3` sets quarter-note triplets, and `:grid 64 bar` puts 256ths in the cursor's
bar. Grid numbers are divisions per quarter; BPM always means quarter notes/minute.
The gutter marks the edit row `>`, playing row `*`, denominator units `.`, and bars
`|`. Status shows bar/unit/tick coordinates and partial final bars.

A cell's `+N` (`+*` for ten or more) counts hidden off-grid starts, not merged notes.
`:next-event` / `:prev-event` inspect exact starts on the cursor track. Single-cell
entry, transpose and deletion touch only that exact position. Visual ranges include
hidden events; cut/clear report the event count. Paste refuses to replace hidden
starts unless you confirm with `:paste!`. Clipboard positions remain exact ticks.
Legacy gate labels use 16 units per sixteenth, independent of grid density;
non-legacy gates show exact PPQ ticks.

In F1 Normal, `:quantize?` previews snapping the exact cursor event to the current
local grid; `:quantize` applies it. In Visual, `:` accepts only these two commands
and uses the selected tick interval/tracks, including hidden onsets. Nearest-grid
ties go forward; bar overrides apply locally. Destinations may leave the selected
interval, but collisions with selected or unselected events reject the whole edit.
Snaps to the pattern-end boundary also reject: no wrapping, clamping, merging or
force/drop variant. The preview reports moved-event, collision and end-hit counts,
plus the first collision's exact track/tick. Refine the grid/selection or edit exact
events to resolve conflicts. Success preserves pitches, gates, IDs, meter, grids
and end, stops playback, and undoes in one transaction. Preview, rejection and
no-op leave playback/history/selection unchanged.

In the arrangement, `]` marks selection and `>` marks the playing Song entry.
Repeated patterns have separate entries.

In Normal mode (in the pattern or arrangement view):

- PageUp/PageDown switches patterns; Ctrl-n creates one, Ctrl-d duplicates it.
- Ctrl-t adds a track to every pattern and selects it.
- `:end 3360t` sets the edited pattern's exact end in 960-PPQ ticks (1–983040),
  including partial rows/bars. Shrinking previews dropped note onsets and bar-grid
  overrides; use `:end! 3360t` to confirm. Gates can extend past the end and are
  never shortened. End edits stop playback and undo together, including event IDs
  and trimmed overrides. Unchanged ends leave playback/history alone.
- `:resize N` sets the edited pattern to 1–4096 legacy sixteenth-note rows across all tracks. New rows
  are empty; shrinking refuses to drop notes unless you use `:resize! N`.
  Repeated order entries share the new length. Resizing stops audio and clears
  selections; undo restores dropped notes together. Existing note gates stay unchanged.
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

The JSON format is experimental: saves upgrade v1–v3 projects to v4 tick events
at 960 ticks per quarter. Legacy row starts become multiples of 240 and gate
lengths scale by 15, preserving timing. v1/v2 files load with neutral mixer settings;
v1 notes receive one-row lengths. Older v3 files without tone fields load with
neutral shelves and Off filters. Older app versions cannot open v4 saves.

v4 stores meter, a default grid and tick-anchored bar overrides; older v4 files
without grid fields default to the original straight grid. Exact sub-row starts
now load, edit and play, including one-tick gates and non-row-aligned ends.
Nothing is rounded or silently dropped.
`:length` retains legacy units; `:duration` uses file ticks. Unsupported versions are rejected;
[format limits](src/project_file.rs) bound decoded projects and encoded files.
v4/runtime budgets count actual events: 65,536 per pattern track and 1,000,000
per project, not hypothetical cells on the finest grid. Empty long patterns and
tracks remain cheap. Timeline extent is separately capped at 983,040 ticks per
pattern, with 128 tracks, 256 patterns and a 16 MiB encoded-file limit. Dense
v1–v3 imports retain their one-million-cell input limit. Paste preflights the
whole replacement before clearing anything; history charges retained events,
track metadata and grid overrides rather than empty projected cells.

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
