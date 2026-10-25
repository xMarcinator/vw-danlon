# fnt-24-import-the-min-volkswagen-trip-export Import the Min Volkswagen trip export

## Conversation Evidence

> user: "My volkswagen have a address listed in the app. Hmm?" / "Per trip in a trip list"
> user: "I have a excel export here" (an .xlsx from the app's trip history, and a screenshot of Ture → Historik with the share icon)

## Goal & Context

The Min Volkswagen app (Connected Cars) exports the trip history as an Excel file with start and end time, start and end address, km, odometer and coordinates per trip. The tool should read that file directly, so the user no longer has to build a trips CSV by hand. A sample export (318 trips, 1 Aug – 24 Oct 2026) showed that the export often leaves out house numbers ("Lysbrohøjen, Lysbro", "Arendalsvej, 8600 Gødvad"), so with today's matching no trip was recognised as home or workplace. Mockup: Settings "Trips source" and Phone · Settings "Trips" at https://claude.ai/artifact/K5zwEk1uPeNuifnnaZjZ2W.
<!-- Source: 30% user / 40% [paraphrase] / 30% [inferred] -->

## Architecture & Data Models

- The export is one sheet "Export": five header lines (fleet, licence plate, export date, driver, period and km totals), then a header row and one row per trip: Starttidspunkt, Sluttidspunkt ("01/08/2026 09:40:07 CEST"), Startadresse, Slutadresse, two Google Maps links with coordinates, Afstand (km, whole km), Varighed (min), Start/Slut kilometerstand, Førernavn, Turtype, Note, CO2. [paraphrase of the sample]
- Rows become the existing trip records (start, end, km, from, to); the km comes from the odometer difference when both readings are present, else Afstand. Coordinates are kept on the trip for place matching. [inferred]
- Imported trips are stored locally (beside the config, like the state file) and merged across exports; a trip with the same start time and start odometer is the same trip. [inferred]

## API Contracts

- CLI: `trips import <file.xlsx>` merges an export into the local trip store and prints how many trips were added, skipped as already imported, and the period covered. Commands that take `-t` keep working; without `-t` they use the trip store.
- Address matching: a trip address without a house number matches a known address on the same street and the same postal code or town; when a trip and a configured place both have coordinates, being within a configurable radius (default 150 m) also matches. [inferred]

## Edge Cases & Constraints

- Timestamps carry CEST/CET; they are read in the configured timezone, including the DST change in October.
- Empty Turtype and Note columns are ignored; the tool still classifies trips itself.
- The export holds the driver's name and licence plate; they are not stored and never written to output files. [inferred]

## Acceptance Criteria

- **R1:** `trips import` reads the Min Volkswagen .xlsx export and adds each row as a trip with start, end, km, from and to. Errors: a file that is not this export (missing header row) is refused naming the missing columns; a row that ends before it starts is refused naming its row number, and nothing is imported.
- **R2:** Importing overlapping exports adds each trip once (same start time and start odometer). No error surface.
- **R3:** A trip address without a house number matches a configured or calendar address on the same street and postal code or town ("Lysbrohøjen, Lysbro" matches "Lysbrohøjen 3, 8600 Silkeborg"; "Arendalsvej, 8600 Gødvad" matches "Arendalsvej 192, 2. tv., 8600 Silkeborg"); a different house number on both sides does not match. No error surface. [inferred]
- **R4:** Optional coordinates for home and the workplace in config make a trip ending within the radius match that place. Errors: an invalid coordinate is refused naming the key. [inferred]
- **R5:** Reports, logbook and checklist use the stored trips when no `-t` is given. Errors: no stored trips and no `-t` → a message naming `trips import`.

## Early proof point

Task fnt-24-import-the-min-volkswagen-trip-export.1 validates reading the export into trips. If it fails, re-evaluate whether to convert via CSV instead before continuing.

## Boundaries

- No login to VW or Connected Cars and no network access for trips; the export file is the only source.
- Nothing is written back to the app (Turtype/Note stay untouched).

## Decision Context

The research found no API that returns trip addresses, but the app's own export does. Reading the file the user already gets keeps the tool offline and invisible.

## Quick commands

```bash
cargo test trips:: address::
cargo run -- trips import ~/Downloads/export.xlsx
```
