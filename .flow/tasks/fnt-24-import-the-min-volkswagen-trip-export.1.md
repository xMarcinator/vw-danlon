---
satisfies: [R1, R2]
---
# fnt-24-import-the-min-volkswagen-trip-export.1 Read the export into a merged trip store

## Description
Read the Min Volkswagen .xlsx export into trips, keep a merged local trip store, and add `trips import`.

**Size:** M
**Files:** `Cargo.toml`, `src/trips.rs`, `src/trip_store.rs` (new), `src/main.rs`
**Touches:** [Cargo.toml, src/trips.rs, src/trip_store.rs, src/main.rs]

### Approach
- Read the sheet with the `calamine` crate (xlsx); find the header row by its column names (`Starttidspunkt` … `Slut kilometerstand`) instead of a fixed row index; skip the summary lines above it.
- Timestamps "dd/mm/yyyy HH:MM:SS CEST|CET": parse the local part in the configured zone (`trips::parse_time` at trips.rs:83 is the pattern); the suffix only resolves the ambiguous hour at the October DST change.
- km = Slut − Start kilometerstand when both present, else Afstand. Keep start/end coordinates parsed from the `?q=lat,lon` links as optional fields on `Trip` (used by task .3).
- `trips::load` also accepts `.xlsx` by extension, so `-t export.xlsx` works.
- Store: JSON beside the config (resolve like the FNT-18 state file), key = start time + start odometer; never store driver name or plate.
- `trips import <file>` prints added / already imported / period.
- Test with a small fixture written by the test (not the user's export, which holds personal data).

### Investigation targets
- `src/trips.rs` — Trip, load, parse_time
- `src/state.rs` — path resolution and atomic write pattern

## Acceptance
- [ ] Test: a fixture export with summary lines, header and 3 rows gives 3 trips with times, km from the odometer, addresses and coordinates
- [ ] Test: a missing header column is refused naming it; a row ending before it starts is refused naming the row, nothing imported
- [ ] Test: importing two overlapping exports adds each trip once
- [ ] Test: a CET timestamp in the repeated hour on the DST change day resolves to the right instant
- [ ] `-t export.xlsx` works for report; `cargo test` passes


## Done summary
Min Volkswagen .xlsx exports are read into trips (`trips::read_export`, also via `-t export.xlsx`) and `trips import <file>` merges them into trips.json beside the config, keyed by start time + start odometer. Tests: trips::tests::{export_rows_become_trips, bad_exports_are_refused, zone_abbreviation_picks_the_repeated_hour}, trip_store::tests::{overlapping_exports_add_each_trip_once, a_bad_row_imports_nothing}. Manual check on the real export: 318 added, then 0 added / 318 already imported; report -t export.xlsx runs. Follow-up for .3: TripStore needs a `trips(tz)` reader; trips.json added to .gitignore.

baseline: green (cargo test, 36 passed)
stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: f89015d5802193603b7dfd17e4f85063592d1a9e
- Tests: cargo test (41 passed), cargo clippy --all-targets (only pre-existing month.rs appointments warning)
- PRs: