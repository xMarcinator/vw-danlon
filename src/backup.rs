//! A backup file, to move the data to another device (the computer's data
//! to the phone): the config, the trips and the state in one JSON file.
//! The calendar link is taken out of the config and the device token is
//! never in the data, so both are entered again on the other device.
//!
//! Also moves the data of a folder the CLI used (`config.toml` with
//! `trips.json`/`state.json`, or its database) into the app's folder.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::db;
use crate::state::State;
use crate::trip_store::{self, TripStore};

/// What the `format` field of a backup file says.
pub const FORMAT: &str = "vw-danlon backup";
const VERSION: u32 = 1;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    pub format: String,
    pub version: u32,
    pub created: DateTime<Utc>,
    /// The config file's text.
    pub config: String,
    pub trips: TripStore,
    pub state: State,
}

impl Backup {
    fn new(config: String, trips: TripStore, state: State) -> Backup {
        Backup {
            format: FORMAT.into(),
            version: VERSION,
            created: Utc::now(),
            config,
            trips,
            state,
        }
    }

    /// The data in the database `db` with the config file at `config`,
    /// without the calendar link.
    pub fn export(config: &Path, db: &Path) -> Result<Backup> {
        let text = std::fs::read_to_string(config)
            .with_context(|| format!("reading config {}", config.display()))?;
        Ok(Backup::new(
            without_calendar_link(&text)?,
            TripStore::load(db)?,
            State::load(db)?,
        ))
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)? + "\n")
    }

    /// Reads a backup file's bytes; `name` names it in errors.
    pub fn parse(name: &str, bytes: &[u8]) -> Result<Backup> {
        #[derive(Deserialize)]
        struct Head {
            format: Option<String>,
            version: Option<u32>,
        }
        let head: Head = serde_json::from_slice(bytes)
            .ok()
            .filter(|h: &Head| h.format.as_deref() == Some(FORMAT))
            .with_context(|| format!("{name} is not a vw-danlon backup"))?;
        if head.version != Some(VERSION) {
            bail!("{name} is a backup from another vw-danlon version; it can't be read here");
        }
        serde_json::from_slice(bytes).with_context(|| format!("reading the backup {name}"))
    }

    /// The data of a folder the CLI used: `config.toml`, and the trips and
    /// state from its database or, before that existed, its `trips.json`
    /// and `state.json` (0 km trips left out). Nothing there is changed. A
    /// relative calendar file path is made absolute, so it still points
    /// there.
    pub fn from_folder(dir: &Path) -> Result<(Backup, FolderData)> {
        let config_path = dir.join("config.toml");
        let text = match std::fs::read_to_string(&config_path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                bail!("{} has no config.toml", dir.display())
            }
            Err(e) => {
                return Err(e).with_context(|| format!("reading {}", config_path.display()));
            }
        };
        let config = with_absolute_calendar_file(&text, dir)?;
        let db_path = dir.join(db::FILE_NAME);
        let mut data = FolderData::default();
        let (trips, state) = if db_path.exists() {
            let conn = db::open_read_only(&db_path)?;
            (
                trip_store::read_all(&conn)?,
                crate::state::read_rows(&conn)?,
            )
        } else {
            let (trips_json, state_json) = (dir.join("trips.json"), dir.join("state.json"));
            let mut trips = match trip_store::read_json(&trips_json)? {
                Some(t) => {
                    data.json_files.push(trips_json);
                    t
                }
                None => TripStore::default(),
            };
            let before = trips.trips.len();
            trips.trips.retain(|t| !trip_store::zero_km(t.distance_km));
            data.zero_km = before - trips.trips.len();
            let state = match crate::state::read_json(&state_json)? {
                Some(s) => {
                    data.json_files.push(state_json);
                    s
                }
                None => State::default(),
            };
            (trips, state)
        };
        Ok((Backup::new(config, trips, state), data))
    }

    /// Makes this the data in the folder `dir` (the app's): its
    /// `config.toml` and the trips and state in its database are replaced;
    /// the calendar cache stays. The data is read back and compared before
    /// the config is written.
    pub fn restore(&self, dir: &Path) -> Result<()> {
        self.config
            .parse::<toml_edit::DocumentMut>()
            .context("the backup's config can't be read")?;
        let db_path = dir.join(db::FILE_NAME);
        let config_path = dir.join("config.toml");
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        db::write(&db_path, |tx| {
            TripStore::replace(tx, &self.trips.trips)?;
            crate::state::write_rows(tx, &self.state)?;
            Ok(())
        })?;
        if TripStore::load(&db_path)? != self.trips || State::load(&db_path)? != self.state {
            bail!(
                "the data read back from {} differs from what was imported",
                db_path.display()
            );
        }
        let tmp = dir.join("config.toml.tmp");
        std::fs::write(&tmp, &self.config).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &config_path)
            .with_context(|| format!("writing {}", config_path.display()))
    }
}

