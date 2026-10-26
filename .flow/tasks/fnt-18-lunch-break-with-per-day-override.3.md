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
Norm = planned calendar time minus the lunch window inside it (R6). Tests updated (calendar norm 12×7,5; Frokost day 7,5) and new asserts for default, 45-min and 0-min overrides.
stage: impl-review - skipped(config: no review backend configured)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 8f1f17f121b18589c14fb74777baf55c0e14fc58
- Tests: cargo test
- PRs: