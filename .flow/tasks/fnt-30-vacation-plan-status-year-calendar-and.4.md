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
- [ ] Mockups approved before any code: drawn first (canvas version 4: Phone25/26 and Desktop14, light and dark), but implemented on "Run to completion" without a separate approval
- [x] An entered balance survives a restart; the difference is unit-tested
- [x] No JavaScript; cargo test, clippy clean

## Done summary
- **Mockups:** the canvas component `Vacation.dc.html` gains a `danlon` prop (none / entered / form). There are six new boards: Phone 25 (balance entered, 0,18 apart), Phone 26 (entering it) and Desktop 14 (entering it), each in dark and light.
- **Storage:** the `danlon_balance` table (database version 6; see task 2) holds one balance per holiday year: the days in hundredths and the date it was read. `service::set_danlon_balance` saves or removes it.
- **Comparison:** `vacation::balance_on(rules, leave, year, date)` gives the app's count on that date: what the year earned in the months ended by then, less the days taken by then.
- **App:** each holiday-year row of the expiry table has "Enter Danløn's balance". It opens an inline form with the days (`parse_days` takes 8,50 / 8.5 / 8) and the date read (today by default), plus Save, Cancel and Remove. A saved balance shows as "Danløn 8,50 · app 9,96 on 31 Oct 2026" with "1,46 apart" in warn colour, or "same as the app" when they match. The app's own count stays the basis of the plan.
- Tests: `vacation::the_balance_on_a_date_is_earned_less_taken`, `vacation_plan::danlon_balances_parse_and_compare`, and the state round trip in task 2.
