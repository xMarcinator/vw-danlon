# fnt-20-vacation-registration-and-balance Vacation from the calendar and balance

## Conversation Evidence

> user (turn 20): "also please add a vaction function to the app"
> user (turn 22, selected): "in calendar but dont count non work days"

## Goal & Context

The user registers vacation as all-day events in the work calendar, as today. The tool must recognise them as vacation, count only working days, show how many vacation days are earned, taken and planned, and report the month's vacation days to Danløn.
<!-- Source: 35% user / 25% [paraphrase] / 40% [inferred] -->

## Architecture & Data Models

- The calendar-title classifier (FNT-17) also types all-day events: vacation, feriefridag, or other day off. The day record's day-off reason becomes typed instead of free text.
- A vacation day is a working day (planned hours, not a public holiday, not a weekend) inside a vacation event; other days in the event are not counted.
- The balance follows the ferieår 1 September – 31 August under samtidighedsferie: 2,08 days earned per month from the configured employment start; feriefridage are a separate yearly count from config. [inferred]

## API Contracts

- Config: vacation title words (default `["Ferie"]`), feriefridag title words (default `["Feriefridag"]`), employment start date, feriefridage per year, days earned per month (default 2,08). [inferred]
- CLI: `vacation` prints, for the current ferieår, earned, taken, planned and each period with its working days and balance after.
- Danløn export adds code 60 Afholdte feriedage with the month's vacation days.

## Edge Cases & Constraints

- A vacation event over a weekend or a public holiday counts only the working days in it.
- An event crossing a month or ferieår splits its days by month and year.
- Without an employment start date, "earned" counts from the start of the ferieår and says so.

## Acceptance Criteria

- **R1:** An all-day calendar event whose title contains a vacation word is vacation, and only its working days count. Errors: an event with no working days counts 0 and is listed with that note.
- **R2:** Vacation days are days off with norm 0 and the reason "Ferie" (or "Feriefridag"). No error surface beyond R1. [paraphrase]
- **R3:** `vacation` shows earned, taken and planned days for the current ferieår and the balance after each period; feriefridage are counted separately. Errors: a balance below zero is flagged as forskudsferie; a missing employment start is stated. [inferred]
- **R4 (dropped 2026-10-25):** ~~Danløn export with code 60~~. The tool stays invisible to payroll and sends nothing to Danløn; vacation days simply show as "nothing to enter" in the Danløn app checklist (FNT-22).

## Early proof point

Task fnt-20-vacation-registration-and-balance.1 validates typed day-off reasons and working-day counting. If it fails, re-evaluate the classifier shape before continuing with .2.

## Boundaries

- No registering vacation in the tool itself; the calendar is the only source. [paraphrase]
- No half days, holiday pay or ferietillæg. [inferred]

## Decision Context

The user chose the calendar as the single source, so nothing can be counted twice and no local vacation store is needed.

## Parked unknowns

- The user's employment start date and feriefridage per year; the user's answer settles them.

## Quick commands

```bash
cargo test month:: vacation:: danlon::
cargo run -- -c config.example.toml vacation
```

## Requirement coverage

| Req | Description | Task(s) | Gap justification |
| --- | --- | --- | --- |
| R1 | An all-day calendar event whose title contains a vacation word is vacation, and only its working days count. Errors: an event with no working days counts 0 and is listed with that note. | fnt-20-vacation-registration-and-balance.1 | — |
| R2 | Vacation days are days off with norm 0 and the reason "Ferie" (or "Feriefridag"). No error surface beyond R1. | fnt-20-vacation-registration-and-balance.1 | — |
| R3 | `vacation` shows earned, taken and planned days for the current ferieår and the balance after each period; feriefridage are counted separately. Errors: a balance below zero is flagged as forskudsferie; a missing employment start is stated. | fnt-20-vacation-registration-and-balance.1 | — |
| R4 | (dropped 2026-10-25): ~~Danløn export with code 60~~. The tool stays invisible to payroll and sends nothing to Danløn; vacation days simply show as "nothing to enter" in the Danløn app checklist (FNT-22). | — | Uncovered |
