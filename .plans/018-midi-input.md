# 018 — MIDI input

Status: held-note monitoring and held-duration live MIDI recording implemented, 2026-10-18; real-device validation pending. Reuse [017](017-quantized-live-recording.md)'s recording decisions and [016](016-metronome-and-record-arming.md)'s session controls. MIDI input only, not output, sync or transport clock. Live recording also depends on [019](019-musical-time-and-grids.md)'s tick timeline and per-bar quantizer; device-free decoding and preview-only work can proceed independently.

## First delivery

Connect one optional MIDI input device, filter one channel (user-facing 1–16) or all channels, and preview notes on the selected Pattern track. Armed Pattern Insert records through the same quantizer/take policy as terminal input. Unarmed playback and Pattern Audition preview without writes; stopped Insert step-writes. Help, prompts, Normal/Visual and other views suppress new MIDI note-ons as well, so external input cannot edit a hidden pattern. Always process release/disconnect cleanup despite focus restrictions.

Implemented Normal commands: `:midi list`, `:midi connect N`, `:midi disconnect`, `:midi channel omni|1..16` (`all` is an alias). N refers to a freshly listed device index; report the connected name and never reconnect silently to a different device when enumeration changes. Default disconnected. Selection/filter are session-only initially; config persistence and auto-reconnect are deferred. No network service or external account is required.

## Input adapter and note policy

- Evaluate `midir` against the current Rust/toolchain, Linux ALSA setup and CI dependency policy. The implementation uses `midir 0.11.1`. Before adding it, check its current primary documentation, supported platforms, native build requirements and compatible release; don't assume a version or claim cross-platform testing. Document any additional setup in README.
- The OS callback only validates/copies bounded messages into a bounded queue and stamps receipt time. Never mutate App/history, construct DSP voices, block on a full queue, or call terminal rendering from the callback. Drain a bounded batch in the existing main loop; wake/poll without busy spinning or starving transport deadlines.
- Normalize driver timestamps, when reliable, into the transport's monotonic domain through one adapter. Otherwise use callback receipt time and expose/document that fallback. Reject old run generations and stale events using 017. Don't confuse microseconds-since-connection with `Instant`, wall clock, or audio sample time. Map normalized high-resolution timestamps through 019's active tempo segments before nearest-grid quantization (ties forward), including per-bar grid changes and loop rollover; never round milliseconds using an independently sampled BPM.
- Decode Note On, Note Off and velocity-zero Note On as Note Off. Track held notes by connection generation/channel/pitch. Clamp nothing silently: reject MIDI pitches outside the tracker's C0–B9 range after explicit octave mapping (define MIDI 60 as C4). Use event ordering to make simultaneous arrivals deterministic.
- Monitoring holds an explicit gate until physical Note Off, then uses the instrument's release; one-shot voices may decay naturally while held. Live MIDI recording uses provisional New length followed by a release-derived gate; terminal recording stays fixed New length and terminal previews use a 200 ms gate. One track remains monophonic: last note-on wins, with one bounded retrigger tail; chords do not create tracks. Releasing an older held key must not cancel a newer preview. Repeated Note On on the same channel/pitch replaces the gate and the first matching Note Off releases it; MIDI cannot distinguish those presses. Held live-recording duration is explicitly user-approved in the amendment below.
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

Monitoring now uses an explicit held gate, not a sentinel duration or a 200 ms
cap. Token-matched Note Off starts the preset release without rebuilding the DSP
graph. Tokens survive input invalidations; stale releases cannot own replacement
voices. MIDI-only lifecycle cancellation clears active and retrigger slots without
stealing terminal/song voices. This amends the earlier fixed-monitoring policy;
monitoring never restores previous held keys. The initial fixed-New recording
recommendation is superseded by the explicitly user-approved held-duration slice
below; terminal recording remains fixed New.
Input uses bounded receipt-time batches, not sample-accurate audio scheduling.
Sustain pedal and CC all-notes-off/all-sound-off remain unsupported. A lost Note
Off can leave a sustained voice ringing until focus invalidation or explicit
`:midi disconnect`; no automatic unplug cleanup is claimed.

