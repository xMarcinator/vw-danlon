---
satisfies: [R1, R2, R3, R8, R9, R12, R13]
---
# fnt-26-home-working-month-to-date-and-mileage.5 App screens for FNT-26 and a new APK

## Description
Month: today marker, to-date KPIs, Working from home card, private card auto-start/calendar-time states; Mileage: Reimbursable / All driving, rule chips, Business tick per trip, fradrag note; Settings: Working from home and Mileage rules (read-only rules, rates); then rebuild the signed APK with scripts/build-apk.sh.

**Size:** M
**Files:** app/src/**, app/assets/app.css, README.md
Mockups: Main, PhoneMonth, PhoneMonthOpen, Mileage, PhoneMileage, Settings.

## Acceptance
- [ ] Screens match the mockups (wide and 390px), light and dark
- [ ] Numbers match the CLI (tests like tests/*_matches_cli.rs)
- [ ] dist APK rebuilt and verified


## Done summary
App screens for FNT-26 and signed APK 0.2.0. Month: "Norm/Worked/Difference to date" KPIs (month norm and hours from home in the sub lines), today row marked (aria-current, accent edge, "Today" chip / label), days after today muted with "Planned" and norm only; Working from home Start/Stop (pill in the sources row when wide, card with running time when narrow, next home-working calendar day); private card shows "started automatically" with Stop, "Started at …" (start_private with EventChoice::Auto) and the grace note; list chips Auto / Calendar time / Recorded, Edit on every recorded entry (incl. calendar time), Skip on planned and auto-started ones (skip_private). Mileage: Reimbursable / All driving tabs, "Why it counts" chips from Reason, Business tick per trip calling set_trip_override with Reset for "set by you", not-reimbursable km by rule, kørselsfradrag note from service::fradrag. Settings: Working from home (keywords, home-address switch, saved to [home_working] only when not default) and Mileage rules (read-only rules + yearly rates table). build-apk.sh now sets versionCode from the version (0.2.0 -> 200; dx always wrote 1).

Decisions: yearly rates are shown read-only in Settings (edit [[mileage.rates]] in config.toml) instead of the mockup's inputs; home-working edit/delete stay in the CLI (`home edit|rm`), the app has Start/Stop as the mockup shows.
Follow-ups: home-working intervals list/edit in the app; the private list on Month starts at today, so past calendar-time entries are edited from the Day screen.

Tests: app setup::tests::home_working_words_are_saved_and_defaults_add_nothing; cargo test --workspace green; clippy (host and aarch64-linux-android mobile) clean.
Live (scratch VW_DANLON_DIR, copied config/db, calendar from the cached copy as a file plus test events; Hyprland headless output, GDK_BACKEND=x11): Month KPIs 127,50 / 124,20 / -3,30 = CLI `report 2026-10`; auto-started Læge -> Started at -> running -> Stop recorded; home Start/Stop recorded; Business tick on a trip -> 9,0 km / 35,46 kr = CLI `logbook --all`; Reset back to rules; fradrag note "18,0 km a day under 24 km, 0 kr" = CLI `fradrag 2026`. Screenshots wide + 390px, light and dark. No Connected Cars calls; user's repo-root files untouched.
APK: dist/vw-danlon-0.2.0-arm64.apk, 16108983 bytes, versionCode 200 / versionName 0.2.0, v2+v3 signed, cert SHA-256 ca2bcf49355953ad8b6a03241630a2545791e808089aa438daba47c7e7eb53d7, not debuggable.

baseline: green (cargo test --workspace pre-edit)
stage: impl-review - skipped(config: review backend not configured; conductor owns review)
Tier: not given by conductor (model: claude-opus-5-5)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 9aaa8bfa8f500fff97775017bba5ca11ba5c1382
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets, cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile, scripts/build-apk.sh
- PRs: