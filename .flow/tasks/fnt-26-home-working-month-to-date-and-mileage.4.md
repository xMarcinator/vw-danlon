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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
