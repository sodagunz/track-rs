# 020 — Three-oscillator subtractive synth

Status: implementation complete, 2026-10-18. Standalone DSP, track-owned persistence/history/audio routing and F4 numeric/choice editing/reset/audition are implemented. Physical listening acceptance remains pending; this isn't sound-quality certification. Plans 021/022 remain unapproved.

## Outcome

A playable instrument with three pitched oscillators, a mixer, one resonant low-pass filter per voice and an amplitude ADSR. Keep the DSP small enough to implement and listen to independently before wiring it into the tracker. No plugin framework, modulation matrix or general synth editor.

## Actual starting point

Baseline: merged `origin/develop` at `fb27cea`, not the older checked-out `feat/channel-tone` branch.

- `src/tracker.rs`: `Instrument` is a serialized enum of eleven built-ins. Keep their names, sounds and assignments intact.
- `src/project.rs`: instrument assignments and channel settings belong to tracks and are shared across patterns. The channel LPF in `src/tone.rs` is mixer processing, not this instrument's resonant filter.
- `src/transport.rs`: `NoteEvent.duration` is the elapsed-time gate derived from `NoteLength`; empty cells aren't note-off commands. [019](019-musical-time-and-grids.md) proposes a different musical-time representation, not a different ADSR contract.
- `src/audio.rs`: `GatedVoice` builds/allocates a FunDSP renderer on the control thread and renders at the output sample rate. Track slots have one active voice and one bounded retiring voice, with a short retrigger fade; previews have separate slots. This is not arbitrary polyphony. Existing outer attack/release shaping is not an ADSR.
- F4 already selects instruments and auditions without note writes. Reuse its editing conventions rather than inventing another modal system.

## Small first patch

Proposed initial controls (confirm exact ranges/defaults before integration):

| Stage | Controls |
| --- | --- |
| Each of three oscillators | Sine, saw, square or triangle; coarse tuning −24..+24 semitones, fine tuning −100..+100 cents, gain 0..1 (zero disables) |
| Mixer | Sum with documented headroom; default oscillator 1 audible, 2/3 silent |
| Resonant LPF | Requested cutoff 20..20000 Hz; bounded resonance/Q with a tested stable maximum for the chosen filter |
| Amplitude ADSR | Attack/decay/release 0..10 seconds; sustain 0..1 |
| Output | Conservative fixed trim initially; existing channel tone, gain and pan follow |

The user implementing the DSP chooses the oscillator, filter and envelope algorithms. Document phase initialization, envelope curves and the actual resonance range; don't prescribe a framework or abstract phase model in advance. If phase/random initialization is offered, make offline runs reproducible with an explicit seed. Noise, sync, FM, unison, drive, filter envelope, LFOs, effects and patch libraries are deferred.

Use sample-rate-aware coefficients and clamp effective cutoff below Nyquist while preserving the requested stored value. Test high resonance, extreme tuning and low sample rates for finite output. Decide and document a band-limiting approach for discontinuous waveforms; naive oscillators may be a useful experiment, but aren't evidence of alias-free production output. Summing three oscillators and resonance can clip: document gain/headroom, don't silently promise a limiter.

## Gate and live-edit contract

- Gate-on starts attack, then decay to sustain. Gate-off starts release **from the current envelope level**, even during attack/decay. Note length ends the gate, not the audible release tail. A zero gate produces no sound; zero-length envelope stages transition without division by zero.
- Initially preserve one active note per track plus bounded retrigger retirement. A new note starts a fresh envelope/phase under the chosen policy; retire the previous voice using the existing short fade, rather than adding unbounded release polyphony or legato. Gate expiry must release only the intended voice.
- Avoid multiplying the new ADSR by the legacy outer attack/release envelope. Retain transport cancellation and retrigger protection deliberately; stop/restart/project load clears song and preview voices/tails as today.
- Continuous gain, tuning, cutoff and resonance changes reach held notes and release tails through bounded, sample-rate-aware smoothing. Choose a short documented ramp (10 ms is the existing channel precedent). Waveform changes need a bounded crossfade or an explicit next-note policy, not an accidental discontinuity.
- Envelope settings are captured at gate-on for that voice; edits apply to the next note. Label this distinction in F4. Patch replacement and undo follow the same live-edit rules.
- Allocate/build off the callback. DSP processing must not allocate, lock, perform file I/O or destroy large graphs there. Bound update queues/work; don't describe existing channel machinery as proof of real-time safety for new DSP.

## State and integration

Recommended first model: a track-owned persistent `SynthPatch`, separate from transient oscillator/filter/envelope state, and a new `Instrument::Synth` assignment. No shared preset identity: editing one track must not change another. Retain its patch when switching to a built-in and back. Confirm the concrete representation with the manual implementer before coding; don't force a payload into every copied `Instrument`/`NoteEvent`.

