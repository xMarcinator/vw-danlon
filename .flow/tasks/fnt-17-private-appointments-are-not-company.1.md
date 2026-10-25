---
satisfies: [R1, R2, R4, R5]
---
# fnt-17-private-appointments-are-not-company.1 Classify calendar events and subtract private appointments from worked time

## Description
Add one calendar-title classifier and use it in the day computation: private appointments are kept out of work addresses, planned norm and the missing-address warning, and their windows are subtracted from the work periods before the break/rounding step. The day record gets the appointment windows and a note.

**Size:** M
**Files:** `src/month.rs`, `src/config.rs`, `config.example.toml`
**Touches:** [src/month.rs, src/config.rs, config.example.toml]

### Approach
- Add `private_keywords` to `CalendarConfig` (config.rs:106) with a serde default fn, like `days_off_keywords`; whole-word, case-insensitive match.
- In `month::build`'s `Occurrence::Timed` arm (month.rs ~163-205) classify first; appointments go to a per-date window map (split at midnight like the planned-time loop) and skip `locations`, `planned`, `missing_location`.
- Add a pure interval-subtraction helper (spans minus windows) next to `work_segments` (month.rs:297); apply it to `segments` before the minutes sum and `worked` (month.rs:346). FNT-18 reuses it.
- Add `appointments: Vec<Span>` to `Day` (month.rs:68); append the note instead of the `note.is_none()` guard pattern (month.rs:266).

### Investigation targets
**Required:**
- `src/month.rs:133-300` — build loop, segments, note handling
- `src/config.rs:100-125` — CalendarConfig and defaults
**Optional:**
- `src/month.rs:368-480` — test helpers `cfg`, `trip`, `located_trip`

### Key context
All config structs use `deny_unknown_fields`; add new keys with defaults. Test helper `cfg()` sets `unpaid_break_minutes = 30`, so expected values must account for it.

## Acceptance
- [ ] Test: a timed "Læge" event with an address is not a work address and adds no norm; no missing-address warning
- [ ] Test: "Lægemiddelfirma" does not match "Læge"
- [ ] Test: at work 07:58–16:00 with Læge 10:00–11:15 (no break) worked = 6,78 h and norm unchanged
- [ ] Test: overlapping appointments deducted once; appointment across midnight split per date
- [ ] Report note shows `Læge 10:00–11:15 · private`
- [ ] `cargo test` passes

## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
