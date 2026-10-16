# Instrument audition

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
