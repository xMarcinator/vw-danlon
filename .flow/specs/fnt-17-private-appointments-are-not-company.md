# fnt-17-private-appointments-are-not-company Private appointments are not company time

## Conversation Evidence

> user (turn 14): "also a usecase that i want is to have is tracking of time spent doctors visits or similar since those are not company tmie"
> user (turn 9): "so all events in the work calendar are work related. They should have an address that can be used for the mileage section."
> user (turn 9): "the primary use case for this is the hours tracking"
> user (turn 22, selected): "Keep the norm (Recommended)"

## Goal & Context

Time spent at a doctor's visit or a similar private appointment during the working day is not company time. Today every timed calendar event is a work address and all time there counts as work. The working-time record must show the appointment and leave it out of worked hours, and trips to and from it must never count as business mileage. The day's norm stays as planned, so the appointment shows as hours to make up.
<!-- Source: 60% user / 30% [paraphrase] / 10% [inferred] -->

## Architecture & Data Models

- One calendar-title classifier decides, for every timed event, whether it is a work entry, a private appointment, or (later, FNT-18) a lunch entry. Private appointments never reach the work-address list, the planned norm or the missing-address warning.
- Per day, worked time is computed in this order: work periods from trips → minus private appointment windows → minus lunch (FNT-18) → rounding. An interval-subtraction helper does the window removal and is reused by FNT-18.
- The day record carries its appointment windows; trip classification receives them to mark trips private.

## API Contracts

- Config: a list of title words for private appointments, default `["Læge", "Tandlæge", "Fysioterapeut"]`. A word matches a whole word of the title, case-insensitively. [inferred]
- The report and the working-time record show each appointment in the day's note, e.g. `Læge 10:00–11:15 · private`.

## Edge Cases & Constraints

- An appointment outside the work periods changes nothing; overlapping appointments are deducted once.
- An all-day event keeps its current meaning (a day off), even when its title matches.
- An appointment crossing midnight is split onto each date it covers.
- An appointment needs no address and never triggers the "calendar entry without address" warning.

## Acceptance Criteria

- **R1:** A timed calendar event whose title contains a configured word as a whole word is a private appointment: it is not used as a work address and adds no planned hours. Errors: an all-day event with a matching title stays a day off; "Lægemiddel" does not match "Læge"; an empty word list disables the feature; an appointment without address gives no missing-address warning.
- **R2:** Worked time for a day excludes the overlap between private appointments and the work periods; e.g. at work 07:58–16:00 with an appointment 10:00–11:15 counts 6,78 h before lunch. Errors: no overlap → no deduction; overlapping appointments are deducted once; an appointment crossing midnight is split per date.
- **R3:** A trip that arrives at or departs from an appointment's address, or starts inside an appointment window, is classified private and never appears in the mileage logbook or the Danløn export. No error surface beyond R1. [paraphrase]
- **R4:** The report and the working-time record show each appointment with its time in the day's note. No error surface beyond R1. [paraphrase]
- **R5:** The day's norm is unchanged by an appointment, so the appointment shows as negative difference. No error surface.

## Early proof point

Task fnt-17-private-appointments-are-not-company.1 validates the core approach (one classifier plus interval subtraction inside the day computation). If it fails, re-evaluate where events are classified before continuing with .2.

## Boundaries

- No reporting of appointments to Danløn as absence or pay. [inferred]
- No norm reduction for appointments; the user chose to keep the norm.

## Decision Context

Title words on calendar events reuse the calendar the user already keeps for working hours, instead of a second input. Whole-word matching avoids a customer meeting titled e.g. "Lægemiddelfirma" silently losing paid hours. A configurable norm policy was dropped: the user chose to keep the norm, and with norms read from timed events an unplanned appointment never added norm hours anyway.

## Quick commands

```bash
cargo test month:: mileage::
cargo run -- -c config.example.toml report 2026-10 -t examples/trips.csv
```

## Requirement coverage

| Req | Description | Task(s) | Gap justification |
| --- | --- | --- | --- |
| R1 | A timed calendar event whose title contains a configured word as a whole word is a private appointment: it is not used as a work address and adds no planned hours. Errors: an all-day event with a matching title stays a day off; "Lægemiddel" does not match "Læge"; an empty word list disables the feature; an appointment without address gives no missing-address warning. | fnt-17-private-appointments-are-not-company.1 | — |
| R2 | Worked time for a day excludes the overlap between private appointments and the work periods; e.g. at work 07:58–16:00 with an appointment 10:00–11:15 counts 6,78 h before lunch. Errors: no overlap → no deduction; overlapping appointments are deducted once; an appointment crossing midnight is split per date. | fnt-17-private-appointments-are-not-company.1 | — |
| R3 | A trip that arrives at or departs from an appointment's address, or starts inside an appointment window, is classified private and never appears in the mileage logbook or the Danløn export. No error surface beyond R1. | fnt-17-private-appointments-are-not-company.2 | — |
| R4 | The report and the working-time record show each appointment with its time in the day's note. No error surface beyond R1. | fnt-17-private-appointments-are-not-company.1 | — |
| R5 | The day's norm is unchanged by an appointment, so the appointment shows as negative difference. No error surface. | fnt-17-private-appointments-are-not-company.1 | — |

