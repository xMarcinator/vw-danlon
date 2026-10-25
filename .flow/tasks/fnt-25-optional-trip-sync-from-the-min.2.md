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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
