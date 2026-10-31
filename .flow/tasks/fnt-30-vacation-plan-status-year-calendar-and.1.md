---
satisfies: [R1, R3, R4, R8, R10]
---
# fnt-30-vacation-plan-status-year-calendar-and.1 Projection, losses, status, repeats and draft in the library; CLI status

## Description
The pure calculation behind the new Vacation tab, in `src/vacation.rs`, with no UI.

- A 24-month projection from today. Per month it gives vacation days used, feriefridage used and the end-of-month balance. Days earned at month end go to the month's holiday year, the oldest year that may be used is drawn first, and what is left on 31 Dec of the following year is lost.
- Losses as {pool, date, amount}, for holiday years and feriefridage. The feriefridage window depends on the open question in the spec: 1 May – 30 Apr, or the holiday year.
- Forskud: the first date on which the days taken but not yet earned go over 5,00.
- The status headline and next step (spec, Architecture).
- `[vacation].repeats` in `src/config.rs`: ISO weeks, or `christmas = true`, with an empty default. A calendar event with the same title in that calendar year replaces the repeat.
- A draft overlay (dates to add, dates to remove) as input to the projection and status.
- The CLI `vacation` command (`src/main.rs` `print_vacation`) prints the status line and the losses.

The worked example in the mockup component (`Vacation.dc.html` on the canvas, its `model()` method) shows the intended numbers as on 31 Oct 2026. For example, 1,32 days of 2025/26 expire on 31 Dec 2026, and adding two December days clears that loss.

**Files:** src/vacation.rs, src/config.rs, src/service.rs, src/main.rs, config.example.toml

## Acceptance
- [ ] Unit tests: the earning and drawing order across a year boundary, the loss on 31 Dec, the feriefridage loss, forskud over 5, a draft removing the loss, a repeat replaced by a calendar event, and the Christmas week when 24 Dec falls on Mon, Fri or Sun
- [ ] Existing configs without `repeats` parse and behave as before
- [ ] `vacation` CLI prints the status line
- [ ] cargo test, clippy clean
