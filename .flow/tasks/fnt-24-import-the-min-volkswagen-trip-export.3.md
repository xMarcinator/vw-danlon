---
satisfies: [R5]
---
# fnt-24-import-the-min-volkswagen-trip-export.3 Use stored trips by default; docs

## Description
Use the trip store by default and document the export routine.

**Size:** S
**Files:** `src/main.rs`, `README.md`
**Touches:** [src/main.rs, README.md]

### Approach
- Make `-t/--trips` optional on report, timesheet, logbook, export and checklist; without it, load the trip store; with neither, say to run `trips import`.
- README: in Min Volkswagen, Ture → Historik → share icon gives the .xlsx; then `trips import`.

## Acceptance
- [ ] `report 2026-10` without `-t` uses imported trips
- [ ] No store and no `-t` gives a message naming `trips import`
- [ ] README documents the export and import; `cargo test` passes


## Done summary
Commands that take -t (report, timesheet, logbook, export; there is no checklist command) now fall back to the trips stored by `trips import` (TripStore::trips(tz), trip_store::stored_trips); a missing or empty store fails naming `trips import`. README documents the Min Volkswagen export (Ture → Historik → share icon), `trips import`, merging of overlapping exports, that driver name/plate are not stored, and optional [places] coordinates.

Tests: trip_store::tests::overlapping_exports_add_each_trip_once (stored trips equal a direct read), trip_store::tests::no_stored_trips_names_trips_import (R5 error). Manual check (scratch config): `report 2026-10` without -t is byte-identical to `-t export.xlsx`; empty store gives the trips import message.
baseline: green (cargo test, 44 passed); after: 45 passed; clippy: only the pre-existing month.rs `appointments` dead-code warning.

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 08463b98bb22abb1007e106705d907b9ce190c03
- Tests: cargo test, cargo clippy --all-targets
- PRs: