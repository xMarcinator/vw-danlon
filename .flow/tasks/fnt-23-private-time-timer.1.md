---
satisfies: [R1, R3, R6]
---
# fnt-23-private-time-timer.1 Timer state and private CLI

## Description
Running timer and recorded private intervals in the state file; `private start|stop|list|edit|rm`.

**Size:** M
**Files:** `src/state.rs`, `src/main.rs`
**Touches:** [src/state.rs, src/main.rs]

### Approach
- Defaulted `private` section: optional running {start, label} and a list of {id, start, end, label}.
- Times in the configured timezone; ids stable and short.

## Acceptance
- [ ] Test: start at 12:30 with Læge 13:00–14:00 in the calendar links the timer and labels it Læge; `--no-event` and `--event` override
- [ ] Test: start/stop round-trip records an interval
- [ ] Start while running and stop when idle are refused with clear messages
- [ ] edit/rm work; end before start, unknown id and >24 h are refused
- [ ] `cargo test` passes

## Done summary
Added a defaulted `private` section to state.json (running timer, recorded intervals with never-reused ids, optional replaced planned window) and the `private start|stop|list|edit|rm` CLI. Timer linking is the pure `state::link_event` (current or next private appointment within 2 h that day; `--event`/`--no-event` override); tests in src/state.rs cover linking, start/stop round-trip, and every R1/R3 refusal.

Edit rule: both times are on the interval's start day, except an end earlier than the start on an interval that already ended on a later day keeps that day; this keeps a forgotten overnight timer fixable and makes the over-24-hours refusal reachable.

baseline: green (cargo test, 30 passed)
stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: e82d607d3c098cf861f5d27d15b121f14c083338
- Tests: cargo test (33 passed), cargo clippy --all-targets (clean apart from pre-existing month.rs appointments warning), manual: private start/stop/list/edit/rm against scratch config + local .ics
- PRs: