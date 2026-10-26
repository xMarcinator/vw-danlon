---
satisfies: [R1, R4, R5]
---
# fnt-27-one-period-selector-on-every-page-day.2 Selector on Mileage, Danløn and Vacation; week filtering; release 0.6.0 prep

## Description
Mileage and Danløn with Week · Month (filter the month data to the ISO week; logbook CSV and print stay monthly and say so); Mileage → Fradrag and Vacation step years with no switch; drop the page headings; desktop too. Bump app version to 0.6.0 (the release workflow builds it from the tag).
**Files:** app/src/screens/mileage.rs, danlon.rs, vacation.rs, app/assets/app.css, app/Cargo.toml

## Acceptance
- [ ] Week view on Mileage and Danløn shows the week's trips/registrations; month unchanged
- [ ] Fradrag and Vacation step years
- [ ] cargo test, clippy clean; version 0.6.0


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
