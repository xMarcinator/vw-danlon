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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
