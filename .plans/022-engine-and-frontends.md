# 022 — Engine and frontends

Status: **draft**, 2026-10-17. Explore an embedded engine seam first, then a headless server with a TUI client and, eventually, a GPUI frontend. Not approval for a distributed rewrite, protocol commitment or scheduled GUI implementation.

## Starting point and goal

Baseline: merged `origin/develop` at `fb27cea`.

- `src/main.rs` owns a loop combining key dispatch, `AudioEngine::sync_channels`, `Transport::update_app`, audition consumption, meter sampling and TUI redraw/deadline waits. Closing the input listener currently stops audio. Playback must not depend on a client redraw loop after extraction.
- `src/state.rs::App` combines the project, playback/editor state and pending requests. `src/transport.rs::Transport::update_app` mutates `App`; `update_project` and row collection already offer narrower device-free operations. Build on these rather than wrapping the whole `App` in a mutex and serializing it.
- Project/storage/history live in `src/project.rs`, `src/project_file.rs` and `src/history.rs`; terminal input and rendering live in `src/bindings.rs`, `src/ui.rs` and `src/ui/presentation.rs`. These are extraction starting points, not a demand for new crates per file.

Aim for a deep engine module: frontends issue musical/project intentions and receive results/snapshots. It hides clocking, scheduling, document validation, history, audio/MIDI ownership and persistence. Don't expose every internal field as a getter/setter or make each editor gesture a network round trip.

## Ownership and interface

| Engine owns | Frontend owns |
| --- | --- |
| Canonical project and revision; validated edits, undo/redo, dirty state, load/save | Viewport, panel/mode, focus, selection presentation, terminal geometry |
| Authoritative transport clock, route/run generation, playhead, gate ends and cancellation | Keycodes, key prefixes/counts, modal prompts, redraw cadence |
| Audio device, render routing, channel targets, MIDI input and recording decisions | Display formatting, gestures and conversion to explicit semantic requests |
| Preview lifecycle, capability reporting and bounded meter/status publication | Cached snapshots, pending-request feedback and reconnect UI |

Illustrative intentions: edit an explicit event/range, set a track parameter, start at an explicit musical position/route, stop, audition a note, undo, save. These are design examples, not implemented command names or a wire schema. Carry validated targets and grouped edits, not `hjkl`, Crossterm events or a serialized frontend cursor. An engine operation must return success/no-op/error and the resulting revision; failed edits leave the project unchanged.

Undo initially has one document-wide history and **one editing client**. Keep selection/cursor restoration as optional frontend context, not musical data. Other clients may observe read-only; no collaborative editing, conflict merge, per-client undo stacks or multiple document writers in the first delivery. The server alone writes project files, including atomic validation/replacement and dirty checks. Client paths refer to the server filesystem and must be labeled accordingly; never let two processes independently save the same live document.

Use the same command/query/event semantics in-process first and over IPC later, without forcing serialization into the audio callback. An authoritative control loop owns mutation and transport scheduling independently of clients; the callback only renders prepared bounded work. Queries return coherent snapshots/revisions, not shared mutable references. Separate reliable command results/document changes from lossy playhead/meter telemetry. Bound queues and per-cycle work; never stream every audio sample to draw meters. On overflow, reject unadmitted edits explicitly; for missed change events invalidate the cache and request a snapshot instead of silently applying an incomplete history.

## Timing, input and lifecycle

[019](019-musical-time-and-grids.md) owns musical-time semantics. Extract against the current scheduler if useful, but don't create a competing clock or duplicate row/tick conversion in each frontend. The engine's monotonic clock/run generation is authoritative; redraws and client wall clocks aren't. Transport must advance with no connected UI. Keep bounded late-wake behavior and stale-run rejection.

Route local keys and future [018 MIDI](018-midi-input.md) through the same note-entry/preview/recording decisions. Engine-owned MIDI timestamps use its active clock; client timestamps require an explicit mapping before recording, not an assumption that process clocks or network delays match. Preserve [017](017-quantized-live-recording.md)'s pinned takes and [016](016-metronome-and-record-arming.md)'s arming/count-in policy. Moving input cannot turn Audition into note writes.

