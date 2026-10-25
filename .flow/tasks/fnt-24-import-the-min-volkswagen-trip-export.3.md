---
satisfies: [R5]
---
# fnt-24-import-the-min-volkswagen-trip-export.3 Use stored trips by default; docs

## Description
Use the trip store by default and document the export routine.

**Size:** S
**Files:** `src/main.rs`, `README.md`
**Touches:** [src/main.rs, README.md]

### Approach
- Make `-t/--trips` optional on report, timesheet, logbook, export and checklist; without it, load the trip store; with neither, say to run `trips import`.
- README: in Min Volkswagen, Ture → Historik → share icon gives the .xlsx; then `trips import`.

## Acceptance
- [ ] `report 2026-10` without `-t` uses imported trips
- [ ] No store and no `-t` gives a message naming `trips import`
- [ ] README documents the export and import; `cargo test` passes


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
