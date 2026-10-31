# fnt-30-vacation-plan-status-year-calendar-and Vacation plan: status, year calendar and 24-month chart

## Conversation Evidence

> user: "a colleque made this prompt for you to create a html page could we dicuss on how to implement similar functionality into the app" (the prompt asks for a personal vacation plan that answers "Are any of my vacation days lost?" and "What should I do now?": a status line with one next step, a January–December calendar where tapping a weekday tries a change, a 24-month chart, expiry warnings and a table, unregistered periods in the next 60 days, repeating vacations)
> user: "Yes, draw mockups" (after the discussion: keep the work calendar as the source of truth, the draft is a what-if, the chart is hand-drawn SVG in rsx with no chart library)
> user: "i approve but please make a complete mockup using the pages that i gave you for the original one" → the Design canvas below
> user: "Do it" (write this spec)

## Goal & Context

The Vacation tab shows balances and periods but does not say what to do. It should answer two questions at a glance: does anything expire, and what is the one next step. You should be able to try a change on a year calendar before putting it in the work calendar, and see the next 24 months as a chart.

The work calendar stays the plan. An all-day "Ferie"/"Feriefridag" event is vacation, as today. The app never writes the calendar and never talks to Danløn.

Mockups (approved): Design canvas https://claude.ai/artifact/WvBtMPpTzBtX5vXNgtGwjT, component `Vacation.dc.html` (props `size` phone|desktop, `view` overview|calendar), shown on the Phone and Desktop pages in both themes. The component's script also holds a worked example of the calculation, with data as on 31 Oct 2026.

## Architecture & Data Models

- **Library (`src/vacation.rs`), pure and unit-tested:**
  - **A projection from today to 24 months ahead.**
    - Per month: vacation days used, feriefridage used, and the balance at month end.
    - Earning: 2,08 days at each month end, to the holiday year of that month.
    - Drawing: each vacation working day draws from the oldest holiday year that still has days and whose use window holds the date (as `overview` does today).
    - Expiry: what a year has left on 31 Dec of the following year is lost on that date.
    - Days in hundredths, as now.
  - **Losses:** a list of {pool, date, amount}, for holiday years and feriefridage.
  - **Forskud:** the first date on which more than 5 days are taken before they are earned (the sum of negative pool balances goes over 5,00).
  - **Status:** the first loss gives "X days expire <date>" and "Plan N more vacation days before <date>", with N = ceil(X). With no loss: a forskud over 5 days gives "Ask management for a forskud form before <date>". Otherwise: "Register <period> (<dates>) in Danløn, Calendar and Teams" for the first period in the next 60 days not yet ticked. Otherwise: "Nothing to do now".
  - **Repeating vacations from config:** ISO weeks (e.g. 7; 28–30; 42) and "the week before Christmas" (the Monday before 24 Dec through 23 Dec, working days only). They count as planned vacation for future years. A vacation event in the calendar in the same calendar year with the same title replaces that year's repeat.
  - **A draft overlay:** a set of dates to add as vacation and dates to remove. The projection and status take it as input; nothing is stored.
- **Config (`[vacation]`):** `repeats = [{ title = "Sommerferie", weeks = [28, 29, 30] }, { title = "Juleferie", christmas = true }]`. It is optional, and its default is empty, so existing configs behave as before. It is edited in config.toml only; a Settings editor needs a mockup first.
- **Registration ticks:** per period (its title and first date), three flags: Danløn, Calendar, Teams. They are stored in vw-danlon.db, in a new table that is migrated like the others and included in backups. They are shown for periods starting in the next 60 days. The Danløn chip shows the hours: working days × 7,4.
- **App (`app/src/screens/vacation.rs`):**
  - **Phone:** Overview and Calendar views, switched by `‹ Overview · Calendar ›` in the bottom dock between the year arrows (the PeriodPicker pattern).
  - **Desktop:** one page with the calendar on the left and the overview on the right.
  - **Chart:** SVG drawn in `rsx!` with no library and colours from the app tokens:
    - green bars downward for vacation used, purple for feriefridage;
    - a blue line for the balance, with a dot at the last month;
    - red vertical lines at deadlines, solid on 31 Dec and dashed on 30 Apr, labelled with the amount lost;
    - grid lines every 5 days;
    - an `aria-label` summary on the SVG and a `<title>` on each bar.
  - **Calendar:**
    - January–December with ISO week numbers, Monday first.
    - Colours: vacation, repeat (dashed), feriefridag, public holiday, Acies day off (Grundlovsdag, juleaften, nytårsaften from `[holidays]`) and weekend. Past days are dimmed, today is ringed.
    - Tapping a future weekday toggles it in the draft. A planned day becomes "remove" (dashed red, struck through); a free day becomes "add" (green outline).
    - The draft bar shows "N changes in the draft", **Discard** and **Copy**. Copy puts the dates to add and remove as all-day "Vacation" events on the clipboard (`platform::copy_text`).

## Acceptance Criteria

- **R1:** The Vacation tab opens with a status line: whether anything expires, and one next step, computed as above. It is warn-coloured when days are lost or a forskud form is needed, otherwise accent-coloured.
- **R2:** A 24-month chart from the current month shows, per month, the vacation and feriefridage used as downward bars and the end-of-month balance as a line. Deadlines are red vertical lines with the amount lost.
- **R3:** Below the chart, short warnings list every loss after the first and any forskud over 5 days ("needs a form signed by management").
- **R4:** A table lists each open holiday year and the feriefridage: what is lost (or "0 lost"), its use-by date, and which periods draw from it, with amounts.
- **R5:** "Not registered yet" lists the periods starting in the next 60 days, each with Danløn (hours), Calendar and Teams chips. Ticking a chip is saved and survives restarts. The list is hidden when it is empty.
- **R6:** The holiday list marks each period Taken, Planned, Feriefridag, or "Repeats · not in the calendar".
- **R7:** A year calendar (Jan–Dec, week numbers, the colours above, past days dimmed) lets you tap future weekdays into a draft. The status, chart, table and lists recalculate at once. Discard clears the draft. Copy gives the dates as text. Nothing is saved.
- **R8:** Repeating vacations from `[vacation].repeats` count as planned in future years, unless a calendar event with the same title in that year replaces them.
- **R9:** Phone and desktop follow the mockups in light and dark. The year arrows step whole years, as today.
- **R10:** The CLI `vacation` command prints the status line and the losses, so the same answer is available in text.

## Open questions (answer before task 1 lands)

- **Feriefridage:** granted 1 May and used by 30 Apr the year after (as the colleague was told, and as the mockups show), or per holiday year (Sep–Aug, as the app counts today)? This is contractual, not Skat's rule.
- **Use order:** should feriefridage be used before vacation days automatically (the handbook says so), or stay a calendar word of their own as now?
- **Balance from Danløn:** should a balance on a date from Danløn be enterable, so the app shows where the two disagree? If yes, that needs a mockup and its own task.

## Boundaries

- Mileage and vacation rules from the law (ferieloven, LL § 9 B) are not made switchable. Only contractual values (feriefridage, repeats) come from config.
- The app does not write the calendar, Danløn or Teams. Copy gives text only.
- No JavaScript and no chart library (CLAUDE.md rule; `app/tests/no_javascript.rs`).
- Never read or change the repo-root config.toml, vw-danlon.db or connected-cars-token; test on copies.

## Quick commands

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
```
