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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
