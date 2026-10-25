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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
