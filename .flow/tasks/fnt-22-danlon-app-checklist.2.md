---
satisfies: [R3, R4, R5]
---
# fnt-22-danlon-app-checklist.2 checklist CLI with tick-off in the state file; docs

## Description
`checklist <month>`, `checklist mark|unmark <date>` storing ticks in the state file; README section on using the Danløn app with the tool.

**Size:** M
**Files:** `src/main.rs`, `src/state.rs`, `README.md`
**Touches:** [src/main.rs, src/state.rs, README.md]

### Approach
- New defaulted `entered` section in the state struct (date set).
- Output groups days by status with progress counts.

## Acceptance
- [ ] `checklist 2026-11` prints registrations and progress
- [ ] `mark`/`unmark` round-trip; marking a fix-first or nothing-to-enter day is refused naming why
- [ ] README explains the Danløn app routine; `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
