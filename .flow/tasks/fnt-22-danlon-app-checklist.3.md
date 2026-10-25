---
satisfies: [R11, R5]
---
# fnt-22-danlon-app-checklist.3 Remove the Danløn API code

## Description
Delete the Danløn API client and everything that reaches it, so the tool has no code path that talks to Danløn.

**Size:** S
**Files:** `src/danlon_api.rs` (delete), `src/main.rs`, `src/config.rs`, `config.example.toml`, `README.md`, `.gitignore`
**Touches:** [src/danlon_api.rs, src/main.rs, src/config.rs, config.example.toml, README.md, .gitignore]

### Approach
- Remove `mod danlon_api`, `export --send`/`--yes`, the `danlon` subcommand (`info`, `delete`), `company_id`, `danlon_info`, `send_to_danlon` (main.rs ~170-320).
- Remove the `[danlon.api]` config struct and the `resolve_token_file` call (config.rs ~287). FNT-18's state file resolves its path the same way: if it reuses `resolve_token_file`, move that helper into `config.rs` under a neutral name instead of deleting it.
- Drop `ureq` from Cargo.toml if nothing else uses it (calendar fetch does; check).
- Keep `danlon-token` in `.gitignore` so an old token file is never committed.
- Do not touch the user's `config.toml`; the lead removes its `[danlon.api]` section.

### Investigation targets
- `src/main.rs:160-330`, `src/config.rs:270-300`, `src/danlon_api.rs:215-235`

## Acceptance
- [ ] `src/danlon_api.rs` gone; `grep -ri "danlon_api\|refresh_token" src` is empty
- [ ] `export --send` and `danlon info|delete` no longer exist (`--help` shows neither)
- [ ] A config with `[danlon.api]` is refused naming the section
- [ ] README and config.example.toml no longer mention the API or tokens
- [ ] `cargo test` and `cargo clippy` pass


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
