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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
