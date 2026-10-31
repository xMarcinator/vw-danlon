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
- [ ] Matches the mockups on phone and desktop, light and dark
- [ ] Ticks survive a restart; tested in the library on a temp db
- [ ] No JavaScript (app/tests/no_javascript.rs); cargo test, clippy clean (host and the Android target)
