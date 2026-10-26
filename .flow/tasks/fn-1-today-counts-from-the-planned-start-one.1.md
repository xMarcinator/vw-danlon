---
satisfies: [R1, R2, R3]
---
# fn-1-today-counts-from-the-planned-start-one.1 Library: today so far, no early flags, workplace fallback, work state

## Description
Expected-so-far norm and worked-so-far for today in month/service (inject now); no missing-departure / no-driving flags for today; workplace detection from the calendar when workplace is unset (most frequent work-event location over 60 days, ≥3 days) used by mileage and month, exposed for Settings/Mileage; service::work_today for the app; CLI report shows today 'so far'.
**Files:** src/month.rs, src/service.rs, src/mileage.rs, src/config.rs, src/main.rs

## Acceptance
- [ ] Tests: late arrival counts from planned start; before start no negative; after end equals whole day
- [ ] Today not flagged without a drive home
- [ ] Workplace unset: detected; home ↔ it is commute; no candidate → warning
- [ ] work_today states: not working (late), office by trip, home timer, private running/auto


## Done summary
Library side of fn-1: today counts so far from the planned start (expected = planned up to now minus passed lunch; open at-work period, running home and private timers up to now), no missing-departure / no-driving flags for today, workplace fallback detected from the calendar (most distinct days in the 60 days up to month end, >= 3, unique) used by month, mileage, checklist and fradrag, service::effective_workplace and service::work_today for the app, and the CLI report's "◀ today (so far)" row with detected-workplace note / no-workplace warning.

Decisions: private time is not subtracted from expected-so-far (it reduces worked, as for the whole day), so after the planned end today equals the whole-day numbers; with the norm from [work_week] (no planned window) today's expected stays the day's norm; effective_workplace takes the month as a third argument (60 days up to its end); a tie for most days is no clear candidate. tests/month_view_matches_report.rs now runs report and month_view at the same real minute and compares the so-far norm column (declared intent: report shows today so far). Tests: month::tests::{today_counts_from_the_planned_start_up_to_now, today_is_not_flagged_before_the_day_is_over, running_timers_count_up_to_now_today, workplace_is_detected_from_the_calendar}, service::tests::{without_a_workplace_the_one_in_the_calendar_is_commuting, work_today_states}.

baseline: green (cargo test --workspace)
stage: impl-review - skipped(config: REVIEW_MODE not set by the conductor)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 856c946212b0ad3c6071f0c023160e9ebcb84daf, 09407ae07065da94c451683b2f1871941e9888d2, b67ddf1e6bf059e873f27fda5c582510ecadf799
- Tests: cargo test --workspace
- PRs: