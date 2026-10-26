---
satisfies: [R3, R4, R8, R11]
---
# fnt-21-dioxus-desktop-and-phone-app.3 Month and Day screens

## Description
Month (KPIs, day table, flags) and Day (timeline, lunch, appointments, trips) screens on top of the library's month report.

**Size:** M
**Files:** `app/src/screens/month.rs`, `app/src/screens/day.rs`
**Touches:** [app/src/screens/**]

### Design context
Mockup boards Main, Day, PhoneMonth, PhoneDay.

## Acceptance
- [ ] The private-time Start/Stop button shows the running time; recorded intervals can be relabelled, edited and deleted
- [ ] Month shows the same norm/worked/difference per day as `report` for the example data
- [ ] Day shows periods, lunch window and source, appointments and trips
- [ ] Layout works at phone width

## Done summary
Month and Day screens in app/src/screens/ on top of new library service functions (month_calendar, month_view, private_time, start_private, change_private, day_trips; Private::relabel; Day::check). Month: month navigation, source chips, the R11 collapsible private-time card (current/next entry with Start/Stop, running status with the planned window it replaces, expanded list of today's and coming calendar/recorded entries with state chips, "Start without an event", Edit -> relabel/retime/Delete for recorded intervals), KPIs and the day table (phone: day list and check flags). Day: KPIs, timeline bars (phone: step list), lunch window and source, private time (recorded intervals editable), trips with Business/Commute/Private/Incomplete type. Calendar HTTPS on Android uses ureq's bundled webpki roots (RootCerts::WebPki under cfg!(target_os = "android")), desktop keeps the platform verifier; the Connected Cars agent uses the same helper.

Equivalence: tests/month_view_matches_report.rs runs the CLI `report 2026-10` on a temp copy of config.example.toml + examples (with a lunch override and a recorded interval in state.json) and compares norm/worked/diff per day and the totals with service::month_view (calendar fetched with the 14-day private lookahead).
Live run (scratch VW_DANLON_DIR, GDK_BACKEND=x11): Month numbers equal the CLI; Start linked today's Læge, Stop recorded it ("replaces the planned 20:00-21:00"), relabel + retime saved, end-before-start refused with "end 18:00 is not after start 19:00" and nothing saved, Start without an event and Delete worked; Day 6/7/8 Oct and 390px layouts checked. Screenshots in the session scratchpad shots/t3-*.png.
Builds: dx build desktop ok; dx build --platform android --target aarch64-linux-android ok (APK not run on a device). clippy clean host and aarch64-linux-android.

Follow-ups: lunch "Change" / Add missing trip / Import trips / Working-time record buttons from the mockup are not built (not in this task's AC); the interval editor closes on a refused save (the error is shown, the input is lost); Android calendar fetch compiled but not run on a device.
Note: an early check ran the spec's quick command `cargo run -- -c config.example.toml report`, which reads the repo-root state.json (the user's); nothing was written. Later runs used a scratch copy.

baseline: green (cargo test --workspace, 69+8 passed at 1a7f60c)
Tier: not reported (model: claude-opus-5-5)

stage: impl-review - skipped(config: REVIEW_MODE=none - no review mode given in the dispatch)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: bb1f8aca395379cc9b1a6ebf7aebcaeb426dfc59
- Tests: cargo test --workspace (72 lib/integration: 71 lib + 1 month_view_matches_report, 8 app), cargo clippy --workspace --all-targets (clean), cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile (clean), dx build --platform desktop (ok), dx build --platform android --target aarch64-linux-android (ok)
- PRs: