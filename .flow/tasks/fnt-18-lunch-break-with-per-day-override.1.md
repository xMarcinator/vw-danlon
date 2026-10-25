---
satisfies: [R2]
---
# fnt-18-lunch-break-with-per-day-override.1 Local state file with per-day lunch overrides and lunch CLI

## Description
Add the first persistent local state: one JSON file beside the config holding per-day lunch overrides (and later FNT-19's upload receipts). Add `lunch set|clear` subcommands.

**Size:** M
**Files:** `src/state.rs` (new), `src/main.rs`, `src/config.rs`
**Touches:** [src/state.rs, src/main.rs, src/config.rs]

### Approach
- Path resolved relative to the config file like `resolve_token_file` (danlon_api.rs:~317); default `state.json`.
- Serde struct with `#[serde(default)]` sections so later specs add fields without migrations; atomic write (temp file + rename in the same dir).
- Corrupt file: error naming the path; never rewrite it.
- New `Lunch` subcommand alongside `Command`/`DanlonAction` (main.rs:39, :92).
- Add `state.json` to `.gitignore`.

### Investigation targets
**Required:**
- `src/main.rs:25-110` — clap command layout
- `src/danlon_api.rs:215-235, 310-325` — token file path resolution
**Optional:**
- `src/config.rs:270-300` — Config::load

## Acceptance
- [ ] Test: round-trip of overrides through save/load
- [ ] Test: corrupt file → error with path, file untouched
- [ ] `lunch set 2026-11-19 12:30 30` then `lunch clear 2026-11-19` work; invalid date/time rejected naming it
- [ ] `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
