---
satisfies: [R5, R6, R7, R8, R9, R14]
---
# fnt-26-home-working-month-to-date-and-mileage.3 Mileage under Skat's rules, per-trip override, all-driving logbook, fradrag estimate

## Description
Classify trips by LL § 9 B (client visit with 60-day rule for home↔client, commute private, errands private, client→home direct km only, between workplaces via override); per-trip Business/Private override (trip business|private|reset); logbook --all lists all driving with reason, reimbursable total, YTD km and required fields; fradrag <year> per LL § 9 C (24 km threshold, 2026 rates 3,17/1,59, yderkommune 3,51; no home-working/vacation days; never the same km as godtgørelse).

**Size:** M
**Files:** src/mileage.rs, src/month.rs, src/service.rs, src/main.rs, src/config.rs

## Acceptance
- [ ] Tests: commute not reimbursable; client visit reimbursable and work hours; 61st home↔client day private; reset after 60 days elsewhere; errand chain private; client→shop→home direct km only
- [ ] Override Business/Private and reset
- [ ] logbook --all shows reasons and totals; fradrag 0 kr at 18 km/day; example above 24 km computed
- [ ] Rates missing for a year with business km reported


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
