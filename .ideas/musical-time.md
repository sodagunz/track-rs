# Musical time: tracker precedents

Research checked 2026-10-17 against official manuals. Recommendation, not implemented behavior: separate musical time from the editor's rows. Delivery contract: [019](../.plans/019-musical-time-and-grids.md).

| Precedent | Useful distinction | Limit / lesson for track-rs |
| --- | --- | --- |
| [Renoise transport](https://tutorials.renoise.com/wiki/Transport_Panel) | BPM controls tempo; LPB controls lines per musical beat. `ZTxx` and `ZLxx` can change them during playback. | LPB changes the time represented by existing lines. Don't copy this as a harmless view-grid command. |
| [Renoise tracker interface](https://tutorials.renoise.com/wiki/Tracker_Interface) | Song LPB is global; patterns have independently chosen lengths, 1–512 lines. The manual illustrates 3/4 with 12 lines at LPB 4. Delay `00–FF` moves a note from its line toward the next line. | Non-4/4 music is documented, but this does not establish an arbitrary per-bar meter-map feature. Delay gives sub-line placement, not unlimited events in one note column. Triplets need compatible resolution or approximate offsets. |
| [Renoise phrase properties](https://tutorials.renoise.com/wiki/Phrase_Editor) | Each phrase has independent LPB, length and loop bounds; shuffle is separate. | Local timing is useful, but an instrument phrase is not a song bar or a meter change. |
| [Renoise editing controls](https://tutorials.renoise.com/wiki/Pattern_Editor) | Live quantization is measured in lines; edit step separately controls cursor advancement. | Keep entry step, gate duration and recording quantization distinct. |
| [OpenMPT song properties](https://wiki.openmpt.org/Manual:_Song_Properties) | Rows/beat and rows/measure drive highlights and exported time-signature metadata. MPTM permits pattern overrides. Modern tempo uses BPM independently of ticks/row; changing rows/beat changes row duration. | Format and tempo mode matter. Classic/Alternative modes have different tempo semantics; don't transplant tracker “speed” into our quarter-note clock. Modern tempo swing varies durations of rows within a beat. |

## Distillation

- Higher line density is a practical tracker solution for triplets or short notes, but it couples storage, playback and display. A global LPB change is not the requested “show a finer grid in this bar without moving my notes”.
- Prefer exact integer musical positions with a bounded supported subdivision set, plus optional per-bar grid overrides. Meter sets bar length; grid sets editing points; tempo maps musical time to elapsed time.
- A delay field alone still leaves track-rs's one-event-per-row ceiling. Use sparse event starts if multiple notes must fit inside a former row. Keep monophonic playback policy separate from event storage.
- Swing, arbitrary tuplets and full song meter automation are later work. The cited manuals establish precedents, not a requirement to reproduce their compatibility machinery.

## Local evidence

Inspected merged `origin/develop` (`fb27cea`) with `git show`, without changing the older checkout: `src/tracker.rs` stores dense `Vec<PatternStep>` tracks, one optional note per row, and `NoteLength(u16)` at 16 ticks/row (1–4096). `src/transport.rs` schedules four rows per quarter and collects row starts. Thus current effective PPQ is 64: a 256th gate is already one tick, but triplets aren't exact and onsets remain row-bound. `src/project_file.rs` writes version 3, migrates versions 1/2 and validates dense dimensions. See 019 for the migration rather than treating LPB as an isolated setting.
