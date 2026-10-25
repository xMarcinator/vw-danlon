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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
