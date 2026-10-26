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
On phone width the Hours dock (Month/Week and Day today) now follows the approved mockups: while working with a private appointment still ahead today, the split button's main part reads "Start private time" over "<title> · <start>" and starts that appointment's private time (Action::Start(Some(title)), the same as the old row Start); ▾ (aria-haspopup, aria-expanded) opens a menu absolutely positioned above the button row, over the content, with "Private time now" (48 px) and "Private since [time] Start" (52 px). With nothing left today the main part is "Private time" and the menu has only Private since; not working it is Start work ▾ with "Started work at [time] Start". The menu closes after a choice or a tap outside (backdrop, as on the desktop menu). Stop work is only as wide as its label (15 px, 14 px padding, 56 px) and the split button is accent-filled with a 52 px caret. The phone work card has no Start on the Next private row, and the row (and its list) is hidden on phone when nothing is running or ahead today. Desktop is unchanged, including its row Start.

Verified live (desktop build, scratch copy of the earlier scratch data with "Læge" 22:30 and later "Tandlæge" 23:15 today, offline, headless output) at 390x844: Start work ▾ menu above the buttons; Start work -> split shows "Start private time / Læge · 22:30" with Stop work narrow; ▾ menu Private time now + Private since; Private time now starts and closes the menu; tapping the main part started Læge; after it the row hid and the main part read Private time with Private since only; Private since Start started private time and closed the menu; Stop work returned to Start work; Day today with "Tandlæge · 23:15" and its menu. At 1280x800 Hours Month showed the unchanged card with the Next private row Start and the header pill. Not verified: dark theme, Android build, Week view (same dock component). Note: while the menu is open, a tap on the main button first closes the menu (backdrop), like the desktop menu.

Tier: not reported (model: claude-opus-5-5)

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: f4666ba014988e35aec6feb9f03b32a8dc363bf7
- Tests: cargo test --workspace, cargo clippy --workspace --all-targets, dx build --platform desktop, cargo fmt --all -- --check
- PRs: