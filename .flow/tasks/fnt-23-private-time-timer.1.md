---
satisfies: [R1, R3]
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
- [ ] Test: start/stop round-trip records an interval
- [ ] Start while running and stop when idle are refused with clear messages
- [ ] edit/rm work; end before start, unknown id and >24 h are refused
- [ ] `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
