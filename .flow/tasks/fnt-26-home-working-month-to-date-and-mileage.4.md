---
satisfies: [R12, R13]
---
# fnt-26-home-working-month-to-date-and-mileage.4 Private time starts automatically, calendar time after grace, always editable

## Description
A private calendar appointment whose start has passed without a recording shows as started automatically; the user can set the real start or stop; untouched until grace (default 60 min after planned end) records the planned window marked calendar time; every entry editable/relabel/delete at any time. Config [private] auto_start, grace_minutes.

**Size:** S
**Files:** src/state.rs or db, src/service.rs, src/month.rs, src/main.rs

## Acceptance
- [ ] Tests: auto-start shown after start; set earlier start; stop; grace expiry records planned window marked calendar time
- [ ] Calendar-time and auto entries editable and deletable; CLI private list shows their kind


## Done summary
Private calendar appointments now start automatically: past their planned start with nothing recorded they show as "started automatically" (CLI `private list`, `service::private_time` -> `PrivateState::StartedAutomatically`); `private start --at HH:MM` sets the real start, `private stop` records planned start..now, `private skip [--event] [--date]` says you did not go; untouched until `[private] grace_minutes` (60) after the planned end, `service::settle_private` records the planned window as an interval of kind calendar time (run by `report`, `private list` and the app's `month_view`; no background service). `private edit` gained `--label`; `private list` shows each entry's kind.

Decisions: deleting any interval linked to a calendar appointment means "I didn't go": the appointment is stored in `private_dismissed` and then counts no private time (month and trip classification) and is never re-created; this replaces FNT-23's "deleting a linked recording falls back to the planned window". Editing the times of calendar time turns it into recorded time (relabel alone keeps the kind). Materialisation has no cutoff: on first load every past untouched private appointment in the loaded calendar range becomes calendar time (numbers unchanged). Stop without a timer picks the latest-started automatic appointment; `start` with no event prefers the automatic one, else the existing current/next link. Schema 4: `private_intervals.calendar_time` + `private_dismissed`, upgrade from 1-3; read-only older DBs read fine. App kept compiling with minimal changes (Stop/Delete call `service::stop_private`/`delete_private`, a "Started" chip); task .5 does the screens.
Note: the user's vw-danlon.db upgrades to schema 4 on the next CLI run; an app built before this commit refuses it until task .5 rebuilds the app.
Tests: service::tests::a_private_appointment_starts_automatically_then_becomes_calendar_time (auto-start shown, grace, calendar time once, numbers unchanged), an_appointment_started_automatically_gets_its_real_start_or_is_stopped (earlier start, future refused, stop, skip), calendar_time_can_be_edited_relabelled_and_deleted (R13 incl. unknown id refused), without_auto_start_an_appointment_stays_planned; db::tests::a_version_1_database_gets_the_later_tables.
Manual: scratch copy of user data (--offline) report 2026-10 identical before/after (no private appointments in it, schema 4 upgrade ok); e2e scratch calendar: list/stop/edit --label/rm/skip behave as above.

baseline: green (cargo test --workspace pre-edit)
stage: impl-review - skipped(config: review backend not configured; conductor owns review)
Tier: not given by conductor (model: claude-opus-5-5)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: ff9cea9d2162e2a5452a0ba8ad1d1c9a3b394daa
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets
- PRs: