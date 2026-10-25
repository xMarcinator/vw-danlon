---
satisfies: [R4]
---
# fnt-21-dioxus-desktop-and-phone-app.1 Split the logic into a library crate in a Cargo workspace

## Description
Move all modules into a library crate, keep the CLI as a thin binary, and set up a workspace for the app. Behaviour-preserving refactor.

**Size:** M
**Files:** `Cargo.toml`, `src/lib.rs` (new), `src/main.rs`, all `src/*.rs` visibility, `README.md`
**Touches:** [Cargo.toml, src/**, README.md]

### Approach
- `src/lib.rs` re-exports modules; move `calendar()`, `mileage_for`, `company_id` and the send plan/executor out of main.rs (main.rs:192, :250, :319, :341); keep printing and stdin prompts in the binary.
- Workspace members: library+CLI package and `app/` (empty placeholder until .2).

### Equivalence harness
Before the change, record `norm`, `report`, `timesheet`, `logbook`, `export` output for `examples/` with `config.example.toml` and October and November 2026; after, diff byte for byte.

### Investigation targets
**Required:**
- `src/main.rs` — all of it
- `Cargo.toml`

## Acceptance
- [ ] Recorded CLI outputs identical before and after
- [ ] `cargo test --workspace` passes
- [ ] README commands still work

## Done summary
Split vw-danlon into a Cargo workspace: the root package is now the `vw_danlon` library (all modules public, plus a new `service` module with the data-producing steps that lived in main.rs) and the `vw-danlon` CLI binary, which keeps clap, prompts and printing; `app/` is a minimal `vw-danlon-app` placeholder for task .2. The calendar's missing-name warning now comes back as a value (`service::Calendar::warning`) and the CLI prints it unchanged.

Equivalence harness: 35 CLI runs (norm, report with and without -t, timesheet, logbook, export, checklist incl. mark/unmark, private list/edit/rm, lunch set, vacation, account status, trips import, error cases) for 2026-10 and 2026-11 against a scratch copy of config.example.toml with examples/ data and a seeded state.json; stdout, stderr, exit codes, written files and final state.json identical before and after (only the -o directory name in "Wrote" lines differed by construction).

baseline: green (cargo test, 69 passed at 218bb91)
Follow-up: `account check`/`trips sync` paths were moved but not exercised (network); covered by compile + unchanged logic.

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 9efe25f4ddfaacbb9397d848b1787bf38c328f40
- Tests: cargo test --workspace (69 passed), cargo clippy --workspace --all-targets (0 warnings), equivalence harness: 35 CLI runs byte-identical before/after
- PRs: