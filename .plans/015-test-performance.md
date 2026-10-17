# Test performance and completion checkpoint

Status: implemented and validated, 2026-10-17. Stacked after numeric entry
settings (PR #45), which includes row shading (PR #42). No production DSP,
dependencies, test cases, sample rates, gate durations or assertions removed.

## Evidence and change

The first timing run used stale shared build artifacts referring to a removed
`.worktrees/row-shading` path; two fixture loads failed and entry tests were absent.
`cargo clean -p track-rs` rebuilt this checkout. The valid pre-change all-target
baseline passed all 190 tests in **165.620 s**, excluding compilation.

The sustained production test renders twice across 3 rates × 3 tempos × 3 octaves
× 3 sustained instruments. Its preset helper needlessly processed all 128 song
and preview channel slots on every sample. Size that helper's slots and meter
sample buffer to the populated instrument count, retaining actual Output/GatedVoice
rendering, deterministic equality, pitch, onset, hold, release, finite output,
continuity and exact-silence assertions. Capacity, routing, retrigger and transport
regressions keep their existing full-size fixtures.

Measured experiments (nextest execution time, excluding compilation):

| Configuration | Sustained test |
| --- | ---: |
| Initial stale-build diagnostic (not a valid suite baseline) | 166.593 s |
| Optimize FunDSP only | 160.170 s |
| Optimize full test profile | 34.850 s |
| Full test optimization + populated-channel fixture | 1.409 s |
| Populated-channel fixture, default test profile | 33.111 s |
| Populated-channel fixture + optimize only track-rs | **3.483 s** |

Keep the last option: `[profile.test.package.track-rs] opt-level = 2` optimizes
our per-sample loop without rebuilding every dependency in optimized mode.
Debug assertions and overflow checks retain the test-profile defaults. The
focused rebuild took 8.28 s; clean-machine build times will differ.

## Final checkpoint

- All-target locked nextest: **190 passed, 0 skipped, 3.824 s**, down from
  165.620 s (about **43× faster**, 97.7% less execution time).
- `cargo fmt --all --check` and strict all-target/all-feature Clippy passed.
- `cargo test --doc --locked`: passed, zero doctests.
- Warning-denied `cargo doc --no-deps --locked`: passed.
- `actionlint` and `cargo deny --locked check`: passed; existing duplicate-crate
  warnings remain, with no dependency changes.
- ALSA-null `just_a_note`: passed. This checks setup, not audible output.
- `git diff --check`: passed.
- Plans 001–014 now distinguish current implementation from historical rollout
  notes. README includes numeric settings and semantic alternating row shading.

Current acceptance update (2026-10-17): all existing PRs are merged per the user.
The current build’s general manual UI/audio gate is provisionally accepted for
now, with UI friction still reported. This does not establish that every listed
specialized check was performed. See the [canonical backlog](README.md) and
plans 016–018 for the next work; the automated evidence above is unchanged.
