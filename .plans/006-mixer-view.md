# Mixer track overview

## Current status — 2026-10-17

Track overview foundation implemented and superseded by the working channel mixer (009), meters and mute/solo (011). These controls are no longer deferred.

Implementation is complete; full manual terminal/theme acceptance and listening
remain pending. User feedback was “so far looks good”, not a full sign-off.
Automated checkpoint results are recorded in [015](015-test-performance.md).
The implementation notes and test counts below are historical; README and the
binding registry describe current controls. MIDI and live recording remain deferred.

## Original scope and implementation record

F3 opens a scrolling track overview; F1/F2 still select pattern/arrangement.
Use an explicit view enum so panels can't overlap. Show shared voice assignments
and note counts for the edited pattern, not invented audio levels.

Reuse h/l and counted horizontal motions, [/] voice selection, undo/redo, prompt,
and transport. Block hidden note edits and vertical row motions. Preserve the
editor row and playback when switching; overlays retain priority.

Test real keys, voice isolation/history, hidden-edit blocking, overlay priority,
and tiny rendering. No dependencies or project-format changes.

This is a bounded mixer foundation: gain, pan, mute, solo and live meters aren't
in the current audio model and remain deferred. Note counts aren't audio meters.
