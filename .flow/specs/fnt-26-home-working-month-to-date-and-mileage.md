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
> user (selected): errands — "Don't count hours for normal trips only for drives to clients." / 60-day rule "On by default" / storage "SQLite anyway"
> user: "Always follow skats rules since they are the law."
> user: "please start the tracking automatically when the time has hit. ... add a grace period of an hour and if I in that grace period ... didn't start it please [use the calendar time] when it had ended. Also allow me to [modify] the private time [if] the fallback time was used."
> user: "Change always allow modifieng the private times."

## Goal & Context

Four gaps found using the app on real data: home working cannot be recorded; month totals include days that have not happened; days without a work event (weekends) are flagged as needing a fix; and the mileage view only knows client visits from the calendar, so short work errands are missed and the difference between "driven" and "reimbursable" is not shown. Pages are also slow to open. Mockups: Month (desktop and phone), Mileage (desktop and phone), Settings (Working from home, Mileage rules) at https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W.
<!-- Source: 55% user / 25% [paraphrase] / 20% [inferred] -->

## Architecture & Data Models

- Home working comes from two sources: a calendar work event whose title contains a home-working word (default "Hjemmearbejde", "Hjemme") or whose location matches home, and a "Working from home" start/stop timer stored in the state file like the private-time timer (FNT-23). Calendar home working counts the event's planned window as worked; the timer counts the measured time. Both minus lunch and private time; no trips are needed. [paraphrase]
- Month totals (norm, worked, difference) count days up to and including today; later days show their planned norm only. Today is marked. [paraphrase]
- Trip classification follows Skat's rules for tax-free kørselsgodtgørelse (LL § 9 B, skat.dk; researched 2026-10-25). They are the law, so they are not switchable; only rates per year and the vehicle are config:
  - client visit (calendar work event at an address that is neither home nor the fixed workplace): reimbursable, and its time is work hours. Work ↔ client always counts; home ↔ client counts for 60 working days per address within a rolling 12 months (several trips a day = one day; only trips that start or end at home count toward it; the count resets only after 60 consecutive working days at other workplaces; per employer);
  - commute (home ↔ fixed workplace): driving, never reimbursable, not work hours;
  - errands on the way home (work → shops → home) and all other private driving: not reimbursable, not work hours; after a client visit, at most the direct client → home km is reimbursable and detour km are private;
  - between workplaces during the day: reimbursable (the user can tick Business when the tool cannot see it).
- Kørselsfradrag/befordringsfradrag (LL § 9 C) is separate: (round-trip km − 24) per commuting day × rate (2026: 3,17 kr/km 25–120 km, 1,59 kr/km above; yderkommune 3,51); none on home-working, holiday or sick days; never for the same km as godtgørelse. Shown for information only (the user enters it in TastSelv, forskudsopgørelse felt 417 / årsopgørelse rubrik 51); nothing is sent to Skat — there is no citizen API, and godtgørelse already reaches Skat through Danløn → eIndkomst.
- Private time from the calendar (FNT-17) starts automatically: when a private appointment's start time has passed and nothing is recorded, it shows as running ("started automatically at HH:MM"); the user can set an earlier real start or stop it. If the user does not touch it within a grace period (default 1 hour after the planned end), the planned calendar window is used as the recorded time and marked "calendar time". Computed when the app or CLI runs; no background service. [paraphrase]
- Every private-time entry (recorded, started automatically, or calendar time) can be edited, relabelled or deleted at any time. [paraphrase]
- A per-trip override (Business / Private) stored in the state file wins over the rules; the logbook marks overridden trips "Set by you".
- The mileage logbook lists all driving with the rule that decided each trip, the reimbursable total, and the not-reimbursable km by reason; only reimbursable trips get an amount.
- Storage: trips, state (ticks, lunch, private and home-working intervals, overrides) and the calendar cache move into a local SQLite database beside the config (user: "SQLite anyway"); screens query one month at a time (pagination by month/date range). The existing JSON files are migrated once on first start and then deleted, only after the migration is verified (user: "Don't keep the backup"). The CLI and app share it.
- Speed: measured on the user's data, a month report takes ~710 ms, of which ~700 ms is fetching the Proton calendar over the network; reading 1042 stored trips takes a few ms. So the calendar is also cached (in the database) and pages render from the cache at once, refreshing in the background (at most every few minutes, and on demand). A database is not needed for this data size. [inferred from measurement]

## API Contracts

- Config `[home_working]`: `keywords` (default ["Hjemmearbejde", "Hjemme"]), `at_home_address` (default true).
- Config `[private]`: `auto_start` (true), `grace_minutes` (60). Config `[mileage]` rates per year stay as today; no rule switches.
- CLI: `home start|stop|list|edit|rm` (like `private`); `trip business|private|reset <start>`; `report`/`logbook` show to-date totals and rules; `logbook --all` lists all driving; `fradrag <year>` shows the befordringsfradrag estimate.
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
- **R5:** Home ↔ workplace trips are driving but never reimbursable and not work hours; client-visit trips are work hours and reimbursable, with the 60-day rule for home ↔ client. Errors: none.
- **R6 (changed 2026-10-25 after the Skat research):** Errands on the way home and other private driving are not reimbursable and not work hours; after a client visit only the direct client → home km is reimbursable. Errors: none.
- **R7:** The rules follow Skat (LL § 9 B) and cannot be switched off; only yearly rates are configurable. Errors: a missing rate for a year with business km is reported naming the year.
- **R8:** Any trip can be set to Business or Private, overriding the rules (e.g. between workplaces), and reset. Errors: unknown trip refused naming it.
- **R9:** The logbook shows all driving with the deciding rule, the reimbursable total, and not-reimbursable km by reason; it carries what Skat requires (name, address, purpose, date, destination with stops, km, rate, calculation, year-to-date km). Errors: none.
- **R10:** Pages open from the calendar cache without waiting for the network; the cache refreshes in the background and on demand. Errors: a failed refresh keeps the cache and shows its age.
- **R11:** Trips, state and the calendar cache are stored in SQLite and read per month; existing JSON files are migrated once, verified, then deleted (a failed migration leaves them untouched); CLI and app give the same results as before. Errors: a damaged database stops with its path, nothing overwritten.
- **R12:** A private calendar appointment whose start has passed without a recording is shown as started automatically; the user can set the real start or stop it; untouched until the grace period (default 60 min) after the planned end, the planned window is recorded and marked "calendar time". Errors: none.
- **R13:** Every private-time entry can be edited, relabelled or deleted at any time, including calendar-time and automatically started ones. Errors: as FNT-23 R3.
- **R14:** The befordringsfradrag estimate (LL § 9 C) is shown separately for information (0 kr below 24 km a day); nothing is sent to Skat. Errors: none.

- **R15:** Trips of 0 km (engine only switched on) are skipped on import, sync and migration, and counted as "skipped (0 km)" in the summary (user: "please filter out trips that are 0 km. No need to import trips where engine was just turned on."). Errors: a manual 0 km trip is refused naming the reason.

## Boundaries

- No reporting to Skat (no citizen API exists); no rule switches that break Skat's rules.

## Decision Context

The user wants all driving visible but only reimbursable driving paid, strictly by Skat's rules. Per-trip overrides cover cases the rules cannot see. SQLite was the user's choice; the measured slowness is the calendar fetch, so the calendar cache is what makes pages fast.

## Quick commands

```bash
cargo test --workspace
cargo run -- report 2026-10
cargo run -- logbook 2026-10 --all
```
