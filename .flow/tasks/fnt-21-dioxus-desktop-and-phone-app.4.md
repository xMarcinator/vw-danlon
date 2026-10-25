---
satisfies: [R3, R4, R7]
---
# fnt-21-dioxus-desktop-and-phone-app.4 Vacation and Mileage screens, trips file import

## Description
Vacation (balance and periods from FNT-20) and Mileage logbook screens, plus importing a trips CSV/JSON via the desktop file picker and the Android document picker.

**Size:** M
**Files:** `app/src/screens/vacation.rs`, `app/src/screens/mileage.rs`, `app/src/import.rs`
**Touches:** [app/src/screens/**, app/src/import.rs]

### Design context
Mockup boards Vacation, Mileage, PhoneVacation, PhoneMileage.

## Acceptance
- [ ] Vacation numbers match `vacation`; mileage matches `logbook`
- [ ] Import works on desktop and Android; a malformed file names the first bad line

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
