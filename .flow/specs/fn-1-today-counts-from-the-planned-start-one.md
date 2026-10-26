# fn-1-today-counts-from-the-planned-start-one Today counts from the planned start, one work card, workplace fallback

## Conversation Evidence

> user: "my drive to between home and work still count as reimbursable drives even though they are to short. Please describe the rules you implemented."
> user: "could you make the difference check count like so it starts counting if im not at work on time in both the header and the day view."
> user: "dont mark it as a warning if there is no drive home on the same day since i have not had a full work day yet."
> user: "the top menu with private time and working from home could they not be merged into one. So the button per default is start work since if i haven't started working it dont make sense to use a unscehduled private time. But when the working is active, including if i have arrived at work, make the button a split button where i can start a private time from. also then show a small submenu under the card for the first upcoming planned private time and with a similar collapsable list."
> user: "The mockups look right. Spec it, build it, including the workplace fallback, and make a new APK."

## Goal & Context

Using the app on the phone showed three problems: home ↔ work drives were paid there (the phone config had no fixed workplace, so every calendar event at the workplace counted as a client visit), today's difference only counted at the end of the day, and today was flagged for a missing drive home before the day was over. The two cards on Month (private time, working from home) are merged into one work card. Mockups: Phone · Month (late, Start work), Phone · Month · working (split button and list open), Phone · Day · today, desktop Month header, at https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W.
<!-- Source: 55% user / 30% [paraphrase] / 15% [inferred] -->

## Architecture & Data Models

- Today's norm is "expected so far": the planned windows of today up to now, minus the part of the lunch window that has passed (and minus private time as today). Worked so far is measured up to now (an at-work period still open runs to now). Difference today = worked so far − expected so far, used in the month totals ("to date") and in the Day view; after the planned end it equals the whole day. [paraphrase]
- Today is never flagged for an arrival without a departure, or for no driving, while the day is not over (before midnight). [paraphrase]
- Workplace fallback: when `workplace` is not set, the fixed workplace is the location of the calendar work event seen on the most working days in the last 60 days (at least 3), matched with address rules; it is used for commute and client-visit rules and shown as "Detected from your calendar" in Settings and on Mileage, with a prompt to save it. [inferred]
- Work state for today, from the library: not working (before or after the planned window, with lateness when past the planned start), working at a work address (from trips), working from home (home timer), private time running (manual or started automatically). [paraphrase]

## API Contracts

- `service::work_today(cfg, state, calendar, trips, now)` → state, since, worked so far, expected so far, next private appointment, actions available.
- App Month: one work card — "Start work" (starts the home-working timer) when not working; while working a split button "Private time ▾" (menu: start private time now, private time started earlier…, stop work — no appointments); while private time runs "Stop private"; under it "Next private …" with the collapsible private list.
- CLI `report` today row: "so far" values; no flag today.

## Acceptance Criteria

- **R1:** Today's expected time counts from the planned start up to now (minus lunch passed); the difference counts lateness in the month totals and the Day view. Errors: none.
- **R2:** Today is not flagged for a missing departure or no driving before the day is over. Errors: none.
- **R3:** With no `workplace` set, the workplace is detected from the calendar and used, so home ↔ that workplace is commute (never reimbursed); Settings and Mileage say it was detected and offer to save it. Errors: no clear candidate → Mileage warns that no fixed workplace is set.
- **R4:** Month has one work card: Start work when not working; split button Private time ▾ while working (by trip or home timer) — its menu holds no appointments, only "Start private time now", "Private time started earlier…" and "Stop work"; Stop private while private time runs; the Next private row has its own Start button for that appointment (user: "the split button should not include the apointments. The first entry there should get a start button.") and the chevron opens the collapsible list, which shows only today's private entries (planned, running, recorded) (user: "the planned events list should only show the current day ones"). Errors: as FNT-23/FNT-26.
- **R5:** Day view for today shows worked so far, expected so far, difference and lateness, and the "no drive home yet is fine" note. Errors: none.
- **R6:** A new signed APK (0.3.0) installs over 0.2.0. Errors: none.

## Boundaries

- Mileage rules stay Skat's (FNT-26); no minimum-distance rule (Skat has none).

## Quick commands

```bash
cargo test --workspace
cargo run -- report 2026-10
scripts/build-apk.sh
```
