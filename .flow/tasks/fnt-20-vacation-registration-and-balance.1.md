---
satisfies: [R1, R2, R5, R6]
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
- `vacation.rs`: pure balance calc per holiday year (1 Sep – 31 Aug earning, use by 31 Dec the year after), 0,07 per calendar day for a partial month, oldest-year-first drawing, expiry warning; calendar loaded from the older open holiday year's start. Config keys with defaults (employment start optional, feriefridage per year 5, 2,08 per month).
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
- [ ] Test: start 1 May 2026 → 2025/26 earns 8,32 usable until 31 Dec 2026; 2026/27 after Sep–Nov = 6,24
- [ ] Test: start mid-month earns 0,07 per remaining calendar day for that month
- [ ] Test: periods draw from the oldest year first; days drawn before earned flagged forskudsferie; unplanned days of a year past its use-by flagged expiring
- [ ] `vacation` prints periods and balance; missing employment start stated
- [ ] `cargo test` passes

## Done summary
All-day calendar events are classified as Ferie / Feriefridag / other day off with a typed `OffReason` on `Day`; only working days (weekday with planned hours, or work_week hours without the calendar norm, not a public holiday) count. New `src/vacation.rs` computes samtidighedsferie per holiday year in exact hundredths (2,08/month, 0,07/day for a partial month, use by 31 Dec the following year, oldest-year-first drawing with splitting, forskudsferie flag, expiry warning, feriefridage per year) and a new `vacation` command prints it; `[vacation]` config with defaults (config.example.toml/README left to task .2). Checklist still shows vacation days as "nothing to enter: Ferie".

Tests: vacation::tests (earned_from_employment_start, partial_month_earns_per_calendar_day, only_working_days_in_calendar_events_count incl. "no working days" note and forskud, periods_draw_from_the_oldest_year_first incl. expiry + missing-start text, a_period_splits_when_the_oldest_year_runs_out), month::tests::vacation_days_have_norm_0_and_reason_ferie, config::tests::vacation_defaults_and_employment_start.
baseline: green (cargo test, 62 passed pre-edit)
Follow-up: a work calendar event without LOCATION still adds a "kalenderpost uden adresse" note on vacation days (pre-existing behaviour; harmless for the checklist).

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 5ac2e2cda61297d395629921051b8b88b97d88e7
- Tests: cargo test (69 passed), cargo clippy --all-targets -- -D warnings
- PRs: