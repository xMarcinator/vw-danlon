---
satisfies: [R1, R3, R4, R5]
---
# fnt-18-lunch-break-with-per-day-override.2 Lunch window resolution and overlap deduction

## Description
Replace the flat unpaid break with a lunch window: override > "Frokost" calendar event > default 12:00/30. Deduct only the overlap with work time left after appointments (FNT-17's subtraction helper). Show window and source in report and timesheet.

**Size:** M
**Files:** `src/month.rs`, `src/config.rs`, `src/timesheet.rs`, `src/main.rs`, `config.example.toml`, `README.md`
**Touches:** [src/month.rs, src/config.rs, src/timesheet.rs, src/main.rs, config.example.toml, README.md]

### Approach
- New `[lunch]` config section; remove `periods.unpaid_break_minutes` and make its presence fail with a message naming `[lunch]` (deny_unknown_fields gives a generic error, so check explicitly).
- Classifier from FNT-17 gains the lunch title; lunch events skip locations/planned like appointments.
- `worked` (month.rs:346) takes segments + lunch window instead of a minute total; `Day.break_minutes` becomes the lunch window + source; timesheet Pause column (timesheet.rs:~44) shows the window.
- Update tests that set `unpaid_break_minutes` (month.rs:368, :476, :577).

### Investigation targets
**Required:**
- `src/month.rs:250-360` — segments, worked, Day fields
- `src/timesheet.rs:15-62` — record columns
**Optional:**
- `src/main.rs:355-410` — print_days

## Acceptance
- [ ] Test: normal day 07:52–16:05 deducts 30 min
- [ ] Test: work ending 11:30 deducts nothing; lunch errand gap 11:45–12:30 deducts nothing more
- [ ] Test: doctor 11:30–12:30 overlapping lunch is not deducted twice
- [ ] Test: Frokost 12:30–13:00 event used; override beats event
- [ ] Old `unpaid_break_minutes` key fails with message naming `[lunch]`
- [ ] Report and timesheet show lunch window and source; README updated
- [ ] `cargo test` passes

## Done summary
Lunch window per day (override > Frokost calendar event > [lunch] default 12:00/30), deducting only its overlap with work time left after private appointments; report and timesheet show window and source (timesheet "Pause (min)" column is now "Frokost"); periods.unpaid_break_minutes fails to load naming [lunch]; README and config.example.toml updated. month::build now takes the lunch overrides and month::timed_entries the lunch title; danlon/mileage tests call them directly (no shims), and the danlon export test expects 8,00 h because the default lunch is deducted (R1, conductor-approved).

Tests: month::tests::lunch_overlap_with_work_time_is_deducted (R1/R3/R4: 07:52-16:05 deducts 30; work ending 11:30 deducts nothing; errand gap 11:45-12:30 deducts nothing; doctor 11:30-12:30 deducted once; Frokost 12:30-13:00 used, not a work address, no planned hours; override beats event; 0 min = none), config::tests::lunch_defaults_and_old_break_key_names_lunch (R5). CLI run on a copy of config.example.toml: report/timesheet show "12:00–12:30 (default)" and "11:00–11:45 (override)"; old key gives the [lunch] message.

baseline: green via handoff (ee0aa9e, cargo test 28 passed)
gate: cargo test - 30 passed, 0 failed
Pre-existing, not touched: dead_code warning on Day.appointments in non-test builds.
Note: repo-root config.toml still has unpaid_break_minutes (conductor handles it).

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 9dc1c2866f3ab230f797c5e2f7650a1f2ef83ec8, a27f21d75c8935ea0cd09b592df032ba889d27f3
- Tests: cargo test
- PRs: