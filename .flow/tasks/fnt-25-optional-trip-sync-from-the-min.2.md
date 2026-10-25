---
satisfies: [R2, R3, R5]
---
# fnt-25-optional-trip-sync-from-the-min.2 trips sync into the trip store; docs

## Description
`trips sync` merging into the FNT-24 trip store, rate-limit handling, docs.

**Size:** M
**Files:** `src/connected_cars.rs`, `src/trip_store.rs`, `src/main.rs`, `README.md`
**Touches:** [src/connected_cars.rs, src/trip_store.rs, src/main.rs, README.md]

### Approach
- Range: `--from` or the day after the newest stored trip; fetch month by month with `fromTime`/`toTime`; write each month only after it fully succeeds.
- Merge through the trip store's existing identity (start time + start odometer) so export and sync never double.
- 429: wait Retry-After once, retry; anything else stops with the status. Record last sync time in the store.
- Several vehicles and none configured: list plates/ids and stop.
- README: optional account sync, unofficial, device token only, export stays the fallback.

## Acceptance
- [ ] Test: syncing trips already imported from an export adds nothing
- [ ] Test: a failed month leaves the store unchanged for that month
- [ ] Test: a 429 then success retries once; two 429s stop with a message
- [ ] Summary prints added / already present / without position / period
- [ ] README documents connect, sync and disconnect; `cargo test` passes


## Done summary
Added `trips sync [--from]`: logs in with the stored device token, picks the vehicle, fetches trips month by month (`Session::trips_on_days`) and merges each fully fetched month into `trips.json` via `trip_store::sync` (start time + start odometer identity), then records `account.last_sync`; 429 is retried once after Retry-After through `connected_cars::retry_429` (injected call and sleep), other errors stop with the HTTP status and keep earlier months. Tests: syncing_trips_already_imported_from_an_export_adds_nothing, a_failed_month_leaves_the_store_unchanged_for_that_month, a_429_is_retried_once_after_retry_after. README documents connect (incl. token_file), sync, status/check/disconnect, unofficial status, export as fallback; the "Not yet done" item is removed.

Decisions: default start is the newest stored trip's DAY (not the day after), so trips later that day missing from a mid-day export are still fetched; dedup makes the overlap free. Empty store defaults to 1 January (year km total). Trips a month's fetch returns that start outside its days are dropped and counted in their own month (no double counting at boundaries). `without_position` now also counts trips with empty addresses. `account check` reuses the shared AccountAccess/trips_on_days. Not verified against the real service (lead runs the live sync).

baseline: green (cargo test, 59 passed)
stage: impl-review - skipped(config: REVIEW_MODE=none)
Tier: not provided (model: claude-opus-5-5)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: bff4e13432a1edb1534f6369c8d6515e2fdd0288
- Tests: cargo test (62 passed), cargo clippy --all-targets (clean)
- PRs: