---
satisfies: [R10, R11]
---
# fnt-26-home-working-month-to-date-and-mileage.1 SQLite storage with migration and calendar cache

## Description
Move trips, state (ticks, lunch, private and home-working intervals, trip overrides) and a calendar cache into one SQLite database beside the config; read per month; migrate trips.json/state.json once and keep them as backup; refresh the calendar in the background or on demand.

**Size:** M
**Files:** src/db.rs (new), src/trip_store.rs, src/state.rs, src/calendar.rs, src/service.rs, src/config.rs, app/src/** (load path), Cargo.toml
- Use rusqlite with the bundled feature (builds for Android).
- Keep the public library API so CLI output stays byte-identical (re-run the FNT-21.1 harness at scratchpad/fnt21-harness).

## Acceptance
- [ ] Migration from existing JSON is lossless (test); JSON kept as .bak
- [ ] CLI outputs identical before/after on the harness
- [ ] Month queries read only that month's rows
- [ ] Calendar loads from cache instantly; refresh updates it; failed refresh keeps cache with age
- [ ] Desktop and Android builds succeed


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
