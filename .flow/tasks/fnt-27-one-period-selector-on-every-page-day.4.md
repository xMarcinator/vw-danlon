# fnt-27-one-period-selector-on-every-page-day.4 Thumb layout on phone: period row and main actions at the bottom

## Description
### What

Phone thumb layout (approved mockups, artifact K5zwEk1uPeNuifnnaZjZ2W, boards `PhoneHoursThumb*`, `PhoneMileageThumb*`, `PhoneDanlonThumb*`, `PhoneVacationThumb*`, component `PhonePeriodDock`). On phone width (`@media (max-width: 719px)`) the controls tapped often move to a dock fixed just above the tab bar; reading content stays at the top and scrolls. Desktop keeps today's `.period-bar` card at the top, unchanged.

### Per page (phone only)

- **Top of every page**: page header `h1` = period title (e.g. "November 2026", "Week 48", "Today", "2026/27") with the sub text beside it. Tapping the header goes back to the current period (it replaces the Today button on phone; disabled look when already current). No swipe gestures (no JS).
- **Dock row (all four pages)**: `‹ [switch] ›` — 48 px square arrows around the segmented view switch (Hours: Day · Week · Month; Mileage/Danløn: Week · Month). Vacation and Mileage → Fradrag show a centred note ("Whole holiday years" / "Whole calendar years") between the arrows instead of the switch.
- **Hours**: above the period row, the work buttons: idle → one 56 px full-width "Start work"; working → "Private time" + "Stop work" side by side (56 px). The work card at the top keeps status text + "Next private" line but loses its buttons on phone. Split-button behaviour (FNT earlier: Start work / Work from home) stays as it is today, just moved.
- **Mileage**: the Reimbursable / All driving / Fradrag switch moves into the dock above the period row (44 px). "Logbook for signature" stays in the content.
- **Danløn**: "Open Danløn app" + "Mark <date> entered" (56 px, for the next day card) move into the dock above the period row; they leave the next-day card.
- **Vacation**: dock holds only the year row; the calendar tip becomes an in-page note card (no longer a fixed overlay).

### How

- Reuse `PeriodBar` state/handlers in `app/src/screens/period.rs`; add a phone dock rendering (e.g. a `PeriodDock` component or a `dock` variant) and a `PageHeader` that takes the title/sub/to-today handler. Render both and let CSS show one per width (desktop: `.period-bar`; phone: header + dock), or render by a width class — no JS for width detection.
- Dock: a `.thumb-dock` container rendered once per page, `position: sticky; bottom: <tabbar height>` (or a flex column so content scrolls above it); content gets bottom padding so the last row is never hidden. Respect `env(safe-area-inset-bottom)` like `.tabbar` (app.css:269/306).
- Tap targets ≥ 44 px, primary buttons 56 px; colours from existing tokens; dark mode via the existing scheme.
- Remove the phone-only Today button and the top `.period-bar` on phone.

### Constraints

- No JavaScript (FNT-27.3 rule; the guard test must stay green).
- Desktop layout unchanged.

## Acceptance
- [ ] On phone width, Hours, Mileage, Danløn and Vacation show the period row (‹ switch ›, or ‹ note ›) in a dock right above the tab bar, and the period title/sub at the top
- [ ] Tapping the header title returns to the current period; it is inert when already current
- [ ] Hours dock: Start work (idle) / Private time + Stop work (working); the work card at the top has no buttons on phone
- [ ] Mileage dock: Reimbursable / All driving / Fradrag switch; Fradrag shows "Whole calendar years" between the arrows
- [ ] Danløn dock: Open Danløn app + Mark <date> entered
- [ ] Vacation dock: year arrows with "Whole holiday years"; calendar tip is an in-page card
- [ ] The last list row is never hidden behind the dock
- [ ] Desktop (≥720 px) unchanged; no JavaScript added (no-JS guard test passes); cargo test and dx build pass


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
