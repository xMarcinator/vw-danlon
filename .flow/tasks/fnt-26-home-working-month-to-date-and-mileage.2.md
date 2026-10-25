---
satisfies: [R1, R2, R3, R4]
---
# fnt-26-home-working-month-to-date-and-mileage.2 Month to date, today marker, no flags without work events, home working

## Description
Totals (norm, worked, difference) up to today; today marked in report and screens' data; days without a work event and without home working never flagged; home working from calendar (keywords, event at home address) and a home-working timer (home start|stop|list|edit|rm) like private time.

**Size:** M
**Files:** src/month.rs, src/service.rs, src/state.rs or db, src/config.rs, src/main.rs

## Acceptance
- [ ] report: totals to date; today marked; future days planned only
- [ ] weekend driving without work event not flagged
- [ ] Hjemmearbejde event counts planned window minus lunch/private; timer time counts
- [ ] home timer CLI round trip; forgotten timer flagged


## Done summary
Month totals now run to today (report marks today, later days planned norm only, whole-month norm printed separately); days without a work event, home working or norm are never flagged; home working counts from calendar events (keywords / home address) and a new `home start|stop|list|edit|rm` timer stored in SQLite (schema v2, v1 upgraded on open, read-only v1 reads as no home time).

Decisions: Timer<K> generic shared by private and home timers (home errors name "home-working interval"; default label "Hjemmearbejde"); EntryKind::Home set in service::timed_entries so mileage/checklist never treat a home event as a client address; checklist flags now read Day.check (note minus private/home lines); a running home timer counts only once stopped (like private); CLI tests compare against the real today since the CLI has no injectable date.
Note: the user's vw-danlon.db is upgraded to schema 2 on the first run of this CLI; an app built before this commit then refuses it as "newer" until task .5 rebuilds the app.
Tests: month::tests::home_working_counts_as_worked_and_totals_run_to_today (R1, R3, R4, overlap), working_time_is_time_at_calendar_address (R4 weekend), checklist::tests::home_working_is_entered_and_weekend_driving_is_nothing, state::tests::home_timer_round_trips_and_names_its_intervals, db::tests::a_version_1_database_gets_the_home_tables, tests/home_timer_cli.rs (R2 CLI round trip, forgotten timer, today marker).

baseline: green (cargo test --workspace pre-edit)
stage: impl-review - skipped(config: review backend not configured; conductor owns review)
Tier: not given by conductor (model: claude-opus-5-5)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 1151ff194501f6483fbe926b833c536bd9a01fb2
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets
- PRs: