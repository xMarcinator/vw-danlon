---
satisfies: [R1, R2, R3, R5]
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
- [ ] Test: Læge 10:00–11:15 gives two Komme/gå (07:58–10:00, 11:15–16:00)
- [ ] Test: lunch errand 11:45–12:30 gives one Komme/gå 07:41–15:55
- [ ] Test: business trip gives Kørsel with km, route, purpose
- [ ] Test: weekend/vacation → nothing to enter; arrival without departure → fix first
- [ ] `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
