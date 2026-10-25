---
satisfies: [R3, R4, R5]
---
# fnt-22-danlon-app-checklist.2 checklist CLI with tick-off in the state file; docs

## Description
`checklist <month>`, `checklist mark|unmark <date>` storing ticks in the state file; README section on using the Danløn app with the tool.

**Size:** M
**Files:** `src/main.rs`, `src/state.rs`, `README.md`
**Touches:** [src/main.rs, src/state.rs, README.md]

### Approach
- New defaulted `entered` section in the state struct (date set).
- Output groups days by status with progress counts.

## Acceptance
- [ ] `checklist 2026-11` prints registrations and progress
- [ ] `mark`/`unmark` round-trip; marking a fix-first or nothing-to-enter day is refused naming why
- [ ] README explains the Danløn app routine; `cargo test` passes

## Done summary
Added `checklist <YYYY-MM>` (per day: status, Komme/gå lines "time · break · total", one Kørsel line "Purpose · route · Add return trip: on/off · km", chain-gap check lines, then "Days entered: x of y · Trips entered: a of b · n to fix first") and `checklist mark|unmark <YYYY-MM-DD>` storing ticks in a defaulted `entered` date set in state.json; mark refuses nothing-to-enter and fix-first days naming the reason, and takes `-t` like report. README gained a "Danløn app checklist" section (keypad times, empty Description, Add return trip toggle). Removed the expect(dead_code) markers on `mod checklist` and `Config.checklist`.

Tests: checklist::only_days_to_enter_can_be_ticked (R3 refusal), checklist::rendered_checklist_shows_registrations_and_progress (R4 progress, R10 no Description), state::entered_days_round_trip (R4 mark/unmark, legacy state file without `entered`); corrupt state file error path reuses State::load (existing corrupt_file test). Manual check with scratch config: mark/unmark round trip, weekend/holiday/fix-first refusals, corrupt state named.

Note R5: the checklist still reads the configured calendar like `report` (a URL source is a network read); it never contacts Danløn and writes only state.json.

baseline: green (cargo test 50 passed)
stage: impl-review - skipped(config: REVIEW_MODE not set by conductor)
Tier: not provided (model: claude-opus-5-5)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: afae9b04bc98acb3f9d174f346a7d76e177ed6e4
- Tests: cargo test (53 passed), cargo clippy --all-targets (clean)
- PRs: