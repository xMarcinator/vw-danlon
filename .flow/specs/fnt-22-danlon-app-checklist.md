# fnt-22-danlon-app-checklist Danløn app checklist

## Conversation Evidence

> user: "Someone else runs payroll."
> user: "I want this app to be invisible to the payroll since this app is not approved I just wanted a tool to ease the procces"
> user (selected): "Danløn app"
> user: "Yes do it. But how does copying work."
> user: "In most cases im interested in the clock for coming and going. The addresses are named in the system so named them here  don't let me copy just list the name and Il match it in the app. It also plans the route using Google maps so the miles need to be shown but don't need a explicit button. Please move copying the address and mileage in a action menu"
> user: "Also these are the inputs for the time logging." (screenshots: Danløn app Komme/gå form with Day, Type, Time "08:00 to 16:00" entered on a keypad, Break "30m", Description, Total time)

## Goal & Context

The user is an employee and reports hours and driving themselves in Danløn's own app (Komme/gå and Kørsel registrations that payroll approves). The tool must stay invisible to payroll: it never talks to Danløn and hands nothing over. Instead it shows, per day, exactly what to type into the Danløn app, with copyable values and a tick-off so nothing is missed or entered twice. Mockup: https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W (boards "Danløn app checklist").
<!-- Source: 50% user / 30% [paraphrase] / 20% [inferred] -->

## Architecture & Data Models

- A pure checklist step turns a month's day records (FNT-17/18/23) and business trips into per-day registrations: Komme/gå (from, to, place) and Kørsel (km, route, purpose).
- One Komme/gå per day: first arrival to last departure, with everything not worked inside it as the break (R7).
- Ticks ("entered") are stored per date in the local state file (FNT-18). Nothing leaves the machine.

## API Contracts

- CLI: `checklist <YYYY-MM>` prints each working day with its registrations and status; `checklist mark <YYYY-MM-DD>` and `checklist unmark <YYYY-MM-DD>`.
- The app screen (FNT-21): Komme/gå shows time, break and hours large (the Danløn app takes times on a keypad, so there is no Copy for them); Kørsel shows place names and km; copying the description, addresses or km sits in a ⋯ action menu.

## Edge Cases & Constraints

- Days with nothing to enter (weekends, public holidays, vacation) are listed as such and cannot be ticked.
- A day flagged in the report (arrival without departure) shows "fix first" and cannot be ticked until the trips are fixed.
- Lunch is never a separate registration.

## Acceptance Criteria

- **R1 (superseded 2026-10-25 by R7):** ~~One Komme/gå per work period~~. The Danløn app takes one time range with a break per registration.
- **R2:** Each business trip appears as a Kørsel registration with km, route (from → to) and purpose. No error surface beyond the mileage rules.
- **R3:** Days without anything to enter are shown as such, and a flagged day shows "fix first" with the reason; neither can be ticked. Errors: `checklist mark` on such a day is refused naming the reason.
- **R4:** Ticking a day records it in the local state file and the checklist shows progress (days and trips entered); unticking removes it. Errors: a corrupt state file stops with its path, as in FNT-18.
- **R5:** The checklist makes no network call and writes nothing but the local state file. No error surface.
- **R6:** Places are shown by the names used in the Danløn app: a calendar entry's title for its address, and configurable names for home and the fixed workplace (default "Hjem" and "Kontoret"); km is shown for checking. Errors: an address with no name falls back to the address itself. [paraphrase]
- **R7:** Each working day gives one Komme/gå registration matching the Danløn app form: Time = first arrival to last departure at work, Break = all time inside that is not worked (lunch deduction, private time, other gaps), Description = the day's calendar entry or private-time labels, and the resulting hours equal the day's worked hours (e.g. 19 Nov: 07:58 to 16:00, break 1h 45m, "Læge 10:00–11:15", 6h 17m; 4 Nov: 07:41 to 15:55, break 45m, 7h 29m). Errors: no departure → "fix first" (R3). [paraphrase]

## Early proof point

Task fnt-22-danlon-app-checklist.1 validates the checklist model (periods, lunch bridging, trips). If it fails, re-evaluate how registrations are derived before continuing with .2.

## Boundaries

- No Danløn API, integration, import file or anything handed to payroll.
- No Flextid registrations for now (the workplace uses Komme/gå and Kørsel). [paraphrase]
- The app screen itself belongs to FNT-21.

## Decision Context

The user is an employee using an unapproved private tool, so its output is what they would type into the Danløn app by hand anyway. Copy helps for text and numbers; the Danløn app most likely uses time pickers, so times are shown large instead.

## Quick commands

```bash
cargo test
cargo run -- checklist 2026-11 -t examples/trips.csv
```

## Requirement coverage

| Req | Description | Task(s) | Gap justification |
| --- | --- | --- | --- |
| R1 | (superseded 2026-10-25 by R7): ~~One Komme/gå per work period~~. The Danløn app takes one time range with a break per registration. | — | Uncovered |
| R2 | Each business trip appears as a Kørsel registration with km, route (from → to) and purpose. No error surface beyond the mileage rules. | fnt-22-danlon-app-checklist.1 | — |
| R3 | Days without anything to enter are shown as such, and a flagged day shows "fix first" with the reason; neither can be ticked. Errors: `checklist mark` on such a day is refused naming the reason. | fnt-22-danlon-app-checklist.1, fnt-22-danlon-app-checklist.2 | — |
| R4 | Ticking a day records it in the local state file and the checklist shows progress (days and trips entered); unticking removes it. Errors: a corrupt state file stops with its path, as in FNT-18. | fnt-22-danlon-app-checklist.2 | — |
| R5 | The checklist makes no network call and writes nothing but the local state file. No error surface. | fnt-22-danlon-app-checklist.1, fnt-22-danlon-app-checklist.2 | — |
| R6 | Places are shown by the names used in the Danløn app: a calendar entry's title for its address, and configurable names for home and the fixed workplace (default "Hjem" and "Kontoret"); km is shown for checking. Errors: an address with no name falls back to the address itself. | fnt-22-danlon-app-checklist.1 | — |
| R7 | Each working day gives one Komme/gå registration matching the Danløn app form: Time = first arrival to last departure at work, Break = all time inside that is not worked (lunch deduction, private time, other gaps), Description = the day's calendar entry or private-time labels, and the resulting hours equal the day's worked hours (e.g. 19 Nov: 07:58 to 16:00, break 1h 45m, "Læge 10:00–11:15", 6h 17m; 4 Nov: 07:41 to 15:55, break 45m, 7h 29m). Errors: no departure → "fix first" (R3). | fnt-22-danlon-app-checklist.1 | — |

