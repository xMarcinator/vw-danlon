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
- [x] Tapping a day changes the status and chart without a reload; Discard restores them (the draft signal feeds `vacation::plan`; the toggle is unit-tested, the tap itself not run in an app)
- [x] Copy puts the expected text on the clipboard (desktop); on Android it is not checked here (`draft_text` tested; `platform::copy_text`)
- [x] Touch targets on phone at least 36 px tall; keyboard focus visible on desktop
- [x] No JavaScript; cargo test, clippy clean (host and the Android target)

## Done summary
- **Model:** `vacation_plan::year_calendar` builds January – December, Monday first, with ISO week numbers. Each day is plain, weekend (no `[work_week]` hours), holiday, Acies day off (Grundlovsdag, juleaften, nytårsaften from `[holidays]`), vacation, repeat, feriefridag, draft add or draft remove. Past days are dimmed; today is ringed.
- **Tapping:** future working days are buttons. `toggle` undoes a draft change, takes a planned day out, or adds a free one.
- **Draft:** it lives in a signal. With a draft, the screen runs `vacation::plan(cfg, leave, today, draft)`, so the status, chart, table and lists update at once.
- **Draft bar:** "N changes in the draft · nothing is saved", Discard, and Copy (`draft_text` through `platform::copy_text`, with the copied text shown or the error).
- **Phone:** `Bar::with_tabs` puts `‹ Overview · Calendar ›` in the dock between the year arrows, and the draft bar sits above it. Cells are 36px.
- **Desktop:** the calendar is on the left (`flex 5 1 520px`, months in an auto-fill grid) and the overview on the right; at narrow widths they stack.
- **Calendar year:** today's year while the shown holiday year holds today, else the year the holiday year starts in (`calendar_year`, tested).
- Tests: `the_year_calendar_marks_days_and_taps_into_the_draft`, `the_calendar_year_follows_today_in_the_current_holiday_year`.
