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
Extra calendars and editable private words. `[[extra_calendars]]` (source or app id, use = private | words_only, name) feed only private appointments through a new `Occurrence::Private`. They never set planned hours, days off or vacation, and their all-day events are ignored. Settings has a Calendars card (name from X-WR-CALNAME, a use chip, last sync and events this month). Add calendar takes a link or .ics file and a use (Working hours replaces the work calendar's link; Private; Words only) and fetches the calendar once to check and name it. Extra calendars can be removed; the work one cannot. A Private appointments card edits `calendar.private_keywords`. The app keeps the extra links in the secret store by id (FileStore/keychain/Android Keystore). Backups strip every calendar source. Refresh fetches every link, and offline the kept copy is used. On Android a shared text/plain link opens Add calendar filled in (workflow mime list + JNI EXTRA_TEXT reader).

Tests: config `extra_calendars_are_optional_and_read_with_their_use`; service `extra_calendars_add_only_private_appointments` (Private vs Words only, hours and days off unchanged, statuses) and `an_extra_calendar_link_uses_the_kept_copy_and_a_missing_one_is_named`; backup round trip now strips an extra link; app `calendars_are_added_with_links_in_the_store_and_removed`, `private_words_are_saved_where_they_apply`, `a_shared_calendar_link_is_found_in_the_text`.

Verified live (desktop dx build, scratch data dir with file secrets, headless Hyprland output, offline plus a localhost http server) at 390x844 and 1280x800. Checks: adding the work calendar from a .ics file; adding Private and Words only calendars; Words only counted 0, then 1 after adding "Frisør" to the private words and saving; Hours showed Next private Gym (Private calendar) and Frisør; Ferie in the Private calendar was not vacation; an unreachable link was refused with its error; Remove; a localhost link showed "Synced 09:36" and its events stayed after the server stopped and the app restarted. CLI with extra calendars: logbook marked a trip during Gym "private appointment", and norm was unchanged. Not verified: Android (clippy only; the share flow and manifest patch were not run on a phone), dark/light theme switching, the backup export from the app UI (unit test only).

Deviations from the mockups: the desktop private words are a comma-separated text field (as on phone and in Working from home), not chips. "Working hours" wraps to two lines in the 390 px Use selector.

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 8e465e6ad98d5b93f6ce8161ded76bbafb40faa7
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets, cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile, dx build --platform desktop
- PRs: