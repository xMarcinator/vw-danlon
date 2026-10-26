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
TBD

## Evidence
- Commits:
- Tests:
- PRs:
