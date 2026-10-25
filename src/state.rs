//! Local state kept beside the config file: per-day lunch overrides (and
//! later sections added by other features). One JSON document, written
//! atomically.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct State {
    /// Lunch break per day, overriding the default and the calendar.
    pub lunch: BTreeMap<NaiveDate, LunchOverride>,
}

/// A day's lunch break. 0 minutes means no lunch that day.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LunchOverride {
    pub start: NaiveTime,
    pub minutes: u32,
}

impl State {
    /// Reads the state file; a missing file is an empty state.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(State::default()),
            Err(e) => return Err(e).with_context(|| format!("reading state {}", path.display())),
        };
        serde_json::from_str(&text).with_context(|| {
            format!(
                "parsing state {} (fix or remove the file; it was not changed)",
                path.display()
            )
        })
    }

    /// Writes the state file atomically: a temp file in the same directory,
    /// then a rename over the old file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = tmp_path(path);
        let write = || -> std::io::Result<()> {
            let mut f = std::fs::File::create(&tmp)?;
            serde_json::to_writer_pretty(&mut f, self)?;
            f.write_all(b"\n")?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        };
        write().with_context(|| format!("writing state {}", path.display()))
    }

    /// Loads the state, applies `change` and saves it. A state file that
    /// cannot be read is never overwritten.
    pub fn update<T>(path: &Path, change: impl FnOnce(&mut State) -> T) -> Result<T> {
        let mut state = State::load(path)?;
        let out = change(&mut state);
        state.save(path)?;
        Ok(out)
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

pub fn parse_date(s: &str) -> Result<NaiveDate> {
    match NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        Ok(d) => Ok(d),
        Err(_) => bail!("invalid date '{s}', expected YYYY-MM-DD"),
    }
}

pub fn parse_time(s: &str) -> Result<NaiveTime> {
    match NaiveTime::parse_from_str(s, "%H:%M") {
        Ok(t) => Ok(t),
        Err(_) => bail!("invalid time '{s}', expected HH:MM"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vw-danlon-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn d(s: &str) -> NaiveDate {
        parse_date(s).unwrap()
    }

    #[test]
    fn overrides_round_trip_through_save_and_load() {
        let dir = temp_dir("state-roundtrip");
        let path = dir.join("state.json");
        assert_eq!(State::load(&path).unwrap(), State::default());

        let set = |date: &str, start: &str, minutes| {
            State::update(&path, |s| {
                s.lunch.insert(
                    d(date),
                    LunchOverride {
                        start: parse_time(start).unwrap(),
                        minutes,
                    },
                )
            })
            .unwrap();
        };
        set("2026-11-19", "12:30", 30);
        set("2026-11-20", "11:00", 0);
        State::update(&path, |s| s.lunch.remove(&d("2026-11-20"))).unwrap();

        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.lunch.len(), 1);
        assert_eq!(
            loaded.lunch[&d("2026-11-19")],
            LunchOverride {
                start: parse_time("12:30").unwrap(),
                minutes: 30
            }
        );
        assert!(!tmp_path(&path).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_file_is_named_and_left_untouched() {
        let dir = temp_dir("state-corrupt");
        let path = dir.join("state.json");
        std::fs::write(&path, "{ not json").unwrap();

        let err = State::update(&path, |s| s.lunch.clear()).unwrap_err();
        assert!(
            format!("{err:#}").contains(&path.display().to_string()),
            "{err:#}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_date_and_time_are_named() {
        for (err, bad) in [
            (parse_date("2026-02-30").unwrap_err(), "2026-02-30"),
            (parse_date("19-11-2026").unwrap_err(), "19-11-2026"),
            (parse_time("25:00").unwrap_err(), "25:00"),
            (parse_time("noon").unwrap_err(), "noon"),
        ] {
            assert!(err.to_string().contains(&format!("'{bad}'")), "{err}");
        }
    }
}
