# 020 — Three-oscillator subtractive synth

Status: in progress, 2026-10-18. The user explicitly approved starting 020. The first checkpoint is the independently renderable DSP core; tracker integration, persistence/history and F4 editing remain unfinished. Plans 021/022 remain unapproved.

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
This is not yet an instrument assignment in the tracker.

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

## Delivery and acceptance

1. Agree on DSP ownership, filter/Q limits, phase/curve policy and default patch. Manually implement the standalone synth if desired; no tracker refactor needed first.
2. Offline render tests, then track-owned persistence/history and audio integration. Adapt the current gate representation or 019's eventual equivalent; 020 is **not blocked by 019 or 022**.
3. Minimal F4 editing and audition; listen before broadening scope.

Device-free tests cover oscillator pitch/mix, attack/decay/sustain/release timing at 44.1/48/96 kHz, zero-time stages, zero gate, early gate-off, retrigger during release, bounded retirement and completion. Assert finite/bounded output over supported extremes, smoothing across block sizes and reproducible renders. Compare elapsed envelope time and tuning across sample rates, not identical sample arrays. Test live edits on held notes/tails, preview isolation, stop cancellation, project migration/round-trip, invalid loads and undo/no-op dirty behavior. Keep existing built-in sound regressions unchanged.

Separately listen for filter character, retrigger/edit clicks, tuning, short gates and clipping with all three oscillators. Automated finite-output checks don't certify sound quality or absence of aliasing. Run focused DSP tests and existing CI when implementation exists; this document changes no audio code.
