# Built-in instrument editor

## Current status — 2026-10-17

Built-in voice selection implemented. Audition was added by 008; custom synthesis parameters remain out of scope.

Implementation is complete and merged. The user provisionally accepts the current
build’s general manual UI/audio gate for now (2026-10-17); the UI still feels
clunky. This does not certify every specialized historical check below.
See the [canonical backlog](README.md) for acceptance scope and next priorities,
and [015](015-test-performance.md) for the automated checkpoint. Original scope,
pending checks and test counts below are historical, not current blockers.

## Original scope and implementation record

F4 opens the selected track's built-in voice editor. List all supported voices,
highlight the actual assignment, and scroll it into view on short terminals.
Reuse [/] cycling, h/l track selection, undo/redo, prompt and transport.
Assignments apply across every pattern, not to individual notes.

Share the mixer command guard to block hidden pattern edits. Keep help/prompt
priority, counts, playback and the editor row intact. Test real keys, cycling,
undo/redo, overlay isolation, cross-panel navigation and tiny rendering.

No custom synthesis parameters, auditions, dependencies or format changes.
