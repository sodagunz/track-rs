# 016 — Metronome, count-in, and record arming

Status: in progress, 2026-10-17. Playback metronome implemented after 019's completed core gate; count-in and record arming below remain proposed, not implemented controls. See the [backlog](README.md). Depends on [019](019-musical-time-and-grids.md)'s meter and clock foundation before full click/count-in integration.

## Contract

Record arming is session state, independent of transport and Pattern Insert/Audition. Default off; never serialize it or add it to undo. Stop retains arming; successful project load disarms, failed load changes nothing. Show `REC ARMED` while stopped, meter-aware `COUNT …` during count-in, and `REC` only when live capture is actually eligible. Show `PREVIEW` when notes cannot write. Text must work without color and at narrow widths.

Proposed Normal commands: `:record on|off|toggle`, `:metronome on|off|toggle`, `:countin 0|1` (bars). Reject extra arguments and invalid values atomically. These commands should work in each view's Normal mode, subject to existing prompt/help ownership. Keep Space, Ctrl-Space and `:restart` unchanged. Do not assign a new shortcut until the binding registry's per-view/mode collision tests and actual terminal events are checked; command fallbacks are sufficient initially.

Arming alone never writes notes. Deliver the control with 017's first working capture slice, or explicitly show capture as unavailable until then; never show a false active REC indicator.

## Click and count-in

- Use the transport's anchored monotonic deadlines, not a second UI timer. Use 019's tick positions and pattern-local meter; click denominator units (or explicit compound grouping), accent local bar starts. Four rows per quarter and sixteen rows per bar describe legacy 4/4 only, not the new scheduling contract.
- Click is a separate bounded audio voice, not a project track, pattern note, or legacy `AudioEngine::play` call. It bypasses channel mute/solo/tone, joins the output sum before master peak/clip measurement, and has a conservative fixed gain. No limiter or clipping guarantee. Stop/restart/load cancels its tail too.
- Metronome defaults off; when enabled it follows playing transport. First slice has no stopped/free-running click. Settings are session-only, outside project/history; retain click/count-in preferences across loads, but cancel any running count-in.
- Count-in defaults zero; one means a full bar of the destination meter's configured clicks before an **armed Pattern-mode start**. Sound those clicks even if the ongoing metronome is off. Unarmed playback and Song playback start normally, without count-in.
- Capture the requested start row/pattern/track at start. Count-in always accents its first beat; musical playback then starts at the requested row, not necessarily a bar boundary. Do not advance the project playhead, trigger song notes, or write count-in input. Input may preview only.
- Space cancels count-in and stops. Restart cancels old voices and starts a fresh count-in when eligible. Disarming during count-in cancels to Stopped. Structural changes, route changes, successful load and shutdown cancel it. Tempo changes apply at the next scheduled beat; song start follows the final beat interval.
- A late UI wake must skip stale clicks, never emit a burst or start song notes early. Expose the next count-in deadline to the main loop's existing wait calculation.

## Implementation slices

1. Add explicit stopped/counting/playing transport phases and deadline tests; keep existing no-count-in behavior identical.
2. Add separate click rendering/cancellation and session settings/commands, then responsive status/help.
3. Integrate arming and count-in eligibility with 017; demonstrate armed versus unarmed input end to end.

Keep clock/count-in decisions device-free in `src/transport.rs`; runtime orchestration stays in `src/main.rs`. `src/state.rs` owns session intent, not a competing clock. Reuse `src/command.rs`, `src/bindings.rs`, `src/audio.rs` and semantic UI styles. 019 owns project-format/meter migration; this session-control layer adds no further schema change or expected dependency.

## Playback metronome checkpoint

