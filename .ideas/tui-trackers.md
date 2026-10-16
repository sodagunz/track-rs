# Practical tracker UIs in a terminal

Research date: 2026-10-16. Primary project docs/source only; these applications were not run or audio-tested.

## Real examples

### PLEBTracker — terminal, C++

The strongest reference here for a complete console workflow. Its [README](https://github.com/danfrz/PLEBTracker/blob/master/README.md) describes an ncurses audio tracker inspired by GoatTracker and MilkyTracker, with synthesized instruments rather than samples. The [official main-screen and instrument-editor screenshots](https://github.com/danfrz/PLEBTracker/tree/master/docs/images) show its layout.

The [manual](https://github.com/danfrz/PLEBTracker/blob/master/docs/manual.txt) makes the density practical through explicit structure:

- A top metadata area exposes instrument, step, octave, beat spacing, and order selection.
- Pattern cells have aligned `note instrument volume effect` fields: `C-1 00 3F 303`.
- Every configurable Nth row is underlined as a beat marker.
- Wave, pulse, and filter tables sit beside the main editor; an instrument view includes its volume table and envelope visualization.
- `Tab` followed by a documented key selects a pane; `:` opens the command bar.
- QWERTY note bindings span the selected octave and the next. Scale/key selection changes those bindings, not existing song data; chromatic C exposes all semitones.
- A playback mark lets entering a note automatically preview the passage between cursor and mark. This is documented excerpt playback, not evidence of Renoise-style key-held audition.

**Useful lesson:** density works when fields stay aligned, timing has visible landmarks, and focus/navigation are explicit—not simply because everything fits on screen.

### ttrk / TekTracker lineage — console MIDI tracker, C++

The archived [ttrk repository and historical README](https://github.com/fundamental/ttrk) describe a MIDI step sequencer descended from b-tektracker and TekTracker. The original project began with Qt in 1999, then moved to a console implementation to run on a machine without X.

Its stated goals are unusually relevant: a Scream Tracker 3-like step editor, pattern sequencing, prominent track-mute controls, editing without stopping playback, and keyboard access to every function. This is a **MIDI sequencer**, not evidence of a built-in sample/synth engine. The README does not establish a specific chromatic keyboard layout or audition behavior.

**Remembered-program candidate:** plausible if the name or late-1990s/early-2000s console MIDI workflow rings a bell. This repository is C++, not a confirmed C match; its installation advice is historical.

### terminal-tracker — ncurses chiptune tracker, C++

[2bt/terminal-tracker](https://github.com/2bt/terminal-tracker) explicitly describes an ncurses pattern editor, MIDI support including polyphony, echo effects, and instruments defined externally in a text editor. Its [source tree](https://github.com/2bt/terminal-tracker/tree/master/src) contains C++ implementation files. The README includes an [official screenshot](https://github.com/2bt/terminal-tracker/blob/master/media/screenshot.png) and documents Space for playback, but lists documentation as unfinished: do not infer a complete note-entry/audition scheme from it.

**Useful lesson:** an intentionally narrow pattern editor is another valid terminal design; instrument editing need not compete for permanent screen space. Its SDL dependency does not make the UI graphical—the project explicitly identifies the editor as ncurses.

### Schism Tracker — graphical retro UI, not a terminal application

[Schism's own README](https://github.com/schismtracker/schismtracker/blob/master/README.md) identifies it as an Impulse Tracker reimplementation running on SDL 2-supported platforms and links an official screenshot. It is a useful visual comparison, but its text-style appearance must not be confused with terminal rendering.

**Remembered-program candidate:** plausible if the memory is of a retro tracker window rather than an actual shell application. None of the terminal examples above confirms the remembered C program. I did not establish reliable identities for `dtr`, `sc-tracker`, or “zyna”; those remain unresolved, not disproven.

## What Renoise actually provides

The official [Pattern Editor manual](https://tutorials.renoise.com/wiki/Pattern_Editor) describes vertical time, side-by-side tracks, optional subcolumns, collapsed tracks, and a mode that expands only the focused track. That is selective detail, not an obligation to show every control simultaneously.

Its [computer-keyboard guide](https://tutorials.renoise.com/wiki/Playing_Notes_with_the_Computer_Keyboard) documents piano-style QWERTY rows: `ZXCVBNM` and `QWERTYU` provide white keys in adjacent octaves, with other keys supplying accidentals. The virtual keyboard remains enabled; octave and velocity have visible controls. Computer keyboards have rollover limits and no velocity sensitivity.

[Recording/editing documentation](https://tutorials.renoise.com/wiki/Recording_and_Editing_Notes) separates playing notes from writing them through edit mode, supports configurable step advance, and allows editing independently of playback follow. This separation is more important to borrow than Renoise's exact shortcuts.

## Recommendations for track-rs — proposed, not reference-app claims

These extend the modal/F-key direction already recorded in [ui.md](ui.md):

1. **Keep a fixed header:** transport, playback position, mode/write status, focused instrument, octave, velocity, and edit step. Make audition versus insertion unmistakable.
2. **Use a stable aligned channel grid:** note, instrument, velocity, duration; retain channel headers and row numbers while scrolling. Duration is a track-rs proposal, not Renoise's delay subcolumn.
3. **Reveal detail contextually:** keep pattern editing central; use the proposed `F1`–`F4` pattern/arrangement/mixer/instrument views rather than squeezing all four onto every screen.
4. **Mark beats and bars:** subtle row accents plus a distinct cursor and playback row. Do not rely on color alone to distinguish these states.
5. **Resolve keyboard ownership explicitly:** conventional chromatic mappings overlap `hjkl`. In note-insert mode, choose and display whether those keys play notes or navigate; never silently promise both. Keep navigation alternatives available.
6. **Show compact contextual help:** a short footer for the active pane/mode, with full bindings on demand. Command mode can handle infrequent actions without crowding the grid.

Start with these structural choices, not a pixel-for-pixel Renoise imitation. PLEBTracker is the best concrete terminal workflow reference here; ttrk is the strongest historical-name lead, with the language/identity uncertainty intact.
