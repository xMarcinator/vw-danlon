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
Timed calendar entries are now classified by title (new `calendar.private_keywords`, whole-word and case-insensitive, defaulting to Læge/Tandlæge/Fysioterapeut). A private appointment is kept out of work addresses, the planned norm and the missing-address warning. Its window is split at midnight, subtracted from the work periods before the break and rounding, stored on `Day.appointments`, and added to the day's note as `Læge 10:00–11:15 · private`. The test `month::tests::private_appointments_are_not_company_time` covers R1, R2, R4 and R5 (6.78 h example, Lægemiddelfirma non-match, no address warning, overlap deducted once, midnight split, empty list disables). Follow-up for .2: `Day.appointments` holds only spans, so trip classification by appointment address will need the location added. The field gives a dead-code warning until .2 reads it.

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: cda0e6a09901ef6925b2362f80dc0f4b9d1cb2cc
- Tests: cargo test (24 passed), cargo run -- -c config.example.toml report 2026-10 -t examples/trips.csv (rc=0)
- PRs: