# 021 — VST3 hosting

Status: **draft**, 2026-10-17. Large exploratory work, not implementation approval or a feature promise. Start with a bounded host experiment before choosing dependencies or scheduling a full host.

## Scope and evidence

“VST support” here means **hosting VST3 instruments**, not building a plugin and not VST2 compatibility. Effects, chains, sidechains, arbitrary buses, plugin editors and other formats follow only after a useful instrument slice. CLAP is a possible later alternative, not an implied second supported format.

Baseline: merged `origin/develop` at `fb27cea`. `Cargo.toml` has CPAL/FunDSP but no plugin-host dependency, and forbids unsafe Rust in this crate. `src/audio.rs` renders built-in per-note graphs; a plugin instance usually needs persistent block processing and explicit note events, not a fresh plugin load for every `NoteEvent`. Track assignment, channel tone/gain/pan and preview isolation must survive. Linux/ALSA is the current documented development path; cross-platform hosting is not established.

Primary-source findings, checked 2026-10-17:

- Steinberg's [VST3 SDK README](https://github.com/steinbergmedia/vst3sdk) describes block-based hosting, distinct processing/controller responsibilities, audio/event buses and sample-offset automation. VST3 is not compatible with earlier VST versions.
- The current SDK states it uses the [MIT license](https://github.com/steinbergmedia/vst3sdk/blob/master/LICENSE.txt); don't repeat obsolete proprietary/GPL-only licensing advice. Trademark usage has separate [SDK usage guidelines](https://github.com/steinbergmedia/vst3sdk). Recheck the exact pinned revision, submodules and wrapper dependencies before redistribution. SDK licensing does not license third-party plugin binaries or test assets.

These sources establish format facts, not Rust crate readiness or this project's compatibility. No host crate has been selected or evaluated.

## First experiment and approval gate

Try one explicitly chosen, licensed Linux VST3 instrument: initialize, configure a supported stereo output, prepare at a known sample rate/block size, send note-on/off at sample offsets, render offline, restore state and shut down. Record exact SDK/wrapper versions, plugin/version/license, supported layouts and observed thread requirements. Evaluate maintained Rust hosting options versus a narrow external FFI adapter; publishing plugins is not evidence a crate hosts them. Keep the root unsafe-code policy unless a separately reviewed design explicitly changes it.

Only proceed after checking lifecycle, buffers, events, ownership, failure behavior and build/licensing costs. Prefer the embedded engine seam from [022](022-engine-and-frontends.md) when practical, but an isolated experiment needn't wait for that refactor. [020](020-three-oscillator-synth.md) is independent.

## Host module contract

Hide format-specific lifecycle and bus negotiation behind a small instrument-host interface: prepare, deliver bounded events/parameter targets, process, capture/restore state off the audio thread, deactivate. Built-ins needn't be forced through a speculative universal plugin framework.

- Start with one persistent plugin instance per assigned track, no effects. Declare supported event/audio layouts and reject unsupported plugins clearly. Initially support a tested f32 stereo path; negotiate or explicitly reject other layouts/precisions rather than silently reinterpreting buffers.
- Adapt variable CPAL callback lengths to a bounded plugin block size without callback allocation. Define any added buffering latency. Preallocate buffers/event storage, cap event counts, and separate loading, state capture, controller work and UI work from processing. Native plugin behavior itself cannot be guaranteed allocation-free by the host.
- Carry note identity, pitch and gate ends explicitly. Honor sample offsets within each block, release on gate expiry, and cancel on stop/restart/load. Preserve isolated previews: a preview cannot cut off or reconfigure the song instance. Budget an independent preview instance or reject preview with a clear capability message; don't reuse it invisibly.
- Stable parameter IDs, not display names or ordering, identify controls and stored automation. Validate finite normalized values and honor plugin conversion/formatting. Start with generic parameter editing; define how edits reach held notes and how controller-originated changes return without feedback loops. General timeline automation is later work.
- Supply only valid transport context. [019](019-musical-time-and-grids.md) should precede tempo/meter-dependent automation and accurate musical-position reporting. The current row scheduler is not proof of sample-accurate playback; translating deadlines into block offsets needs its own tests.
- Track declared latency and changes. A prototype may reject nonzero-latency plugins; production support needs measured delay compensation across built-ins/plugin tracks, with defined reconfiguration while playing. Don't silently play tracks out of alignment.
- Distinguish note release, plugin tail, bypass and hard stop. Bound tail processing and define silent/infinite-tail policy; silence flags alone aren't permission to discard a tail. Future effects need explicit dry/wet bypass, routing and compensation rules, not just another instrument slot.

## Discovery, trust and recovery

Scan only explicit user-selected paths. Cache metadata keyed by format/class identity and binary version/fingerprint; invalidate stale entries. Never load arbitrary discovered binaries at startup merely to populate a picker. Probe candidates in a helper process with timeout, crash detection and quarantine/retry controls. Scanning runs native code: a helper contains some crashes/hangs, **not** malicious code or all resource abuse. Runtime hosting in-process can still crash the application; process-isolated rendering is a separate latency/IPC design, not a free consequence of safe scanning.

Store stable plugin class identity, format, state bytes with strict size limits and user-readable metadata. Paths are lookup hints, not identity. Restore state with negotiated configuration and documented ordering; capture off-callback at a consistent point, without racing processing. Project state and exported plugin presets are distinct formats. Never serialize native pointers or depend on parameter names for restoration.

Missing, incompatible or failed plugins must not abort project load or erase their state: retain a disabled placeholder, warn, and allow replacement/relinking. Bound file/state sizes and fail atomically on malformed project data. Decide undo granularity for parameter edits and whole-state/preset replacement; saving must preserve recoverable opaque state even when the plugin is unavailable.

## Editors and headless constraints

Generic named parameter editing in F4 comes first. Native editors require platform window handles, event dispatch, resize/focus and lifecycle ownership; the TUI cannot simply embed one. A separate native window/bridge needs a platform-specific experiment and main-thread review. A future GPUI frontend is not proof of plugin-editor embedding support.

Keep audio callback, host control and GUI/main-thread responsibilities explicit. Some plugin/controller initialization may require a GUI event loop even without an open editor; test and publish supported headless capabilities rather than assuming every plugin works on a display-less server. Coordinate with 022 before promising separate server/client editor windows or moving a plugin controller across processes.

## Delivery and tests

1. Offline experiment and dependency/lifecycle decision record; approval to continue.
2. Explicit discovery/cache and generic instrument hosting, versioned persistence, recovery placeholders and parameter controls. Single-track first, then bounded multiple tracks and isolated preview.
3. Timing/latency/tail hardening and real-plugin compatibility matrix. Only then consider native editors or effects; each is separate approval/scope.

Device-free CI uses a deterministic fake adapter at the host seam for blocks, sample offsets, note pairing/cancellation, queue overflow, latency changes, tails, state round-trip, missing plugins and malformed data. This tests host logic, not ABI compatibility. Add an opt-in offline test with a reproducibly built open-source plugin fixture only after verifying its license/build dependencies; don't download commercial plugins in CI. Test non-stereo rejection/conversion, sample-rate/block-size changes, scan hangs/crashes and teardown separately. Hardware/listening/editor tests report plugin, OS, audio configuration and exact versions actually tested. No application tests or host experiments ran for this draft.
