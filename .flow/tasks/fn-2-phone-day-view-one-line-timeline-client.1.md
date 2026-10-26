---
satisfies: [R1, R2, R6]
---
# fn-2-phone-day-view-one-line-timeline-client.1 One-line timeline on phone and desktop Day, client colour everywhere

## Description
Phone Day: one-line 06–18 timeline above the day's list (planned dashed outline, work green for office and home, client visits violet, private blue, lunch own grey segment between work blocks, driving light blue on the same line, today a thin now line, legend). Desktop Day timeline collapsed to one line with the same segments. Client violet tokens (light #5b3f8c on #efe9f8, bar #7a5bb5; dark #c3aef0 on #2a2238, bar #9d82d6) used on the timeline, the Hours day-list chip for client days and the Mileage Client visit rule chip.
**Files:** app/src/screens/day.rs, app/src/screens/month.rs (Hours list), app/src/screens/mileage.rs, app/assets/app.css; library only if a segment list helper is missing (src/service.rs).

## Acceptance
- [ ] Phone and desktop Day show one timeline line with all segment kinds and legend; lunch is its own segment
- [ ] Client violet on timeline, Hours chip and Mileage chip, light and dark
- [ ] Screenshots match the approved mockups


## Done summary
Phone and desktop Day show a one-line 06–18 timeline (dashed planned outline; work green for office and home; client violet; private blue; lunch its own grey segment; driving light blue; today a 2px now line) with the legend Planned · Work · Client · Private · Lunch · Driving. Client violet tokens (light #5b3f8c/#efe9f8/#7a5bb5, dark #c3aef0/#2a2238/#9d82d6) are used on the timeline, the Hours chip for client days (shows "N km business") and the Mileage Client visit rule chip. Library: Day::clients (time at a non-workplace work address) and service::day_timeline (non-overlapping segment list), tests in month::tests::workplace_counts_on_calendar_days_elsewhere and service::tests::day_timeline_is_one_line_without_overlaps.

baseline: green (cargo test --workspace)
Live: desktop app on a scratch VW_DANLON_DIR (copied config + db, calendar file from the cached copy plus a test client event on 22 Oct, account removed, no Connected Cars), screenshots wide + 390px light/dark in the session scratchpad shots/fn2-*.png.
Follow-up: the phone Day list still shows one "At work" step for the merged period (client part not split out).

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: 4faf4d4e7883a45714a006502fd5274b94928629
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets, cargo clippy -p vw-danlon-app -p vw-danlon --target aarch64-linux-android --no-default-features --features vw-danlon-app/mobile
- PRs: