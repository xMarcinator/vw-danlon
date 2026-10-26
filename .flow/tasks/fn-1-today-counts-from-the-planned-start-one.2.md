---
satisfies: [R4, R5, R6]
---
# fn-1-today-counts-from-the-planned-start-one.2 App: one work card, today Day view, detected workplace notices, APK 0.3.0

## Description
Month: merged work card per mockups (Start work / split Private time ▾ whose menu has no appointments / Stop private; the Next private row has its own Start button and a chevron for the collapsible list of today's private entries only), header KPIs with today so far; Day today: worked so far, expected so far, diff, lateness, timeline note; desktop header control; Settings and Mileage show a detected workplace with Save; bump to 0.3.0 and build the signed APK.
**Files:** app/src/**, app/Cargo.toml, README.md

## Acceptance
- [ ] Screens match mockups (wide and 390px)
- [ ] Numbers match CLI report
- [ ] dist/vw-danlon-0.3.0-arm64.apk signed and verified


## Done summary
Finished the app side of fn-1 from the WIP commit f8eb190. Month has one work card (Start work, then a split Private time button whose menu has Start private time now, Private time started earlier… and Stop work, then Stop private; a Next private row with its own Start and a chevron for today's private list). The header KPIs count today so far, and the wide header carries the work pill. Today's Day view shows worked so far, expected so far, the difference, how late, and the "no drive home yet is fine" note, and is now titled "Today · Mon 26 Oct" as in the mockup (commit f92b102). Settings and Mileage show a detected workplace with Save. The freeze fix moves the month computation off the UI thread, bounds calendar refreshes with timeouts, and skips missed refresh ticks. The version is 0.3.0.

Checked in the desktop app (WebView2 over CDP, the stylesheet injected because asset!() is not served without dx) against two fixtures made from config.example.toml, at 390px and 1280px, compared with the PhoneMonth, PhoneMonthOpen, PhoneDayToday and Main mockups:
- At work (a trip arriving 08:25 today), the split menu open, and private time started earlier at 10:00 then stopped.
- Late and not at work, Start work from home, then Stop working from home.
- The detected-workplace notice on Mileage and Settings; Save wrote `workplace` to config.toml and the notice went away.
- Every Month KPI and today's row equal `report 2026-10` on the same fixture at the same minute (85,57 / 21,22 / −64,35 / today −2,77; with private time 85,55 / 22,80 / −62,75 / −1,17).

Follow-ups, not part of this task:
- The phone kicker "Today · planned from 08:00 · now 10:46" is cut off at 390px beside the Start work button.
- "Private time started earlier…" prefills the time from the machine's local clock, not the configured timezone.

Decisions: on wide screens the Next private row and list stay in the card under the header pill instead of a header text chip, so the Start button and the list stay reachable. The WIP also added "Stop work (left early)" (the `work_end` table, schema version 5, `service::set_left_work`), which the menu's Stop work item needs while you are at a work address. It is kept as the WIP had it. No other src/ changes.

Left to the conductor: building and signing dist/vw-danlon-0.3.0-arm64.apk (scripts/build-apk.sh was not run). scripts/build-apk.sh was changed in the working tree while this task ran (Windows support, mtime 10:37). That was not this worker, so it is left unstaged.

baseline: red via handoff (cargo test --workspace at 1c84ed9: 104 passed, 2 failed. db::tests::a_version_1_database_gets_the_later_tables, os error 32, and backup::tests::a_cli_folder_moves_in_and_its_json_files_are_deleted, path separator; both Windows-only and not fixed)
Tier: (model: claude-opus-5-5)
stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: f8eb1907b3fbb1ea488494187236180f7964b32d, 1c84ed99a805c26fea6d56a02dc8b7c3d34f7982, f92b102e5db6c04bd8a123b5d6a97a45ce8f3e93
- Tests: cargo test --workspace --no-fail-fast (106 lib: 104 passed, 2 Windows-only failures same as baseline; all integration and app tests passed), cargo test -p vw-danlon-app (16 passed, after the Day title change), cargo fmt --all -- --check (clean), desktop app via WebView2 CDP at 390px and 1280px vs mockups; Month numbers equal cargo run -- report 2026-10 on the same fixture
- PRs: