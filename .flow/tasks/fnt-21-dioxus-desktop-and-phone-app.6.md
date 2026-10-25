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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
