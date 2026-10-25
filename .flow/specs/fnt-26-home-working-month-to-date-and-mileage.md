# fnt-26-home-working-month-to-date-and-mileage Home working, month to date, and mileage under Danish rules

## Conversation Evidence

> user: "there is no way to work from home hours."
> user (selected): working from home by "Calendar and timer"
> user: "the short drives that I do are elible for drive compensation."
> user (selected): short drives "Both" (automatic rule and per-trip override)
> user: "In the month overview there the future work days are counted as well. Please mark the current day and ignore the future days in the calculation."
> user: "please don't count days that don't have work events like weekends that they need to be fixed. That's normal."
> user: "The drives between home and work are not counted as working hours but counts as milage but work to a client (a non work or home address) in the calendar it counts as work hours and counts as milage."
> user: "for the milage report please ignore or mark any that are not counted under Danish milage regulations. Perhaps those rules could be configurable."
> user: "could sqllite with pagination not be used to improve load times of the pages?"

## Goal & Context

Four gaps found using the app on real data: home working cannot be recorded; month totals include days that have not happened; days without a work event (weekends) are flagged as needing a fix; and the mileage view only knows client visits from the calendar, so short work errands are missed and the difference between "driven" and "reimbursable" is not shown. Pages are also slow to open. Mockups: Month (desktop and phone), Mileage (desktop and phone), Settings (Working from home, Mileage rules) at https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W.
<!-- Source: 55% user / 25% [paraphrase] / 20% [inferred] -->

## Architecture & Data Models

- Home working comes from two sources: a calendar work event whose title contains a home-working word (default "Hjemmearbejde", "Hjemme") or whose location matches home, and a "Working from home" start/stop timer stored in the state file like the private-time timer (FNT-23). Calendar home working counts the event's planned window as worked; the timer counts the measured time. Both minus lunch and private time; no trips are needed. [paraphrase]
- Month totals (norm, worked, difference) count days up to and including today; later days show their planned norm only. Today is marked. [paraphrase]
- Trip classification gets explicit, configurable mileage rules with Danish defaults (tax-free kørselsgodtgørelse, LL § 9 B) [paraphrase]:
  - client visit: to/from a calendar work event at an address that is neither home nor the fixed workplace → business, and its time is work hours (unchanged);
  - commute: home ↔ fixed workplace → driving, not reimbursable, not work hours (default off; configurable);
  - work errand: a trip starting at the workplace (or at a client/errand stop) during working hours, and the chain of trips after it until arriving home or at work → business, and work hours until leaving the last errand stop [inferred];
  - 60-day rule: the same client/errand address stops counting after 60 working days within 12 months [inferred];
  - everything else → private.
- A per-trip override (Business / Private) stored in the state file wins over the rules; the logbook marks overridden trips "Set by you".
- The mileage logbook lists all driving with the rule that decided each trip, the reimbursable total, and the not-reimbursable km by reason; only reimbursable trips get an amount.
- Speed: measured on the user's data, a month report takes ~710 ms, of which ~700 ms is fetching the Proton calendar over the network; reading 1042 stored trips takes a few ms. So the calendar is cached locally (ICS beside the config) and pages render from the cache at once, refreshing in the background (at most every few minutes, and on demand). A database is not needed for this data size. [inferred from measurement]

## API Contracts

- Config `[home_working]`: `keywords` (default ["Hjemmearbejde", "Hjemme"]), `at_home_address` (default true).
- Config `[mileage.rules]`: `client_visits` (true), `work_errands` (true), `sixty_day_rule` (true), `commute` (false), `min_km` (0).
- CLI: `home start|stop|list|edit|rm` (like `private`); `trip business|private|reset <start>`; `report`/`logbook` show to-date totals and rules; `logbook --all` lists all driving.
- App: "Working from home" card on Month, today marker, Reimbursable / All driving tabs on Mileage with a Business tick per trip, Settings sections "Working from home" and "Mileage rules".

## Edge Cases & Constraints

- A day with neither a work event nor home working is never "needs fixing", even when the car was driven (weekend driving is private). [paraphrase]
- Home working and a trip to work on the same day: both periods count; overlap is counted once.
- A running home-working timer past midnight or > 12 h is flagged like the private timer.
- A cached calendar older than a day is labelled with its age; with no network the cache is used and the page says so.

## Acceptance Criteria

- **R1:** A calendar work event marked as home working counts its planned window as worked (minus lunch and private time) without trips. Errors: none beyond FNT-17.
- **R2:** A home-working timer can be started, stopped, listed, edited and deleted; recorded time counts as worked. Errors: as FNT-23 R1/R3.
- **R3:** Month norm, worked and difference count only days up to today; today is marked; later days show planned norm only. Errors: none.
- **R4:** Days without a work event and without home working are never flagged as needing a fix. Errors: none.
- **R5:** Home ↔ workplace trips are driving but not reimbursable and not work hours by default; client-visit trips are work hours and reimbursable. Errors: none.
- **R6:** Work errands (trips from work during working hours through each stop until home or work) are reimbursable and their time counts as work by default. [inferred] Errors: none.
- **R7:** Mileage rules are configurable (client visits, work errands, 60-day rule, commute, minimum km), with Danish defaults. Errors: invalid values are refused naming the key.
- **R8:** Any trip can be set to Business or Private, overriding the rules, and reset. Errors: unknown trip refused naming it.
- **R9:** The logbook shows all driving with the deciding rule, reimbursable total, and not-reimbursable km by reason. Errors: none.
- **R10:** Pages open from a local calendar cache without waiting for the network; the cache refreshes in the background and on demand. Errors: a failed refresh keeps the cache and shows its age.

## Boundaries

- No database; the stored files stay JSON/TOML. [inferred from measurement]
- No befordringsfradrag calculation (only a note that commute above 24 km a day may give a tax deduction). [inferred]

## Decision Context

The user wants all driving visible but only reimbursable driving paid; Danish rules are the default and each rule can be switched. Per-trip overrides cover the cases rules cannot know.

## Quick commands

```bash
cargo test --workspace
cargo run -- report 2026-10
cargo run -- logbook 2026-10 --all
```
