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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
