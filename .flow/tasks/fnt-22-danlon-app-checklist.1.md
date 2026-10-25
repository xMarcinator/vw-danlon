---
satisfies: [R2, R3, R5, R6, R8]
---
# fnt-22-danlon-app-checklist.1 Checklist model: Komme/gå and Kørsel registrations per day

## Description
Pure function from a month report (day records with periods, private time, lunch window, flags) and the month's business trips to per-day registrations and a day status (to enter, nothing to enter, fix first).

**Size:** M
**Files:** `src/checklist.rs` (new), `src/month.rs` (read-only use)
**Touches:** [src/checklist.rs, src/main.rs]

### Approach
- Komme/gå = `Day.segments` after private time, bridging gaps that overlap `Day.lunch.window`.
- Kørsel from the mileage entries (km, from → to, purpose); split entries (two rates) are one registration.
- Unit tests with the month.rs test helpers.

## Acceptance
- [ ] Test: Kørsel shows "Hjem → Kunde A" with 74,0 km (calendar title as name); an unnamed address falls back to the address
- [ ] Test: 19 Nov gives two entries: 07:58 to 10:00 (no break, 2h 02m) and 11:15 to 16:00 (break 30m, 4h 15m); Læge is not registered
- [ ] Test: 4 Nov lunch errand gives 07:41 to 15:55, break 45m, 7h 29m
- [ ] Test: for every day, the entries' spans minus their breaks add up to the day's worked hours
- [ ] Test: business trip gives Kørsel with km, route, purpose
- [ ] Test: weekend/vacation → nothing to enter; arrival without departure → fix first
- [ ] `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:



