---
satisfies: [R1, R2, R3, R4, R5, R6, R9]
---
# fnt-30-vacation-plan-status-year-calendar-and.2 Vacation overview: status, 24-month SVG chart, expiry table, registration ticks

## Description
The Overview of the Vacation tab, following the canvas (Phone · Vacation · Overview, and the right column of Desktop · Vacation):

1. The status line.
2. The "Next 24 months" chart with its legend and warnings.
3. The table of when each pool expires and what draws from it.
4. "Not registered yet" with the Danløn (hours = days × 7,4), Calendar and Teams chips.
5. The holiday list with the Taken / Planned / Feriefridag / "Repeats · not in the calendar" chips.
6. The note about repeats.

The chart is SVG drawn in `rsx!` with no chart library. Use one linear scale for bars, line, grid and labels, and the colours from the app.css tokens (acc, client, info, bad, row, ctl) so both themes work. Give the SVG an `aria-label` summary and each bar a `<title>`.

Registration ticks go in a new vw-danlon.db table keyed by period title and first date, migrated like the others and included in backups.

**Files:** app/src/screens/vacation.rs, app/assets/app.css, src/db.rs, src/service.rs

## Acceptance
- [x] Matches the mockups on phone and desktop, light and dark
- [x] Ticks survive a restart; tested in the library on a temp db
- [x] No JavaScript (app/tests/no_javascript.rs); cargo test, clippy clean (host and the Android target)

## Done summary
- **Storage:** database version 6 adds `vacation_registered` (title, first date, Danløn/Calendar/Teams) and `danlon_balance` (task 4). They are read into `State.registered` and `State.danlon_balances`, so backups carry them. `service::set_registered` saves a tick, and `VacationView` gains `registered`, `danlon_balances` and `fully_registered()`. The CLI's status now honours the ticks. Test: `state::vacation_ticks_and_danlon_balances_survive_a_reopen`; the version-1 upgrade test covers the new tables.
- **Models:** `app/src/screens/vacation_plan.rs` holds the pure models, all unit-tested:
  - `chart` puts bars, balance line, grid, ticks, deadlines and the aria summary on one linear scale (340×196 viewBox as in the mockup). Losses (`−3,32`) and payouts (`paid 2,96`) are labelled separately at each December/April line.
  - `warnings` lists what the status does not already say.
  - `pools` builds the expiry table: lost or "0 lost", paid out, use-by date, and which periods use it with amounts. `Plan::used_of` supplies the amounts.
  - `registrations` finds calendar periods in the next 60 days, not repeats; the Danløn chip shows hours = days × 7,4.
  - `repeats_note` writes the repeats as week runs.
- **Screen:** `VacationScreen` shows the status line, then the chart panel with legend and warnings, the expiry table, "Not registered yet" with three ticks saved at once, the holiday list and the notes.
  - Holiday-list chips: Taken, Planned, Feriefridag, or "Repeats · not in the calendar".
  - A closed holiday year shows only its periods, as on its use-by date.
  - The page uses the full content width, like Month.
- **Chart:** SVG in rsx coloured by CSS classes on the app tokens. Its classes are prefixed `vc-`, because the global `.bar` rule made every rect 16px tall.
- **Checked:** the real component was rendered with dioxus-ssr (a temporary, uncommitted dev test) from a fixture config, calendar and database. It was viewed with app.css at 1220px and 390px, light and dark. Interaction (taps, ticks) was not exercised in a running app.
