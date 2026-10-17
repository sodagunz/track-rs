# UI direction

Status: implemented foundation on develop, updated 2026-10-17. Current priorities
and provisional manual acceptance live in the [canonical backlog](../.plans/README.md).

- Custom themes: implemented by [012](../.plans/012-custom-themes.md), including
  terminal-palette defaults and optional truecolor overrides.
- Modal editing: implemented. Normal/Visual use counted `hjkl` motions. Pattern
  Insert/Audition use chromatic piano keys and **arrows for navigation**, not
  `hjkl`; `Esc` returns to Normal. See [010](../.plans/010-pattern-entry-and-ui.md).
- F1–F4 select Pattern, Arrangement, Mixer and built-in Instruments across
  view-local modes; help/prompts retain input priority. Custom synthesis
  parameters are not implemented.
- `:` commands, contextual status, aligned note/length fields, beat/bar markers
  and distinct edit/play positions are implemented. Numeric New length and
  step are covered by [014](../.plans/014-entry-settings.md).

The UI still feels clunky to the user, who is rusty with trackers. Record a
concrete awkward workflow before planning another UI pass; don't treat the
implemented foundation as a claim that discoverability is solved.

[Terminal tracker research](tui-trackers.md) supplies references, not acceptance
evidence or a second backlog. Its recommendations are historical proposals;
velocity fields and richer instrument editing are not current features.
