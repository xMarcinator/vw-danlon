---
satisfies: [R7, R9]
---
# fnt-30-vacation-plan-status-year-calendar-and.3 Year calendar with draft and Copy; Overview · Calendar switch; desktop layout

## Description
The year calendar, following the canvas (Phone · Vacation · Calendar, and the left column of Desktop · Vacation).

- **Layout:** January–December, Monday first, with ISO week numbers.
- **Colours:** vacation, repeat (dashed), feriefridag, public holiday, Acies day off and weekend. Past days are dimmed; today is ringed.
- **Draft:** tapping a future weekday toggles it. A free day becomes "add", a planned day becomes "remove". Weekends, holidays and past days can't be tapped.
- **Recalculation:** the draft feeds the projection from task 1, and the status, chart, table and lists update at once.
- **Draft bar:** "N changes in the draft", **Discard** and **Copy**. Copy uses `platform::copy_text` and gives "Add all-day “Vacation”: …" and "Remove: …" lines. The draft lives in a signal and is never saved.
- **Phone:** the bottom dock gets `‹ Overview · Calendar ›` between the year arrows. The arrows step whole years, as today.
- **Desktop:** the calendar is a grid of months on the left and the overview on the right; at narrow widths they stack.

**Files:** app/src/screens/vacation.rs, app/src/screens/mod.rs (PeriodPicker views), app/assets/app.css

## Acceptance
- [ ] Tapping a day changes the status and chart without a reload; Discard restores them
- [ ] Copy puts the expected text on the clipboard (desktop); on Android it is not checked here
- [ ] Touch targets on phone at least 36 px tall; keyboard focus visible on desktop
- [ ] No JavaScript; cargo test, clippy clean (host and the Android target)
