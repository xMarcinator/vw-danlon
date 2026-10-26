---
satisfies: [R3]
---
# fnt-17-private-appointments-are-not-company.2 Mark trips to and from private appointments as private; docs

## Description
Trip classification learns about appointments: a trip arriving at or departing from an appointment's address, or starting inside an appointment window, is private. Update README and the example calendar.

**Size:** M
**Files:** `src/mileage.rs`, `src/main.rs`, `README.md`, `examples/arbejdstid.ics`
**Touches:** [src/mileage.rs, src/main.rs, README.md, examples/**]

### Approach
- `mileage::calendar_purposes` (mileage.rs:70) must skip appointment events (today a located appointment would become a business visit).
- Pass appointment windows/addresses into `compute` (mileage.rs:166) and return `Status::Private` in `classify` (mileage.rs:38) before the purpose check; update callers in main.rs (logbook, `mileage_for`).
- Reuse the classifier from task .1.
- README: "How working time is matched" and "Calendar (Proton)" sections; add a Læge event to the example .ics.

### Investigation targets
**Required:**
- `src/mileage.rs:28-200` — classify, calendar_purposes, compute
- `src/main.rs:130-145, 335-350` — callers
**Optional:**
- `src/mileage.rs:290-360` — test helpers `cfg`, `trip`, `visit`

## Acceptance
- [ ] Test: Kontoret → Læge and Læge → Kontoret trips are Private and absent from the logbook
- [ ] Test: a located appointment never becomes a business visit purpose
- [ ] README documents appointments and the deduction order
- [ ] `cargo test` passes

## Done summary
Trips arriving at or leaving a private appointment's address on its date, or starting during an appointment, now classify as Private and never reach the mileage logbook or Danløn export; `calendar_purposes` skips appointments, so a located Læge event is never a business-visit purpose. Appointments come from the calendar via `mileage::appointments`, which reuses `month::classify` with `calendar.private_keywords`; `compute` takes the keywords, and main.rs passes them through a `private_keywords(cfg)` helper. README ("Calendar (Proton)", "How working time is matched") documents appointments and the deduction order; examples/arbejdstid.ics gains a Læge event on 2026-10-08.

Tests: `trips_to_and_from_private_appointments_are_private` (Kontor → Læge and Læge → Kontor with a purpose are Private and absent from the written logbook; a trip starting inside an address-less appointment window is Private; the same address on another day stays business) and `located_appointment_is_never_a_visit_purpose`.
Checked by hand: `logbook 2026-10` with Kontor ↔ Søndergade 3 trips (purpose Møde) on 2026-10-08 lists only the two Service Kunde A trips; `report 2026-10` shows "Læge 10:00–11:15 · private" on 2026-10-08.

Outside Touches, needed by the change: src/month.rs (`classify` and `EventKind` made pub(crate) so they can be reused) and src/danlon.rs (a test caller of `compute` gets the new argument).
Follow-up: mileage reads appointments from the calendar, not from `Day.appointments`, because it needs addresses and the year's earlier months. So the dead_code warning on `Day.appointments` stays; it is only read in tests.
Follow-up: the spec's Quick command `cargo test month:: mileage::` is rejected by cargo (one filter only), so I ran each filter on its own.
baseline: green via handoff (99e4db8, cargo test 25 passed)
Tier: (not provided in dispatch) (model: claude-opus-5-5)

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 3fba4f88f737641dc6b5cbbb24ec657e46064eac
- Tests: cargo test (27 passed), cargo test mileage:: (5 passed), cargo test month:: (9 passed)
- PRs: