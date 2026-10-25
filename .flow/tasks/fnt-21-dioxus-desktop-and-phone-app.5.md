---
satisfies: [R3, R9]
---
# fnt-21-dioxus-desktop-and-phone-app.5 Danløn app checklist screen

## Description
The checklist from FNT-22 as a screen: per-day Komme/gå and Kørsel registrations, one Komme/gå per day with time, break and hours shown large, place names and km on Kørsel, copy actions in a ⋯ menu, tick-off per day, progress. Never connects to Danløn.

**Size:** S
**Files:** `app/src/screens/danlon.rs`
**Touches:** [app/src/screens/danlon.rs]

## Design context
Mockup boards "Danløn app checklist" (desktop and phone).

## Acceptance
- [ ] Shows the same registrations and status as `checklist <month>`
- [ ] Komme/gå shows time, break and hours large without Copy; Kørsel shows names and km; Copy description, from/to address and km sit in a ⋯ menu (desktop and Android)
- [ ] Ticking a day is stored and shown as progress

## Done summary
Danløn app checklist screen (app/src/screens/danlon.rs) built on the library: per day the Komme/gå entries (several when private time or a non-lunch gap splits the day; "HH:MM to HH:MM" large in mono, "Break X · total", no Copy) and one Kørsel (route by name, "Add return trip: on/off", Purpose, km; gaps flagged), no Description values. A ⋯ menu on Kørsel copies the purpose, the address of each stop (configured home/workplace address or the calendar entry's location) and the km, via document::eval with navigator.clipboard and a textarea/execCommand fallback for Android WebView, confirmed by a "Copied …" toast. Tick per day (stored in state.json through service::set_entered, refused for fix-first / nothing-to-enter days), progress bar with days/trips/fix-first, To enter / Entered / All tabs, month navigation, next-to-enter marker; wide and narrow layouts.

Library: checklist::progress (render uses it; output unchanged), Korsel.addresses, service::checklist_view and service::set_entered (CLI mark/unmark now go through it; output unchanged).

Tests: tests/checklist_view_matches_cli.rs (checklist_view rendered == `checklist 2026-10` stdout for the example data with a tick; a tick from the screen shows in the CLI; weekend and fix-first ticks refused with the same reason as `checklist mark`), checklist lib tests for addresses and progress, app test for the ⋯ menu actions.

Live desktop check (scratch VW_DANLON_DIR, GDK_BACKEND=x11): Danløn screen matched `checklist 2026-10` on the same dir; Copy address and Copy km put "Vestergade 12, 5000 Odense C, Danmark" / "196,5" on the clipboard (wl-paste) with the toast; ticking 5/6 Oct updated state.json and the CLI ("Days entered: 1 of 4 · Trips entered: 1 of 1"); fix-first checkbox disabled; untick removed it; dark theme and 390px layout with the menu scrolled into view above the tab bar. Fixed during the check: rows keyed by date (a reused checkbox kept its checked state). Screenshots: scratchpad/shots/t5-*.png. Android: clipboard path compiled into the APK, not run on a device.

Builds: cargo clippy --workspace --all-targets clean; Android clippy clean (needs CC_aarch64_linux_android/AR/linker from the NDK in env); dx build desktop ok; dx build --platform android --target aarch64-linux-android ok.

baseline: green (cargo test --workspace at 78f1b64)
Tier: not reported (model: claude-opus-5-5)

stage: impl-review - skipped(config: REVIEW_MODE=none - no review mode given in the dispatch)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 2f2f5f77b6de065ffb630b0a0bd8a71f720f4048
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets, cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile, dx build --platform desktop, dx build --platform android --target aarch64-linux-android
- PRs: