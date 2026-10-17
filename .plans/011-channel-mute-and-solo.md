# Channel mute and solo

## Current status — 2026-10-17

Mute/solo implemented with history, migration, target synchronization and explicit header flags.

Implementation is complete; full manual terminal/theme acceptance and listening
remain pending. User feedback was “so far looks good”, not a full sign-off.
Automated checkpoint results are recorded in [015](015-test-performance.md).
The implementation notes and test counts below are historical; README and the
binding registry describe current controls. MIDI and live recording remain deferred.

## Original scope and implementation record

Add project-wide track flags to the existing channel settings. Both default to false in older files; keep format v3. Mute wins over solo. With any solo flag set, only unmuted solo channels are audible (including when all solo channels are muted).

Mixer Normal mode: `m` toggles mute, `M` toggles solo on the cursor track. No rectangle/count toggles, Insert bindings, or parameter rows. Preserve Visual `s` adjustment. Show explicit M/S/excluded flags in headers and dim excluded cells. Parameter reset leaves flags alone. Changes use channel history and normal dirty tracking.

Apply audibility through the existing post-tone 10 ms gain ramp, including audition. Keep sequencing, voices, and filters advancing while inaudible: unmute reveals an existing sustain, never replays missed notes. Meters remain post-processing/post-audibility, with existing peak decay/clip hold. Recompute affected targets when the global solo set changes; unchanged synchronization sends nothing.

Validation: persistence defaults and malformed flags, history/reset, view/mode bindings, solo combinations and cross-channel synchronization, ramp continuity and neutral behavior; full fmt, strict clippy, nextest, doctests, and null-output smoke where available. Listening and terminal interaction remain manual.

Status: implemented. Validation passed: fmt, strict all-target clippy, all-target nextest (170 passed, none skipped), doctests (zero), warning-as-error docs, null-ALSA example and diff whitespace check. Regression tests cover channel flag/file round trips, old channel defaults and malformed flags, mute/solo truth table, solo-set synchronization, unchanged-target coalescing, undo/reset, counts/repeats/mode isolation, explicit monochrome header flags, and bit-exact resumed song/preview timing at 8/48/96 kHz. Existing ramp, meter, migration and append tests remain green. Listening and real-terminal interaction/resize/theme checks remain pending; null output checks setup, not sound.
