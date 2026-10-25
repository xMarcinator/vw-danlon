# fnt-22-danlon-app-checklist Danløn app checklist

## Conversation Evidence

> user: "Someone else runs payroll."
> user: "I want this app to be invisible to the payroll since this app is not approved I just wanted a tool to ease the procces"
> user (selected): "Danløn app"
> user: "Yes do it. But how does copying work."
> user: "In most cases im interested in the clock for coming and going. The addresses are named in the system so named them here  don't let me copy just list the name and Il match it in the app. It also plans the route using Google maps so the miles need to be shown but don't need a explicit button. Please move copying the address and mileage in a action menu"
> user: "Also these are the inputs for the time logging." (screenshots: Danløn app Komme/gå form with Day, Type, Time "08:00 to 16:00" entered on a keypad, Break "30m", Description, Total time)
> user: "I Want private time to disrupt the work to disrupt the entry but break time is just break time."
> user: (screenshot) Danløn app Mileage form: Day, License plate (prefilled), Purpose, Description, Route Information with ordered destinations and "Add destination", map, Distance (km), and an "Add return trip" toggle.
> user: "There is a add return trip at the bottom I just didn't catch it."

## Goal & Context

The user is an employee and reports hours and driving themselves in Danløn's own app (Komme/gå and Kørsel registrations that payroll approves). The tool must stay invisible to payroll: it never talks to Danløn and hands nothing over. Instead it shows, per day, exactly what to type into the Danløn app, with copyable values and a tick-off so nothing is missed or entered twice. Mockup: https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W (boards "Danløn app checklist").
<!-- Source: 50% user / 30% [paraphrase] / 20% [inferred] -->

## Architecture & Data Models

- A pure checklist step turns a month's day records (FNT-17/18/23) and business trips into per-day registrations: Komme/gå (from, to, place) and Kørsel (km, route, purpose).
- Komme/gå entries per day: private time splits the day; the break is only lunch (R8).
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
- **R2 (superseded 2026-10-25 by R9):** ~~One Kørsel registration per business trip~~.
- **R3:** Days without anything to enter are shown as such, and a flagged day shows "fix first" with the reason; neither can be ticked. Errors: `checklist mark` on such a day is refused naming the reason.
- **R4:** Ticking a day records it in the local state file and the checklist shows progress (days and trips entered); unticking removes it. Errors: a corrupt state file stops with its path, as in FNT-18.
- **R5:** The checklist makes no network call and writes nothing but the local state file. No error surface.
- **R6:** Places are shown by the names used in the Danløn app: a calendar entry's title for its address, and configurable names for home and the fixed workplace (default "Hjem" and "Kontoret"); km is shown for checking. Errors: an address with no name falls back to the address itself. [paraphrase]
- **R7 (superseded 2026-10-25 by R8):** ~~One Komme/gå per day with private time in the break~~.
- **R8:** Private time splits the day into separate Komme/gå entries and is never registered; each entry is Time = start to end of that stretch, Break = only the lunch that falls inside it (the lunch deduction, or a lunch-window gap such as an errand), Description = the calendar entry's name. Other unworked gaps also split the day. [inferred for other gaps] The entries' hours add up to the day's worked hours (19 Nov: 07:58 to 10:00 no break 2h 02m + 11:15 to 16:00 break 30m 4h 15m; 4 Nov: one entry 07:41 to 15:55, break 45m, 7h 29m). Errors: no departure → "fix first" (R3). [paraphrase]
- **R9:** Each day with business trips gives one Kørsel registration matching the Danløn app form: Purpose = the visited calendar entries' names, Route = the ordered stops by name (consecutive duplicates removed), "Add return trip" on when the day's business driving ends where it started (then the route lists only the way out), Distance = the day's business km for checking. E.g. 18 Nov: Purpose "Kunde A", Hjem → Kunde A, Add return trip on, 148,0 km. Errors: a day whose business trips do not chain (a gap between one trip's end and the next start) lists the stops in time order and flags the gap. [paraphrase]

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
| R2 | (superseded 2026-10-25 by R9): ~~One Kørsel registration per business trip~~. | — | Uncovered |
| R3 | Days without anything to enter are shown as such, and a flagged day shows "fix first" with the reason; neither can be ticked. Errors: `checklist mark` on such a day is refused naming the reason. | fnt-22-danlon-app-checklist.1, fnt-22-danlon-app-checklist.2 | — |
| R4 | Ticking a day records it in the local state file and the checklist shows progress (days and trips entered); unticking removes it. Errors: a corrupt state file stops with its path, as in FNT-18. | fnt-22-danlon-app-checklist.2 | — |
| R5 | The checklist makes no network call and writes nothing but the local state file. No error surface. | fnt-22-danlon-app-checklist.1, fnt-22-danlon-app-checklist.2 | — |
| R6 | Places are shown by the names used in the Danløn app: a calendar entry's title for its address, and configurable names for home and the fixed workplace (default "Hjem" and "Kontoret"); km is shown for checking. Errors: an address with no name falls back to the address itself. | fnt-22-danlon-app-checklist.1 | — |
| R7 | (superseded 2026-10-25 by R8): ~~One Komme/gå per day with private time in the break~~. | — | Uncovered |
| R8 | Private time splits the day into separate Komme/gå entries and is never registered; each entry is Time = start to end of that stretch, Break = only the lunch that falls inside it (the lunch deduction, or a lunch-window gap such as an errand), Description = the calendar entry's name. Other unworked gaps also split the day. [inferred for other gaps] The entries' hours add up to the day's worked hours (19 Nov: 07:58 to 10:00 no break 2h 02m + 11:15 to 16:00 break 30m 4h 15m; 4 Nov: one entry 07:41 to 15:55, break 45m, 7h 29m). Errors: no departure → "fix first" (R3). | fnt-22-danlon-app-checklist.1 | — |
| R9 | Each day with business trips gives one Kørsel registration matching the Danløn app form: Purpose = the visited calendar entries' names, Route = the ordered stops by name (consecutive duplicates removed), "Add return trip" on when the day's business driving ends where it started (then the route lists only the way out), Distance = the day's business km for checking. E.g. 18 Nov: Purpose "Kunde A", Hjem → Kunde A, Add return trip on, 148,0 km. Errors: a day whose business trips do not chain (a gap between one trip's end and the next start) lists the stops in time order and flags the gap. | fnt-22-danlon-app-checklist.1 | — |