Version and validate persistence explicitly. Old project versions retain their exact built-in assignments and neutral migration behavior; missing synth settings get a documented default, never a rewrite of existing built-in presets. Reject invalid/non-finite values atomically. Preserve format size limits and track alignment across add-track, load, undo and redo. Save patch values, not DSP state or runtime handles. Patch edits are undoable, mark dirty only on change, and don't alter notes.

Expose a small concrete DSP interface: construct/prepare with a validated patch and sample rate, gate-on/off, apply live targets, render samples/blocks, report finished. Offline tests use that same interface. Don't add a mock framework or generalized synth trait just for one implementation.

F4 first shows the selected track's patch as numeric/choice controls with reset, validation and existing undo conventions. Audition uses that patch and the same channel processing; it remains ephemeral, NO WRITE, and doesn't dirty the project. Pattern entry previews stay isolated from song voices. Numeric editing is enough; no graphical envelope editor.

## DSP checkpoint — 2026-10-18

`src/synth.rs` exposes a concrete `SynthVoice`: validated construction, gate-on/off,
live patch targets, sample/block rendering and completion. `examples/synth_render.rs`
renders a three-second mono PCM WAV without an audio device:

```sh
cargo run --example synth_render -- /tmp/track-synth.wav
```

The destination must not already exist. Edit the example's patch to experiment.
At this first checkpoint, this was not yet an instrument assignment in the tracker;
the integration checkpoint below adds it.

Decisions for this checkpoint:

- Use the proposed parameter ranges; Q is 0.5–4.0, default 0.707. Default cutoff
  is 4000 Hz, oscillator 1 is saw at gain 1, others silent. ADSR defaults are
  0.01/0.15/0.7/0.2 (seconds except sustain).
- Phases restart at zero. Saw/square use PolyBLEP edge correction; triangle is
  analytic and not band-limited. No alias-free claim. Tuned frequency and effective
  filter cutoff clamp at 45% of actual sample rate; requested values stay intact.
- Use a topology-preserving state-variable low-pass and linear envelope segments.
  Release starts at the current level; repeating gate-off doesn't restart it.
- Divide the oscillator sum by three, then apply 0.25 output trim. No limiter;
  resonance can boost peaks. The WAV writer counts clipped samples.
- Gain/tuning/cutoff/Q use a 10 ms exponential time constant, independent of block
  size. Waveform and ADSR changes apply at the next gate-on. Each render sample
  does fixed work without allocation, locking, I/O or graph destruction.
- The standalone voice does not own transport, note duration or retirement slots.
  Integration must preserve existing bounded retirement and avoid double envelopes.

Nine device-free tests cover pitch/mix, filter attenuation, envelope timing,
zero stages/gate, live release targets, invalid values, deterministic retrigger,
block-size independence and finite high-Q/high-tuning output at 8/12/48/96 kHz.
Physical listening and integration acceptance below remain pending.

## Tracker checkpoint — 2026-10-18

`Instrument::Synth` is appended after the eleven unchanged built-ins. F4's existing
`[`/`]` selection and Enter audition work, as do Pattern/Song playback, terminal
entry previews and held MIDI monitoring. The control thread resolves the selected
track's patch when preparing each voice; `Instrument`/`NoteEvent` don't carry a
patch payload. Unsynchronized callers use the default. `AudioEngine::play` stays
the existing Pluck-only legacy helper; the legacy sequencer's Synth path uses the
default patch through a small FunDSP adapter.

`Project` owns one independent patch per track and retains it when switching
instruments. `Edit::set_synth_patch` validates atomically, ignores no-ops and joins
normal revision/undo/redo handling without editing notes. Track append/load/history
keep patch, instrument and channel dimensions aligned.

Persistence extends v4 with an optional `synth_patches` array of complete value-only
patches, in track order. Missing/null settings and v1–v3 imports use the documented
v4 default patch. Explicit arrays must match track count; invalid values or unknown
nested fields reject the whole load. Files using Synth always store their patches;
all-default built-in-only projects omit the extension to preserve compact neutral
files. Older binaries can reject the new field/variant. Version, input/encoded size
limits and atomic save behavior are unchanged; no DSP state is serialized.

Synth bypasses the legacy outer envelope. Gate duration starts its release rather
than truncating it; physical MIDI release uses the existing source token. Synth
voices/boxes are prepared on the control thread and retired through the existing
control-thread collection path. Existing 5 ms retrigger retirement, isolated
song/terminal/MIDI slots, cancellation and downstream channel processing remain.
No arbitrary release cap is added. Synth's supported output range is 8–192 kHz.

Live targets reach active and retiring voices in all three routes. The new patch
queue holds at most 128 updates, drained at most 128 per output sample. Sending is
nonblocking; full-queue updates retry on the next sync. New notes use the current
validated document snapshot, not that queue's lagging cache. This doesn't certify
the pre-existing command/retirement queues as hard real-time safe. Waveform/ADSR
remain next-note settings; gain/tuning/cutoff/Q retain DSP smoothing.

Eight new tests cover routed samples against the standalone ADSR at 8/12/48/96 kHz,
one-second release tails, MIDI token ownership/live tail targets, bounded retrigger,
preview isolation/channel processing/stop, queue backpressure/no-ops, patch history,
track alignment, persistence migration/invalid-load atomicity and F4 no-write
audition. Existing built-in sound regressions remain unchanged. The
`tracker_synth` example exercises real output setup and all three routes; a null
output smoke isn't evidence of sound quality. F4 numeric editing/reset and physical
listening remain the next checkpoint, not completed acceptance.

Validation: 366 tests pass (358 inherited, eight new) with both cargo test and
nextest. Formatting, strict all-target/all-feature Clippy, doc-tests and
warning-denied documentation pass. Dependency policy passes with existing
duplicate-version warnings; no dependency changes. All three examples pass,
including both output-device examples with null ALSA. No physical listening.

## F4 editor checkpoint — 2026-10-18

F4 shows the synth patch only when the selected track uses Synth. Built-ins keep
the existing instrument list. All 18 controls are grouped vertically as OSC 1–3,
FILTER and AMP; scrolling retains group names, units and a text selection marker.
The parameter cursor is independent of Pattern row count and saved per view.

- Normal: h/l selects tracks, j/k selects parameters, [/] selects instruments,
  Enter auditions while stopped (NO WRITE), i enters adjustment mode.
- Insert: h/l adjusts, j/k moves, Enter opens exact numeric/choice entry; Esc
  cancels entry, then Esc returns to Normal. Voice cycling is Normal-only in F4.
- Counts apply to movement/adjustment; gk/gj jumps to first/last control.
- Normal d/x and Insert Backspace reset only the selected control to its patch
  default. Normal u/Ctrl-R uses existing undo/redo. Clamped/no-op actions don't
  create revisions; repeated adjustments use the mixer's bounded gesture timing.
- Numeric entry rejects non-finite, out-of-range and wrong-unit values atomically.
  Plain envelope values are seconds, with s/ms suffixes; cutoff accepts Hz/kHz;
  coarse/fine accept st/ct. Gain/sustain are linear 0..1. Waveform accepts the four
  displayed names, case-insensitively. No value prompt falls through to commands.
- Keyboard steps: coarse 1 st, fine 1 ct, gain/sustain 0.01, cutoff 100 Hz, Q 0.05,
  envelope times 0.01 s. Waveforms cycle deterministically. Requested cutoff is
  displayed/stored; effective DSP cutoff still clamps by sample rate.

Rows and prompt/status text explicitly label waveform/ADSR as next-note settings
and gain/tuning/cutoff/Q as live 10 ms-smoothed settings. Editing doesn't stop
playback or disarm recording; undo while armed retains the existing transport-stop
policy. MIDI note input remains suppressed in F4, with existing focus/ownership
rules unchanged. Help, browser and prompt precedence prevent hidden note edits.

Nine new tests exercise real key events for every control, units/validation,
reset/undo/no-op/count/repeat behavior, track isolation, persistence, view-local
navigation, arm/playback policy, overlay guards, no-write audition, registry help,
Unicode prompts and scrolling through zero/narrow/full viewports. DSP and audio
routing are unchanged in this checkpoint. Physical listening for filter character,
clicks, tuning, short gates and clipping remains a separate acceptance step.

Validation: 375 tests pass (366 inherited, nine new) with nextest; strict
all-target/all-feature Clippy, formatting, doc-tests and warning-denied docs pass.
Dependency policy passes with existing duplicate-version warnings; no dependencies
added. All three examples pass, with output-device examples using null ALSA.
No physical listening or manual terminal acceptance is claimed.

## Delivery and acceptance

1. Agree on DSP ownership, filter/Q limits, phase/curve policy and default patch. Manually implement the standalone synth if desired; no tracker refactor needed first.
2. Offline render tests, then track-owned persistence/history and audio integration. Adapt the current gate representation or 019's eventual equivalent; 020 is **not blocked by 019 or 022**.
3. Minimal F4 editing and audition; listen before broadening scope.

Device-free tests cover oscillator pitch/mix, attack/decay/sustain/release timing at 44.1/48/96 kHz, zero-time stages, zero gate, early gate-off, retrigger during release, bounded retirement and completion. Assert finite/bounded output over supported extremes, smoothing across block sizes and reproducible renders. Compare elapsed envelope time and tuning across sample rates, not identical sample arrays. Test live edits on held notes/tails, preview isolation, stop cancellation, project migration/round-trip, invalid loads and undo/no-op dirty behavior. Keep existing built-in sound regressions unchanged.

Separately listen for filter character, retrigger/edit clicks, tuning, short gates and clipping with all three oscillators. Automated finite-output checks don't certify sound quality or absence of aliasing. Run focused DSP tests and existing CI for each implementation checkpoint.
