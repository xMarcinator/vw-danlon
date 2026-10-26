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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
