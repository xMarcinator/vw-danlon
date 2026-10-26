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
Phone Day shows Worked · Norm · Diff in one card (today: Worked so far · Expected so far · Diff with their small lines); Trips section with start time, From → To, km, type chip (Commute grey, Client visit violet, including the detour legs of a client drive), heading with the day's km driven and reimbursed kr ("Trips · 23,0 km · 55,16 kr"), and "+ Add missing trip" at its end. The lunch line in the day list has Edit opening a lunch sheet (Start, Minutes, Use default, Save) via service::set_lunch; a day with no lunch keeps a "No lunch" line so it can be set again. The phone has no Lunch break or Mileage card; on desktop both stay (as in the desktop mockup) and Change opens the same sheet. Fixes from review: driving #5a87ab light, private its own blue in dark (#4fb0f0), timeline fixed 06–18 (outside clipped), client visits split out of At work in the day list (service::day_work_steps, test service::tests::day_work_steps_split_the_client_visit_out). app/Cargo.toml 0.5.0; no local release APK (the release workflow builds and signs it from the tag).

baseline: green (cargo test --workspace)
Live: desktop app on the scratch VW_DANLON_DIR, wide + 390px light/dark, lunch sheet open; 45 min saved then Use default cleared, 22 Oct numbers matched `vw-danlon report 2026-10` (4,48/8,00/−3,52 with 45 min; 4,73/8,25/−3,52 default). Screenshots: session scratchpad shots/fn2b-*.png.
Checks: cargo test --workspace, cargo clippy --workspace --all-targets, Android clippy (aarch64-linux-android, mobile), dx build --platform android (unsigned debug) all clean.

stage: impl-review - skipped(config: review backend not configured (ASK), none requested)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 16a262d3c67099a1c24170f4a844b6cdfd466ff4
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets, cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile, dx build --platform android --target aarch64-linux-android
- PRs: