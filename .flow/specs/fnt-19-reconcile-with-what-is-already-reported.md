# fnt-19-reconcile-with-what-is-already-reported Reconcile with what is already reported in Danløn

## Conversation Evidence

> user (turn 18): "also please take into account the things that are already reported in danløn"
> user (turn 11): "Yes, add sending the export to Danløn through its API. I'll get the refresh token from Danløn."
> user (turn 22, selected): "Replace own lines (Recommended)"

## Goal & Context

Before the tool sends pay lines it must know what is already waiting in Danløn for the employee, so nothing is sent twice. Re-sending a corrected month is the normal case: the tool replaces the lines it sent itself, and never changes lines someone else entered.
<!-- Source: 50% user / 35% [paraphrase] / 15% [inferred] -->

## Architecture & Data Models

- Source: the waiting pay parts Danløn's GraphQL API returns for the employee. They carry id, code, units, rate and amount, but no dates.
- The tool records each upload in the local state file from FNT-18: month, code, created pay part ids, time. These receipts tell "own" waiting parts from foreign ones.
- A pure planning step takes (lines to send, waiting parts, receipts) and returns per code: send, replace own (delete ids then create), or conflict with foreign parts. The CLI only renders and executes that plan.

## API Contracts

- `export --send` prints: already in Danløn (code, description, units, own or not), then the plan per code, then asks once.
- `danlon info` marks waiting parts this tool created.

## Edge Cases & Constraints

- Danløn gives no dates for pay parts, so "own" is decided by the recorded ids, never by month guessing.
- A recorded id no longer waiting (paid out or deleted in Danløn) is dropped from consideration.
- The API allows no parallel requests and 30 requests a minute; a run makes a handful.

## Acceptance Criteria

- **R1:** Before sending, the tool reads the employee's waiting pay parts and shows them next to the lines it would send, marking the ones it created. Errors: Danløn unreachable or the token refused → the send stops with that reason and the file export still works. [paraphrase]
- **R2:** When this tool's own waiting parts exist for a code it sends again, it deletes them and creates the new ones after confirmation, and records the new ids. Errors: a delete that fails stops before creating anything; a create that fails after deletes reports the deleted ids and the lines not sent.
- **R3:** When foreign waiting parts exist for a code, the tool shows them and sends that code only on explicit confirmation; with `--yes` that code is refused. No error surface beyond R1. [inferred]
- **R4:** Pay parts this tool did not create are never changed or deleted by it. No error surface. [paraphrase]

## Early proof point

Task fnt-19-reconcile-with-what-is-already-reported.1 validates the pure send plan against own and foreign waiting parts. If it fails, re-evaluate how own parts are identified before continuing with .2.

## Boundaries

- No per-day matching of Danløn absences to dates; vacation (code 60) comparison belongs to FNT-20.
- `report` stays offline and never calls Danløn. [inferred]

## Decision Context

Recording created ids is the only reliable way to recognise own parts, because the API returns no dates or authorship. Replacing own parts was the user's choice over warn-and-stop.

## Parked unknowns

- Whether Danløn's API returns pay parts already processed in an earlier payroll, or only waiting ones; a call with a real token answers it.

## Quick commands

```bash
cargo test danlon_api:: state::
cargo run -- export 2026-10 -t examples/trips.csv -c config.example.toml --send
```

## Requirement coverage

| Req | Description | Task(s) | Gap justification |
| --- | --- | --- | --- |
| R1 | Before sending, the tool reads the employee's waiting pay parts and shows them next to the lines it would send, marking the ones it created. Errors: Danløn unreachable or the token refused → the send stops with that reason and the file export still works. | fnt-19-reconcile-with-what-is-already-reported.2 | — |
| R2 | When this tool's own waiting parts exist for a code it sends again, it deletes them and creates the new ones after confirmation, and records the new ids. Errors: a delete that fails stops before creating anything; a create that fails after deletes reports the deleted ids and the lines not sent. | fnt-19-reconcile-with-what-is-already-reported.1, fnt-19-reconcile-with-what-is-already-reported.2 | — |
| R3 | When foreign waiting parts exist for a code, the tool shows them and sends that code only on explicit confirmation; with `--yes` that code is refused. No error surface beyond R1. | fnt-19-reconcile-with-what-is-already-reported.1, fnt-19-reconcile-with-what-is-already-reported.2 | — |
| R4 | Pay parts this tool did not create are never changed or deleted by it. No error surface. | fnt-19-reconcile-with-what-is-already-reported.1 | — |

