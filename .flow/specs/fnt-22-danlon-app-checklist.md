# fnt-22-danlon-app-checklist Danløn app checklist

## Conversation Evidence

> user: "Someone else runs payroll."
> user: "I want this app to be invisible to the payroll since this app is not approved I just wanted a tool to ease the procces"
> user (selected): "Danløn app"
> user: "Yes do it. But how does copying work."

## Goal & Context

The user is an employee and reports hours and driving themselves in Danløn's own app (Komme/gå and Kørsel registrations that payroll approves). The tool must stay invisible to payroll: it never talks to Danløn and hands nothing over. Instead it shows, per day, exactly what to type into the Danløn app, with copyable values and a tick-off so nothing is missed or entered twice. Mockup: https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W (boards "Danløn app checklist").
<!-- Source: 50% user / 30% [paraphrase] / 20% [inferred] -->

## Architecture & Data Models

- A pure checklist step turns a month's day records (FNT-17/18/23) and business trips into per-day registrations: Komme/gå (from, to, place) and Kørsel (km, route, purpose).
- Komme/gå registrations are the work periods after private time is removed; a gap that overlaps the day's lunch window is bridged, so a lunch errand does not split the day. [inferred]
- Ticks ("entered") are stored per date in the local state file (FNT-18). Nothing leaves the machine.

## API Contracts

- CLI: `checklist <YYYY-MM>` prints each working day with its registrations and status; `checklist mark <YYYY-MM-DD>` and `checklist unmark <YYYY-MM-DD>`.
- The app screen (FNT-21) shows the same data with one Copy button per text or number field; times are shown large for the Danløn app's time picker and have no Copy button.

## Edge Cases & Constraints

- Days with nothing to enter (weekends, public holidays, vacation) are listed as such and cannot be ticked.
- A day flagged in the report (arrival without departure) shows "fix first" and cannot be ticked until the trips are fixed.
- Lunch is never a separate registration.

## Acceptance Criteria

- **R1:** For each working day the checklist lists one Komme/gå registration per work period after private time, bridging gaps that overlap the lunch window; e.g. 19 Nov with Læge 10:00–11:15 gives 07:58–10:00 and 11:15–16:00, and 4 Nov with a lunch errand 11:45–12:30 gives one 07:41–15:55. No error surface beyond FNT-17/18.
- **R2:** Each business trip appears as a Kørsel registration with km, route (from → to) and purpose. No error surface beyond the mileage rules.
- **R3:** Days without anything to enter are shown as such, and a flagged day shows "fix first" with the reason; neither can be ticked. Errors: `checklist mark` on such a day is refused naming the reason.
- **R4:** Ticking a day records it in the local state file and the checklist shows progress (days and trips entered); unticking removes it. Errors: a corrupt state file stops with its path, as in FNT-18.
- **R5:** The checklist makes no network call and writes nothing but the local state file. No error surface.

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
| R1 | For each working day the checklist lists one Komme/gå registration per work period after private time, bridging gaps that overlap the lunch window; e.g. 19 Nov with Læge 10:00–11:15 gives 07:58–10:00 and 11:15–16:00, and 4 Nov with a lunch errand 11:45–12:30 gives one 07:41–15:55. No error surface beyond FNT-17/18. | fnt-22-danlon-app-checklist.1 | — |
| R2 | Each business trip appears as a Kørsel registration with km, route (from → to) and purpose. No error surface beyond the mileage rules. | fnt-22-danlon-app-checklist.1 | — |
| R3 | Days without anything to enter are shown as such, and a flagged day shows "fix first" with the reason; neither can be ticked. Errors: `checklist mark` on such a day is refused naming the reason. | fnt-22-danlon-app-checklist.1, fnt-22-danlon-app-checklist.2 | — |
| R4 | Ticking a day records it in the local state file and the checklist shows progress (days and trips entered); unticking removes it. Errors: a corrupt state file stops with its path, as in FNT-18. | fnt-22-danlon-app-checklist.2 | — |
| R5 | The checklist makes no network call and writes nothing but the local state file. No error surface. | fnt-22-danlon-app-checklist.1, fnt-22-danlon-app-checklist.2 | — |