Backend discovery/connect run on the control thread; they are not isolated with
a hard timeout. No reliable hot-unplug detection is claimed. No physical device
or non-Linux platform was tested. The list is status-line output and can clip on
narrow terminals. Real-device mapping, placement, mixer level and unplug/reconnect
checks remain manual follow-up, not completed acceptance evidence.

## User-approved held-duration recording

The parent request explicitly approves MIDI held-duration live capture, independent
of terminal New length. Store a provisional onset immediately, then quantize both
onset and release to their active local grids, nearest with ties forward. Subtract
unwrapped musical positions, with a one-tick minimum; don't multiply wall seconds
by the release BPM. Transport anchors preserve tempo segments and loop cycles.
The active capture grid stays pinned for the current bar, including grid edits.
Gates may exceed pattern length; clamp above the schema's 61,440 ticks visibly.

Each channel/pitch owns a captured event identity, exact baseline value, run
generation and original take revision. Repeated Note On finalizes the previous
capture first. Replacement onsets revoke older ownership of the cell, even for
identical values. A changed/moved/removed event cannot receive a stale gate update.
Note Off without a capture only releases monitoring. Count-in holds don't become
captures after the count. Velocity magnitude and CCs remain unsupported.

Stop/disarm finalize at the last synchronized transport position, before clock
cleanup. Terminal focus/target changes finalize remaining valid holds. Successful
load discards pending captures. Disconnect/reconnect/filter changes and overflow
discard pending durations, retaining provisional onsets rather than guessing an
ambiguous end. History joins only when the original take revision is still on top;
a late release after another pass/edit is a separate transaction, never merged
into that newer take. Undo clears pending ownership before late releases.

Preview remains held independently of stored gate length. No DSP rewrite or song
retrigger occurs on finalization. A still-held provisional note can play on a later
loop alongside monitoring; that existing monitor/song overlap isn't solved here.
Stopped MIDI step entry remains fixed New length. No dependencies or schema change.

Held-duration checkpoint: 324 tests pass (313 inherited, 11 new). Fake-device
shared-loop tests cover quantized boundaries/minimum, 32nd/triplet grids, pinned
grid edits, tempo segments, loop rollover, stop/disarm, count-in, retrigger/channel
ownership, identity-preserving edits, load/source reset, duration clamping and
no-op onset history isolation. Existing offline held-monitor tests remain intact.
Formatting, strict Clippy, docs/doc-tests, dependency policy, actionlint and the
ALSA-null example pass; dependency policy retains duplicate-version warnings.
No physical MIDI validation was performed. The Pluck 8 kHz NaN issue below is
unchanged and remains outside this slice.

## Regression hardening

Main-loop regression tests reproduced revision-triggered take splitting with MIDI
disconnected and future MIDI receipt times dropping earlier terminal hits. The
shared device-free loop now keeps recording-owned revisions in one take and drains
MIDI only through the pending key receipt (equal times: MIDI first). Packet budgets
also cover stale/filtered input; future packets retain generation/filter/overflow
checks. Real-device validation remains pending.

Held-monitoring coverage uses fake input through the shared loop into offline
audio, checks overflow cancellation, token replacement, early release, zero fades,
channel processing/meters and bit-exact song isolation. Offline held-gate checks
run at 8, 44.1 and 48 kHz; the full checkpoint has 313 passing tests. Formatting,
strict Clippy, docs/doc-tests, dependency policy, actionlint and the ALSA-null
example pass (dependency policy reports duplicate-version warnings).

A longer probe also found Pluck producing NaNs at 8 kHz after about 579 ms in
both held and unchanged timed gates. That oscillator/filter issue is not fixed
here; held one-shot coverage uses Kick instead. Do not interpret this checkpoint
as all-preset long-hold validation. Real MIDI hardware remains unchecked.