- Delivered the usable click slice first, without adding a placeholder Counting phase or a fake record-arm/REC indicator. Normal `:metronome on|off|toggle` works in every view; defaults off, stays session-only, retains preferences across loads and has wide/compact text cues. Other mode/prompt/help ownership remains intact. No shortcut or dependency added.
- A click calendar derives absolute tick positions and deadlines from the active transport tempo segment and pinned route. The main wait includes click deadlines; no competing UI timer. Clicks accent local bar starts. Compound meters now group three denominator units (6/8 is 3+3; 9/8 and 12/8 likewise); simple/irregular meters retain denominator units. Grouping is automatic, not configurable. Partial Song occurrences reset local phase without padding. Mid-unit starts/enabling do not replay earlier clicks; late wakes emit at most one fresh click and skip stale ones.
- Added a separate fixed-size, control-thread-prepared procedural voice: ten-millisecond gate, fixed conservative gain, no callback allocation/graph construction for clicks. It bypasses channel processing and joins the master meter sum. Disabling cancels only click; graph cancellation also clears it. Notes/previews are not retriggered or reset by click commands.
- Added deterministic meter/tempo, exact deadline, cursor/toggle, late-wake, partial Song, cancellation, command/load/overlay, narrow-status and offline audio/meter regressions. Existing combined 019 timing trace also remains identical with clicks enabled.
- Validation: full locked all-target nextest 252 passed; strict Clippy, formatting, doctests (zero cases), warning-free docs and whitespace checks passed. ALSA-null note-example smoke passed (stream setup, not a physical click audition). No hardware listening, latency or sample-accuracy certification.
- Compound grouping checkpoint: default 3-unit groups in compound meters; deterministic 20/120/300 BPM coverage also includes 9/8, 12/8, 6/16, simple 3/8 and 6/4, mid-group starts and exact partial Song resets.

## Count-in clock checkpoint

- Implemented device-free `Transport::start_count_in` and explicit Stopped/Counting/Playing phase reporting. It pins the destination Pattern/cursor and counts a full destination-meter bar independently of the pattern's partial length. The same anchored tick clock supplies grouped clicks, the main-loop deadline and the exact musical-start anchor; song ticks and snapshots don't advance during the count.
- Pending beat deadlines survive tempo edits; subsequent intervals use the new tempo. Late wakes skip stale clicks arithmetically without bursts or shifting the musical-start anchor. Stop/restart clears count state and pending clicks; existing app cancellation paths also cover structural edits, route changes, successful load and shutdown. Failed load retains the count. Count clicks work independently of the ongoing metronome; the destination click isn't swallowed by tail cancellation.
- Ten deterministic count-in tests cover the tempo/meter matrix, pinned cursor, partial extent, pre-deadline silence, first-note single dispatch, exact off-grid onset/gate timing, delayed wake, tempo transition, cancellation, generations and app orchestration without dirty/history changes.
- Combined checkpoint: locked all-target nextest 263 passed; strict all-target Clippy, formatting, doctests (zero cases), warning-free docs and whitespace checks passed. ALSA-null `just_a_note` smoke passed (stream setup only); no physical listening or output-latency claim.
- This is an implemented **transport seam, not an enabled terminal feature**. Session arming, `:countin`, eligibility, count status/progress and preview-only input during counting still need integration with working 017 capture. No `:record` / `:countin` commands or false REC indicator were added. Current playing Insert still step-edits. 016 is not complete; 020 remains untouched.

## Acceptance

Deterministic tests cover 20/120/300 BPM, cursor/restart starts, partial patterns, beat/bar accents in 3/4, 7/8 and grouped 6/8, partial-bar starts, delayed wakes, tempo transitions and every cancellation path. Offline audio tests cover finite output, bounded click tails, mute/solo independence, metering and stop silence. Real command/key tests cover invalid input, repeat/release filtering, overlays, unchanged history/dirty state, load semantics and narrow text cues. Manually check audible click level and count-in/start alignment; automated timing is not an output-latency measurement.

Use focused tests per slice and the full existing CI checkpoint before completion. Preserve 015's fast fixtures; don't add sleeps or require audio hardware for automated tests.
