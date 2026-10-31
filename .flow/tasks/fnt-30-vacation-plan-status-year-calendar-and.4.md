---
satisfies: [R12]
---
# fnt-30-vacation-plan-status-year-calendar-and.4 Danløn balance: enter it and show where it differs

## Description
Mockups first (CLAUDE.md): add boards for entering a balance from the Danløn app to the Design canvas (https://claude.ai/artifact/WvBtMPpTzBtX5vXNgtGwjT), on phone and desktop in both themes, and get them approved.

Then implement the entry and the comparison:
- **Entry:** per holiday year, the days and the date they were read. Stored in vw-danlon.db and included in backups.
- **Comparison:** the Vacation tab shows the difference between the app's count and Danløn's on that date, for example "Danløn 9,50 · app 9,32 on 31 Oct 2026 · 0,18 apart".
- **Unchanged:** the app's own count stays the basis of the projection. The app never reads Danløn; the value is typed in by hand.

**Files:** the canvas (Vacation.dc.html), app/src/screens/vacation.rs, src/vacation.rs, src/db.rs

## Acceptance
- [ ] Mockups approved before any code
- [ ] An entered balance survives a restart; the difference is unit-tested
- [ ] No JavaScript; cargo test, clippy clean
