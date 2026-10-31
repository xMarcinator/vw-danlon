---
satisfies: [R1, R3, R4, R8, R10, R11]
---
# fnt-30-vacation-plan-status-year-calendar-and.1 Projection, losses, status, repeats and draft in the library; CLI status

## Description
The pure calculation behind the new Vacation tab, in `src/vacation.rs`, with no UI. The rules are in the spec's "Legal basis" table. Follow its § references and keep them in the doc comments.

- **24-month projection from today.** Per month: vacation days used, feriefridage used and the end-of-month balance. Days earned at month end go to the month's holiday year, and each year tracks what it earned.
- **Feriefridage first.** 5 from 1 May, used by 30 Apr (`feriefridage_from`, contract). A leave working day uses a feriefridag while one is left, then a vacation day from the oldest year that may be used.
- **31 Dec of the year after.** The leftover beyond 4 weeks, min(left, max(0, earned − 20)), is paid out (§§ 23–24); the rest is lost to Arbejdsmarkedets Feriefond (§ 34). Leftover feriefridage are lost on 30 Apr.
- **Forskud.** The first date on which the days taken but not yet earned go over `max_forskud` (5, contract).
- **Status headline and next step**, in the order the spec gives.
- **`[vacation]` config:** `repeats` (ISO weeks, or `christmas = true`), `feriefridage_from` and `max_forskud`, all optional. A calendar event with the same title in that calendar year replaces the repeat.
- **Draft overlay** (dates to add, dates to remove) as input to the projection and status.
- **CLI:** the `vacation` command (`src/main.rs` `print_vacation`) prints the status line, the losses and the payouts.

The worked example in the mockup component (`Vacation.dc.html` on the canvas, `model()`) shows the intended mechanics as on 31 Oct 2026; its loss rule predates the legal check. With the rule here, the 1,32 days left of 2025/26 (25 earned) are paid out, not lost.

**Files:** src/vacation.rs, src/config.rs, src/service.rs, src/main.rs, config.example.toml

## Acceptance
- [x] Unit tests:
  - earning and drawing order across a year boundary
  - 31 Dec split: all paid out (20 days or more used), all lost (the year earned 20 days or less), mixed (fewer than 20 used of more than 20 earned). Corrected from "all lost (fewer than 15 used)": the part beyond 4 weeks is paid out however few days were used.
  - feriefridage used first, and their loss on 30 Apr
  - forskud over 5
  - a draft that removes a loss
  - a repeat replaced by a calendar event
  - the Christmas week when 24 Dec falls on a Monday, a Friday and a Sunday
- [x] Existing configs without the new keys parse. The only change in behaviour is the feriefridage period (1 May – 30 Apr) and their use before vacation days.
- [x] The `vacation` CLI prints the status line
- [x] cargo test, clippy clean

## Done summary
`src/vacation.rs`:
- **One engine.** `overview` now runs on `run`, which also logs where each working day came from (`Spent`: a holiday year, a feriefridage period, or before the employment start). A working day uses a feriefridag while its period (`FridagYear`, from `feriefridage_from`) has one left; an event with a feriefridag word always does. Days before the oldest open year's start count only against feriefridage.
- **`project`** gives 24 `MonthPoint`s (vacation and feriefridage used, balance of the usable years at month end, feriefridage left), the 31 Dec `losses` and `payouts` (paid = min(left, max(0, earned − 20,00)), § 23–24 / § 34), leftover feriefridage lost on the period's last day, and the first future date `forskud` goes over `max_forskud`.
- **`plan(cfg, calendar, today, draft)`** adds `repeat_days` (ISO weeks Mon–Sun, or the Monday before 24 Dec – 23 Dec; replaced for a calendar year by a vacation event whose title contains the repeat's) and applies the `Draft` (`with_draft`: removed dates dropped, added dates as "Vacation").
- **`status`**: loss → forskud → payout → a period starting in the next 60 days not in `registered` → "Nothing to do now"; `render_plan` prints it with the losses, payouts and forskud.

Config: `[vacation] repeats`, `feriefridage_from = "05-01"`, `max_forskud = 5`, all optional and checked in `validate`. Service: `VacationView` gains `leave` (the calendar's leave days through the end of the projection) and `plan`; the calendar is loaded from the start of the feriefridage period holding the oldest open year to the later of its use-by date and the projection's end. CLI: `vacation` prints the status block before the overview.

Tests: `projection_draws_across_the_year_boundary_month_by_month`, `what_is_left_on_31_december_is_paid_out_beyond_4_weeks_and_lost_within`, `feriefridage_are_used_first_and_lost_on_30_april`, `forskud_over_the_limit_needs_a_form`, `a_draft_can_remove_a_loss`, `a_repeat_is_replaced_by_a_calendar_event_in_that_year`, `the_christmas_repeat_runs_from_the_monday_before_24_december`, `status_takes_the_first_match_in_order`, config `vacation_plan_keys_default_parse_and_are_checked`. The older drawing tests use a rule without feriefridage, since vacation days now use feriefridage first. `tests/vacation_and_mileage_match_cli.rs` expects the status block.

Deviations: the CLI passes no registration ticks (the table comes with task 2), so it lists every period starting in the next 60 days. Repeat and draft days count as working days by `[work_week]` and `[holidays]`; calendar days still by the day builder. The app's Vacation screen is unchanged apart from the new `Rules` fields; its feriefridage card now shows the 1 May – 30 Apr period.

