---
satisfies: [R5, R6, R7, R8, R9, R14]
---
# fnt-26-home-working-month-to-date-and-mileage.3 Mileage under Skat's rules, per-trip override, all-driving logbook, fradrag estimate

## Description
Classify trips by LL § 9 B (client visit with 60-day rule for home↔client, commute private, errands private, client→home direct km only, between workplaces via override); per-trip Business/Private override (trip business|private|reset); logbook --all lists all driving with reason, reimbursable total, YTD km and required fields; fradrag <year> per LL § 9 C (24 km threshold, 2026 rates 3,17/1,59, yderkommune 3,51; no home-working/vacation days; never the same km as godtgørelse).

**Size:** M
**Files:** src/mileage.rs, src/month.rs, src/service.rs, src/main.rs, src/config.rs

## Acceptance
- [ ] Tests: commute not reimbursable; client visit reimbursable and work hours; 61st home↔client day private; reset after 60 days elsewhere; errand chain private; client→shop→home direct km only
- [ ] Override Business/Private and reset
- [ ] logbook --all shows reasons and totals; fradrag 0 kr at 18 km/day; example above 24 km computed
- [ ] Rates missing for a year with business km reported


## Done summary
Trips are now classified by Skat's rules (LL § 9 B) with the deciding rule per trip (client visit, 60-day rule, commute, errand, private, detour, private appointment, set by you); `trip business|private|reset <YYYY-MM-DD HH:MM>` overrides are stored in the database (schema 3, upgrade from 1/2); `logbook <month> [--all] [-o file]` prints the reimbursable logbook, every trip with its rule and not-reimbursed km by reason, and the logbook carries address, CPR and year-to-date business km; `fradrag <year>` estimates befordringsfradrag (LL § 9 C).

Decisions: 60-day day = a date with a trip starting/ending at home to/from that client (calendar visit address, else the trip's other end); reset when >= 60 calendar working days (dates with a work entry) lie strictly between the last day at the client and the new day; home-working days neither count nor break; trips and calendar are now read from 1 January of the year before (`service::history_start`). Client -> stops -> home (or workplace) is one logbook line at the direct km = shortest single trip recorded earlier between the two places, either direction; none -> Incomplete "mangler direkte km" for the user to override; errands on the way TO a client are not chained (the stop -> client leg stays business). Business override without a purpose gets "Erhvervskørsel". Overrides key on the trip start to the minute and live in State (so backups carry them). Fradrag one-way km = median commuting trip of the year; excluded days: home working, days off, days with reimbursed km from/to home; yderkommune 3,51 applies to km above 120 a day; per-year `[[mileage.fradrag_rates]]` with 2026 built in. A missing mileage rate errors only when the year has business km.
Note: the user's vw-danlon.db is upgraded to schema 3 on the first run of this CLI; an app built before this commit refuses it until task .5 rebuilds the app.
Tests: mileage::tests::commute_is_driving_and_a_client_visit_is_reimbursed, home_to_a_client_is_private_from_the_61st_day, the_60_day_count_starts_over_after_60_working_days_elsewhere, errands_on_the_way_home_are_private, client_then_shops_then_home_gets_the_direct_km_only, a_trip_set_by_you_wins_over_the_rules, a_missing_rate_is_an_error_only_with_business_km (R7), logbook_carries_skats_fields (R9), befordringsfradrag_starts_above_24_km_a_day (R14); service::tests::a_trip_set_to_business_or_private_and_reset (R8 incl. unknown trip refused); db::tests::a_version_1_database_gets_the_home_and_override_tables. Work hours for client visits: existing month::tests::working_time_is_time_at_calendar_address.
Scratch copy (user data, --offline): 2026-09 85 trips 1000 km, 0 reimbursed (commute 27/250 km, errand 36/174 km, private 22/576 km); 2026-10 85 trips 658 km, 0 reimbursed (commute 28/259, errand 28/139, private 29/260); fradrag 2026: 9 km one way, 18 km/day, 172 commuting days, 0 kr.

baseline: green (cargo test --workspace pre-edit)
stage: impl-review - skipped(config: review backend not configured; conductor owns review)
Tier: not given by conductor (model: claude-opus-5-5)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: dfdbe7364426e916e110d43bcb3dd15198203b5b
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets
- PRs: