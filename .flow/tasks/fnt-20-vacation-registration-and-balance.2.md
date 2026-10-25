---
satisfies: [R4]
---
# fnt-20-vacation-registration-and-balance.2 Send vacation days to Danløn as code 60; docs

## Description
Add a code 60 Afholdte feriedage line with the month's vacation days to the export and send; own waiting code-60 parts are replaced through FNT-19's plan, and foreign ones are compared and the difference shown.

**Size:** S
**Files:** `src/danlon.rs`, `src/main.rs`, `README.md`
**Touches:** [src/danlon.rs, src/main.rs, README.md]

### Approach
- `danlon::lines` (danlon.rs:29) takes the month's vacation count; config `vacation_lonart` default `60`.
- Comparison against foreign waiting code-60 units in the send plan output.
- README: vacation section and monthly routine.

### Investigation targets
**Required:**
- `src/danlon.rs:20-70, 160-210` — lines and tests

## Acceptance
- [ ] Test: month with 6 vacation days exports a `<employee>;60;6,00` line (file) and a code-60 part (API)
- [ ] Foreign waiting code-60 units differing from the calendar are shown with both counts
- [ ] README updated; `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
