# fnt-29-car-picker-word-pills-logbook-buttons.1 Car picker, word pills, logbook buttons, padding, shorter text
## Description
Implement R1–R5 of the spec in one pass.

Pointers:
- R1: app/assets/app.css (the phone status card on the Hours page; app/src/screens/month.rs builds it).
- R2: app/src/screens/mileage.rs ~810-835 (`actions` div, labels with {month_name}); CSS for a two-button row on phone.
- R3: strings across app/src/screens/*.rs, app/src/settings.rs (TRIPS_NOTE, DATA_NOTE, notes), app/src/setup.rs messages.
- R4: src/connected_cars.rs (`vehicles`, `parse_vehicles`, `pick_vehicle`; add model if the GraphQL `vehicle` exposes it, else plate only), src/service.rs (~1662-1830 access/session), src/config.rs `ConnectedCarsConfig.vehicle`, app/src/setup.rs (add a save for vehicle + registration number, following `save_workplace` at ~424), app/src/settings.rs account box (~870-930) — load in background like other `in_background` calls.
- R5: app/src/settings.rs `private_words` (~188, ~672) and `hw_keywords` (~212, ~770); a reusable `WordPills` component; pure helpers for parsing/adding words with unit tests. Fix setup save so it never writes `[calendar]` without `source`.

## Acceptance
- [ ] R1 status card padding even top/bottom on phone
- [ ] R2 buttons on one row, labels without month/year
- [ ] R3 copy shortened across the app
- [ ] R4 car list + selection saved, sync uses it, reg. number follows
- [ ] R5 pill input for both word lists, desktop and phone
- [ ] tests + clippy green, no JavaScript

## Done summary
R1–R5 in one pass. R1: the phone work card's head has 12px 14px padding (status line below it no longer doubles the gap, and keeps 12px below it on phone). R2: the Mileage logbook buttons sit on one row (two equal halves on phone, status line above them), labelled "Logbook CSV" (desktop) / "CSV" (phone) and "Print for signature". R3: Settings/setup/mileage copy shortened ("Whole words, every calendar.", "Every 15 min · unofficial · password never stored", "Events at home count too", "Links kept in {store}.", "No links or tokens.", shorter calendar Use hints, problem messages). R4: when the account is connected, Settings lists its cars (plate; the API's model is not queried, see deviations) as radio rows under "Car", loaded in the background on open and after connect, errors inline; picking one writes `connected_cars.vehicle` (plate, else id) and, with `[mileage]`, `registration_number`; sync already uses `connected_cars.vehicle`. R5: a `WordPills` component edits both word lists (× removes; Enter, comma or blur adds; duplicates any case and empty words ignored), pure helpers `add_words`/`remove_word`/`clean_words` in app/src/setup.rs. Caveat fix: `Config::parse` reads a `[calendar]` without `source` (what the app writes, its link being in the secret store) as no work calendar, so the CLI reads the app's config.

Tests: app `typed_words_are_added_once_and_removed` (add, comma, duplicate, empty, remove), `a_picked_car_is_synced_and_on_the_logbook` (round trip on a temp config, pick_vehicle picks it, logbook plate set, no-logbook case), `private_words_are_saved_where_they_apply` now also loads the saved file with `Config::load` (CLI). cargo test --workspace 162 passed; clippy (host and aarch64-linux-android mobile) clean; no_javascript passes. CSS checked in a static page with app.css at 375px and 1000px.

Deviations: no vehicle model (the GraphQL schema is not known from the repo; querying an unknown field would fail the vehicle list and sync), plate only. The sourceless-[calendar] caveat is fixed on the reading side (lib) rather than by the app not writing it: the app has no other place for the private words without the link. "Mileage · this month" does not exist in the app; untouched. Not verified on a phone or in the running app: pill Enter/comma/blur on the Android keyboard, the car list against the live account (never called).

stage: impl-review - skipped(config: REVIEW_MODE not given by conductor)
Tier: not given by conductor (model: claude-opus-5-5)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: cbad2f6d6dbd2cba8ab4ad0317b2a73f42df7e76
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets, cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile
- PRs: