//! Trips imported from Min Volkswagen exports or synced from the account,
//! merged in the database beside the config (see [`crate::db`]), and trips
//! added by hand. A trip is identified by its start time and start odometer
//! reading, so overlapping exports add each trip once; a trip added by hand
//! has no odometer and is identified by its start minute, so a later import
//! of the same trip does not double it. Trips of 0 km (the engine only
//! turned on) are not stored. Driver name and licence plate are never
//! stored.

use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate};
use chrono_tz::Tz;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::db;
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
    /// Added by hand in the app; only these can be deleted.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub manual: bool,
}

impl StoredTrip {
    fn same_trip(&self, other: &StoredTrip) -> bool {
        if self.manual || other.manual {
            return same_minute(self.start, other.start);
        }
        self.start == other.start && self.start_odometer == other.start_odometer
    }
}

fn same_minute(a: DateTime<FixedOffset>, b: DateTime<FixedOffset>) -> bool {
    a.timestamp().div_euclid(60) == b.timestamp().div_euclid(60)
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
            manual: false,
        }
    }
}

/// Whether a trip of `distance_km` is 0 km: the engine was only turned on.
/// Such trips are not stored.
pub fn zero_km(distance_km: Option<f64>) -> bool {
    distance_km == Some(0.0)
}

/// Adds `trip` to the store in the database `db` as a trip added by hand.
/// Refused when a stored trip starts the same minute, the trip ends before
/// it starts, or it is 0 km.
pub fn add_manual(db: &Path, trip: &Trip) -> Result<()> {
    if trip.end <= trip.start {
        bail!(
            "the trip ends {} before it starts {}",
            trip.end.format("%H:%M"),
            trip.start.format("%H:%M")
        );
    }
    if trip.from.trim().is_empty() || trip.to.trim().is_empty() {
        bail!("from and to are needed");
    }
    if trip.distance_km.is_some_and(|k| k.is_nan() || k < 0.0) {
        bail!("km must be 0 or more");
    }
    if zero_km(trip.distance_km) {
        bail!("a 0 km trip is not stored: the engine was only turned on");
    }
    let mut new = StoredTrip::from(&ExportTrip {
        trip: trip.clone(),
        start_odometer: None,
    });
    new.manual = true;
    db::write(db, |tx| {
        let start = new.start.timestamp().div_euclid(60) * 60;
        let near = read_where(
            tx,
            "start_ts >= ?1 AND start_ts < ?2",
            params![start, start + 60],
        )?;
        if let Some(s) = near.iter().find(|s| s.same_trip(&new)) {
            bail!(
                "a trip starting {} is already stored ({} → {})",
                s.start.format("%Y-%m-%d %H:%M"),
                s.from,
                s.to
            );
        }
        insert(tx, std::slice::from_ref(&new))?;
        Ok(())
    })
}

/// Deletes the trip added by hand that starts at `start`'s minute and
/// returns it. Imported and synced trips are refused.
pub fn delete_manual(db: &Path, start: DateTime<FixedOffset>) -> Result<StoredTrip> {
    db::write(db, |tx| {
        let at = start.format("%Y-%m-%d %H:%M");
        let minute = start.timestamp().div_euclid(60) * 60;
        let mut st = tx.prepare(&format!(
            "SELECT id, {COLUMNS} FROM trips WHERE start_ts >= ?1 AND start_ts < ?2 ORDER BY id"
        ))?;
        let found: Vec<(i64, StoredTrip)> = st
            .query_map(params![minute, minute + 60], |r| {
                Ok((r.get(0)?, row(r, 1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let Some((id, trip)) = found.iter().find(|(_, t)| t.manual).cloned() else {
            if !found.is_empty() {
                bail!(
                    "the trip starting {at} was imported; only trips added by hand can be deleted"
                );
            }
            bail!("no trip starting {at} is stored");
        };
        tx.execute("DELETE FROM trips WHERE id = ?1", [id])?;
        Ok(trip)
    })
}

/// What an import changed.
#[derive(Debug, PartialEq, Eq)]
pub struct Merged {
    pub added: usize,
    pub already: usize,
    /// Trips of 0 km, not stored.
    pub zero_km: usize,
}

impl Merged {
    /// ", 2 skipped (0 km)", or nothing when none were.
    pub fn zero_km_text(&self) -> String {
        if self.zero_km == 0 {
            String::new()
        } else {
            format!(", {} skipped (0 km)", self.zero_km)
        }
    }
}

/// What an import read and changed: the counts, and the first and last
/// trip day of the file.
pub type Imported = (Merged, Option<(NaiveDate, NaiveDate)>);

/// Reads the whole file (a Min Volkswagen .xlsx export, or a CSV/JSON trips
/// file), then merges it into the store in the database `db`. A bad row
/// fails before the store is touched, so nothing is imported.
pub fn import(db: &Path, file: &Path, tz: Tz) -> Result<Imported> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    import_bytes(db, &file.display().to_string(), &bytes, tz)
}

/// [`import`] of a file's bytes; `name` (its file name) picks the format by
/// its extension and names the file in errors.
pub fn import_bytes(db: &Path, name: &str, bytes: &[u8], tz: Tz) -> Result<Imported> {
    let imported = read_trips_file(name, bytes, tz)?;
    let merged = merge_into(db, &imported)?;
    let period = imported
        .first()
        .zip(imported.last())
        .map(|(f, l)| (f.trip.start.date_naive(), l.trip.start.date_naive()));
    Ok((merged, period))
}

/// Adds the trips of `imported` not stored yet, in one transaction.
fn merge_into(db: &Path, imported: &[ExportTrip]) -> Result<Merged> {
    db::write(db, |tx| {
        let Some((first, last)) = imported
            .iter()
            .map(|e| e.trip.start.timestamp())
            .fold(None, |r: Option<(i64, i64)>, t| {
                Some(r.map_or((t, t), |(a, b)| (a.min(t), b.max(t))))
            })
        else {
            return Ok(Merged {
                added: 0,
                already: 0,
                zero_km: 0,
            });
        };
        // Only stored trips starting in the same span (to the minute) can
        // be the same trip.
        let mut store = TripStore {
            trips: read_where(
                tx,
                "start_ts >= ?1 AND start_ts < ?2",
                params![first.div_euclid(60) * 60, (last.div_euclid(60) + 1) * 60],
            )?,
        };
        let before = store.trips.len();
        let merged = store.merge(imported);
        insert(tx, &store.trips[before..])?;
        Ok(merged)
    })
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
/// `fetch(first_day, last_day)` and merges each month into the store in
/// `db` once it has been fetched whole. A failed month stops the sync:
/// the months before it stay stored, it and the later ones are not touched.
/// Trips a fetch returns that start outside its days are left to the month
/// they start in, so a trip across midnight is counted once.
pub fn sync(
    db: &Path,
    from: NaiveDate,
    to: NaiveDate,
    tz: Tz,
    mut fetch: impl FnMut(NaiveDate, NaiveDate) -> Result<Vec<ExportTrip>>,
) -> Result<Synced> {
    let mut synced = Synced {
        merged: Merged {
            added: 0,
            already: 0,
            zero_km: 0,
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
        let merged = merge_into(db, &fetched)?;
        synced.merged.added += merged.added;
        synced.merged.already += merged.already;
        synced.merged.zero_km += merged.zero_km;
        synced.without_position += crate::connected_cars::without_position(&fetched);
        first = last + Duration::days(1);
    }
    Ok(synced)
}

/// The day a sync without `--from` starts: the newest stored trip's day
/// (trips later that day may be missing from an export), else 1 January of
/// `today`'s year.
pub fn sync_start(db: &Path, tz: Tz, today: NaiveDate) -> Result<NaiveDate> {
    Ok(summary(db, tz)?
        .last_start
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap()))
}

/// How many trips are stored and the days they span.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub count: usize,
    /// The first trip's start day.
    pub first: Option<NaiveDate>,
    /// The newest trip's start day, and the day it ended.
    pub last_start: Option<NaiveDate>,
    pub last_end: Option<NaiveDate>,
}

/// The stored trips' count and span, without reading them all.
pub fn summary(db: &Path, tz: Tz) -> Result<Summary> {
    let conn = db::open(db)?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM trips", [], |r| r.get(0))?;
    let first = read_where(&conn, "1 ORDER BY start_ts, id LIMIT 1", [])?;
    let last = read_where(&conn, "1 ORDER BY start_ts DESC, id DESC LIMIT 1", [])?;
    let day = |t: DateTime<FixedOffset>| t.with_timezone(&tz).date_naive();
    Ok(Summary {
        count: count as usize,
        first: first.first().map(|t| day(t.start)),
        last_start: last.first().map(|t| day(t.start)),
        last_end: last.first().map(|t| day(t.end)),
    })
}

/// The start times of the trips added by hand.
pub fn manual_starts(db: &Path) -> Result<Vec<DateTime<FixedOffset>>> {
    let conn = db::open(db)?;
    Ok(read_where(&conn, "manual = 1 ORDER BY start_ts, id", [])?
        .into_iter()
        .map(|t| t.start)
        .collect())
}

/// The stored trips, for commands run without `-t`. An empty store is an
/// error naming `trips import`.
pub fn stored_trips(db: &Path, tz: Tz) -> Result<Vec<Trip>> {
    let trips = TripStore::load(db)?.trips(tz);
    if trips.is_empty() {
        bail!(no_trips(db));
    }
    Ok(trips)
}

fn no_trips(db: &Path) -> String {
    format!(
        "no stored trips in {}; run `trips import <export.xlsx>` or pass -t <file>",
        db.display()
    )
}

/// The stored trips starting `from` through `to` (days in `tz`), reading
/// only those rows. An empty store is an error naming `trips import`.
pub fn stored_trips_between(
    db: &Path,
    tz: Tz,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<Trip>> {
    let conn = db::open(db)?;
    let midnight = |d: NaiveDate| {
        d.and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(tz)
            .earliest()
            .map_or(d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp(), |t| {
                t.timestamp()
            })
    };
    let trips = read_where(
        &conn,
        "start_ts >= ?1 AND start_ts < ?2 ORDER BY start_ts, id",
        params![midnight(from), midnight(to + Duration::days(1))],
    )?;
    if trips.is_empty() {
        let any: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM trips)", [], |r| r.get(0))?;
        if !any {
            bail!(no_trips(db));
        }
    }
    Ok(TripStore { trips }.trips(tz))
}

/// The trips in a JSON file an earlier version wrote; `None` when there
/// is no file.
pub(crate) fn read_json(path: &Path) -> Result<Option<TripStore>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(e).with_context(|| format!("reading trips {}", path.display()));
        }
    };
    serde_json::from_str(&text).map(Some).with_context(|| {
        format!(
            "parsing trips {} (fix or remove the file; it was not changed)",
            path.display()
        )
    })
}

const COLUMNS: &str = "start, end, start_odometer, distance_km, from_place, to_place, \
    start_lat, start_lon, end_lat, end_lon, purpose, business, manual";

fn position(lat: Option<f64>, lon: Option<f64>) -> Option<Position> {
    Some(Position {
        lat: lat?,
        lon: lon?,
    })
}

fn row(r: &rusqlite::Row, at: usize) -> rusqlite::Result<StoredTrip> {
    Ok(StoredTrip {
        start: r.get(at)?,
        end: r.get(at + 1)?,
        start_odometer: r.get(at + 2)?,
        distance_km: r.get(at + 3)?,
        from: r.get(at + 4)?,
        to: r.get(at + 5)?,
        start_pos: position(r.get(at + 6)?, r.get(at + 7)?),
        end_pos: position(r.get(at + 8)?, r.get(at + 9)?),
        purpose: r.get(at + 10)?,
        business: r.get(at + 11)?,
        manual: r.get(at + 12)?,
    })
}

/// The trips matching `filter` (an SQL condition, with any ORDER BY).
fn read_where(
    conn: &Connection,
    filter: &str,
    params: impl rusqlite::Params,
) -> rusqlite::Result<Vec<StoredTrip>> {
    let mut st = conn.prepare(&format!("SELECT {COLUMNS} FROM trips WHERE {filter}"))?;
    st.query_map(params, |r| row(r, 0))?.collect()
}

/// All stored trips, by start.
pub(crate) fn read_all(conn: &Connection) -> rusqlite::Result<TripStore> {
    Ok(TripStore {
        trips: read_where(conn, "1 ORDER BY start_ts, id", [])?,
    })
}

pub(crate) fn insert(conn: &Connection, trips: &[StoredTrip]) -> rusqlite::Result<()> {
    let mut st = conn.prepare(&format!(
        "INSERT INTO trips (start_ts, {COLUMNS}) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"
    ))?;
    for t in trips {
        st.execute(params![
            t.start.timestamp(),
            t.start,
            t.end,
            t.start_odometer,
            t.distance_km,
            t.from,
            t.to,
            t.start_pos.map(|p| p.lat),
            t.start_pos.map(|p| p.lon),
            t.end_pos.map(|p| p.lat),
            t.end_pos.map(|p| p.lon),
            t.purpose,
            t.business,
            t.manual,
        ])?;
    }
    Ok(())
}

impl TripStore {
    /// Reads all stored trips.
    pub fn load(db: &Path) -> Result<Self> {
        let conn = db::open(db)?;
        Ok(read_all(&conn)?)
    }

    /// Replaces all stored trips with these.
    pub(crate) fn replace(tx: &Connection, trips: &[StoredTrip]) -> rusqlite::Result<()> {
        tx.execute("DELETE FROM trips", [])?;
        insert(tx, trips)
    }

    /// The start times of the trips added by hand.
    pub fn manual_starts(&self) -> Vec<DateTime<FixedOffset>> {
        self.trips
            .iter()
            .filter(|t| t.manual)
            .map(|t| t.start)
            .collect()
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

    /// Adds the trips not stored yet, keeping the store sorted by start;
    /// 0 km trips are skipped.
    pub fn merge(&mut self, imported: &[ExportTrip]) -> Merged {
        let (mut added, mut zero) = (0, 0);
        let before = self.trips.len();
        for e in imported {
            if zero_km(e.trip.distance_km) {
                zero += 1;
                continue;
            }
            let t = StoredTrip::from(e);
            if !self.trips.iter().any(|s| s.same_trip(&t)) {
                self.trips.push(t);
                added += 1;
            }
        }
        self.trips[before..].sort_by_key(|t| t.start);
        Merged {
            added,
            already: imported.len() - added - zero,
            zero_km: zero,
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
        let store = dir.join(db::FILE_NAME);
        let (a, b) = (dir.join("a.xlsx"), dir.join("b.xlsx"));
        write_export(&a, &HEADER, &ROWS[..2]);
        write_export(&b, &HEADER, &ROWS[1..]);

        let (merged, period) = import(&store, &a, TZ).unwrap();
        assert_eq!(
            merged,
            Merged {
                added: 2,
                already: 0,
                zero_km: 0
            }
        );
        let day = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(period, Some((day("2026-08-01"), day("2026-08-02"))));
        let (merged, _) = import(&store, &b, TZ).unwrap();
        assert_eq!(
            merged,
            Merged {
                added: 1,
                already: 1,
                zero_km: 0
            }
        );
        let (merged, _) = import(&store, &b, TZ).unwrap();
        assert_eq!(
            merged,
            Merged {
                added: 0,
                already: 2,
                zero_km: 0
            }
        );

        let stored = TripStore::load(&store).unwrap().trips;
        assert_eq!(stored.len(), 3);
        assert!(stored.windows(2).all(|w| w[0].start < w[1].start));
        let direct = crate::trips::load(&b, TZ).unwrap();
        let trips = stored_trips(&store, TZ).unwrap();
        assert_eq!(&trips[1..], &direct[..]);
        let bytes = std::fs::read(&store).unwrap();
        let has = |s: &str| bytes.windows(s.len()).any(|w| w == s.as_bytes());
        assert!(!has("Test Testesen") && !has("AB 12 345"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_stored_trips_names_trips_import() {
        let dir = temp_dir("store-none");
        let err = stored_trips(&dir.join(db::FILE_NAME), TZ).unwrap_err();
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
        let store = dir.join(db::FILE_NAME);
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
        let store = dir.join(db::FILE_NAME);
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
        let store = dir.join(db::FILE_NAME);
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
        let purposes = stored.iter().filter(|t| !t.purpose.is_empty()).count();
        assert_eq!(purposes, 1, "only the CSV trip has a purpose");

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

    #[test]
    fn a_manual_trip_is_added_once_and_only_it_can_be_deleted() {
        let dir = temp_dir("store-manual");
        let store = dir.join(db::FILE_NAME);
        // An imported trip, then one added by hand.
        let (old, _) = import_bytes(
            &store,
            "trips.csv",
            b"start,end,distance_km,from,to\n2026-09-21 07:00,2026-09-21 07:30,10,Hjem,Kunde A\n",
            TZ,
        )
        .unwrap();
        assert_eq!(old.added, 1);
        assert!(manual_starts(&store).unwrap().is_empty());

        let mut manual = trip(22, 8, 0, 0.0).trip;
        manual.purpose = "Service Kunde B".into();
        add_manual(&store, &manual).unwrap();
        let err = add_manual(&store, &manual).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("a trip starting 2026-09-22 08:00 is already stored"),
            "{err}"
        );
        for (change, want) in [
            (
                (|t: &mut Trip| t.end = t.start) as fn(&mut Trip),
                "the trip ends 08:00 before it starts 08:00",
            ),
            (|t: &mut Trip| t.to = " ".into(), "from and to are needed"),
            (
                |t: &mut Trip| t.distance_km = Some(-1.0),
                "km must be 0 or more",
            ),
            (
                |t: &mut Trip| t.distance_km = Some(0.0),
                "a 0 km trip is not stored: the engine was only turned on",
            ),
        ] {
            let mut bad = trip(23, 8, 0, 0.0).trip;
            change(&mut bad);
            assert_eq!(add_manual(&store, &bad).unwrap_err().to_string(), want);
        }

        // The export later has the same trip, a few seconds off and with an
        // odometer reading: it is not added again.
        let mut exported = trip(22, 8, 0, 4321.0);
        exported.trip.start += chrono::Duration::seconds(20);
        let mut store_now = TripStore::load(&store).unwrap();
        assert_eq!(
            store_now.merge(&[exported]),
            Merged {
                added: 0,
                already: 1,
                zero_km: 0
            }
        );
        let stored = TripStore::load(&store).unwrap();
        assert_eq!(stored.trips.len(), 2);
        assert_eq!(stored.manual_starts(), [manual.start.fixed_offset()]);
        assert_eq!(stored.trips(TZ)[1].purpose, "Service Kunde B");

        let imported = stored.trips[0].start;
        let err = delete_manual(&store, imported).unwrap_err();
        assert!(
            err.to_string()
                .contains("only trips added by hand can be deleted"),
            "{err}"
        );
        let removed = delete_manual(&store, manual.start.fixed_offset()).unwrap();
        assert!(removed.manual);
        let err = delete_manual(&store, manual.start.fixed_offset()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "no trip starting 2026-09-22 08:00 is stored"
        );
        assert_eq!(TripStore::load(&store).unwrap().trips.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn zero_km_trips_are_skipped_on_import_and_sync() {
        let dir = temp_dir("store-zero");
        let store = dir.join(db::FILE_NAME);
        // Distance 0 without an odometer, and the export's odometer
        // difference 0 (start 1000.0, end 1000.0).
        let csv = "start,end,distance_km,from,to\n\
                   2026-10-06 07:45,2026-10-06 09:05,98.0,Hjem,Kunde A\n\
                   2026-10-06 12:00,2026-10-06 12:01,0,Kunde A,Kunde A\n";
        let (merged, _) = import_bytes(&store, "trips.csv", csv.as_bytes(), TZ).unwrap();
        assert_eq!(
            merged,
            Merged {
                added: 1,
                already: 0,
                zero_km: 1
            }
        );
        assert_eq!(merged.zero_km_text(), ", 1 skipped (0 km)");
        let export = dir.join("export.xlsx");
        let mut rows = ROWS;
        rows[2].8 = rows[2].7;
        write_export(&export, &HEADER, &rows);
        let (merged, _) = import(&store, &export, TZ).unwrap();
        assert_eq!((merged.added, merged.zero_km), (2, 1));

        let mut zero = trip(21, 8, 0, 300.0);
        zero.trip.distance_km = Some(0.0);
        let day = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let synced = sync(&store, day("2026-09-21"), day("2026-09-21"), TZ, |_, _| {
            Ok(vec![trip(21, 7, 0, 200.0), zero.clone()])
        })
        .unwrap();
        assert_eq!((synced.merged.added, synced.merged.zero_km), (1, 1));
        let stored = TripStore::load(&store).unwrap().trips;
        assert_eq!(stored.len(), 4);
        assert!(stored.iter().all(|t| t.distance_km != Some(0.0)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_month_reads_only_its_rows() {
        let dir = temp_dir("store-month");
        let store = dir.join(db::FILE_NAME);
        let csv = "start,end,distance_km,from,to\n\
                   2026-09-30 23:50,2026-10-01 00:20,10,A,B\n\
                   2026-10-01 00:10,2026-10-01 00:30,10,B,C\n\
                   2026-10-31 23:59,2026-11-01 00:20,10,C,D\n\
                   2026-11-01 00:00,2026-11-01 00:20,10,D,E\n";
        import_bytes(&store, "trips.csv", csv.as_bytes(), TZ).unwrap();
        let day = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let october =
            stored_trips_between(&store, TZ, day("2026-10-01"), day("2026-10-31")).unwrap();
        let to: Vec<&str> = october.iter().map(|t| t.to.as_str()).collect();
        assert_eq!(to, ["C", "D"]);
        assert!(
            stored_trips_between(&store, TZ, day("2027-01-01"), day("2027-01-31"))
                .unwrap()
                .is_empty()
        );
        let s = summary(&store, TZ).unwrap();
        assert_eq!(s.count, 4);
        assert_eq!(
            (s.first, s.last_start),
            (Some(day("2026-09-30")), Some(day("2026-11-01")))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