Previews carry ephemeral session/voice identity, never project events or dirty changes. Disconnect releases/cancels that session's previews and closes any active take. Initial safe policy: loss of the editing client stops playback, disarms recording and clears all sounding notes/tails; an explicit server setting may later allow unattended playback. Ordinary absence of a client after launching an idle server doesn't shut down the engine. Reserve a bounded stop/panic path so a full edit queue cannot leave held notes sounding. Stop/restart/load invalidate pending events; reconnect never replays stale preview or edit requests.

## Local server, then optional GUI

1. **Embedded extraction:** keep the TUI working in one process. Separate frontend state from document/transport ownership; exercise the engine without a terminal. No IPC, new framework or headless flags needed to prove this seam.
2. **Local headless server:** proposed CLI mode such as `--headless` (not implemented), with a separate TUI client using a local Unix socket. Start with an available audio device required; don't quietly present an inaudible server as successful playback. Report unavailable audio/MIDI/plugin capabilities clearly. A deliberate offline/no-output mode can be designed separately.
3. **Protocol hardening:** version handshake, bounded length-delimited framing, validated message sizes, request IDs, ordered acknowledgments and revision-based snapshots/change events. Define incompatible-version errors. Use restrictive socket/directory permissions, peer/ownership checks and a stale-socket policy; never unlink another live server's socket blindly. No TCP listener or network exposure by default. Remote access/authentication/encryption is separate work.
4. **Reconnect and observation:** identify the session, get capabilities and a full coherent snapshot before accepting edits; resume changes from a known revision or resnapshot after gaps. Prevent duplicate edits when an acknowledgment is lost: scoped request deduplication or an explicit unknown-outcome/reconcile flow, not blind retries. Add read-only observers before considering multiple writers.
5. **GPUI experiment:** a second frontend consuming the proven interface. Reuse musical operations and validation, not Ratatui cell layouts. Rework grid selection, text/numeric entry, focus, pointer gestures, scrolling and accessibility for a windowed UI. Keep the TUI independently buildable/runnable without GUI dependencies.

GPUI is a Rust GPU-accelerated UI framework; its [official site](https://www.gpui.rs/) provides an application/window example and notes its close tie to Zed (checked 2026-10-17). This is not evidence of a drop-in terminal renderer, native plugin-editor compatibility or this project's supported platform matrix. Before choosing a dependency/version, inspect the official build/platform requirements and run a minimal window on the actual target graphics stack. No GPU/display requirement belongs in the headless engine just to support a future frontend.

## Plugins and sequencing

[021](021-vst3-hosting.md) requires distinct audio, controller and GUI-thread ownership. Engine extraction does not make plugins thread-safe or remotely embeddable. A display-less server may support only a tested subset; report unsupported initialization/editor capabilities. Decide whether native plugin windows live alongside the engine or require a dedicated local bridge before promising editor windows in a remote client. Don't send raw window handles over IPC or run GUI work in the audio callback.

020 can land independently. A bounded 021 offline prototype can also proceed independently; integrating a persistent host is easier after the embedded seam exists. 019 should precede accurate musical transport context and timeline automation, but needn't block initial state extraction. Deliver the embedded seam **before** splitting processes or starting GPUI. Don't make today's tracker wait for a full server architecture.

## Acceptance and open decisions

Use deterministic clocks and the public engine interface to test edits, no-ops/errors, history, persistence, transport/gates and previews without a TUI or audio device. A fake output adapter is justified when paired with the actual audio adapter; don't add hypothetical seams everywhere. Preserve existing regression coverage and prove that a stalled renderer cannot stall the engine scheduler.

When IPC exists, test framing/version/size rejection, permissions, ordered acknowledgments, lost acknowledgments, duplicate requests, revision gaps, bounded overflow/resnapshot, disconnect during preview/capture/playback, stale events, reconnect and graceful shutdown. Integration tests use temporary sockets/files and controlled clocks where possible, not sleep-based note timing. Test TUI behavior through the embedded adapter first, then parity through the IPC adapter. Hardware tests separately verify audio ownership/device errors; future plugin/GPUI tests state their display/thread requirements. No application tests ran for this draft.

Before implementation approval, decide the minimum command/snapshot vocabulary, state split, thread model and disconnect policy. Before process separation, approve protocol limits and local security/lifecycle rules. Before GPUI or plugin editor work, validate platform and main-thread constraints experimentally. Keep these decisions small and evidence-driven rather than designing collaboration or remote audio transport in advance.
