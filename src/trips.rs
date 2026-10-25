//! Trips driven, read from CSV, JSON or a Min Volkswagen .xlsx export.
//!
//! CSV/JSON fields: `start`, `end` (required), `distance_km`, `from`, `to`,
//! `purpose`, `business` (optional). Timestamps are RFC 3339
//! (`2026-10-05T07:42:00+02:00`) or local time without offset
//! (`2026-10-05 07:42`), interpreted in the configured zone.

use std::io::{Cursor, Read, Seek};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use calamine::{Data, Reader, Xlsx, open_workbook};
use chrono::{DateTime, LocalResult, NaiveDateTime, TimeZone};
use chrono_tz::{OffsetName, Tz};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub struct Trip {
    pub start: DateTime<Tz>,
    pub end: DateTime<Tz>,
    pub distance_km: Option<f64>,
    pub from: String,
    pub to: String,
    /// Business purpose (formål). Required for mileage allowance.
    pub purpose: String,
    /// Explicit business/private flag; when absent, a trip with a purpose
    /// counts as business.
    pub business: Option<bool>,
    /// Coordinates where the trip started and ended, when known.
    pub start_pos: Option<Position>,
    pub end_pos: Option<Position>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Deserialize)]
struct RawTrip {
    start: String,
    end: String,
    #[serde(default)]
    distance_km: Option<f64>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    purpose: Option<String>,
    #[serde(default)]
    business: Option<bool>,
}

pub fn load(path: &Path, tz: Tz) -> Result<Vec<Trip>> {
    if is_xlsx(path) {
        return Ok(read_export(path, tz)?.into_iter().map(|r| r.trip).collect());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading trips {}", path.display()))?;
    parse_text(&text, is_json(path), tz)
}

fn is_json(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
}

/// Trips from CSV or JSON text. A bad trip fails the whole text and is
/// named: "row N" counts CSV lines with the header as row 1, "trip N"
/// counts JSON entries from 1.
pub fn parse_text(text: &str, json: bool, tz: Tz) -> Result<Vec<Trip>> {
    let raw: Vec<(String, Result<RawTrip>)> = if json {
        let raw: Vec<RawTrip> = serde_json::from_str(text).context("parsing trips JSON")?;
        raw.into_iter()
            .enumerate()
            .map(|(i, r)| (format!("trip {}", i + 1), Ok(r)))
            .collect()
    } else {
        csv::Reader::from_reader(text.as_bytes())
            .deserialize()
            .enumerate()
            .map(|(i, r)| (format!("row {}", i + 2), r.context("parsing trips CSV")))
            .collect()
    };
    let mut trips = raw
        .into_iter()
        .map(|(at, r)| r.and_then(|r| trip_of(r, tz)).context(at))
        .collect::<Result<Vec<_>>>()?;
    trips.sort_by_key(|t| t.start);
    Ok(trips)
}

fn trip_of(r: RawTrip, tz: Tz) -> Result<Trip> {
    let start = parse_time(&r.start, tz)?;
    let end = parse_time(&r.end, tz)?;
    if end < start {
        return Err(anyhow!(
            "trip ends before it starts: {} → {}",
            r.start,
            r.end
        ));
    }
    let text = |s: Option<String>| s.map(|s| s.trim().to_string()).unwrap_or_default();
    Ok(Trip {
        start,
        end,
        distance_km: r.distance_km,
        from: text(r.from),
        to: text(r.to),
        purpose: text(r.purpose),
        business: r.business,
        start_pos: None,
        end_pos: None,
    })
}

pub fn parse_time(s: &str, tz: Tz) -> Result<DateTime<Tz>> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&tz));
    }
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            return tz
                .from_local_datetime(&naive)
                .earliest()
                .ok_or_else(|| anyhow!("time {s:?} does not exist in {tz}"));
        }
    }
    Err(anyhow!("unrecognised time {s:?}"))
}

pub fn is_xlsx(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
}

/// A trip from a Min Volkswagen export, with the start odometer reading
/// that identifies it across exports.
#[derive(Debug, Clone)]
pub struct ExportTrip {
    pub trip: Trip,
    pub start_odometer: Option<f64>,
}

const START: &str = "Starttidspunkt";
const END: &str = "Sluttidspunkt";
const FROM: &str = "Startadresse";
const TO: &str = "Slutadresse";
const DISTANCE: &str = "Afstand (km)";
const ODO_START: &str = "Start Kilometerstand";
const ODO_END: &str = "Slut kilometerstand";
const REQUIRED: [&str; 7] = [START, END, FROM, TO, DISTANCE, ODO_START, ODO_END];
const POS_START: &str = "Startposition Google Maps Link";
const POS_END: &str = "Slutposition Google Maps Link";

/// Reads the first sheet of a Min Volkswagen (Connected Cars) trip export.
/// The header row is found by its column names; the summary lines above it
/// are skipped. Driver name, plate, trip type and note are not read. Any bad
/// row fails the whole file.
pub fn read_export(path: &Path, tz: Tz) -> Result<Vec<ExportTrip>> {
    let book: Xlsx<_> =
        open_workbook(path).with_context(|| format!("opening {}", path.display()))?;
    export_trips(book, &path.display().to_string(), tz)
}

/// [`read_export`] of an export's bytes; `name` names it in errors.
pub fn read_export_bytes(bytes: &[u8], name: &str, tz: Tz) -> Result<Vec<ExportTrip>> {
    let book = Xlsx::new(Cursor::new(bytes)).with_context(|| format!("opening {name}"))?;
    export_trips(book, name, tz)
}

fn export_trips<RS: Read + Seek>(
    mut book: Xlsx<RS>,
    name: &str,
    tz: Tz,
) -> Result<Vec<ExportTrip>> {
    let sheet = book
        .worksheet_range_at(0)
        .ok_or_else(|| anyhow!("{name} has no sheets"))?
        .with_context(|| format!("reading {name}"))?;
    let first_row = sheet.start().map_or(0, |(r, _)| r as usize);
    let rows: Vec<&[Data]> = sheet.rows().collect();

    let cell_name = |c: &Data| c.to_string().trim().to_lowercase();
    let found =
        |row: &[Data], col: &str| row.iter().position(|c| cell_name(c) == col.to_lowercase());
    // The header is the row with the most of the required columns.
    let (header_at, header) = rows
        .iter()
        .enumerate()
        .max_by_key(|(i, row)| {
            let n = REQUIRED.iter().filter(|c| found(row, c).is_some()).count();
            (n, std::cmp::Reverse(*i))
        })
        .map(|(i, row)| (i, *row))
        .unwrap_or((0, &[]));
    let missing: Vec<&str> = REQUIRED
        .iter()
        .copied()
        .filter(|c| found(header, c).is_none())
        .collect();
    if !missing.is_empty() {
        bail!(
            "{name} is not a Min Volkswagen trip export: missing column(s) {}",
            missing.join(", ")
        );
    }
    let col = |c: &str| found(header, c).unwrap();

    let mut trips = Vec::new();
    for (i, row) in rows.iter().enumerate().skip(header_at + 1) {
        if row.iter().all(|c| matches!(c, Data::Empty)) {
            continue;
        }
        let number = first_row + i + 1;
        let cell = |c: usize| row.get(c).unwrap_or(&Data::Empty);
        let text = |c: usize| cell(c).to_string().trim().to_string();
        let read = || -> Result<ExportTrip> {
            let start = parse_export_time(&text(col(START)), tz)?;
            let end = parse_export_time(&text(col(END)), tz)?;
            if end < start {
                bail!(
                    "trip ends before it starts: {} → {}",
                    text(col(START)),
                    text(col(END))
                );
            }
            let start_odometer = number_of(cell(col(ODO_START)));
            let distance_km = match (start_odometer, number_of(cell(col(ODO_END)))) {
                (Some(s), Some(e)) if e >= s => Some(e - s),
                _ => number_of(cell(col(DISTANCE))),
            };
            let pos = |c: Option<usize>| c.and_then(|c| position(&text(c)));
            Ok(ExportTrip {
                trip: Trip {
                    start,
                    end,
                    distance_km,
                    from: text(col(FROM)),
                    to: text(col(TO)),
                    purpose: String::new(),
                    business: None,
                    start_pos: pos(found(header, POS_START)),
                    end_pos: pos(found(header, POS_END)),
                },
                start_odometer,
            })
        };
        trips.push(read().with_context(|| format!("{name} row {number}"))?);
    }
    trips.sort_by_key(|t| t.trip.start);
    Ok(trips)
}

/// An export timestamp, "01/08/2026 09:40:07 CEST": local time in `tz`. The
/// zone abbreviation only picks the instant in the hour repeated when
/// summer time ends.
pub fn parse_export_time(s: &str, tz: Tz) -> Result<DateTime<Tz>> {
    let s = s.trim();
    if s.is_empty() {
        bail!("missing time");
    }
    let (local, zone) = match s.rsplit_once(' ') {
        Some((l, z)) if z.chars().all(|c| c.is_ascii_alphabetic()) => (l, Some(z)),
        _ => (s, None),
    };
    let naive = NaiveDateTime::parse_from_str(local, "%d/%m/%Y %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(local, "%d/%m/%Y %H:%M"))
        .map_err(|_| anyhow!("unrecognised time {s:?}"))?;
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(t) => Ok(t),
        LocalResult::Ambiguous(a, b) => {
            let is = |t: &DateTime<Tz>| zone.is_some() && t.offset().abbreviation() == zone;
            Ok(if is(&b) && !is(&a) { b } else { a })
        }
        LocalResult::None => bail!("time {s:?} does not exist in {tz}"),
    }
}

fn number_of(c: &Data) -> Option<f64> {
    match c {
        Data::Int(i) => Some(*i as f64),
        Data::Float(f) => Some(*f),
        Data::String(s) => s.trim().replace(',', ".").parse().ok(),
        _ => None,
    }
}

/// The coordinates in a "https://maps.google.com/?q=56.19,9.59" link.
fn position(link: &str) -> Option<Position> {
    let q = link.split_once("q=")?.1.split('&').next()?;
    let (lat, lon) = q.split_once(',')?;
    Some(Position {
        lat: lat.trim().parse().ok()?,
        lon: lon.trim().parse().ok()?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    const TZ: Tz = chrono_tz::Europe::Copenhagen;

    pub const HEADER: [&str; 14] = [
        START,
        END,
        FROM,
        TO,
        POS_START,
        POS_END,
        DISTANCE,
        "Varighed (min)",
        ODO_START,
        ODO_END,
        "Førernavn",
        "Turtype",
        "Note",
        "CO2 udledning (kg)",
    ];

    /// One trip row: start, end, from, to, start/end position link,
    /// distance, start/end odometer.
    pub type Row<'a> = (
        &'a str,
        &'a str,
        &'a str,
        &'a str,
        &'a str,
        &'a str,
        f64,
        Option<f64>,
        Option<f64>,
    );

    /// Writes a fixture export like Min Volkswagen's: summary lines, the
    /// header row, then one row per trip. Empty cells are left out.
    pub fn write_export(path: &Path, header: &[&str], rows: &[Row]) {
        use rust_xlsxwriter::Workbook;
        let mut book = Workbook::new();
        let sheet = book.add_worksheet();
        sheet.set_name("Export").unwrap();
        for (r, line) in [
            ["Flådenavn", "Testflåde"],
            ["Nummerplade", "AB 12 345"],
            ["Kaldenavn", "Testbil"],
            ["Dato for eksport", "25/10/2026"],
            ["Førernavn", "Test Testesen"],
        ]
        .iter()
        .enumerate()
        {
            for (c, text) in line.iter().enumerate() {
                sheet.write_string(r as u32, c as u16, *text).unwrap();
            }
        }
        for (c, name) in header.iter().enumerate() {
            sheet.write_string(5, c as u16, *name).unwrap();
        }
        let col = |name: &str| header.iter().position(|h| *h == name).map(|c| c as u16);
        for (i, row) in rows.iter().enumerate() {
            let r = 6 + i as u32;
            let (start, end, from, to, pos_start, pos_end, km, odo_start, odo_end) = *row;
            for (name, text) in [
                (START, start),
                (END, end),
                (FROM, from),
                (TO, to),
                (POS_START, pos_start),
                (POS_END, pos_end),
            ] {
                if let Some(c) = col(name).filter(|_| !text.is_empty()) {
                    sheet.write_string(r, c, text).unwrap();
                }
            }
            for (name, n) in [
                (DISTANCE, Some(km)),
                (ODO_START, odo_start),
                (ODO_END, odo_end),
            ] {
                if let (Some(c), Some(n)) = (col(name), n) {
                    sheet.write_number(r, c, n).unwrap();
                }
            }
        }
        book.save(path).unwrap();
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vw-danlon-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub const ROWS: [Row; 3] = [
        (
            "01/08/2026 09:40:07 CEST",
            "01/08/2026 10:05:30 CEST",
            "Testvej 1, 8000 Aarhus",
            "Prøvegade, 8600 Silkeborg",
            "https://maps.google.com/?q=56.1,10.2",
            "https://maps.google.com/?q=56.2,9.5",
            40.0,
            Some(1000.0),
            Some(1041.0),
        ),
        (
            "02/08/2026 16:00:00 CEST",
            "02/08/2026 16:30:00 CEST",
            "Prøvegade, 8600 Silkeborg",
            "Testvej 1, 8000 Aarhus",
            "",
            "",
            42.0,
            None,
            None,
        ),
        (
            "03/08/2026 07:15:00 CEST",
            "03/08/2026 07:20:00 CEST",
            "Testvej 1, 8000 Aarhus",
            "Eksempelvej 3, Lysby",
            "https://maps.google.com/?q=56.1,10.2",
            "https://maps.google.com/?q=56.11,10.21",
            2.0,
            Some(1083.0),
            Some(1086.0),
        ),
    ];

    fn at(s: &str) -> DateTime<Tz> {
        parse_time(s, TZ).unwrap()
    }

    #[test]
    fn export_rows_become_trips() {
        let dir = temp_dir("export-read");
        let path = dir.join("export.xlsx");
        write_export(&path, &HEADER, &ROWS);

        let trips = read_export(&path, TZ).unwrap();
        assert_eq!(trips.len(), 3);
        let t = &trips[0].trip;
        assert_eq!(
            (t.start, t.end),
            (at("2026-08-01 09:40:07"), at("2026-08-01 10:05:30"))
        );
        assert_eq!(
            (t.from.as_str(), t.to.as_str()),
            ("Testvej 1, 8000 Aarhus", "Prøvegade, 8600 Silkeborg")
        );
        assert_eq!(
            t.start_pos,
            Some(Position {
                lat: 56.1,
                lon: 10.2
            })
        );
        assert_eq!(
            t.end_pos,
            Some(Position {
                lat: 56.2,
                lon: 9.5
            })
        );
        assert_eq!(trips[0].start_odometer, Some(1000.0));
        // km from the odometer, else Afstand.
        let km: Vec<_> = trips.iter().map(|t| t.trip.distance_km).collect();
        assert_eq!(km, [Some(41.0), Some(42.0), Some(3.0)]);
        assert_eq!(trips[1].trip.start_pos, None);

        // `-t export.xlsx` reads the same trips.
        let loaded = load(&path, TZ).unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[2].to, "Eksempelvej 3, Lysby");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_exports_are_refused() {
        let dir = temp_dir("export-bad");
        let path = dir.join("export.xlsx");
        let header: Vec<&str> = HEADER.iter().copied().filter(|h| *h != ODO_END).collect();
        write_export(&path, &header, &ROWS);
        let err = read_export(&path, TZ).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing column(s) Slut kilometerstand"),
            "{err}"
        );

        let mut rows = ROWS;
        rows[1].1 = "02/08/2026 15:00:00 CEST";
        write_export(&path, &HEADER, &rows);
        let err = format!("{:#}", read_export(&path, TZ).unwrap_err());
        // Spreadsheet row 8: five summary lines, the header, then the rows.
        assert!(err.contains("row 8: trip ends before it starts"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn zone_abbreviation_picks_the_repeated_hour() {
        let utc = |s: &str| parse_export_time(s, TZ).unwrap().naive_utc().to_string();
        assert_eq!(utc("25/10/2026 02:30:00 CEST"), "2026-10-25 00:30:00");
        assert_eq!(utc("25/10/2026 02:30:00 CET"), "2026-10-25 01:30:00");
        assert_eq!(utc("25/10/2026 03:30:00 CET"), "2026-10-25 02:30:00");
    }
}
