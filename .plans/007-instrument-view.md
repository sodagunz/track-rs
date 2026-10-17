# Built-in instrument editor

## Current status — 2026-10-17

Built-in voice selection implemented. Audition was added by 008; custom synthesis parameters remain out of scope.

Implementation is complete; full manual terminal/theme acceptance and listening
remain pending. User feedback was “so far looks good”, not a full sign-off.
Automated checkpoint results are recorded in [015](015-test-performance.md).
The implementation notes and test counts below are historical; README and the
binding registry describe current controls. MIDI and live recording remain deferred.

## Original scope and implementation record

F4 opens the selected track's built-in voice editor. List all supported voices,
highlight the actual assignment, and scroll it into view on short terminals.
Reuse [/] cycling, h/l track selection, undo/redo, prompt and transport.
Assignments apply across every pattern, not to individual notes.

Share the mixer command guard to block hidden pattern edits. Keep help/prompt
priority, counts, playback and the editor row intact. Test real keys, cycling,
undo/redo, overlay isolation, cross-panel navigation and tiny rendering.

No custom synthesis parameters, auditions, dependencies or format changes.
