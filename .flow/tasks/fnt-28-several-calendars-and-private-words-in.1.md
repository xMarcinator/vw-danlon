# fnt-28-several-calendars-and-private-words-in.1 Calendars in Settings: extra calendars (Private / Words only) and editable private words

## Description
### What
Implement R1–R7 of the spec in one change (library, CLI config, app Settings, Android share of a link).

### How (pointers)
- Config: `CalendarConfig` in src/config.rs (`private_keywords`, `source`); add a list of extra calendars (source + use + optional name), `deny_unknown_fields` stays; old configs without it keep working.
- Fetch/cache: `calendar::fetch` (src/calendar.rs:62), `fetch_and_keep` / `refresh_calendar` in src/service.rs (~151); `db::store_calendar` keyed by source already — reuse per calendar.
- Classification: `classify` / `has_word` / `timed_entries` in src/month.rs (~844–918) and the service's `timed_entries`; events from extra calendars feed only the private-appointment path (EntryKind::Private), never Work or Lunch, never days off.
- Secrets: how the current link is kept (app/src/setup.rs `with_calendar_link`, settings.rs `calendar_link`, src/backup.rs `without_calendar_link`) — extend to the extra links, and keep them out of backups.
- Settings UI: app/src/settings.rs "Work calendar" card (~429) becomes "Calendars" per the mockups; add the "Private appointments" card with the words field next to "Working from home" (~518).
- Android share: the release workflow's intent-filter patch (.github/workflows/release.yml ~218) and `platform::take_shared_file`; add text/plain SEND handling that routes an http(s)/webcal link to Add calendar.
- Tests for: config round-trip, Private vs Words only classification, extra calendar never sets hours/days off, backups exclude extra links.

## Acceptance
- [ ] R1–R7 of the spec met
- [ ] Desktop verified at 390 px and 1280 px on a scratch copy (two .ics files served locally or as files), offline
- [ ] cargo test, clippy (no warnings), dx build pass; no JavaScript


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
