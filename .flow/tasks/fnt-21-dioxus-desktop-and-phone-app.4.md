---
satisfies: [R3, R4, R7]
---
# fnt-21-dioxus-desktop-and-phone-app.4 Vacation and Mileage screens, trips file import

## Description
Vacation (balance and periods from FNT-20) and Mileage logbook screens, plus importing the Min Volkswagen .xlsx export (or CSV/JSON) via the desktop file picker and the Android document picker, and a "Sync now" for the account sync (FNT-25) in Settings → Trips.

**Size:** M
**Files:** `app/src/screens/vacation.rs`, `app/src/screens/mileage.rs`, `app/src/import.rs`
**Touches:** [app/src/screens/**, app/src/import.rs]

### Design context
Mockup boards Vacation, Mileage, PhoneVacation, PhoneMileage.

## Acceptance
- [ ] Vacation numbers match `vacation`; mileage matches `logbook`
- [ ] Import works on desktop and Android; a malformed file names the first bad row; overlapping exports merge
- [ ] Sync now fetches new trips (FNT-25) and shows added / already present

## Done summary
Vacation and Mileage logbook screens, trips file import and the account sync in Settings → Trips source. Vacation shows the open holiday years, feriefridage, expiring days and each period with the year it draws from (service::vacation); Mileage shows the month's logbook (this month, business km toward the low-rate threshold, business trips table/phone list, not reimbursed by kind, trips to fix) and saves logbook-YYYY-MM.csv beside the config exactly as `logbook -o` writes it. Without [mileage], the Mileage screen sets it up (name, registration number, home, the example config's 2026 rates, lønart 39 when unset; validated with Config::from_text), which also unblocks Home in Settings (its note links there).

Import: library `trip_store::import_bytes` reads the Min Volkswagen .xlsx export from bytes (calamine over a Cursor; also sniffed by the zip header when the name has no extension) or a CSV/JSON trips file; the CLI `trips import` goes through it too, so it now also takes CSV/JSON. Bad rows are named (xlsx "row N", CSV "row N" with the header as row 1, JSON "trip N") and nothing is stored; overlapping files merge; CSV/JSON purpose/business are kept in trips.json (serde-default, skipped when empty, so export/sync files are unchanged). File picking: desktop uses Dioxus's file input, which opens the native dialog (rfd/xdg portal); Android uses a file input created outside the Dioxus tree so the wry webview's own chooser (Android document picker) opens, read with FileReader and handed back as base64 via document::eval - compiled and in the APK, not run on a device (none attached).

Account (Settings): connect form (email + password used once, never stored; device token into the app's secret store, account remembered in state.json), Sync now (service::sync_trips_with with the app-store token; shows "N added, M already present · period" and the without-position note), Disconnect (deletes the token via the new SecretStore::delete and forgets the account). Library split: sync_trips_with, remember_account, forget_account (CLI unchanged).

Tests: tests/vacation_and_mileage_match_cli.rs (vacation view == `vacation` output; mileage from trips imported into the store == `logbook -t examples/trips.csv` file byte for byte), trip_store byte import/CSV purpose/bad rows named (table), app tests for mileage setup, sync/connect refusals without account or token, disconnect, secret delete, period and km formatting.

Live desktop check (VW_DANLON_DIR scratch dir, fake exports only): Vacation numbers identical to `vacation` on the same data; mileage setup refused empty then created [mileage]; import of export-a.xlsx (3 added), overlapping export-b.xlsx (1 added, 2 already imported), export-bad.xlsx (row 8 named, nothing stored), trips.csv (10 added) through the real portal dialog; Mileage 196,5 km / 774,21 kr = CLI; saved logbook diff-identical to the CLI's; seeded fake token → Connected with Sync now/Disconnect, Disconnect cleared token and account; empty Connect refused locally. Sync now and Connect were NOT run live (no Connected Cars calls by instruction); covered by the library sync tests with a fake fetch. Screenshots: scratchpad/shots/t4-*.png.

Builds: dx build desktop ok; dx build --platform android --target aarch64-linux-android ok (app-debug.apk). clippy clean host and aarch64-linux-android. cargo test --workspace 87 passed.

Follow-ups: Android picker and Keystore to verify on a device; the logbook CSV lands in the app's files dir on Android (needs a share/save target); rates for years after 2026 must be added to config.toml; mockup's "Print for signature" not built.

baseline: green (cargo test --workspace, 80 passed at c08627f)
Tier: not reported (model: claude-opus-5-5)

stage: impl-review - skipped(config: REVIEW_MODE=none - no review mode given in the dispatch)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 0457de134801ffc8a5ec865c278c26132f232284
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets, cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile, dx build --platform desktop, dx build --platform android --target aarch64-linux-android
- PRs: