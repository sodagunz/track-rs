# 018 — MIDI input

Status: functional fixed-length input implemented, 2026-10-18; real-device validation pending. Reuse [017](017-quantized-live-recording.md)'s recording decisions and [016](016-metronome-and-record-arming.md)'s session controls. MIDI input only, not output, sync or transport clock. Live recording also depends on [019](019-musical-time-and-grids.md)'s tick timeline and per-bar quantizer; device-free decoding and preview-only work can proceed independently.

## First delivery

Connect one optional MIDI input device, filter one channel (user-facing 1–16) or all channels, and preview notes on the selected Pattern track. Armed Pattern Insert records through the same quantizer/take policy as terminal input. Unarmed playback and Pattern Audition preview without writes; stopped Insert step-writes. Help, prompts, Normal/Visual and other views suppress new MIDI note-ons as well, so external input cannot edit a hidden pattern. Always process release/disconnect cleanup despite focus restrictions.

Implemented Normal commands: `:midi list`, `:midi connect N`, `:midi disconnect`, `:midi channel omni|1..16` (`all` is an alias). N refers to a freshly listed device index; report the connected name and never reconnect silently to a different device when enumeration changes. Default disconnected. Selection/filter are session-only initially; config persistence and auto-reconnect are deferred. No network service or external account is required.

## Input adapter and note policy

- Evaluate `midir` against the current Rust/toolchain, Linux ALSA setup and CI dependency policy. The implementation uses `midir 0.11.1`. Before adding it, check its current primary documentation, supported platforms, native build requirements and compatible release; don't assume a version or claim cross-platform testing. Document any additional setup in README.
- The OS callback only validates/copies bounded messages into a bounded queue and stamps receipt time. Never mutate App/history, construct DSP voices, block on a full queue, or call terminal rendering from the callback. Drain a bounded batch in the existing main loop; wake/poll without busy spinning or starving transport deadlines.
- Normalize driver timestamps, when reliable, into the transport's monotonic domain through one adapter. Otherwise use callback receipt time and expose/document that fallback. Reject old run generations and stale events using 017. Don't confuse microseconds-since-connection with `Instant`, wall clock, or audio sample time. Map normalized high-resolution timestamps through 019's active tempo segments before nearest-grid quantization (ties forward), including per-bar grid changes and loop rollover; never round milliseconds using an independently sampled BPM.
- Decode Note On, Note Off and velocity-zero Note On as Note Off. Track held notes by connection generation/channel/pitch. Clamp nothing silently: reject MIDI pitches outside the tracker's C0–B9 range after explicit octave mapping (define MIDI 60 as C4). Use event ordering to make simultaneous arrivals deterministic.
- Initial monitoring and recording use fixed New length, with the existing 200 ms preview gate, not held sustain. Note-offs update the ledger and cancel the current MIDI preview, without changing the recorded fixed gate. One track remains monophonic: last note-on wins; chords do not create tracks. Releasing an older held key must not cancel a newer preview. True key-held monitoring and duration capture remain deferred.
- Accept velocity 1–127 but initially ignore its magnitude for both preview and storage; show/document fixed dynamics. The project has no per-note velocity. Defer schema changes, sustain pedal, CC mapping, pitch bend, aftertouch, program changes, SysEx and MIDI clock. Ignore unsupported messages without affecting transport.

## Failure and lifecycle

Queue overflow must be visible: latch a dropped-input warning, discard the ambiguous pending batch, clear held-note state, cancel MIDI-owned previews and suspend capture until explicit reconnect. Don't replay stale note-ons or drop note-offs invisibly. Disconnect/device loss performs the same cleanup and closes the current take, without undoing already recorded cells or stopping song playback. A failed connection leaves the existing connection usable if possible; otherwise report the disconnected state explicitly. No device is a normal startup condition, not fatal.

Focus loss, stop/restart, successful load, track/target invalidation and shutdown clear MIDI input generations and pending previews. Keep terminal previews/song voices intact when cancelling only MIDI-owned previews; add source ownership where needed, not a global stop as a shortcut. Device discovery/connection must not stall the audio callback; bound or isolate potentially blocking OS work. Do not promise hotplug detection latency before testing the chosen backend.

## Implementation slices

1. Device-free message decoding/normalization, bounded ingestion and fake-device adapter tests.
2. Optional real input connection, enumeration, disconnect/error feedback and preview-only delivery.
3. Step/live recording through the shared module, complete lifecycle tests and user-facing documentation. Validate dependency/license/native-build changes at this checkpoint.

Test through the same input/recording interface used by the real adapter: channel filtering, octave mapping and limits, malformed/unsupported messages, zero velocity, chord ordering, repeated note-ons, release after focus changes, overflow, stale timestamps, reconnect generations, count-in, mode guards, pinned targets and grouped undo. Use deterministic clocks and offline audio; no physical MIDI device in CI. Run focused tests then the full existing CI gate. Separately perform a real-device check for discovery, note mapping, disconnect/reconnect, preview level and recording placement; list the OS/device actually checked rather than claiming universal MIDI support.

## Checkpoint limits and amendment

The adapter uses the installed midir 0.11.1 primary Rust source API (`MidiInput`,
`Ignore::None`, port handles and `MidiInputConnection`) and Linux ALSA builds.
Preview slots are separate from terminal and transport slots and use channel
processing/meters. The callback only calls bounded `InputSender::receive`.
Receipt-time filtering invalidates pending input after terminal focus/target/run
changes without replacing the active callback producer. Reconnect replaces the
queue generation; overflow closes the connection and disarms until reconnect.

Monitoring remains bounded to 200 ms, but explicit Note Off now cancels the
current MIDI slot; it does not capture held duration or restore previous keys.
This deliberately amends the original note-off-does-not-shorten-preview policy.

Backend discovery/connect run on the control thread; they are not isolated with
a hard timeout. No reliable hot-unplug detection is claimed. No physical device
or non-Linux platform was tested. The list is status-line output and can clip on
narrow terminals. Real-device mapping, placement, mixer level and unplug/reconnect
checks remain manual follow-up, not completed acceptance evidence.