/// Where a folder's data came from (see [`Backup::from_folder`]).
#[derive(Debug, Default, PartialEq)]
pub struct FolderData {
    /// The JSON files read; none when the folder has a database.
    pub json_files: Vec<PathBuf>,
    /// Trips of 0 km left out.
    pub zero_km: usize,
}

/// What [`import_folder`] did.
#[derive(Debug, PartialEq)]
pub struct FolderImport {
    pub trips: usize,
    pub zero_km: usize,
    /// The JSON files deleted from the folder.
    pub deleted: Vec<PathBuf>,
}

/// Moves the data of the CLI folder `from` into the app folder `to`
/// ([`Backup::from_folder`], then [`Backup::restore`]). Once that is stored
/// and read back, the folder's `trips.json` and `state.json` are deleted;
/// its config, secret files and any database stay. When anything fails,
/// nothing there is deleted.
pub fn import_folder(from: &Path, to: &Path) -> Result<FolderImport> {
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if same(from, to) {
        bail!("{} is the app's own folder", from.display());
    }
    let (backup, data) = Backup::from_folder(from)?;
    backup.restore(to)?;
    for file in &data.json_files {
        std::fs::remove_file(file).with_context(|| {
            format!(
                "{} was imported but can't be deleted; delete it yourself",
                file.display()
            )
        })?;
    }
    Ok(FolderImport {
        trips: backup.trips.trips.len(),
        zero_km: data.zero_km,
        deleted: data.json_files,
    })
}

/// The config text without `[calendar] source` (the calendar link, which
/// holds the key to the calendar, or a file on this machine).
pub fn without_calendar_link(text: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("parsing the config")?;
    if let Some(cal) = doc.get_mut("calendar").and_then(|c| c.as_table_like_mut()) {
        cal.remove("source");
    }
    Ok(doc.to_string())
}

/// The config text with a relative calendar file path made absolute
/// against `dir`.
fn with_absolute_calendar_file(text: &str, dir: &Path) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("parsing config.toml")?;
    if let Some(cal) = doc.get_mut("calendar").and_then(|c| c.as_table_like_mut())
        && let Some(source) = cal.get("source").and_then(|s| s.as_str())
        && !source.contains("://")
        && Path::new(source).is_relative()
    {
        let absolute = dir.join(source).to_string_lossy().into_owned();
        cal.insert("source", toml_edit::value(absolute));
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trips::tests::temp_dir;

    const CONFIG: &str = "employee_number = \"1\"\n\n[calendar]\n\
        source = \"https://calendar.proton.me/api/calendar/v1/url/SECRET/calendar.ics\"\n\
        name = \"Arbejdstid\"\n\n[connected_cars]\nemail = \"a@example.com\"\n\
        token_file = \"connected-cars-token\"\n\n[danlon]\n";

    #[test]
    fn a_backup_round_trips_without_secrets() {
        let dir = temp_dir("backup-round-trip");
        let (computer, phone) = (dir.join("computer"), dir.join("phone"));
        std::fs::create_dir_all(&computer).unwrap();
        std::fs::write(computer.join("config.toml"), CONFIG).unwrap();
        std::fs::write(computer.join("connected-cars-token"), "TOKEN-SECRET").unwrap();
        let db = computer.join(db::FILE_NAME);
        trip_store::import_bytes(
            &db,
            "trips.csv",
            b"start,end,distance_km,from,to,purpose\n2026-10-06 07:45,2026-10-06 09:05,98.0,Hjem,Kunde A,Service\n",
            chrono_tz::Europe::Copenhagen,
        )
        .unwrap();
        State::update(&db, |s| {
            s.entered
                .insert(chrono::NaiveDate::from_ymd_opt(2026, 10, 6).unwrap());
            s.account = Some(crate::state::Account {
                email: "a@example.com".into(),
                last_sync: None,
            });
        })
        .unwrap();

        let json = Backup::export(&computer.join("config.toml"), &db)
            .unwrap()
            .to_json()
            .unwrap();
        assert!(
            !json.contains("SECRET") && !json.contains("calendar.proton.me"),
            "{json}"
        );
        let backup = Backup::parse("backup.json", json.as_bytes()).unwrap();
        // The phone already has data: it is replaced.
        std::fs::create_dir_all(&phone).unwrap();
        std::fs::write(
            phone.join("config.toml"),
            "employee_number = \"9\"\n[danlon]\n",
        )
        .unwrap();
        State::update(&phone.join(db::FILE_NAME), |s| {
            s.entered
                .insert(chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap());
        })
        .unwrap();
        backup.restore(&phone).unwrap();

        let phone_db = phone.join(db::FILE_NAME);
        assert_eq!(
            TripStore::load(&phone_db).unwrap(),
            TripStore::load(&db).unwrap()
        );
        assert_eq!(State::load(&phone_db).unwrap(), State::load(&db).unwrap());
        let config = std::fs::read_to_string(phone.join("config.toml")).unwrap();
        assert!(config.contains("name = \"Arbejdstid\"") && !config.contains("source"));

        for (bytes, want) in [
            (&b"{\"trips\": []}"[..], "x.json is not a vw-danlon backup"),
            (&b"not json"[..], "x.json is not a vw-danlon backup"),
            (
                &b"{\"format\": \"vw-danlon backup\", \"version\": 99}"[..],
                "x.json is a backup from another vw-danlon version",
            ),
        ] {
            let err = Backup::parse("x.json", bytes).unwrap_err().to_string();
            assert!(err.starts_with(want), "{err}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_cli_folder_moves_in_and_its_json_files_are_deleted() {
        let dir = temp_dir("backup-folder");
        let (cli, app) = (dir.join("cli"), dir.join("app"));
        std::fs::create_dir_all(&cli).unwrap();
        let config =
            "employee_number = \"1\"\n[calendar]\nsource = \"cal/arbejdstid.ics\"\n[danlon]\n";
        std::fs::write(cli.join("config.toml"), config).unwrap();
        std::fs::write(cli.join("connected-cars-token"), "{}").unwrap();
        let trips = r#"{"trips": [
            {"start": "2026-10-05T07:32:00+02:00", "end": "2026-10-05T08:05:00+02:00", "start_odometer": null,
             "distance_km": 24.1, "from": "Hjem", "to": "Kontor", "start_pos": null, "end_pos": null},
            {"start": "2026-10-05T12:00:00+02:00", "end": "2026-10-05T12:01:00+02:00", "start_odometer": null,
             "distance_km": 0.0, "from": "Kontor", "to": "Kontor", "start_pos": null, "end_pos": null}]}"#;
        std::fs::write(cli.join("trips.json"), trips).unwrap();
        std::fs::write(cli.join("state.json"), r#"{"entered": ["2026-10-05"]}"#).unwrap();

        // A failure deletes nothing: here the app folder is a file.
        std::fs::write(&app, "").unwrap();
        assert!(import_folder(&cli, &app).is_err());
        assert!(cli.join("trips.json").exists() && cli.join("state.json").exists());
        std::fs::remove_file(&app).unwrap();
        let err = import_folder(&cli, &cli).unwrap_err().to_string();
        assert!(err.ends_with("is the app's own folder"), "{err}");

        let done = import_folder(&cli, &app).unwrap();
        assert_eq!((done.trips, done.zero_km), (1, 1));
        assert_eq!(
            done.deleted,
            [cli.join("trips.json"), cli.join("state.json")]
        );
        let mut left: Vec<_> = std::fs::read_dir(&cli)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["config.toml", "connected-cars-token"]);
        let db = app.join(db::FILE_NAME);
        assert_eq!(TripStore::load(&db).unwrap().trips.len(), 1);
        assert_eq!(State::load(&db).unwrap().entered.len(), 1);
        let want = format!("source = \"{}\"", cli.join("cal/arbejdstid.ics").display());
        let copied = std::fs::read_to_string(app.join("config.toml")).unwrap();
        assert!(copied.contains(&want), "{copied}");

        let err = import_folder(&dir.join("nothing"), &app)
            .unwrap_err()
            .to_string();
        assert!(err.ends_with("has no config.toml"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
