---
satisfies: [R2, R4, R5]
---
# fnt-23-private-time-timer.2 Recorded private time counts like appointments; report flag; docs

## Description
Feed recorded intervals into `month::build` alongside calendar private appointments, so worked time, notes, trip classification and the checklist all use them; flag a timer left running.

**Size:** M
**Files:** `src/month.rs`, `src/mileage.rs`, `src/main.rs`, `README.md`
**Touches:** [src/month.rs, src/mileage.rs, src/main.rs, README.md]

## Acceptance
- [ ] Test: a recorded 10:02–11:14 interval is deducted and noted like a calendar Læge event
- [ ] Test: a trip starting inside a recorded interval is private
- [ ] A timer running past midnight or >12 h is flagged in `report`
- [ ] README documents the timer; `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
