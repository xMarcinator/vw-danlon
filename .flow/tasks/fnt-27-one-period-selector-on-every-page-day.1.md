---
satisfies: [R1, R2, R3, R6]
---
# fnt-27-one-period-selector-on-every-page-day.1 Shared period selector; Hours with Day · Week · Month

## Description
A period-selector component (app/src/screens/, CSS in app.css) per the mockup; Hours uses it with Day · Week · Month; the Day view moves into Hours (Day mode, today by default, arrows step days, Today button); Week/Month day rows open Day on that date; phone and desktop.
**Files:** app/src/screens/mod.rs, month.rs, week.rs, day.rs, app/src/main.rs, app/assets/app.css

## Acceptance
- [ ] Selector matches the mockup (spacing, Today disabled on the current period), light and dark, 390px and wide
- [ ] Day · Week · Month on Hours; Day opens on today; arrows and Today work; tapping a day opens Day
- [ ] cargo test, clippy (desktop and Android) clean


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
