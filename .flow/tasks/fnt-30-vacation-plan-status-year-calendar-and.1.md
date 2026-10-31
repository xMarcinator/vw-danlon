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
- [ ] Unit tests:
  - earning and drawing order across a year boundary
  - 31 Dec split: all paid out (more than 20 days used), all lost (fewer than 15 used), mixed
  - feriefridage used first, and their loss on 30 Apr
  - forskud over 5
  - a draft that removes a loss
  - a repeat replaced by a calendar event
  - the Christmas week when 24 Dec falls on a Monday, a Friday and a Sunday
- [ ] Existing configs without the new keys parse. The only change in behaviour is the feriefridage period (1 May – 30 Apr) and their use before vacation days.
- [ ] The `vacation` CLI prints the status line
- [ ] cargo test, clippy clean
