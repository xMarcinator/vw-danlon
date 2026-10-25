# fnt-20-vacation-registration-and-balance Vacation from the calendar and balance

## Conversation Evidence

> user (turn 20): "also please add a vaction function to the app"
> user (turn 22, selected): "in calendar but dont count non work days"
> user: "First of May. Also please look at the dansih vacation rules for more details." / "1 May 2026"
> user (feriefridage): "5 days"

## Goal & Context

The user registers vacation as all-day events in the work calendar, as today. The tool must recognise them as vacation, count only working days, and show how many vacation days are earned, taken, planned and left in each holiday year.
<!-- Source: 35% user / 25% [paraphrase] / 40% [inferred] -->

## Architecture & Data Models

- The calendar-title classifier (FNT-17) also types all-day events: vacation, feriefridag, or other day off. The day record's day-off reason becomes typed instead of free text.
- A vacation day is a working day (planned hours, not a public holiday, not a weekend) inside a vacation event; other days in the event are not counted.
- The balance follows Danish samtidighedsferie (ferieloven): days are earned per holiday year 1 September – 31 August at 2,08 days per month of employment, and a partial month earns 0,07 days per calendar day employed. Days earned in a month can be taken from the following month, and each holiday year's days must be used by 31 December of the following calendar year (the ferieafholdelsesperiode, 1 Sep – 31 Dec 16 months). [paraphrase of ferieloven]
- Taken and planned days are drawn from the oldest holiday year that still has days, so two holiday years can be open at once (Sep–Dec). Feriefridage are a separate yearly count from the contract, per holiday year. [inferred]
- The user's values: employment start 1 May 2026, 5 feriefridage. So 2025/26 earns 8,32 days (May–Aug 2026), usable until 31 Dec 2026, and 2026/27 earns from 1 Sep 2026.

## API Contracts

- Config: vacation title words (default `["Ferie"]`), feriefridag title words (default `["Feriefridag"]`), employment start date, feriefridage per year, days earned per month (default 2,08). [inferred]
- CLI: `vacation` prints, per open holiday year, earned, taken, planned, left and use-by date, then each period with its working days and which holiday year it draws from.
- Mockup: boards "Vacation" and "Phone · Vacation" at https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W.

## Edge Cases & Constraints

- A vacation event over a weekend or a public holiday counts only the working days in it.
- An event crossing a month or ferieår splits its days by month and year.
- Without an employment start date, "earned" counts from the start of the ferieår and says so.
- Days not planned by 31 December of their use-by year are flagged as expiring (they can only be transferred by agreement with the employer, which the tool does not track). [paraphrase]

## Acceptance Criteria

- **R1:** An all-day calendar event whose title contains a vacation word is vacation, and only its working days count. Errors: an event with no working days counts 0 and is listed with that note.
- **R2:** Vacation days are days off with norm 0 and the reason "Ferie" (or "Feriefridag"). No error surface beyond R1. [paraphrase]
- **R3 (superseded 2026-10-25 by R5):** ~~`vacation` shows earned, taken and planned days for the current ferieår~~.
- **R4 (dropped 2026-10-25):** ~~Danløn export with code 60~~. The tool stays invisible to payroll and sends nothing to Danløn; vacation days simply show as "nothing to enter" in the Danløn app checklist (FNT-22).

- **R5:** `vacation` shows each open holiday year (earned at 2,08 per month and 0,07 per day for a partial month from the employment start, taken, planned, left, use-by 31 December the following year) and draws periods from the oldest year first; feriefridage (5 per year by default from config) are counted separately. With start 1 May 2026 on 25 Oct 2026: 2025/26 earned 8,32, 2026/27 earned 2,08 per completed month from Sep. Errors: days drawn before they are earned are flagged as forskudsferie; a missing employment start is stated.
- **R6:** Days of a holiday year that are neither taken nor planned before its use-by date are shown as expiring with the count and date (e.g. "1,32 days from 2025/26 expire on 31 Dec 2026"). No error surface.

## Early proof point

Task fnt-20-vacation-registration-and-balance.1 validates typed day-off reasons and working-day counting. If it fails, re-evaluate the classifier shape before continuing with .2.

## Boundaries

- No registering vacation in the tool itself; the calendar is the only source. [paraphrase]
- No half days, holiday pay or ferietillæg. [inferred]

## Decision Context

The user chose the calendar as the single source, so nothing can be counted twice and no local vacation store is needed.

## Quick commands

```bash
cargo test month:: vacation:: danlon::
cargo run -- -c config.example.toml vacation
```

