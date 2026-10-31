# fnt-30-vacation-plan-status-year-calendar-and Vacation plan: status, year calendar and 24-month chart

## Conversation Evidence

> user: "a colleque made this prompt for you to create a html page could we dicuss on how to implement similar functionality into the app" (the prompt asks for a personal vacation plan that answers "Are any of my vacation days lost?" and "What should I do now?": a status line with one next step, a January–December calendar where tapping a weekday tries a change, a 24-month chart, expiry warnings and a table, unregistered periods in the next 60 days, repeating vacations)
> user: "Yes, draw mockups" (after the discussion: keep the work calendar as the source of truth, the draft is a what-if, the chart is hand-drawn SVG in rsx with no chart library)
> user: "i approve but please make a complete mockup using the pages that i gave you for the original one" → the Design canvas below
> user: "Do it" (write this spec)
> user: "Yes to all but for anything regarding the holidays please research it online since it rules are laws in denmark" (yes to: feriefridage 1 May – 30 Apr, feriefridage used before vacation days, entering Danløn's balance)

## Goal & Context

The Vacation tab shows balances and periods but does not say what to do. It should answer two questions at a glance: is anything lost or paid out, and what is the one next step. You should be able to try a change on a year calendar before putting it in the work calendar, and see the next 24 months as a chart.

The work calendar stays the plan. An all-day "Ferie"/"Feriefridag" event is vacation, as today. The app never writes the calendar and never talks to Danløn.

Mockups (approved): Design canvas https://claude.ai/artifact/WvBtMPpTzBtX5vXNgtGwjT, component `Vacation.dc.html` (props `size` phone|desktop, `view` overview|calendar), shown on the Phone and Desktop pages in both themes. The component's script also holds a worked example of the calculation, with data as on 31 Oct 2026. The status copy in the mockup predates the legal check: where it says days "expire", the rules below decide whether they are lost or paid out.

## Legal basis (ferieloven) and contract

Checked on 31 Oct 2026 against ferieloven, lovbekendtgørelse nr. 152 af 20.02.2024 as amended (themis.dk reproduction, to be confirmed on retsinformation.dk), and borger.dk. These are law and are not made switchable.

| Rule | Source |
|---|---|
| The holiday year (ferieår) runs 1 Sep – 31 Aug | § 4 |
| 2,08 days are earned per month of employment in the holiday year | § 5 |
| The days can be taken in the ferieafholdelsesperiode: the holiday year plus 1 Sep – 31 Dec | § 6, stk. 1 |
| Vacation cannot fall on the weekly day off or on søgnehelligdage (public holidays) | § 6, stk. 3 |
| Vacation before it is earned (forskud) needs an agreement with the employer. It is deducted from days earned later in the same holiday year. The law sets no maximum. | § 7 |
| Hovedferie: the right to 3 weeks in a row between 1 May and 30 Sep. Other days are given in blocks of at least 5 where possible. | § 8 |
| Days beyond 4 weeks can be carried into the next period by written agreement made by 31 Dec | § 21 |
| Days within 4 weeks carry over only when vacation was prevented (illness, leave) | § 22 |
| Days beyond 4 weeks are paid out by agreement after 31 Aug; after 31 Dec they are paid out automatically by 31 Mar if full-time all period, or can be ordered | §§ 23–24; borger.dk |
| Untaken days within the first 4 weeks go to Arbejdsmarkedets Feriefond on 31 Dec, so they are lost to the employee | § 34 |

Contract (Acies handbook and the colleague's information; not law, so these are config values):
- 5 feriefridage are granted 1 May and used by 30 Apr the year after. Feriefridage are not in ferieloven; their period and expiry come from the contract.
- Feriefridage are used before vacation days.
- At most 5 days in advance, and only with a form signed by management.
- Vacation in whole days. Paid days off: Grundlovsdag, juleaften, nytårsaften.

## Architecture & Data Models

- **Library (`src/vacation.rs`), pure and unit-tested:**
  - **A projection from today to 24 months ahead.** Per month: vacation days used, feriefridage used, and the balance at month end.
    - Earning: 2,08 days at each month end, to the holiday year of that month. Each year tracks what it earned.
    - Feriefridage first: a leave working day uses a feriefridag while one is left in the current 1 May – 30 Apr period; after that it draws a vacation day. An event with a feriefridag word is always a feriefridag.
    - Vacation days: drawn from the oldest holiday year that still has days and whose ferieafholdelsesperiode holds the date (as `overview` does today).
    - Days in hundredths, as now.
  - **31 Dec of the year after a holiday year:** what is left splits in two.
    - **Paid out:** the part beyond 4 weeks, min(left, max(0, earned − 20,00)). It is paid by 31 Mar unless it is taken or a transfer is agreed in writing by 31 Dec.
    - **Lost:** the rest, which goes to Arbejdsmarkedets Feriefond (§ 34).
  - **Feriefridage on 30 Apr:** what is left is lost (contract).
  - **Results:** losses as {pool, date, amount}; payouts as {pool, date, amount}.
  - **Forskud:** the first date on which more than the contract's 5 days are taken before they are earned (the sum of negative pool balances goes over 5,00).
  - **Status, first match:**
    1. A loss: "X days are lost <date>" and "Plan N more vacation days before <date>", with N = ceil(X). For feriefridage: "feriefridage".
    2. A forskud over 5: "Nothing is lost" and "Ask management for a forskud form before <date>".
    3. A payout: "Nothing is lost" and "X days beyond 4 weeks are paid out after <date>: take them or agree a transfer by <date>".
    4. A period starting in the next 60 days not yet ticked: "Register <period> (<dates>) in Danløn, Calendar and Teams".
    5. Otherwise: "Nothing to do now".
  - **Repeating vacations from config:** ISO weeks (e.g. 7; 28–30; 42) and "the week before Christmas" (the Monday before 24 Dec through 23 Dec, working days only). They count as planned vacation for future years. A vacation event in the calendar in the same calendar year with the same title replaces that year's repeat.
  - **A draft overlay:** a set of dates to add as vacation and dates to remove. The projection and status take it as input; nothing is stored.
  - **Danløn balance:** an optional balance per holiday year on a date, typed in from the Danløn app. The projection reports the difference between the app's own balance on that date and Danløn's. It does not replace the app's own count.
- **Config (`[vacation]`):**
  - `repeats = [{ title = "Sommerferie", weeks = [28, 29, 30] }, { title = "Juleferie", christmas = true }]`
  - `feriefridage_from = "05-01"`, the start of the feriefridage period (contract; default 1 May)
  - `max_forskud = 5`
  - All are optional with these defaults, so existing configs behave as before except that feriefridage now follow 1 May – 30 Apr. They are edited in config.toml only; a Settings editor needs a mockup first.
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

- **R1:** The Vacation tab opens with a status line: whether anything is lost or paid out, and one next step, computed as above. It is warn-coloured for a loss, a payout or a needed forskud form, otherwise accent-coloured.
- **R2:** A 24-month chart from the current month shows, per month, the vacation and feriefridage used as downward bars and the end-of-month balance as a line. Deadlines are red vertical lines with the amount lost; payouts are labelled separately from losses.
- **R3:** Below the chart, short warnings list every loss and payout after the first, and any forskud over the contract's limit ("needs a form signed by management").
- **R4:** A table lists each open holiday year and the feriefridage: what is lost, what is paid out (or "0 lost"), its last day of use, and which periods draw from it, with amounts.
- **R5:** "Not registered yet" lists the periods starting in the next 60 days, each with Danløn (hours), Calendar and Teams chips. Ticking a chip is saved and survives restarts. The list is hidden when it is empty.
- **R6:** The holiday list marks each period Taken, Planned, Feriefridag, or "Repeats · not in the calendar".
- **R7:** A year calendar (Jan–Dec, week numbers, the colours above, past days dimmed) lets you tap future weekdays into a draft. The status, chart, table and lists recalculate at once. Discard clears the draft. Copy gives the dates as text. Nothing is saved.
- **R8:** Repeating vacations from `[vacation].repeats` count as planned in future years, unless a calendar event with the same title in that year replaces them.
- **R9:** Phone and desktop follow the mockups in light and dark. The year arrows step whole years, as today.
- **R10:** The CLI `vacation` command prints the status line, the losses and the payouts, so the same answer is available in text.
- **R11:** Feriefridage follow 1 May – 30 Apr and are used before vacation days. Leftover feriefridage are lost on 30 Apr.
- **R12:** A balance read from Danløn can be entered per holiday year with its date. The Vacation tab shows where it differs from the app's count. This needs a mockup first (task 4).

## Boundaries

- The ferieloven rules above are law and are not made switchable. Only contract values (feriefridage period and count, forskud limit, paid days off, repeats) come from config.
- Not modelled: transfers under §§ 21–22 and payouts by agreement under § 23. The status says when they are possible, and the user does them with the employer.
- The app does not write the calendar, Danløn or Teams. Copy gives text only, and the Danløn balance is typed in by hand.
- No JavaScript and no chart library (CLAUDE.md rule; `app/tests/no_javascript.rs`).
- Never read or change the repo-root config.toml, vw-danlon.db or connected-cars-token; test on copies.

## Quick commands

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
```
