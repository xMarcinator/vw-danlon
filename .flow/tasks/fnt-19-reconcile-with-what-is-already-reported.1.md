---
satisfies: [R2, R3, R4]
---
# fnt-19-reconcile-with-what-is-already-reported.1 Upload receipts and a pure send plan for own and foreign waiting parts

## Description
Record each upload's created pay part ids in the state file (FNT-18) and add a pure function that turns lines to send, waiting parts and receipts into a per-code plan: send, replace own (delete ids, then create), or foreign conflict.

**Size:** M
**Files:** `src/danlon_api.rs`, `src/state.rs`
**Touches:** [src/danlon_api.rs, src/state.rs]

### Approach
- Receipt: month, code, ids, time; added as a defaulted section of the state struct.
- Plan function next to `to_pay_parts` (danlon_api.rs:~265), unit-tested with its `code()`/`line()` helpers.
- Receipt ids no longer waiting are ignored.

### Investigation targets
**Required:**
- `src/danlon_api.rs:160-200, 260-330` — pay_parts, create, delete, to_pay_parts and tests
- `src/main.rs:250-318` — current send_to_danlon flow

## Acceptance
- [ ] Test: own waiting code-39 part → plan replaces it
- [ ] Test: foreign waiting code-39 part → plan marks conflict, never deletes it
- [ ] Test: stale receipt id not in waiting list is ignored
- [ ] `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
