---
satisfies: [R3, R5, R6, R8, R9, R10]
---
# fnt-22-danlon-app-checklist.1 Checklist model: Komme/gå and Kørsel registrations per day

## Description
Pure function from a month report (day records with periods, private time, lunch window, flags) and the month's business trips to per-day registrations and a day status (to enter, nothing to enter, fix first).

**Size:** M
**Files:** `src/checklist.rs` (new), `src/month.rs` (read-only use)
**Touches:** [src/checklist.rs, src/main.rs]

### Approach
- Komme/gå = `Day.segments` after private time, bridging gaps that overlap `Day.lunch.window`.
- Kørsel from the mileage entries (km, from → to, purpose); split entries (two rates) are one registration.
- Unit tests with the month.rs test helpers.

## Acceptance
- [ ] Test: stops use calendar titles and configured home/workplace names; an unnamed address falls back to the address
- [ ] Test: 19 Nov gives two entries: 07:58 to 10:00 (no break, 2h 02m) and 11:15 to 16:00 (break 30m, 4h 15m); Læge is not registered
- [ ] Test: 4 Nov lunch errand gives 07:41 to 15:55, break 45m, 7h 29m
- [ ] Test: for every day, the entries' spans minus their breaks add up to the day's worked hours
- [ ] Test: 18 Nov gives one Kørsel: Purpose Kunde A, Hjem → Kunde A, return on, 148,0 km
- [ ] Test: Kontoret → Kunde C → Kontoret gives Kontoret → Kunde C, return on; a non-returning day lists all stops with return off
- [ ] Test: an unworked gap outside the lunch window (not private time) also splits the entry
- [ ] Registrations carry no Description value (R10)
- [ ] Test: weekend/vacation → nothing to enter; arrival without departure → fix first
- [ ] `cargo test` passes

## Done summary
Added the pure checklist model (src/checklist.rs): per day a status (to enter / nothing to enter / fix first), Komme/gå entries (private time and non-lunch gaps split the day, a lunch-window gap is the break) and one Kørsel registration (purpose, named stops, return trip, km, chain gaps). Stop names come from the new [checklist] home_name/workplace_name (default Hjem/Kontoret) via address/coordinate matching, else the day's calendar title, else the address. No Description field exists on either registration (R10).

Tests in src/checklist.rs: private_time_splits_the_day (19 Nov), lunch_errand_is_the_break (4 Nov), unworked_gap_outside_lunch_splits_the_day, entries_add_up_to_worked_hours, statuses (weekend, Ferie, arrival without departure), return_trip_to_the_start (18 Nov incl. a rate-split trip, Kontoret → Kunde C, non-returning day), trips_that_do_not_chain_flag_the_gap (R9 error), stops_are_named_as_in_the_app (R6 incl. address fallback).

Notes for .2: the module is marked expect(dead_code) in non-test builds (main.rs mod line, Config.checklist field) until the CLI uses it; remove those attributes then. Fix-first flags are the report remark parts not ending in " · private"; on weekends/days off without work, flags are ignored. Entries sum to worked hours exactly only with periods.round_to_minutes = 0 (rounding is applied to the report total, not to Danløn entries).

baseline: green (cargo test 42 passed; clippy 1 pre-existing dead_code warning on Day.appointments, now resolved)
stage: impl-review - skipped(config: REVIEW_MODE not set by conductor)
Tier: not provided (model: claude-opus-5-5)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 8bce218563b92ece3978abe7598701cc9d0abffa
- Tests: cargo test (50 passed), cargo clippy --all-targets (0 warnings)
- PRs: