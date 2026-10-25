---
satisfies: [R3, R7, R9, R11]
---
# fnt-21-dioxus-desktop-and-phone-app.6 Close the mockup gaps: lunch change, manual trip, print/share logbook, open Danløn, edit errors

## Description
Close the gaps between the built screens and the mockups (user: "Fix the gaps and build a apk for me").

**Size:** M
**Files:** `app/src/screens/*.rs`, `app/src/main.rs`, `app/assets/app.css`, `src/service.rs`, `src/trip_store.rs`, `src/state.rs`
**Touches:** [app/**, src/service.rs, src/trip_store.rs, src/state.rs]

### Approach
- **Day: change lunch** — the mockup's lunch "Change" sets the day's lunch override (start, minutes, or no lunch) through the same state as `lunch set|clear` (FNT-18); "Use default" clears it.
- **Day/Month: add missing trip** — a form (start, end, from, to, km, optional purpose) that adds a manual trip to the trip store, marked as manual so a later export/sync with the same start time does not double it; manual trips can be deleted. Fixes "arrival without departure" days.
- **Mileage: print for signature** — a print view of the month's logbook (kørselsregnskab) with a signature line; desktop uses the webview's print; on Android, share the logbook (CSV and the printable HTML) through the Android share sheet so it can be saved or printed. The existing "Logbook CSV" also offers share on Android instead of writing to the private folder only.
- **Danløn: open Danløn app** — a button that launches the Danløn app (Android intent by package name, falling back to its Play Store page; on desktop open https://danlon.dk/ in the browser). Read-only; nothing is sent.
- **Private-time edit** — when an edit is refused, keep the editor open with the typed values and the error.

## Acceptance
- [ ] Day: lunch can be changed, set to none and reset to default; Month/Day numbers update and match `report`
- [ ] A manual trip can be added and deleted; a later import/sync of the same trip does not double it
- [ ] Print view of the logbook with signature line (desktop print); Android shares CSV/printable logbook
- [ ] "Open Danløn app" launches the app on Android, opens danlon.dk on desktop
- [ ] Refused private-time edit keeps the typed values
- [ ] `cargo test --workspace`, clippy (desktop + Android) clean; desktop and Android builds succeed


## Done summary
Closed the mockup gaps. Day screen: the lunch break can be changed, set to no lunch or reset to the default (via service::set_lunch, which `lunch set|clear` now uses too), and a missing trip can be added by hand. Manual trips are stored in trips.json with `manual: true`, older files still load, a later import or sync with the same start minute is not added twice, and only manual trips can be deleted. Mileage screen: a print view of the kørselsregnskab with signature lines (Navn, Dato, Underskrift for employee and employer); the desktop prints it with the webview's print dialog, and Android shares it (and the CSV) through the share sheet as EXTRA_TEXT/EXTRA_HTML_TEXT. Danløn screen: "Open Danløn app" starts dk.danlon.app's launcher activity, falling back to market:// and then the https Play page; on the desktop it opens https://danlon.dk/. A refused private-time edit keeps the editor open with the typed values.

Tests: trip_store::tests::a_manual_trip_is_added_once_and_only_it_can_be_deleted, service::tests::lunch_changes_and_manual_trips_reach_the_month, mileage tests (printable logbook). Live desktop check passed: add trip, lunch 45/none/default, matches `report`, delete trip, refused edit, print to PDF, open danlon.dk through a stub xdg-open. Android: clippy and `dx build` succeed, but nothing was run on a device (no device or emulator available). The Danløn package name dk.danlon.app comes from a Play Store search and has not been checked on a phone.
Follow-ups: the lunch editor and trip form use <input type=time>, and WebKitGTK's typed entry is awkward.

baseline: green (cargo test --workspace)
stage: impl-review - skipped(config: REVIEW_MODE not set by conductor)
Tier: not provided (model: claude-opus-5-5)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 58daac4e73d359a67680c2c14a90fd81418b522d
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets, cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile, dx build --platform android --target aarch64-linux-android, dx build --platform desktop
- PRs: