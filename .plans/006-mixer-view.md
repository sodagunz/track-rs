# Mixer track overview

## Current status — 2026-10-17

Track overview foundation implemented and superseded by the working channel mixer (009), meters and mute/solo (011). These controls are no longer deferred.

Implementation is complete and merged. The user provisionally accepts the current
build’s general manual UI/audio gate for now (2026-10-17); the UI still feels
clunky. This does not certify every specialized historical check below.
See the [canonical backlog](README.md) for acceptance scope and next priorities,
and [015](015-test-performance.md) for the automated checkpoint. Original scope,
pending checks and test counts below are historical, not current blockers.

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
