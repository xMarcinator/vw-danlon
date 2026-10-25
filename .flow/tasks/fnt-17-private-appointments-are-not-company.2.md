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
Trip classification now knows about private appointments. `month::timed_entries` exposes the task .1 title classifier, and `mileage::compute`/`calendar_purposes` take the classified entries: only work entries become business-visit purposes, and a trip arriving at or departing from an appointment's address (same date) or starting inside its window is `Status::Private`, so it never reaches the logbook or Danløn export. The test `mileage::tests::private_appointment_trips_are_private` covers Kontoret → Læge, Læge → Kontoret, a trip starting during the appointment, and that a located appointment gives no visit purpose; both halves of the fix were checked to fail the test when disabled. README documents private appointments ("Calendar (Proton)") and the worked-time deduction order ("How working time is matched"); `examples/arbejdstid.ics` has a Læge event on 2026-10-08.

Design note: the classified slice replaces `&[Occurrence]` in `compute` instead of adding a parameter, so the `danlon.rs` test call (outside this task's files) compiles unchanged, and appointments earlier in the year are excluded from `km_before` too. `Day.appointments` stays unread by the binary (dead-code warning from .1); follow-up: drop it or use it.

baseline: green (cargo test, 24 passed)
Tier: not provided by conductor (model: claude-opus-5-5)

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: e6fcb2aeadb040315a1e5ade0518bd6f171a7c5f
- Tests: cargo test (25 passed), cargo run -- -c config.example.toml report 2026-10 -t examples/trips.csv (2026-10-08 note shows Læge 10:00–11:15 · private; no appointment trips in mileage)
- PRs: