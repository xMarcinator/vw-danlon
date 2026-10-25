---
satisfies: []
---
# fnt-20-vacation-registration-and-balance.2 Vacation docs and config example

## Description
R4 (code 60 to Danløn) was dropped: the tool stays invisible to payroll. This task documents the vacation overview and its config keys instead.

**Size:** S
**Files:** `README.md`, `config.example.toml`
**Touches:** [README.md, config.example.toml]

## Acceptance
- [ ] README explains vacation events, working-day counting and the `vacation` command
- [ ] config.example.toml shows the vacation keys with defaults

## Done summary
README gains a "Vacation" section (all-day Ferie/Feriefridag events, working-day counting, holiday years 1 Sep-31 Aug at 2,08/month and 0,07/day, use-by 31 Dec the following year, oldest year first, forskudsferie and expiry warnings, feriefridage, sample `vacation` output, "nothing to enter" in the checklist); config.example.toml gets a commented [vacation] block with defaults, verified to parse when uncommented.

stage: impl-review - skipped(policy: risk - docs-only change)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 95aa55d9823c1f5e82e4c8d6f3b574630f837eaf
- Tests: cargo test, cargo run -- -c config.example.toml vacation
- PRs: