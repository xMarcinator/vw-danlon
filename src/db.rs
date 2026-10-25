//! The database beside the config file (`vw-danlon.db`): the trips, the
//! local state and the calendar cache, shared by the CLI and the app.
//!
//! It is created on first use. A `trips.json` and `state.json` found beside
//! it (the files earlier versions wrote) are moved in then: the database is
//! built under a temporary name, read back and compared with the files,
//! and only then put in place and the JSON files deleted. If anything
//! fails, the JSON files are left as they were and no database is created.
//! Trips of 0 km are not moved in.
//!
//! A database that cannot be read stops every command with its path; it is
//! never overwritten.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

/// The database's file name, beside the config file.
pub const FILE_NAME: &str = "vw-danlon.db";

/// The schema version, kept in `PRAGMA user_version`. Version 2 added the
/// home-working timer ([`HOME_SCHEMA`]), version 3 the trip overrides
/// ([`OVERRIDE_SCHEMA`]).
const VERSION: i32 = 3;

const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE trips (
    id INTEGER PRIMARY KEY,
    start TEXT NOT NULL,
    start_ts INTEGER NOT NULL,
    end TEXT NOT NULL,
    start_odometer REAL,
    distance_km REAL,
    from_place TEXT NOT NULL,
    to_place TEXT NOT NULL,
    start_lat REAL,
    start_lon REAL,
    end_lat REAL,
    end_lon REAL,
    purpose TEXT NOT NULL,
    business INTEGER,
    manual INTEGER NOT NULL
);
CREATE INDEX trips_start ON trips (start_ts);
CREATE TABLE lunch (day TEXT PRIMARY KEY, start TEXT NOT NULL, minutes INTEGER NOT NULL);
CREATE TABLE entered (day TEXT PRIMARY KEY);
CREATE TABLE private_intervals (
    id INTEGER PRIMARY KEY,
    start TEXT NOT NULL,
    end TEXT NOT NULL,
    label TEXT NOT NULL,
    replaces_title TEXT,
    replaces_start TEXT,
    replaces_end TEXT
);
CREATE TABLE private_running (
    one INTEGER PRIMARY KEY CHECK (one = 1),
    start TEXT NOT NULL,
    label TEXT NOT NULL,
    replaces_title TEXT,
    replaces_start TEXT,
    replaces_end TEXT
);
CREATE TABLE account (
    one INTEGER PRIMARY KEY CHECK (one = 1),
    email TEXT NOT NULL,
    last_sync TEXT
);
CREATE TABLE calendar_cache (
    source TEXT PRIMARY KEY,
    body TEXT NOT NULL,
    fetched_at TEXT NOT NULL
);
";

/// The tables version 2 added; a version 1 database gets them when it is
/// opened for writing.
const HOME_SCHEMA: &str = "
CREATE TABLE home_intervals (
    id INTEGER PRIMARY KEY,
    start TEXT NOT NULL,
    end TEXT NOT NULL,
    label TEXT NOT NULL
);
CREATE TABLE home_running (
    one INTEGER PRIMARY KEY CHECK (one = 1),
    start TEXT NOT NULL,
    label TEXT NOT NULL
);
";

/// The table version 3 added: trips set to business or private by hand,
/// by their start to the minute.
const OVERRIDE_SCHEMA: &str = "
CREATE TABLE trip_overrides (start TEXT PRIMARY KEY, business INTEGER NOT NULL);
";

/// The JSON files earlier versions kept beside the config file.
const TRIPS_JSON: &str = "trips.json";
const STATE_JSON: &str = "state.json";

fn unreadable(path: &Path) -> String {
    format!(
        "the database {} can't be read (fix or remove it; it was not changed)",
        path.display()
    )
}

/// Opens the database at `path`, creating it (and moving in the JSON files
/// beside it) on first use.
pub fn open(path: &Path) -> Result<Connection> {
    if !path.exists() {
        create(path)?;
    }
    let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .with_context(|| unreadable(path))?;
    if check_version(&conn, path)? < VERSION {
        upgrade(&mut conn, path)?;
    }
    Ok(conn)
}

/// Brings an older database up to [`VERSION`] in one transaction; another
/// process may have done it meanwhile.
fn upgrade(conn: &mut Connection, path: &Path) -> Result<()> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .with_context(|| unreadable(path))?;
    let version: i32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version == 1 {
        tx.execute_batch(HOME_SCHEMA)?;
    }
    if version <= 2 {
        tx.execute_batch(OVERRIDE_SCHEMA)?;
    }
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.commit()
        .with_context(|| format!("upgrading the database {}", path.display()))
}

/// Whether the database has the table `name`.
pub(crate) fn has_table(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [name],
        |r| r.get(0),
    )
}

/// Opens an existing database read-only, without creating or migrating
/// anything (the CLI folder the app imports from).
pub fn open_read_only(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| unreadable(path))?;
    check_version(&conn, path)?;
    Ok(conn)
}

/// The database's schema version: [`VERSION`] or an older one.
fn check_version(conn: &Connection, path: &Path) -> Result<i32> {
    conn.busy_timeout(Duration::from_secs(10))?;
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .with_context(|| unreadable(path))?;
    match version {
        1..=VERSION => Ok(version),
        v if v > VERSION => bail!(
            "the database {} was written by a newer vw-danlon (schema {v}); it was not changed",
            path.display()
        ),
        _ => bail!(
            "{} is not a vw-danlon database (fix or remove it; it was not changed)",
            path.display()
        ),
    }
}

/// Runs `f` in one write transaction: everything it changes is stored, or
/// nothing when it fails.
pub fn write<T>(path: &Path, f: impl FnOnce(&rusqlite::Transaction) -> Result<T>) -> Result<T> {
    let mut conn = open(path)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .with_context(|| unreadable(path))?;
    let out = f(&tx)?;
    tx.commit()
        .with_context(|| format!("writing the database {}", path.display()))?;
    Ok(out)
}

/// When and from which files the JSON data was moved in, if it was.
#[derive(Debug, Clone, PartialEq)]
pub struct Migrated {
    pub at: DateTime<Utc>,
    pub files: Vec<String>,
}

pub fn migrated(path: &Path) -> Result<Option<Migrated>> {
    let conn = open(path)?;
    let get = |key: &str| -> Result<Option<String>> {
        conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .with_context(|| unreadable(path))
    };
    let (Some(at), Some(files)) = (get("migrated_at")?, get("migrated_from")?) else {
        return Ok(None);
    };
    Ok(Some(Migrated {
        at: DateTime::parse_from_rfc3339(&at)
            .with_context(|| unreadable(path))?
            .with_timezone(&Utc),
        files: files.split(',').map(str::to_string).collect(),
    }))
}

/// Builds the database under a temporary name, with the JSON data beside
/// it, checks it and puts it in place; then deletes the JSON files.
fn create(path: &Path) -> Result<()> {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".new");
    let tmp = PathBuf::from(tmp);
    // A leftover from a run that stopped half-way holds nothing of value:
    // the JSON files were not deleted then.
    let _ = std::fs::remove_file(&tmp);
    let built = build(path, &tmp);
    let moved = match built {
        Ok(moved) => moved,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };
    if path.exists() {
        // Another process created it meanwhile, from the same files.
        let _ = std::fs::remove_file(&tmp);
        return Ok(());
    }
    std::fs::rename(&tmp, path).with_context(|| format!("creating {}", path.display()))?;
    for file in moved {
        std::fs::remove_file(&file).with_context(|| {
            format!(
                "{} was moved into {} but can't be deleted; delete it yourself",
                file.display(),
                path.display()
            )
        })?;
    }
    Ok(())
}

/// Writes the new database to `tmp` and returns the JSON files moved in,
/// after reading them back from it.
fn build(path: &Path, tmp: &Path) -> Result<Vec<PathBuf>> {
    let trips_json = path.with_file_name(TRIPS_JSON);
    let state_json = path.with_file_name(STATE_JSON);
    let mut trips = crate::trip_store::read_json(&trips_json)?;
    if let Some(t) = &mut trips {
        t.trips
            .retain(|t| !crate::trip_store::zero_km(t.distance_km));
    }
    let state = crate::state::read_json(&state_json)?;

    let mut conn = Connection::open(tmp).with_context(|| format!("creating {}", tmp.display()))?;
    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    tx.execute_batch(HOME_SCHEMA)?;
    tx.execute_batch(OVERRIDE_SCHEMA)?;
    let mut moved = Vec::new();
    if let Some(t) = &trips {
        crate::trip_store::insert(&tx, &t.trips)?;
        moved.push(trips_json);
    }
    if let Some(s) = &state {
        crate::state::write_rows(&tx, s)?;
        moved.push(state_json);
    }
    if !moved.is_empty() {
        let names: Vec<String> = moved
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('migrated_at', ?1), ('migrated_from', ?2)",
            params![Utc::now().to_rfc3339(), names.join(",")],
        )?;
    }
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.commit()
        .with_context(|| format!("creating {}", tmp.display()))?;
    drop(conn);

    // Read back what was written: it must equal what the files hold.
    let conn = open_read_only(tmp)?;
    if let Some(t) = &trips
        && crate::trip_store::read_all(&conn)? != *t
    {
        bail!(
            "the trips read back from {} differ from {}",
            tmp.display(),
            TRIPS_JSON
        );
    }
    if let Some(s) = &state
        && crate::state::read_rows(&conn)? != *s
    {
        bail!(
            "the state read back from {} differs from {}",
            tmp.display(),
            STATE_JSON
        );
    }
    Ok(moved)
}

/// A stable short key for a calendar source, so the link (which holds the
/// key to the calendar) is not kept in the database: FNV-1a, 64 bits.
fn source_key(source: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in source.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The calendar last fetched from `source`, and when.
pub fn cached_calendar(path: &Path, source: &str) -> Result<Option<(String, DateTime<Utc>)>> {
    let conn = open(path)?;
    conn.query_row(
        "SELECT body, fetched_at FROM calendar_cache WHERE source = ?1",
        [source_key(source)],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .with_context(|| unreadable(path))
}

/// Keeps `body` as the calendar fetched from `source` at `fetched_at`; true
/// when it differs from the copy kept before.
pub fn store_calendar(
    path: &Path,
    source: &str,
    body: &str,
    fetched_at: DateTime<Utc>,
) -> Result<bool> {
    write(path, |tx| {
        let key = source_key(source);
        let old: Option<String> = tx
            .query_row(
                "SELECT body FROM calendar_cache WHERE source = ?1",
                [&key],
                |r| r.get(0),
            )
            .optional()?;
        tx.execute(
            "INSERT OR REPLACE INTO calendar_cache (source, body, fetched_at) VALUES (?1, ?2, ?3)",
            params![key, body, fetched_at],
        )?;
        Ok(old.as_deref() != Some(body))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;
    use crate::trip_store::TripStore;
    use crate::trips::tests::temp_dir;

    const TRIPS: &str = r#"{
  "trips": [
    {"start": "2026-10-05T07:32:00+02:00", "end": "2026-10-05T08:05:00+02:00", "start_odometer": 1000.5,
     "distance_km": 24.1, "from": "Hjem", "to": "Kontor", "start_pos": {"lat": 56.18901234567, "lon": 10.182},
     "end_pos": null},
    {"start": "2026-10-05T12:00:00+02:00", "end": "2026-10-05T12:01:00+02:00", "start_odometer": 1024.6,
     "distance_km": 0.0, "from": "Kontor", "to": "Kontor", "start_pos": null, "end_pos": null},
    {"start": "2026-10-26T16:10:00+01:00", "end": "2026-10-26T16:48:00+01:00", "start_odometer": null,
     "distance_km": 24.3, "from": "Kontor", "to": "Hjem", "start_pos": null,
     "end_pos": {"lat": 56.1, "lon": 10.2}, "purpose": "Service", "business": true, "manual": true},
    {"start": "2026-11-02T07:00:00.123456789+01:00", "end": "2026-11-02T07:30:00+01:00", "start_odometer": null,
     "distance_km": null, "from": "A", "to": "B", "start_pos": null, "end_pos": null, "business": false}
  ]
}"#;

    const STATE: &str = r#"{
  "lunch": {"2026-10-07": {"start": "11:30:00", "minutes": 45}, "2026-11-04": {"start": "12:00:00", "minutes": 0}},
  "private": {
    "running": {"start": "2026-10-20T08:00:00", "label": "Glemt", "replaces": null},
    "intervals": [
      {"id": 1, "start": "2026-10-08T13:00:00", "end": "2026-10-08T14:30:00", "label": "Læge",
       "replaces": {"title": "Læge", "start": "2026-10-08T13:00:00", "end": "2026-10-08T14:00:00"}},
      {"id": 3, "start": "2026-10-09T23:00:00", "end": "2026-10-10T01:00:00", "label": "Privat", "replaces": null}
    ],
    "next_id": 4
  },
  "entered": ["2026-10-05", "2026-10-06"],
  "account": {"email": "a@example.com", "last_sync": "2026-10-24T18:00:00+00:00"}
}"#;

    #[test]
    fn json_moves_in_losslessly_without_0_km_trips_and_is_deleted() {
        let dir = temp_dir("db-migrate");
        std::fs::write(dir.join(TRIPS_JSON), TRIPS).unwrap();
        std::fs::write(dir.join(STATE_JSON), STATE).unwrap();
        let mut want_trips: TripStore = serde_json::from_str(TRIPS).unwrap();
        want_trips.trips.remove(1);
        let want_state: State = serde_json::from_str(STATE).unwrap();
        let db = dir.join(FILE_NAME);

        assert_eq!(TripStore::load(&db).unwrap(), want_trips);
        assert_eq!(State::load(&db).unwrap(), want_state);
        assert!(!dir.join(TRIPS_JSON).exists() && !dir.join(STATE_JSON).exists());
        assert!(!dir.join("vw-danlon.db.new").exists());
        let m = migrated(&db).unwrap().unwrap();
        assert_eq!(m.files, ["trips.json", "state.json"]);

        // A JSON file appearing later is not moved in again.
        std::fs::write(dir.join(TRIPS_JSON), r#"{"trips": []}"#).unwrap();
        assert_eq!(TripStore::load(&db).unwrap(), want_trips);
        assert!(dir.join(TRIPS_JSON).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_migration_leaves_the_json_and_creates_no_database() {
        let dir = temp_dir("db-migrate-fail");
        std::fs::write(dir.join(TRIPS_JSON), TRIPS).unwrap();
        let bad = r#"{"lunch": {"2026-10-07": "noon"}}"#;
        std::fs::write(dir.join(STATE_JSON), bad).unwrap();
        let db = dir.join(FILE_NAME);
        let err = format!("{:#}", State::load(&db).unwrap_err());
        assert!(
            err.contains("parsing state") && err.contains("state.json"),
            "{err}"
        );
        assert!(!db.exists() && !dir.join("vw-danlon.db.new").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join(TRIPS_JSON)).unwrap(),
            TRIPS
        );
        assert_eq!(std::fs::read_to_string(dir.join(STATE_JSON)).unwrap(), bad);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_damaged_database_is_named_and_not_changed() {
        let dir = temp_dir("db-damaged");
        let db = dir.join(FILE_NAME);
        let junk = b"this is not a database, but it is the user's file".repeat(100);
        std::fs::write(&db, &junk).unwrap();
        let err = format!("{:#}", State::load(&db).unwrap_err());
        assert!(
            err.contains(&db.display().to_string()) && err.contains("it was not changed"),
            "{err}"
        );
        assert!(State::update(&db, |s| s.entered.clear()).is_err());
        assert_eq!(std::fs::read(&db).unwrap(), junk);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_version_1_database_gets_the_home_and_override_tables() {
        let dir = temp_dir("db-upgrade");
        let db = dir.join(FILE_NAME);
        State::update(&db, |s| s.entered.insert("2026-10-05".parse().unwrap())).unwrap();
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "DROP TABLE home_intervals; DROP TABLE home_running; DROP TABLE trip_overrides; \
             PRAGMA user_version = 1;",
        )
        .unwrap();
        drop(conn);

        // Read-only (the CLI folder the app imports from): not upgraded.
        let conn = open_read_only(&db).unwrap();
        let s = crate::state::read_rows(&conn).unwrap();
        assert_eq!((s.entered.len(), s.home.intervals.len()), (1, 0));
        assert!(!has_table(&conn, "home_intervals").unwrap());
        assert!(s.trip_overrides.is_empty());
        drop(conn);

        let trip_start: chrono::DateTime<chrono::FixedOffset> =
            "2026-10-06T07:45:00+02:00".parse().unwrap();
        State::update(&db, |s| {
            s.trip_overrides.insert(trip_start, true);
            s.home
                .start("2026-10-06T08:00:00".parse().unwrap(), None, None)
                .map(|_| ())
        })
        .unwrap()
        .unwrap();
        let s = State::load(&db).unwrap();
        assert_eq!((s.entered.len(), s.home.running.is_some()), (1, true));
        assert_eq!(s.trip_overrides.get(&trip_start), Some(&true));
        let conn = open(&db).unwrap();
        let v: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, VERSION);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn calendar_cache_keeps_the_last_copy_without_the_link() {
        let dir = temp_dir("db-calendar");
        let db = dir.join(FILE_NAME);
        let link = "https://calendar.proton.me/api/calendar/v1/url/SECRET/calendar.ics";
        assert_eq!(cached_calendar(&db, link).unwrap(), None);
        let t = DateTime::parse_from_rfc3339("2026-10-25T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(store_calendar(&db, link, "BEGIN:VCALENDAR", t).unwrap());
        assert!(!store_calendar(&db, link, "BEGIN:VCALENDAR", t).unwrap());
        assert_eq!(
            cached_calendar(&db, link).unwrap(),
            Some(("BEGIN:VCALENDAR".to_string(), t))
        );
        assert_eq!(cached_calendar(&db, "https://other").unwrap(), None);
        let bytes = std::fs::read(&db).unwrap();
        assert!(!bytes.windows(6).any(|w| w == b"SECRET"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
