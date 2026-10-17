# Numeric entry settings

Add F1 Pattern Normal `:step N` (0..=4096 rows) and `:length Nt` / `:length Nr`
(1..=4096 ticks; 16 ticks per row). Require ASCII decimal integers and explicit
lowercase length units; reject signs, fractions, extra arguments and overflow
before changing state. Counts don't multiply settings. Other views reject these
commands; Insert/Audition must return to Normal to open the command prompt.

Keep settings on App, like entry octave and step: session-only, retained across
view switches and successful/failed project loads, outside document/history.
Length defaults to one row and applies only to empty cells. Pitch replacement
preserves the existing gate; Insert `{`/`}` still edits only the cursor note.
Same-note re-entry remains a history no-op. Previews keep their 200 ms gate.

Show the actual default in Insert's separate `New` field, with integer rows when
exact and ticks otherwise. Keep Cell detail independent and Audition unchanged.
Tab retains 0/1/2/4 cycling, returning arbitrary values to zero; step zero never
advances. No schema, dependencies, audio or unrelated formatting changes.

## Checks

- Real prompt/key tests: bounds, strict parsing, counts, modes/views, prompt
  ownership, chromatic/octave entry, actual inserted lengths and row clamping.
- History/dirty snapshots, preserved cell lengths, no-op re-entry, redo and
  settings surviving view switches and project loads.
- Context rendering at narrow widths, separate New/Cell, bounded previews.
- Full formatting, strict all-target Clippy, nextest (183 baseline), rustdoc,
  doctests and diff whitespace checks. Manual terminal/listening remains pending.
