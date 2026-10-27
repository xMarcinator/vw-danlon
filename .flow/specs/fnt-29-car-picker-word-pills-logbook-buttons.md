# Car picker, word pills, logbook buttons and shorter text

Feedback on v0.7.0 from the phone. Mockups: https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W
(version 149: PhoneMileage, PhoneSettings, Settings).

## Requirements

- **R1 Hours header padding.** On the Hours page the "work today" status card
  (Planned 08:00–16:00 / Not working · 7:41) has no bottom padding: the text
  sits on the bottom border. Give it the mockup's even padding (12px 14px on
  phone) top and bottom; check the other phone headers/cards for the same fault.
- **R2 Logbook buttons.** The mileage page's two logbook buttons sit side by
  side on one row (phone: two equal halves in the content, as in PhoneMileage),
  and their labels drop the month and year: "CSV" (phone) / "Logbook CSV"
  (desktop) and "Print for signature". The month is already in the title.
- **R3 Shorter text.** Slim the app's copy: notes, subtitles, status lines and
  button labels say the same in fewer words (examples in the mockups: "Whole
  words, every calendar.", "Every 15 min · unofficial · password never stored",
  "Events at home count too", "Links kept in Android Keystore.", "No links or
  tokens.", "Mileage" instead of "Mileage · this month"). Meaning and Skat
  wording (LL § 9 B etc.) stay; no information that a user needs to act is lost.
- **R4 Car picker.** When the Min Volkswagen account is connected, Settings
  lists the account's cars (plate, and model if the API gives it) as radio
  rows under "Car"; the selected one is `connected_cars.vehicle` in the
  config. Choosing one saves it; sync then uses that car. The list loads in
  the background when Settings opens (and after connect); errors show inline.
  Selecting a car also sets `mileage.registration_number` to its plate when the
  logbook is set up (the logbook must name the car driven). One car only: shown
  selected, still listed. The vehicle list is never stored except the chosen plate.
- **R5 Trigger words as pills.** Both word lists — Private appointments and
  Working from home — are edited in a pill input: each word is a pill inside the
  input box with an × to remove it; typing a word and pressing Enter (or a
  comma, or leaving the field) adds it. Duplicates (case-insensitive) and empty
  words are ignored. Same on desktop and phone (phone pills ≥32px tall, × target
  32px). Saved as lists exactly as today's comma-separated values were. Phone
  Settings gains the Working from home card (words + "Events at home count too").

## Boundaries
- No JavaScript (no eval/script); Dioxus events only.
- Never call Connected Cars from tests; parse/pick logic is unit-tested with fixtures.
- Never touch the repo-root config.toml, vw-danlon.db, connected-cars-token.
- The app writing config must keep the CLI able to read it (fix the known
  caveat: editing the private words must not write `[calendar]` without a source).

## Acceptance
- `cargo test --workspace` and `cargo clippy` clean; app/tests/no_javascript.rs passes.
- Pill input: add, remove, duplicate and comma cases covered by tests on the
  pure helper (split/merge words).
- Picking a car round-trips through config (test on a temp copy).
