# fnt-28-several-calendars-and-private-words-in Several calendars and private words in Settings

## Conversation Evidence

> user: "Please do that and support multiple calendars since the current work calendar is meant for the ideal working hours. I might use other calendars. So it would be if it could read multiple or just talk to proton and list the calendars"
> user (on "do that"): the offer to add a "Private time: event titles containing" field to Settings, next to the working-from-home one.
> user: "Please continue" (after the Calendars mockups, artifact version 146)

## Goal & Context

Private appointments are found only by title words in the one work calendar, and the words can only be changed in config.toml. The work calendar holds the ideal working hours; private appointments may live in other calendars. Proton has no public API to list calendars, so each extra calendar is added by its own "Share with anyone" link. Mockups (approved): Settings, PhoneSettings, SettingsAddCalendar, PhoneSettingsAddCalendar at https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W.

## Architecture & Data Models

- Calendars: the existing work calendar plus any number of extra calendars, each a Proton share link (https/webcal) or a local .ics file, with a use [paraphrase of the mockup]:
  - **Working hours**: exactly one, the existing `[calendar]` source: planned hours, days off, vacation, lunch, home working — unchanged.
  - **Private**: every timed event in it is a private appointment (same handling as a private-word event today: not company time, auto-start, trips private).
  - **Words only**: only timed events whose title has a private word are private appointments; the rest is ignored (no hours, no days off).
- Extra calendars never set planned hours, days off or vacation.
- Private words become editable in Settings ("Private appointments · Event titles containing", whole words, comma-separated), checked in every calendar; stored as today's `private_keywords`.
- Links are secrets like the current link: kept in the system keychain / Android Keystore, never in backups.
- Each calendar is fetched and cached like the work calendar (falls back to its kept copy when offline); the row shows its name (X-WR-CALNAME) and last sync.
- Android: sharing a text/URL to vw-danlon opens Settings with "Add calendar" filled with the link.

## Acceptance Criteria

- **R1:** Settings shows a Calendars card listing the work calendar and extra calendars with name, use and last sync; extra calendars can be removed, the working-hours one cannot.
- **R2:** Add calendar takes a link or .ics path and a use (Private / Words only); it is fetched once to check it and to read its name before saving.
- **R3:** Timed events from a Private calendar, and private-word events from a Words only calendar, show and behave as private appointments (Next private, split button, auto-start, trips private) exactly like private-word events in the work calendar today.
- **R4:** The private words are editable in Settings and apply to every calendar.
- **R5:** Extra links are kept with the secrets, left out of backups, and survive restarts; offline, the kept copy is used.
- **R6:** On Android, sharing a calendar link to vw-danlon opens Add calendar with the link filled in.
- **R7:** The CLI and config.toml keep working; existing configs without extra calendars behave as before.

## Boundaries

- No Proton sign-in or password; links only.
- No change to how hours, mileage or vacation are calculated from the work calendar.
- No JavaScript (CLAUDE.md rule).

## Quick commands

```bash
cargo test --workspace
```
