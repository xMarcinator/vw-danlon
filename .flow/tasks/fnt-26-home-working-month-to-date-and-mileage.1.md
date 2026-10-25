---
satisfies: [R10, R11]
---
# fnt-26-home-working-month-to-date-and-mileage.1 SQLite storage with migration and calendar cache

## Description
Move trips, state (ticks, lunch, private and home-working intervals, trip overrides) and a calendar cache into one SQLite database beside the config; read per month; migrate trips.json/state.json once and keep them as backup; refresh the calendar in the background or on demand.

**Size:** M
**Files:** src/db.rs (new), src/trip_store.rs, src/state.rs, src/calendar.rs, src/service.rs, src/config.rs, app/src/** (load path), Cargo.toml
- Use rusqlite with the bundled feature (builds for Android).
- Migration of existing data (user: "could you include a migration function in the app so the current old data is transfered over"):
  - automatic: on first start the library imports trips.json and state.json next to the config into SQLite (lossless, JSON kept as .bak);
  - in the app (Settings → Data → "Import old data"): desktop picks the old folder (e.g. the project folder with config.toml, trips.json, state.json) and copies config, trips, state and secrets-file option into the app's data folder; Android imports a backup file;
  - `backup export <file>` (CLI and a Settings button) writes one file with config (without secrets), trips and state, so the phone can import the computer's data; the calendar link and device token are not included and must be re-entered.
- Keep the public library API so CLI output stays byte-identical (re-run the FNT-21.1 harness at scratchpad/fnt21-harness).

## Acceptance
- [ ] Migration from existing JSON is lossless (test); JSON kept as .bak
- [ ] App "Import old data" moves the CLI folder's data into the app (desktop) and a backup file (Android); backup export/import round-trip test; no secrets in the backup
- [ ] CLI outputs identical before/after on the harness
- [ ] Month queries read only that month's rows
- [ ] Calendar loads from cache instantly; refresh updates it; failed refresh keeps cache with age
- [ ] Desktop and Android builds succeed


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
