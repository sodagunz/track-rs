# Plans and backlog

Canonical backlog, updated 2026-10-17 against `origin/develop` at `fb27cea`.
All existing PRs are merged per the user; plans 001–015 are implemented.

## Acceptance now

The user provisionally accepts the current build's manual UI/audio gate **for
now**. Don't keep general manual acceptance as an implementation blocker.
The UI still feels clunky, and the user is rusty with trackers; neither is a
specific bug report or evidence that every interaction is satisfactory.

This is not a claim that every historical listening case, physical output device,
sample-rate stress case, resize/no-color/light-dark combination or truecolor
terminal was tested. Old evidence, test counts and unchecked specialized checks
remain historical records, not new certifications. Revisit those checks when a
related change or concrete reproduction warrants it. The automated checkpoint
is in [015](015-test-performance.md); no new application tests ran for this
planning update.

## Next, in delivery order

| Priority | Plan | First useful delivery |
| --- | --- | --- |
| 1 | [019 — Musical time, meter and local grids](019-musical-time-and-grids.md) | Exact event timeline, compatible migration, non-4/4 meter and lossless per-bar grids |
| 2 | [016 — Metronome, count-in and arming](016-metronome-and-record-arming.md) | Optional click, one-bar count-in and explicit record state; integrate arming with working capture |
| 3 | [017 — Quantized live recording](017-quantized-live-recording.md) | One pinned Pattern track, musical-grid quantization, fixed gate, undo per pass |
| 4 | [018 — MIDI input](018-midi-input.md) | Optional device preview, then step/live input through the same recording decisions |

019's core implementation gate is complete (240-test integration/UI checkpoint;
manual listening remains pending), so 016 may proceed. Delivered: sparse 960-PPQ events and
v4 migration, stable runtime IDs, tick-based history/clipboard, anchored tick
scheduling, non-4/4 meter and mixed per-bar grids. Exact gate/end controls admit
sub-row starts, general durations and partial bars without rounding. Runtime/v4
and history budgets count actual events/metadata, not projected cells; dense
legacy imports retain their input cell guard. Explicit onset quantization now
previews and rejects collisions/end-boundary snaps atomically, preserving IDs.
See [019 checkpoints](019-musical-time-and-grids.md#implementation-checkpoint)
for validation and remaining integration acceptance, not hardware certification.

016 now has usable `:record on|off|toggle` and `:countin 0|1` controls, meter-aware
full-bar count-in and honest ARM/COUNT/REC status. 017's first terminal slice
records fixed-gate notes on one pinned playing Pattern track, using receipt
timestamps, local-grid quantization and undo per pass. Stopped Insert step-edits;
playing Insert previews when unarmed and records only when armed and eligible.
Audition always means no note writes; Song capture is explicitly unavailable.
Automated acceptance is recorded in 016/017; physical listening remains pending.
018 now implements optional MIDI input, held-gate monitoring and explicitly
user-approved held-duration live capture. Terminal capture and stopped MIDI step
entry retain fixed New length. See 018 for lifecycle/history limits and validation;
physical MIDI checks remain pending. The known provisional-note/monitor overlap
on later loops is fixed with owned-event suppression; shared-loop and exact-sample
offline proof is recorded in 018. The separate Pluck 8 kHz NaN issue remains open.
No 020 implementation is implied.

Musical-time scope is resolved in favor of 019's proposed sparse event timeline and fixed 960 PPQ, separating meter, pattern length, grid, edit step and duration. Per-bar grid changes must not retime music. Pattern-local meter first; arbitrary tuplets and continuous song meter automation remain deferred. The user approved starting implementation in this order. Approval is not an implemented-status label.

## Planned separately / exploratory drafts

| Plan | Status and scope |
| --- | --- |
| [020 — Three-oscillator synth](020-three-oscillator-synth.md) | Planned, manual-friendly DSP: three oscillators, resonant LPF and amplitude ADSR. Ownership optional; do not auto-implement in a batch without an explicit request. Independent of 019/022. |
| [021 — VST3 hosting](021-vst3-hosting.md) | Draft only: bounded offline instrument-host experiment before dependency choice or full-host approval; effects and native editors are later scope. |
| [022 — Engine and frontends](022-engine-and-frontends.md) | Draft only: embedded engine seam first, local headless server/TUI client later, GPUI last. No distributed rewrite or GUI implementation approved. |

These entries don't change the recording delivery order above. The embedded seam
in 022 should precede process separation and GPUI; a 021 prototype needn't wait,
though integration should reuse that seam when practical. 019 precedes accurate
plugin musical context and timeline automation, not standalone synth DSP.

## Later / needs evidence

- **UI friction:** capture the next awkward task with expected action, actual
  keys/mode/view, terminal size and result. Use that reproduction to choose a
  focused improvement; don't invent a redesign from “clunky”. Existing research
  is [terminal tracker workflows](../.ideas/tui-trackers.md), not user testing.
- Extend live capture only after the Pattern workflow lands: Song/shared-pattern
  policy, swing, latency calibration,
  velocity, chord allocation and MIDI expression/sync/output remain deferred.
- Sample loading, general effects/routing and track removal have no scheduled
  implementation here. Custom synthesis now has the bounded proposal in 020;
  plugin effects remain outside 021's first instrument slice. Reassess other
  scope from an actual task, not duplicated historical “deferred” lists.

## Completed implementation archive

| Plan | Implemented scope |
| --- | --- |
| [001](001-tracker-controls-audio-and-ui.md) | Commands, help, transport, tempo, voices, grid |
| [002](002-modal-editing-and-projects.md) | Modal editing, clipboard/history, projects |
| [003](003-tracks-sustains-and-playback.md) | Tracks, cursor playback, lengths, sustains, help |
| [004](004-contextual-status-and-buffer-lines.md) | Context/status and pattern buffer lines |
| [005](005-arrangement-view.md) | Arrangement view |
| [006](006-mixer-view.md) | Mixer overview foundation, extended by 009/011 |
| [007](007-instrument-view.md) | Built-in instrument selection |
| [008](008-instrument-audition.md) | Instrument audition |
| [009](009-channel-mixer.md) | Gain/pan, tone, bulk editing, meters |
| [010](010-pattern-entry-and-ui.md) | Chromatic entry, grid/context, Pattern Audition |
| [011](011-channel-mute-and-solo.md) | Channel mute/solo |
| [012](012-custom-themes.md) | Themes and restored alternating rows |
| [013](013-pattern-length.md) | Explicit pattern resize |
| [014](014-entry-settings.md) | Numeric New length and step |
| [015](015-test-performance.md) | Faster sample regressions and checkpoint |

The archive preserves original scope and rollout evidence. This page owns current
priorities and acceptance; the main README and binding registry own implemented
controls. [UI ideas](../.ideas/ui.md) points here rather than maintaining another backlog.
