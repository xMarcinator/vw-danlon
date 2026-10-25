---
satisfies: [R1, R2, R3]
---
# fnt-19-reconcile-with-what-is-already-reported.2 Send flow: show waiting parts, replace own, confirm foreign; docs

## Description
Rewire `export --send` to print what is already in Danløn (marking own parts), execute the plan (delete own then create, record new receipt), and require confirmation for codes with foreign parts; `--yes` refuses those codes. `danlon info` marks own parts.

**Size:** M
**Files:** `src/main.rs`, `README.md`
**Touches:** [src/main.rs, README.md]

### Approach
- Replace the existing waiting-parts warning and global bail (main.rs:283-296).
- On delete failure stop before creating; on create failure after deletes print deleted ids and lines not sent.
- README "Sending to Danløn (API)" section.

### Investigation targets
**Required:**
- `src/main.rs:190-320` — danlon_info, send_to_danlon

## Acceptance
- [ ] Re-sending the same month replaces own lines after one confirmation and records the new ids
- [ ] A code with foreign waiting parts needs explicit confirmation; `--yes` refuses it
- [ ] Danløn unreachable → send stops with reason; file export still written
- [ ] README updated; `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
