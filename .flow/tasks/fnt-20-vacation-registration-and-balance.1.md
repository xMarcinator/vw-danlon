---
satisfies: [R1, R2, R3]
---
# fnt-20-vacation-registration-and-balance.1 Vacation days from calendar events, typed day-off reasons and balance

## Description
Classify all-day events as vacation / feriefridag / other day off, make the day-off reason typed, count only working days, and add a `vacation` command showing earned, taken, planned and balance per period for the current ferieår.

**Size:** M
**Files:** `src/month.rs`, `src/vacation.rs` (new), `src/config.rs`, `src/main.rs`, `config.example.toml`
**Touches:** [src/month.rs, src/vacation.rs, src/config.rs, src/main.rs, config.example.toml]

### Approach
- Extend FNT-17's classifier for `Occurrence::AllDay` (month.rs ~157); `Day.off_reason` becomes an enum with display text (holiday name, Ferie, Feriefridag, other title).
- Working day = would have planned hours / work_week hours and is not a public holiday or weekend; holidays are inserted first into `off`, keep that order.
- `vacation.rs`: pure balance calc over a year of occurrences (calendar loaded from 1 Sep), config keys with defaults (employment start optional, feriefridage per year, 2,08 per month).
- New `Vacation` subcommand.

### Investigation targets
**Required:**
- `src/month.rs:133-215` — off map and AllDay arm
- `src/holidays.rs` — public holidays
**Optional:**
- `src/main.rs:319-340` — calendar() loading range

## Acceptance
- [ ] Test: Ferie event Fri–Mon counts 2 days; over a public holiday the holiday is not counted
- [ ] Test: vacation day has norm 0 and reason Ferie
- [ ] Test: balance with start 1 Sep after 3 months = 6,24 earned; negative balance flagged forskudsferie
- [ ] `vacation` prints periods and balance; missing employment start stated
- [ ] `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
