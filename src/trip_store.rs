//! Trips imported from Min Volkswagen exports or synced from the account,
//! merged in one JSON file beside the config. A trip is identified by its start time and start
//! odometer reading, so overlapping exports add each trip once. Driver name
//! and licence plate are never stored.

use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::month::Month;
use crate::trips::{ExportTrip, Position, Trip};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TripStore {
    pub trips: Vec<StoredTrip>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredTrip {
    pub start: DateTime<FixedOffset>,
    pub end: DateTime<FixedOffset>,
    pub start_odometer: Option<f64>,
    pub distance_km: Option<f64>,
    pub from: String,
    pub to: String,
    pub start_pos: Option<Position>,
    pub end_pos: Option<Position>,
    /// From an imported CSV/JSON trips file; exports and the account have
    /// none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub business: Option<bool>,
}

impl StoredTrip {
    fn same_trip(&self, other: &StoredTrip) -> bool {
        self.start == other.start && self.start_odometer == other.start_odometer
    }
}

impl From<&ExportTrip> for StoredTrip {
    fn from(e: &ExportTrip) -> Self {
        let t = &e.trip;
        StoredTrip {
            start: t.start.fixed_offset(),
            end: t.end.fixed_offset(),
            start_odometer: e.start_odometer,
            distance_km: t.distance_km,
            from: t.from.clone(),
            to: t.to.clone(),
            start_pos: t.start_pos,
            end_pos: t.end_pos,
            purpose: t.purpose.clone(),
            business: t.business,
        }
    }
}

/// What an import changed.
#[derive(Debug, PartialEq, Eq)]
pub struct Merged {
    pub added: usize,
    pub already: usize,
}

/// What an import read and changed: the counts, and the first and last
/// trip day of the file.
pub type Imported = (Merged, Option<(NaiveDate, NaiveDate)>);

/// Reads the whole file (a Min Volkswagen .xlsx export, or a CSV/JSON trips
/// file), then merges it into the store at `store`. A bad row fails before
/// the store is touched, so nothing is imported.
pub fn import(store: &Path, file: &Path, tz: Tz) -> Result<Imported> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    import_bytes(store, &file.display().to_string(), &bytes, tz)
}

/// [`import`] of a file's bytes; `name` (its file name) picks the format by
/// its extension and names the file in errors.
pub fn import_bytes(store: &Path, name: &str, bytes: &[u8], tz: Tz) -> Result<Imported> {
    let imported = read_trips_file(name, bytes, tz)?;
    let mut trips = TripStore::load(store)?;
    let merged = trips.merge(&imported);
    if merged.added > 0 {
        trips.save(store)?;
    }
    let period = imported
        .first()
        .zip(imported.last())
        .map(|(f, l)| (f.trip.start.date_naive(), l.trip.start.date_naive()));
    Ok((merged, period))
}

fn read_trips_file(name: &str, bytes: &[u8], tz: Tz) -> Result<Vec<ExportTrip>> {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let json = match ext.as_deref() {
        Some("json") => true,
        Some("csv") => false,
        // An .xlsx file is a zip archive.
        _ if ext.as_deref() == Some("xlsx") || bytes.starts_with(b"PK") => {
            return crate::trips::read_export_bytes(bytes, name, tz);
        }
        _ => bail!("{name} is not a Min Volkswagen export (.xlsx) or a CSV/JSON trips file"),
    };
    let text = std::str::from_utf8(bytes).with_context(|| format!("{name} is not UTF-8 text"))?;
    let trips = crate::trips::parse_text(text, json, tz).with_context(|| name.to_string())?;
    Ok(trips
        .into_iter()
        .map(|trip| ExportTrip {
            trip,
            start_odometer: None,
        })
        .collect())
}

/// What a sync changed, over the whole period.
#[derive(Debug, PartialEq, Eq)]
pub struct Synced {
    pub merged: Merged,
    /// Fetched trips without an address or coordinates.
    pub without_position: usize,
}

/// Fetches the trips starting `from` through `to` month by month with
/// `fetch(first_day, last_day)` and merges each month into the store at
/// `store` once it has been fetched whole. A failed month stops the sync:
/// the months before it stay stored, it and the later ones are not touched.
/// Trips a fetch returns that start outside its days are left to the month
/// they start in, so a trip across midnight is counted once.
pub fn sync(
    store: &Path,
    from: NaiveDate,
    to: NaiveDate,
    tz: Tz,
    mut fetch: impl FnMut(NaiveDate, NaiveDate) -> Result<Vec<ExportTrip>>,
) -> Result<Synced> {
    let mut synced = Synced {
        merged: Merged {
            added: 0,
            already: 0,
        },
        without_position: 0,
    };
    let mut first = from;
    while first <= to {
        let month = Month {
            year: first.year(),
            month: first.month(),
        };
        let last = month.last_day().min(to);
        let mut fetched = fetch(first, last).with_context(|| {
            format!("syncing {month} stopped; nothing from {first} on was stored")
        })?;
        fetched.retain(|t| (first..=last).contains(&t.trip.start.with_timezone(&tz).date_naive()));
        let mut trips = TripStore::load(store)?;
        let merged = trips.merge(&fetched);
        if merged.added > 0 {
            trips.save(store)?;
        }
        synced.merged.added += merged.added;
        synced.merged.already += merged.already;
        synced.without_position += crate::connected_cars::without_position(&fetched);
        first = last + chrono::Duration::days(1);
    }
    Ok(synced)
}

/// The day a sync without `--from` starts: the newest stored trip's day
/// (trips later that day may be missing from an export), else 1 January of
/// `today`'s year.
pub fn sync_start(store: &Path, tz: Tz, today: NaiveDate) -> Result<NaiveDate> {
    Ok(TripStore::load(store)?
        .trips
        .iter()
        .map(|t| t.start.with_timezone(&tz).date_naive())
        .max()
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap()))
}

/// The stored trips, for commands run without `-t`. An empty or missing
/// store is an error naming `trips import`.
pub fn stored_trips(store: &Path, tz: Tz) -> Result<Vec<Trip>> {
    let trips = TripStore::load(store)?.trips(tz);
    if trips.is_empty() {
        bail!(
            "no stored trips in {}; run `trips import <export.xlsx>` or pass -t <file>",
            store.display()
        );
    }
    Ok(trips)
}

impl TripStore {
    /// Reads the store; a missing file is an empty store.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(e).with_context(|| format!("reading trips {}", path.display()));
            }
        };
        serde_json::from_str(&text).with_context(|| {
            format!(
                "parsing trips {} (fix or remove the file; it was not changed)",
                path.display()
            )
        })
    }

    /// The stored trips in `tz`, sorted by start, ready for the reports.
    pub fn trips(&self, tz: Tz) -> Vec<Trip> {
        let mut trips: Vec<Trip> = self
            .trips
            .iter()
            .map(|t| Trip {
                start: t.start.with_timezone(&tz),
                end: t.end.with_timezone(&tz),
                distance_km: t.distance_km,
                from: t.from.clone(),
                to: t.to.clone(),
                purpose: t.purpose.clone(),
                business: t.business,
                start_pos: t.start_pos,
                end_pos: t.end_pos,
            })
            .collect();
        trips.sort_by_key(|t| t.start);
        trips
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        crate::state::write_json(path, self)
            .with_context(|| format!("writing trips {}", path.display()))
    }

    /// Adds the trips not stored yet, keeping the store sorted by start.
    pub fn merge(&mut self, imported: &[ExportTrip]) -> Merged {
        let mut added = 0;
        for e in imported {
            let t = StoredTrip::from(e);
            if !self.trips.iter().any(|s| s.same_trip(&t)) {
                self.trips.push(t);
                added += 1;
            }
        }
        self.trips.sort_by_key(|t| t.start);
        Merged {
            added,
            already: imported.len() - added,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trips::tests::{HEADER, ROWS, temp_dir, write_export};

    const TZ: Tz = chrono_tz::Europe::Copenhagen;

    #[test]
    fn overlapping_exports_add_each_trip_once() {
        let dir = temp_dir("store-merge");
        let store = dir.join("trips.json");
        let (a, b) = (dir.join("a.xlsx"), dir.join("b.xlsx"));
        write_export(&a, &HEADER, &ROWS[..2]);
        write_export(&b, &HEADER, &ROWS[1..]);

        let (merged, period) = import(&store, &a, TZ).unwrap();
        assert_eq!(
            merged,
            Merged {
                added: 2,
                already: 0
            }
        );
        let day = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(period, Some((day("2026-08-01"), day("2026-08-02"))));
        let (merged, _) = import(&store, &b, TZ).unwrap();
        assert_eq!(
            merged,
            Merged {
                added: 1,
                already: 1
            }
        );
        let (merged, _) = import(&store, &b, TZ).unwrap();
        assert_eq!(
            merged,
            Merged {
                added: 0,
                already: 2
            }
        );

        let stored = TripStore::load(&store).unwrap().trips;
        assert_eq!(stored.len(), 3);
        assert!(stored.windows(2).all(|w| w[0].start < w[1].start));
        let direct = crate::trips::load(&b, TZ).unwrap();
        let trips = stored_trips(&store, TZ).unwrap();
        assert_eq!(&trips[1..], &direct[..]);
        let text = std::fs::read_to_string(&store).unwrap();
        assert!(!text.contains("Test Testesen") && !text.contains("AB 12 345"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_stored_trips_names_trips_import() {
        let dir = temp_dir("store-none");
        let err = stored_trips(&dir.join("trips.json"), TZ).unwrap_err();
        assert!(err.to_string().contains("trips import"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn trip(day: u32, hour: u32, min: u32, odometer: f64) -> ExportTrip {
        use chrono::TimeZone;
        let start = TZ.with_ymd_and_hms(2026, 9, day, hour, min, 0).unwrap();
        ExportTrip {
            trip: Trip {
                start,
                end: start + chrono::Duration::minutes(20),
                distance_km: Some(5.0),
                from: "A".into(),
                to: "B".into(),
                purpose: String::new(),
                business: None,
                start_pos: None,
                end_pos: None,
            },
            start_odometer: Some(odometer),
        }
    }

    #[test]
    fn a_failed_month_leaves_the_store_unchanged_for_that_month() {
        let dir = temp_dir("store-sync-fail");
        let store = dir.join("trips.json");
        let day = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let mut asked = Vec::new();
        let err = sync(
            &store,
            day("2026-09-20"),
            day("2026-11-05"),
            TZ,
            |first, last| {
                asked.push((first, last));
                match first.month() {
                    9 => Ok(vec![trip(21, 8, 0, 100.0), trip(30, 23, 50, 200.0)]),
                    _ => Err(anyhow::anyhow!("Connected Cars answered HTTP 500")),
                }
            },
        )
        .unwrap_err();
        assert_eq!(
            asked,
            [
                (day("2026-09-20"), day("2026-09-30")),
                (day("2026-10-01"), day("2026-10-31"))
            ],
            "the sync stops at the failed month"
        );
        assert!(
            format!("{err:#}").contains("syncing 2026-10 stopped; nothing from 2026-10-01 on was stored: Connected Cars answered HTTP 500"),
            "{err:#}"
        );
        let stored = TripStore::load(&store).unwrap().trips;
        assert_eq!(stored.len(), 2, "September is stored, October is not");
        assert!(stored.iter().all(|t| t.start.month() == 9));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_bad_row_imports_nothing() {
        let dir = temp_dir("store-bad");
        let store = dir.join("trips.json");
        let export = dir.join("export.xlsx");
        let mut rows = ROWS;
        rows[2].1 = "03/08/2026 07:00:00 CEST";
        write_export(&export, &HEADER, &rows);
        assert!(import(&store, &export, TZ).is_err());
        assert!(!store.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bytes_of_any_trips_file_import_and_bad_rows_are_named() {
        let dir = temp_dir("store-bytes");
        let store = dir.join("trips.json");
        let export = dir.join("export.xlsx");
        write_export(&export, &HEADER, &ROWS);
        let bytes = std::fs::read(&export).unwrap();
        // The Android picker may give a name without an extension.
        let (merged, _) = import_bytes(&store, "document:1234", &bytes, TZ).unwrap();
        assert_eq!(merged.added, 3);

        let csv = "start,end,distance_km,from,to,purpose,business\n\
                   2026-10-06 07:45,2026-10-06 09:05,98.0,Hjem,Kunde A,Service Kunde A,\n";
        let (merged, _) = import_bytes(&store, "trips.csv", csv.as_bytes(), TZ).unwrap();
        assert_eq!((merged.added, merged.already), (1, 0));
        let (merged, _) = import_bytes(&store, "trips.csv", csv.as_bytes(), TZ).unwrap();
        assert_eq!((merged.added, merged.already), (0, 1));
        let stored = stored_trips(&store, TZ).unwrap();
        assert_eq!(stored[3].purpose, "Service Kunde A", "purpose is kept");
        let text = std::fs::read_to_string(&store).unwrap();
        assert_eq!(text.matches("\"purpose\"").count(), 1, "{text}");

        let bad_csv =
            "start,end\n2026-10-06 07:45,2026-10-06 09:05\n2026-10-07 09:00,2026-10-07 08:00\n";
        let json = r#"[{"start": "2026-10-08 08:00", "end": "2026-10-08 08:30"}, {"start": "8 Oct", "end": "2026-10-08 09:00"}]"#;
        for (name, bytes, want) in [
            (
                "bad.csv",
                bad_csv.as_bytes(),
                "bad.csv: row 3: trip ends before it starts",
            ),
            (
                "bad.json",
                json.as_bytes(),
                "bad.json: trip 2: unrecognised time \"8 Oct\"",
            ),
            (
                "notes.txt",
                b"hello".as_slice(),
                "notes.txt is not a Min Volkswagen export",
            ),
        ] {
            let err = format!("{:#}", import_bytes(&store, name, bytes, TZ).unwrap_err());
            assert!(err.starts_with(want), "{err}");
        }
        assert_eq!(
            TripStore::load(&store).unwrap().trips.len(),
            4,
            "nothing added"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
