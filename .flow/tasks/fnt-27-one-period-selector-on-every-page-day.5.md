# fnt-27-one-period-selector-on-every-page-day.5 Split button starts the next private event; ▾ menu above the buttons

## Description
### What

Change the Hours work buttons in the phone dock (task .4) to the approved mockups (artifact K5zwEk1uPeNuifnnaZjZ2W, version 144; local copies `PhoneMonth.dc.html`, `PhoneDayToday.dc.html` in the scratchpad period/project folder).

User: "give the stop work button les space since the private time button is a split button and let the private time button the title of the next private event and move the private time button to the split part but keep it as the button next if there is no events for today" / "Please remove the extra start private time button. That should be in drop-down of the split button" / "Please ad a mockup of the drop down expanded" / "Build it".

### Behaviour

- **Working (office or home), a private event still ahead today**: split button main part = two lines, small "Start private time" over "<event title> · <start>" (e.g. "Doctor · 13:00"); tapping starts that event's private time now (same as the old Start on the Next private row). ▾ opens a dropdown menu above the buttons with "Private time now" and a row "Private since [time] Start".
- **Working, no private event left today**: main part = "Private time"; ▾ menu has only "Private since [time] Start".
- **Not working**: unchanged "Start work ▾"; the menu holds "Started work at [time] Start".
- **Private time running**: unchanged "Stop private time".
- **Stop work**: takes only the width of its label (15 px text, 14 px side padding, 56 px tall); the split button takes the rest.
- **Work card**: no Start button on the "Next private" row any more (on phone and desktop alike, unless desktop has no dock — on desktop keep its existing buttons but drop the row's Start only if the split button there offers the same; otherwise leave desktop as is). The "Next private" row is hidden when no private event remains today.
- The menu is a popover: absolutely positioned above the button row, over the content, 48–52 px rows, closes on choice; ▾ has aria-expanded. Same on Hours Month/Week and Day (today).

### How

- Dock work buttons from task .4 in app/src/screens/month.rs (and the Day-today dock); the next private event comes from the same data that fills the "Next private" row today.
- CSS in app/assets/app.css next to the task .4 dock rules. No JavaScript.

## Acceptance
- [ ] Working with a private event ahead today: main part shows "Start private time" / "<title> · <time>" and starts it; ▾ menu offers Private time now and Private since
- [ ] Working with none left today: main part is "Private time"; ▾ menu offers Private since
- [ ] Not working: Start work ▾ with Started work at in the menu
- [ ] Stop work is only as wide as its label
- [ ] The Next private row has no Start button and hides when nothing is left today
- [ ] The ▾ menu opens above the buttons over the content and closes after a choice
- [ ] Same on Day (today); desktop unchanged apart from the removed duplicate Start; no JavaScript; cargo test, clippy and dx build pass


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
