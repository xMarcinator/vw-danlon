---
satisfies: [R3, R4]
---
# fnt-24-import-the-min-volkswagen-trip-export.2 Match addresses without house numbers and by coordinates

## Description
Match trip addresses without a house number, and optionally by coordinates.

**Size:** S
**Files:** `src/address.rs`, `src/config.rs`, `src/mileage.rs`, `src/month.rs`, `config.example.toml`
**Touches:** [src/address.rs, src/config.rs, src/mileage.rs, src/month.rs, config.example.toml]

### Approach
- `address::matches` (address.rs:39): when one side's street part has no house number, compare the street names and require the same postal code or the same town word (export "Lysbrohøjen, Lysbro" has a town but no postal code; "Arendalsvej, 8600 Gødvad" has a postal code with another town name). Two different house numbers still never match.
- Optional `[places]` coordinates for home and workplace with a radius (default 150 m); a trip end with coordinates inside the radius matches. Pass coordinates where mileage.rs:84/102 and month.rs:246/307/315 call `matches`.

## Acceptance
- [ ] Test: "Lysbrohøjen, Lysbro" matches "Lysbrohøjen 3, 8600 Silkeborg"; "Arendalsvej, 8600 Gødvad" matches "Arendalsvej 192, 2. tv., 8600 Silkeborg"
- [ ] Test: "Arendalsvej 12, 8600 Silkeborg" does not match "Arendalsvej 192, …"; same street in another postal code does not match
- [ ] Test: a trip ending 100 m from configured workplace coordinates matches; 300 m does not; an invalid coordinate is refused naming the key
- [ ] `cargo test` passes


## Done summary
TBD

## Evidence
- Commits:
- Tests:
- PRs:
