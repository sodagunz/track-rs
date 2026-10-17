# Instrument audition

## Current status — 2026-10-17

Instrument audition implemented. Separate bounded preview slots now also support Insert previews and Pattern Audition (010); the original transport-stopped F4 guard remains.

Implementation is complete and merged. The user provisionally accepts the current
build’s general manual UI/audio gate for now (2026-10-17); the UI still feels
clunky. This does not certify every specialized historical check below.
See the [canonical backlog](README.md) for acceptance scope and next priorities,
and [015](015-test-performance.md) for the automated checkpoint. Original scope,
pending checks and test counts below are historical, not current blockers.

## Original scope and implementation record

In F4, Enter auditions the assigned voice at C in the current entry octave
for 200 ms plus its release. Only allow it while transport is stopped; repeated
key events, help and the command prompt must not trigger it. No note edits,
history entries, project fields or format changes.

Pass a one-shot request from App to the runtime and use the existing track voice
renderer. Retrigger the selected track instead of accumulating preview voices.
Transport stop/start cancels previews through the existing graph replacement.

Test keys, guards, one-shot consumption, assignment/octave capture and unchanged
document/history. Run formatting, strict clippy, all-target nextest, docs and
null-output example checks. Audition is not custom synthesis or a mixer control.
