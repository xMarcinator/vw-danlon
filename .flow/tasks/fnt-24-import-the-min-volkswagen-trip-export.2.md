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
Trip addresses without a house number now match the same street with a number when postal codes agree; when neither side has a postal code, the towns must share a word. Optional `[places]` coordinates for home and workplace (radius_m, default 150 m) match a trip end by distance, and an invalid coordinate is refused naming its key. Tests: address::tests::street_without_house_number, address::tests::trip_end_near_place_coordinates, config::tests::places_coordinates. Manual check on the real export (2026-09): days with arrival at the workplace went from 0 to 17.

Known limitation: a numberless address with a postal code on only one side matches on the street name alone, so "Lysbrohøjen, Lysbro" matches "Lysbrohøjen 3, 8600 Silkeborg" as the AC requires. The same rule also accepts a same-named street in another town when that side has no postal code.

stage: impl-review - skipped(config: REVIEW_MODE=none)

stage: plan-sync - skipped(config: planSync.enabled != true)
## Evidence
- Commits: f62590263badcb56ecf3f88fe4270322f687aaba
- Tests: cargo test, cargo clippy --all-targets
- PRs: