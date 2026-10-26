---
satisfies: [R1, R4, R5]
---
# fnt-27-one-period-selector-on-every-page-day.2 Selector on Mileage, Danløn and Vacation; week filtering; release 0.6.0 prep

## Description
Mileage and Danløn with Week · Month (filter the month data to the ISO week; logbook CSV and print stay monthly and say so); Mileage → Fradrag and Vacation step years with no switch; drop the page headings; desktop too. Bump app version to 0.6.0 (the release workflow builds it from the tag).
**Files:** app/src/screens/mileage.rs, danlon.rs, vacation.rs, app/assets/app.css, app/Cargo.toml

## Acceptance
- [ ] Week view on Mileage and Danløn shows the week's trips/registrations; month unchanged
- [ ] Fradrag and Vacation step years
- [ ] cargo test, clippy clean; version 0.6.0


## Done summary
Mileage and Danløn now open with the shared period selector with Week · Month: a week shows that ISO week's trips or days, taken from its one or two months, each month computed exactly as the monthly view computes it (tests: `a_week_shows_its_trips_from_both_months`, `a_week_holds_its_days_and_months`). The logbook CSV and print stay monthly and name the month in their buttons ("Logbook CSV · October 2026", "Print October 2026 for signature"). Mileage → Fradrag steps calendar years and Vacation steps holiday years, both with a note ("Whole calendar years" / "Whole holiday years") in place of the switch. Vacation lists the chosen year's holidays, and shows a closed year as it stood on its use-by date (tests: `year_pages_title_their_years`, `a_holiday_year_is_seen_on_today_or_its_use_by_date`). The page headings are gone on phone and desktop, so the selector comes first. Each page's period is kept while the app runs (`PagePeriods` context). The `.month-nav`/`.nav-btn`/`.month-title` CSS is removed. The app version is now 0.6.0.

Also fixed: an empty week showed "-0,00 kr" because a float `.sum()` starts at -0.0. Shown totals now use `fold(0.0)`.

`HoursPeriod` is renamed `Period`, and `HoursBar` is replaced by a `PeriodPicker` that takes the views. Live-checked on the desktop app with scratch data (offline): wide and 390 px, light and dark.

Follow-ups: Fradrag for 2025 says "Needs home and the workplace in Settings." on the scratch data, which has no 2025 data. That message comes from `service::fradrag` and was already reachable before this change. The Mileage KPI card is not redesigned to match the combined card in the mockup.

Tier: not reported (model: claude-opus-5-5)

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 73f9430bd0256f773d7ba935f87ef07a608bb9ae
- Tests: cargo test --workspace (pass, 151), cargo clippy --workspace --all-targets (0 warnings), cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile (0 warnings), cargo fmt --all -- --check (clean)
- PRs: