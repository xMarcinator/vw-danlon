---
satisfies: [R3, R4, R5]
---
# fn-2-phone-day-view-one-line-timeline-client.2 Day numbers, Trips section, lunch Edit sheet; APK 0.5.0

## Description
Day numbers: Worked · Norm · Diff only (today: Worked so far · Expected so far · Diff with their small lines); no Mileage card. Trips section under the day's list: time, From → To, km, type chip (Commute grey, Client visit violet), + Add missing trip (existing form); heading 'Trips · 37,0 km · 0,00 kr'. Lunch line in the list gets Edit opening a sheet (Start, Minutes, Use default, Save) via the lunch override; remove the separate Lunch break card. Bump to 0.5.0 and build the signed APK with scripts/build-apk.sh.
**Files:** app/src/screens/day.rs, app/assets/app.css, app/Cargo.toml

## Acceptance
- [ ] Day numbers and Trips section match the mockups; heading shows day km and kr
- [ ] Lunch Edit sheet saves, Use default clears; numbers match CLI report
- [ ] dist/vw-danlon-0.5.0-arm64.apk signed and verified


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
