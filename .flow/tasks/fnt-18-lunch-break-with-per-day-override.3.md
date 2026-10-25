---
satisfies: [R6]
---
# fnt-18-lunch-break-with-per-day-override.3 Norm excludes the lunch window

## Description
The planned calendar time includes lunch; subtract the day's lunch window (override, calendar or default) from the planned spans before summing the norm.

**Size:** S
**Files:** `src/month.rs`
**Touches:** [src/month.rs]

### Approach
- Resolve the lunch window before the norm in `month::build` and reuse `subtract`.

## Acceptance
- [ ] 08:00–16:00 with default lunch gives norm 7,50
- [ ] A 45-min override gives 7,25; a 0-min override keeps 8,00
- [ ] `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
