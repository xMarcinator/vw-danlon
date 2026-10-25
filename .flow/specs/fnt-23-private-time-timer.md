# fnt-23-private-time-timer Private-time timer

## Conversation Evidence

> user: "Also for doctors visits I don't use my car so could add a way for me to track that time. Like a button that starts counting when I presses and stops when I press again."
> user: "For the timer could you match it with the current or upcoming events. Like I might plan the doctor for 13 but leave before for travel and waiting time."

## Goal & Context

Private time during the working day (a doctor's visit, an errand) is often not visible in the car's trips and not always planned in the calendar. The user wants to press a button when it starts and again when it ends, and have that time handled as private, not company time. Mockup: the "Private time" card on the phone Month board.
<!-- Source: 40% user / 40% [paraphrase] / 20% [inferred] -->

## Architecture & Data Models

- A running timer (start time and label) and the recorded intervals live in the local state file (FNT-18).
- Recorded intervals are fed into the day computation exactly like calendar private appointments (FNT-17): subtracted from work periods, shown in the note, trips starting inside them are private.

## API Contracts

- CLI: `private start [label] [--event <title>|--no-event]`, `private stop`, `private list <YYYY-MM>`, `private edit <id> <HH:MM> <HH:MM>`, `private rm <id>`. The label defaults to "Privat". [inferred]
- The app (FNT-21) shows a Start/Stop button with the running time, and lets the user relabel, edit or delete a recorded interval.

## Edge Cases & Constraints

- Only one timer can run at a time.
- A timer still running at midnight, or for more than 12 hours, is flagged in the report for the user to fix. [inferred]

## Acceptance Criteria

- **R1:** `private start` records a running timer and `private stop` turns it into a recorded interval with its label. Errors: start while running → refused naming the running start; stop with nothing running → refused.
- **R2:** Recorded intervals are subtracted from worked time and shown in the day's note exactly like calendar private appointments, and they split the day's Komme/gå registrations (FNT-22). No error surface beyond FNT-17.
- **R3:** The user can list, edit and delete recorded intervals. Errors: end before start, an unknown id, or an interval over 24 hours → refused naming the problem.
- **R4:** A trip starting inside a recorded interval is private. No error surface beyond FNT-17's trip rule.
- **R5:** A timer running past midnight or longer than 12 hours is flagged in the report. No error surface. [inferred]
- **R6:** Starting the timer links it to the current or next private calendar event that day (e.g. Læge 13:00–14:00 when started at 12:30) and takes its title as the label; on stop the measured interval replaces that event's planned window, so travel and waiting time count and nothing is counted twice. The user can link another event or none. Errors: no private event later that day → a plain timer; stopping after the event's day ended still replaces only that event. [paraphrase]

## Early proof point

Task fnt-23-private-time-timer.1 validates the timer state (start/stop/edit round-trip in the state file). If it fails, re-evaluate the state layout before continuing with .2.

## Boundaries

- No background notifications or reminders. [inferred]
- No automatic detection of private time.

## Decision Context

Reusing the private-appointment path from FNT-17 keeps one rule for all private time, whether it comes from the calendar or the button.

## Quick commands

```bash
cargo test
cargo run -- private list 2026-11
```

## Requirement coverage

| Req | Description | Task(s) | Gap justification |
| --- | --- | --- | --- |
| R1 | `private start` records a running timer and `private stop` turns it into a recorded interval with its label. Errors: start while running → refused naming the running start; stop with nothing running → refused. | fnt-23-private-time-timer.1 | — |
| R2 | Recorded intervals are subtracted from worked time and shown in the day's note exactly like calendar private appointments, and they split the day's Komme/gå registrations (FNT-22). No error surface beyond FNT-17. | fnt-23-private-time-timer.2 | — |
| R3 | The user can list, edit and delete recorded intervals. Errors: end before start, an unknown id, or an interval over 24 hours → refused naming the problem. | fnt-23-private-time-timer.1 | — |
| R4 | A trip starting inside a recorded interval is private. No error surface beyond FNT-17's trip rule. | fnt-23-private-time-timer.2 | — |
| R5 | A timer running past midnight or longer than 12 hours is flagged in the report. No error surface. | fnt-23-private-time-timer.2 | — |
| R6 | Starting the timer links it to the current or next private calendar event that day (e.g. Læge 13:00–14:00 when started at 12:30) and takes its title as the label; on stop the measured interval replaces that event's planned window, so travel and waiting time count and nothing is counted twice. The user can link another event or none. Errors: no private event later that day → a plain timer; stopping after the event's day ended still replaces only that event. | fnt-23-private-time-timer.1, fnt-23-private-time-timer.2 | — |

