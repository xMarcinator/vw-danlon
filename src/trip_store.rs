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
        }
    }
}

/// What an import changed.
#[derive(Debug, PartialEq, Eq)]
pub struct Merged {
    pub added: usize,
    pub already: usize,
}

/// Reads the whole export, then merges it into the store at `store`. A bad
/// row fails before the store is touched, so nothing is imported. Returns
/// the counts and the first and last trip day of the export.
pub fn import(
    store: &Path,
    export: &Path,
    tz: Tz,
) -> Result<(Merged, Option<(NaiveDate, NaiveDate)>)> {
    let imported = crate::trips::read_export(export, tz)?;
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
                purpose: String::new(),
                business: None,
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
}
