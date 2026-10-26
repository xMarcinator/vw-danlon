---
satisfies: [R1, R4, R5]
---
# fnt-25-optional-trip-sync-from-the-min.1 Connected Cars login, device token and trips query

## Description
Connected Cars client: login, device token in the keychain, and one trips query; `account connect|disconnect|status`.

**Size:** M
**Files:** `Cargo.toml`, `src/connected_cars.rs` (new), `src/main.rs`, `src/config.rs`
**Touches:** [Cargo.toml, src/connected_cars.rs, src/main.rs, src/config.rs]

### Approach
- HTTP with the existing `ureq` 3 client (as `calendar.rs` does); JSON via serde. Endpoints, headers and the trips field list are in the spec's Architecture section (from jnxxx/homeassistant-connectedcars_io `client.py:22-79,216-255,947-995` and connectedcars/docs `auth-api.md:236-270,458-490,1076-1135`).
- Keychain via the `keyring` crate (Linux Secret Service, macOS Keychain, Windows Credential Manager); service "vw-danlon", user = account email. No fallback file.
- Password read with `rpassword` (no echo). Never log tokens or the password.
- Optional config `[connected_cars]`: `email`, `vehicle` (plate or id), `namespace` default "semler:minvolkswagen".
- Map GraphQL trip items to `trips::Trip` (km = endOdometer − startOdometer, else mileage; positions from lat/lon).
- Unit-test the response mapping and error mapping with canned JSON; no network in tests.

## Acceptance
- [ ] Test: a canned trips response maps to trips with addresses, coordinates, odometer km
- [ ] Test: a trip with null addresses/coordinates still maps and is counted as without position
- [ ] Test: login error and expired-token responses map to the spec's messages
- [ ] `account connect` stores only the device token in the keychain; `status` and `disconnect` work
- [ ] Manual (user): `account connect` and one query against the real account confirm the field list
- [ ] `cargo test` passes


## Done summary
Added `src/connected_cars.rs` (email/password login once, registerDevice, deviceToken login, GraphQL vehicles and trips query mapped to `ExportTrip`, typed `HttpError`, keychain via `keyring`) and `account connect|disconnect|status|check` with an optional `[connected_cars]` config and an `account` record (email, last_sync) in state.json. `Session::trips` is the fetch-one-range function for .2; no sync command. Tests: trips_map_with_addresses_coordinates_and_odometer_km, trip_without_addresses_or_coordinates_still_maps, errors_map_to_the_spec_messages, vehicle_is_picked_by_plate_or_id, trips_query_quotes_times_and_refuses_odd_ids. Open: the manual check against the real account (`account connect`, `account check`) is the user's; `account check` is an extra read-only command for that proof point. User-Agent is honest (`vw-danlon/0.1.0`), unlike the HA integration's iOS UA; a query is capped at 1000 trips and refused if filled.

baseline: green (cargo test, 53 passed)
stage: impl-review - skipped(config: REVIEW_MODE=none)
Tier: not provided (model: claude-opus-5-5)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 8470d84264606f4e2616f76aaf00acc8a3e816c0
- Tests: cargo test (58 passed), cargo clippy --all-targets (clean)
- PRs: