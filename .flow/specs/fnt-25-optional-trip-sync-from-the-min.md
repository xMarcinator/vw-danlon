# fnt-25-optional-trip-sync-from-the-min Optional trip sync from the Min Volkswagen account

## Conversation Evidence

> user: "Could you not use the api for volkswagen? Advise don't implement"
> user: "Please research"
> user (after the advice to keep the export as the main route and add an optional, read-only account sync): "Write it"

## Goal & Context

FNT-24 imports the Excel export the user makes by hand in the Min Volkswagen app. This adds an optional, read-only sync that fetches the same trips from the user's own Min Volkswagen account (the Connected Cars backend), so trips arrive without a manual export, for example daily. The export stays the fallback. Mockup: Settings "Trips source" (account sync card) and Phone · Settings "Trips" at https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W.
<!-- Source: 20% user / 50% [paraphrase] / 30% [inferred] -->

## Architecture & Data Models

- Research (2026-10-25, from Connected Cars' public docs and the maintained Home Assistant integration jnxxx/homeassistant-connectedcars_io): login `POST https://auth-api.connectedcars.io/auth/login/email/password` with header `X-Organization-Namespace: semler:minvolkswagen` returns a JWT valid one hour; `POST /user/registerDevice` issues a device token (deleted after a year unused) and `POST /auth/login/deviceToken` logs in with it; data via `POST https://api.connectedcars.io/graphql` with the bearer token and the namespace header. Trips: `vehicle(id){ trips(fromTime, toTime, last, ignoreEmpty){ items{ startTime endTime startAddressString endAddressString startLatitude startLongitude endLatitude endLongitude startOdometer endOdometer mileage duration tripType note } } }`. Addresses and coordinates are null without the position permission. [paraphrase of research]
- Synced trips become the same trip records as FNT-24 and merge into the same local trip store with the same identity (start time + start odometer), so export and sync never double a trip. [inferred]
- Only the device token is kept, in the operating system's keychain; the password is used once and never stored. [paraphrase]

## API Contracts

- CLI: `account connect` asks for email and password interactively (password not echoed), registers a device and stores the device token; `account disconnect` deletes it locally; `account status` shows connected or not and the last sync.
- CLI: `trips sync [--from YYYY-MM-DD]` fetches trips since the last synced trip (or `--from`), month by month, merges them and prints added / already present / period. Without a stored token it says to run `account connect` or use `trips import`.

## Edge Cases & Constraints

- The access is unofficial (Connected Cars offers official access only to business partners), so any unexpected response is reported plainly and the export path keeps working. [paraphrase]
- Rate limits: a 429 response is retried once after its Retry-After, otherwise the sync stops and says so. At most one automatic sync a day; `trips sync` by hand is always allowed. [inferred]
- Trips with no address or coordinates (permission off) are still stored and flagged in the sync summary. [inferred]
- Several vehicles on the account: the configured licence plate or vehicle id picks one; with more than one and none configured, the sync lists them and stops. [inferred]
- Nothing is written to the account (no trip tagging). Driver name and plate are not stored.

## Acceptance Criteria

- **R1:** `account connect` logs in once with email and password, registers a device and keeps only the device token in the keychain. Errors: wrong credentials → "login refused" with the service's message, nothing stored; no keychain available → refused naming the reason and the `token_file` option, nothing written to disk. With `connected_cars.token_file` set (user: "for testing here i dont have a secret service so please allow it to use a file"), the token is kept in that file instead, readable only by the user (0600) and git-ignored; the file holds the account email so another account's token is never used.
- **R2:** `trips sync` fetches trips in a date range and merges them into the trip store; a trip already imported from an export is not added again. Errors: no token → message naming `account connect` and `trips import`; expired or deleted device token → "reconnect with account connect".
- **R3:** A 429 is retried once after Retry-After; any other failure stops the sync with the HTTP status and leaves the store unchanged for that month. No partial month is written.
- **R4:** `account disconnect` removes the token; `account status` shows connection and last sync time. No error surface beyond a missing keychain.
- **R5:** Synced trips without addresses or coordinates are counted in the summary ("12 trips without position: turn on position sharing in the app"). No error surface.

## Early proof point

Task fnt-25-optional-trip-sync-from-the-min.1 validates login and one trips query against the user's own account (run by the user). If the fields differ from the research, update the field list before .2.

## Boundaries

- Read-only; no writes to the account. [paraphrase]
- The Excel import (FNT-24) stays and is never replaced.
- No password storage, ever.
- Never part of anything handed to payroll; trips stay local.

## Decision Context

The export is official and stable but manual; the account sync is convenient but unofficial and may break. Keeping both, with one shared trip store, gets the convenience without depending on it.

## Quick commands

```bash
cargo test connected_cars:: trip_store::
cargo run -- account status
cargo run -- trips sync --from 2026-10-01
```
