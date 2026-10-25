//! Trips imported from Min Volkswagen exports, merged in one JSON file
//! beside the config. A trip is identified by its start time and start
//! odometer reading, so overlapping exports add each trip once. Driver name
//! and licence plate are never stored.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, NaiveDate};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::trips::{ExportTrip, Position};

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
        let text = std::fs::read_to_string(&store).unwrap();
        assert!(!text.contains("Test Testesen") && !text.contains("AB 12 345"));
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
