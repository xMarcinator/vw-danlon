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
Added the shared period selector (app/src/screens/period.rs, `.period-bar` CSS) per the approved mockup, with a year mode (note in place of the switch) ready for task .2; Hours uses it with Day · Week · Month, replacing the old Month/Week switch and back button. Day now lives inside Hours (opens on today, arrows step days, Today disabled on the current period, a day in Week/Month opens Day on that date; Day's heading/back link/breadcrumb removed), and Hours' view + date sit in an app-level signal so they survive tab switches (R6). Live-checked on the desktop app (scratch data, wide + 390px, light + dark); unit tests cover stepping, Today and view switching.

Follow-ups: the wide Day view keeps "Add missing trip / Add time" in a row under the selector; the `.month-nav`/`.nav-btn` rules stay for Mileage and Danløn until task .2. Feature route changed: the Day view is reached from Hours (Day switch or tapping a day), no longer a separate screen with a back link.

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 06aac3afbd15ee273a635c56cee60b2aa572ab66
- Tests: cargo test --workspace (pass), cargo clippy --workspace --all-targets (0 warnings), cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile (0 warnings), cargo fmt --all -- --check (clean after fmt)
- PRs: