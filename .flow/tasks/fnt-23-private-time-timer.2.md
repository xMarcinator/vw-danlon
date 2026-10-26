---
satisfies: [R2, R4, R5, R6]
---
# fnt-23-private-time-timer.2 Recorded private time counts like appointments; report flag; docs

## Description
Feed recorded intervals into `month::build` alongside calendar private appointments, so worked time, notes, trip classification and the checklist all use them; flag a timer left running.

**Size:** M
**Files:** `src/month.rs`, `src/mileage.rs`, `src/main.rs`, `README.md`
**Touches:** [src/month.rs, src/mileage.rs, src/main.rs, README.md]

## Acceptance
- [ ] Test: a linked 12:30–14:10 interval replaces the planned Læge 13:00–14:00 (deducted once, 1:40)
- [ ] Test: a recorded 10:02–11:14 interval is deducted and noted like a calendar Læge event
- [ ] Test: a trip starting inside a recorded interval is private
- [ ] A timer running past midnight or >12 h is flagged in `report`
- [ ] README documents the timer; `cargo test` passes

## Done summary
Recorded private intervals now feed `month::build` like calendar private appointments (deducted, noted, split at midnight); an interval linked to an appointment replaces that appointment's planned window, so it counts once. Trips starting during recorded time are private (via `month::recorded_entries` in mileage), `report` flags a timer running past midnight or over 12 h (`RunningTimer::forgotten`), and the README documents the timer.

Tests: month::recorded_private_time_counts_like_an_appointment (10:02–11:14 noted/deducted; linked 12:30–14:10 replaces Læge 13:00–14:00 → 1:40), mileage::trips_during_recorded_private_time_are_private, state::forgotten_timer. Manually ran `report` with a temp config: note and forgotten-timer flag shown.

Follow-up: clippy still warns `Day.appointments` is never read (pre-existing); nothing in this change reads it outside tests — FNT-22's Komme/gå split is the natural reader.

baseline: green (cargo test, 33 passed)
stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: be6503ccce17618b28093302697022b9aa83ecac
- Tests: cargo test (36 passed), cargo clippy --all-targets (1 pre-existing warning: Day.appointments never read)
- PRs: