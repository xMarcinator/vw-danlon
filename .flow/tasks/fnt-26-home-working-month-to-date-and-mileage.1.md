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
  - in the app (Settings → Data → "Import old data"): desktop picks the old folder (e.g. the project folder with config.toml, trips.json, state.json) and copies config, trips, state and secrets-file option into the app's data folder, then deletes the old folder's trips.json and state.json once the import is verified (user: "when the data is imported delete the old file"); config.toml and secret files stay; Android imports a backup file;
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
Trips, state and a calendar cache moved into SQLite (vw-danlon.db beside the config) with a verified one-time JSON migration that deletes the JSON only after read-back, 0 km trip filtering (R15), CLI `backup export` and `--offline`, and the app's Settings > Data (import old data from the CLI folder on desktop, export/import backup). Harness: all CLI stdout/stderr identical except the "no stored trips in <path>" error now naming vw-danlon.db; final trips/state equal to the JSON baseline.

Decisions: CLI fetches the calendar link fresh by default, falls back to the kept copy with a warning, `--offline` uses the copy only; local .ics files are never cached. Importing a backup or old data replaces the app's data with no .bak copy (user asked for no backups). The calendar link is keyed in the cache by an FNV-1a hash so the link itself is not stored.

baseline: green (cargo test --workspace, cargo clippy pre-edit build; harness base0 recorded)
stage: impl-review - skipped(config: review backend not configured; conductor owns review)
Tier: not given by conductor (model: claude-opus-5-5)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: c1bbf0e977a660bf0e072d8c7f91479f1ed11139, 2f40ad07c2f79726d3a4587cf066c015f06a1ea5, 7fef84404ee3743b0726ebb0af56dbfc77969bd7, 600e5fa4624e37bcfa05d7cd74348ac1790930c2
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets, cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile, dx build --platform android --target aarch64-linux-android, dx build --platform desktop, fnt21-harness run.sh after2 vs base0
- PRs: