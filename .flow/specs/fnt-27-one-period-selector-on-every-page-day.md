# fnt-27-one-period-selector-on-every-page-day One period selector on every page, Day in Hours

## Conversation Evidence

> user: "please reuse the period selector on all pages from the hours page. Also please increase the spacing between the selector but also consider a redesign because the month and week seems misplaced."
> user: "Also please move the day view mockups to the today selection for the hours page"
> user: "Build it."

## Goal & Context

Hours had its own Month/Week switch in the top-left corner above the title, which looked misplaced, and the other pages had no period selector. One shared selector now sits at the top of Hours, Mileage, Danløn and Vacation (phone and desktop), and the Day view becomes the Day choice on Hours. Mockups (approved): "Component · Period selector", the Hours · Month/Week/Day rows, Mileage, Danløn and Vacation phone boards at https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W.

## Architecture & Data Models

- One app component, the period selector: a card with ‹ arrow, the period title and a sub line, › arrow (44 px framed buttons, 12 px apart); 14 px below it a full-width segmented switch and a Today button, enabled only when another period is shown. [paraphrase of the mockup]
- Modes: Day · Week · Month (Hours), Week · Month (Mileage, Danløn), whole years without a switch (Vacation: holiday years; Mileage → Fradrag: calendar years).
- Hours · Day replaces the separate Day screen: Today by default, ‹ › step days, a day in the Week/Month lists opens Day on that date. The Day screen's own back link and heading go; the bottom bar shows Hours.
- Mileage and Danløn by week: the same data filtered to the ISO week (Mon–Sun); the logbook CSV and print stay per month (Skat's logbook is per period of the employer's choice; monthly as now) and say which month they cover.
- The page headings ("Mileage", "Danløn app", "Vacation") are dropped; the selector is the first thing on every page, as on Hours.

## Acceptance Criteria

- **R1:** A single selector component is used on Hours, Mileage, Danløn and Vacation, phone and desktop, matching the mockup spacing.
- **R2:** Hours has Day · Week · Month; Day opens on today; arrows step the chosen unit; Today returns to the current period and is disabled when already there.
- **R3:** The Day view is shown inside Hours with the selector and the bottom bar; tapping a day in Week or Month opens Day on that date.
- **R4:** Mileage and Danløn have Week · Month; week shows that week's trips/registrations; logbook export and print stay monthly.
- **R5:** Vacation steps holiday years; Mileage → Fradrag steps calendar years; no Week/Month switch there.
- **R6:** The chosen view per page is remembered while the app runs.

## Boundaries

- No change to how hours, mileage or vacation are calculated.

## Quick commands

```bash
cargo test --workspace
```
