# Mixer track overview

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
