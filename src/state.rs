//! Local state kept beside the config file: per-day lunch overrides and the
//! private-time timer. One JSON document, written atomically.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::month::{EntryKind, TimedEntry};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct State {
    /// Lunch break per day, overriding the default and the calendar.
    pub lunch: BTreeMap<NaiveDate, LunchOverride>,
    /// The private-time timer and the intervals it recorded.
    pub private: Private,
}

/// A day's lunch break. 0 minutes means no lunch that day.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LunchOverride {
    pub start: NaiveTime,
    pub minutes: u32,
}

/// Private time measured with the timer. Times are local, in the configured
/// timezone, to the minute.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Private {
    pub running: Option<RunningTimer>,
    pub intervals: Vec<PrivateInterval>,
    /// The id the next recorded interval gets; ids are never reused.
    pub next_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunningTimer {
    pub start: NaiveDateTime,
    pub label: String,
    /// The calendar appointment this timer measures, if linked.
    pub replaces: Option<PlannedWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrivateInterval {
    pub id: u32,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub label: String,
    /// The calendar appointment whose planned window this interval replaces.
    pub replaces: Option<PlannedWindow>,
}

/// A private calendar appointment as planned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannedWindow {
    pub title: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

/// The label of a timer started without one and without a linked event.
pub const DEFAULT_LABEL: &str = "Privat";

/// Which calendar appointment `private start` links to.
pub enum EventChoice {
    /// The current private appointment, or the next one starting within
    /// [`LINK_AHEAD`] the same day.
    Auto,
    /// The private appointment that day with this title.
    Title(String),
    None,
}

/// How far ahead an upcoming private appointment is linked automatically.
const LINK_AHEAD: Duration = Duration::hours(2);

/// The private appointment a timer started at `now` measures. Only private
/// entries on `now`'s day count; a named one that is not there is an error.
pub fn link_event(
    entries: &[TimedEntry],
    now: DateTime<Tz>,
    choice: &EventChoice,
) -> Result<Option<PlannedWindow>> {
    let mut today: Vec<&TimedEntry> = entries
        .iter()
        .filter(|e| e.kind == EntryKind::Private && e.start.date_naive() == now.date_naive())
        .collect();
    today.sort_by_key(|e| e.start);
    let found = match choice {
        EventChoice::None => None,
        EventChoice::Auto => today
            .into_iter()
            .find(|e| e.end > now && e.start <= now + LINK_AHEAD),
        EventChoice::Title(title) => {
            let wanted = title.trim().to_lowercase();
            match today
                .iter()
                .find(|e| e.summary.trim().to_lowercase() == wanted)
            {
                Some(e) => Some(*e),
                None => {
                    let names: Vec<&str> = today.iter().map(|e| e.summary).collect();
                    bail!(
                        "no private calendar event {title:?} on {}; that day has: {}",
                        now.date_naive(),
                        if names.is_empty() {
                            "none".to_string()
                        } else {
                            names.join(", ")
                        }
                    )
                }
            }
        }
    };
    Ok(found.map(|e| PlannedWindow {
        title: e.summary.to_string(),
        start: e.start.naive_local(),
        end: e.end.naive_local(),
    }))
}

fn minute(t: NaiveDateTime) -> NaiveDateTime {
    t.with_second(0)
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(t)
}

fn stamp(t: NaiveDateTime) -> String {
    t.format("%Y-%m-%d %H:%M").to_string()
}

/// How long a timer runs before the report flags it as forgotten.
const FORGOTTEN_AFTER: Duration = Duration::hours(12);

impl RunningTimer {
    /// Whether the timer has run past midnight or longer than
    /// [`FORGOTTEN_AFTER`] at `now`; the report flags it for fixing.
    pub fn forgotten(&self, now: NaiveDateTime) -> bool {
        now.date() > self.start.date() || now - self.start > FORGOTTEN_AFTER
    }
}

impl Private {
    /// Starts the timer at `now`. The label defaults to the linked event's
    /// title, then [`DEFAULT_LABEL`].
    pub fn start(
        &mut self,
        now: NaiveDateTime,
        label: Option<String>,
        replaces: Option<PlannedWindow>,
    ) -> Result<&RunningTimer> {
        if let Some(r) = &self.running {
            bail!(
                "a timer is already running since {} ({}); stop it first",
                stamp(r.start),
                r.label
            );
        }
        let label = label
            .or_else(|| replaces.as_ref().map(|w| w.title.clone()))
            .unwrap_or_else(|| DEFAULT_LABEL.to_string());
        Ok(self.running.insert(RunningTimer {
            start: minute(now),
            label,
            replaces,
        }))
    }

    /// Stops the timer at `now` and records the interval.
    pub fn stop(&mut self, now: NaiveDateTime) -> Result<&PrivateInterval> {
        let Some(r) = self.running.take() else {
            bail!("no timer is running");
        };
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.intervals.push(PrivateInterval {
            id,
            start: r.start,
            end: minute(now).max(r.start),
            label: r.label,
            replaces: r.replaces,
        });
        Ok(self.intervals.last().unwrap())
    }

    /// Sets an interval's start and end times. Both are on the interval's
    /// start day, except an end earlier than the start when the interval
    /// already ended on a later day (it ran past midnight): that end stays
    /// on its own day.
    pub fn edit(&mut self, id: u32, start: NaiveTime, end: NaiveTime) -> Result<&PrivateInterval> {
        let i = self.find(id)?;
        let interval = &mut self.intervals[i];
        let day = interval.start.date();
        let new_start = day.and_time(start);
        let new_end = if end > start || interval.end.date() == day {
            day.and_time(end)
        } else {
            interval.end.date().and_time(end)
        };
        if new_end <= new_start {
            bail!(
                "end {} is not after start {}",
                end.format("%H:%M"),
                start.format("%H:%M")
            );
        }
        if new_end - new_start > Duration::hours(24) {
            bail!(
                "interval {} – {} is over 24 hours",
                stamp(new_start),
                stamp(new_end)
            );
        }
        interval.start = new_start;
        interval.end = new_end;
        Ok(interval)
    }

    /// Deletes an interval.
    pub fn remove(&mut self, id: u32) -> Result<PrivateInterval> {
        let i = self.find(id)?;
        Ok(self.intervals.remove(i))
    }

    fn find(&self, id: u32) -> Result<usize> {
        match self.intervals.iter().position(|i| i.id == id) {
            Some(i) => Ok(i),
            None => bail!("no private interval with id {id}"),
        }
    }
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

    pub fn save(&self, path: &Path) -> Result<()> {
        write_json(path, self).with_context(|| format!("writing state {}", path.display()))
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

/// Writes `value` as JSON atomically: a temp file in the same directory,
/// then a rename over the old file.
pub fn write_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let tmp = tmp_path(path);
    let mut f = std::fs::File::create(&tmp)?;
    serde_json::to_writer_pretty(&mut f, value)?;
    f.write_all(b"\n")?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
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

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn entry(
        kind: EntryKind,
        summary: &'static str,
        start: &str,
        end: &str,
    ) -> TimedEntry<'static> {
        let tz = chrono_tz::Europe::Copenhagen;
        TimedEntry {
            kind,
            start: at(start).and_local_timezone(tz).unwrap(),
            end: at(end).and_local_timezone(tz).unwrap(),
            summary,
            location: "",
        }
    }

    #[test]
    fn timer_links_the_current_or_next_private_appointment() {
        let tz = chrono_tz::Europe::Copenhagen;
        let entries = [
            entry(
                EntryKind::Work,
                "Kunde",
                "2026-11-03 08:00",
                "2026-11-03 16:00",
            ),
            entry(
                EntryKind::Private,
                "Læge",
                "2026-11-03 13:00",
                "2026-11-03 14:00",
            ),
            entry(
                EntryKind::Private,
                "Tandlæge",
                "2026-11-03 16:00",
                "2026-11-03 17:00",
            ),
            entry(
                EntryKind::Private,
                "Fysioterapeut",
                "2026-11-04 12:45",
                "2026-11-04 13:30",
            ),
        ];
        let now = |s: &str| at(s).and_local_timezone(tz).unwrap();
        let title = |choice: EventChoice, t: &str| {
            link_event(&entries, now(t), &choice)
                .unwrap()
                .map(|w| w.title)
        };
        assert_eq!(
            title(EventChoice::Auto, "2026-11-03 12:30").as_deref(),
            Some("Læge")
        );
        assert_eq!(
            title(EventChoice::Auto, "2026-11-03 13:40").as_deref(),
            Some("Læge")
        );
        // More than two hours ahead, or only on another day: a plain timer.
        assert_eq!(title(EventChoice::Auto, "2026-11-03 10:30"), None);
        assert_eq!(title(EventChoice::Auto, "2026-11-03 17:30"), None);
        assert_eq!(title(EventChoice::None, "2026-11-03 12:30"), None);
        assert_eq!(
            title(EventChoice::Title("tandlæge".into()), "2026-11-03 12:30").as_deref(),
            Some("Tandlæge")
        );
        let err = link_event(
            &entries,
            now("2026-11-03 12:30"),
            &EventChoice::Title("Kunde".into()),
        )
        .unwrap_err();
        assert!(err.to_string().contains("\"Kunde\""), "{err}");

        let window = link_event(&entries, now("2026-11-03 12:30"), &EventChoice::Auto).unwrap();
        let mut p = Private::default();
        let r = p.start(at("2026-11-03 12:30"), None, window).unwrap();
        assert_eq!(r.label, "Læge");
        assert_eq!(
            r.replaces,
            Some(PlannedWindow {
                title: "Læge".into(),
                start: at("2026-11-03 13:00"),
                end: at("2026-11-03 14:00"),
            })
        );
    }

    #[test]
    fn timer_start_stop_records_an_interval() {
        let dir = temp_dir("state-timer");
        let path = dir.join("state.json");
        State::update(&path, |s| {
            s.private.start(at("2026-11-03 12:30"), None, None).cloned()
        })
        .unwrap()
        .unwrap();
        let err = State::update(&path, |s| {
            s.private
                .start(at("2026-11-03 12:40"), None, None)
                .map(|_| ())
        })
        .unwrap()
        .unwrap_err();
        assert!(err.to_string().contains("2026-11-03 12:30"), "{err}");

        State::update(&path, |s| s.private.stop(at("2026-11-03 14:15")).cloned())
            .unwrap()
            .unwrap();
        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.private.running, None);
        assert_eq!(
            loaded.private.intervals,
            vec![PrivateInterval {
                id: 1,
                start: at("2026-11-03 12:30"),
                end: at("2026-11-03 14:15"),
                label: DEFAULT_LABEL.into(),
                replaces: None,
            }]
        );
        let err = State::update(&path, |s| {
            s.private.stop(at("2026-11-03 15:00")).map(|_| ())
        })
        .unwrap()
        .unwrap_err();
        assert_eq!(err.to_string(), "no timer is running");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn intervals_are_edited_and_removed() {
        let t = |s| parse_time(s).unwrap();
        let mut p = Private::default();
        p.start(at("2026-11-03 12:30"), Some("Bank".into()), None)
            .unwrap();
        p.stop(at("2026-11-03 13:00")).unwrap();
        // A timer left running into the next morning.
        p.start(at("2026-11-04 15:00"), None, None).unwrap();
        p.stop(at("2026-11-05 09:00")).unwrap();

        let i = p.edit(1, t("12:15"), t("13:20")).unwrap();
        assert_eq!(
            (i.start, i.end),
            (at("2026-11-03 12:15"), at("2026-11-03 13:20"))
        );
        // An earlier end stays on the later day the interval ended on.
        let i = p.edit(2, t("15:00"), t("08:00")).unwrap().clone();
        assert_eq!(i.end, at("2026-11-05 08:00"));
        let i = p.edit(2, t("15:00"), t("16:30")).unwrap();
        assert_eq!(i.end, at("2026-11-04 16:30"));

        for (err, want) in [
            (
                p.edit(1, t("13:00"), t("12:00")).unwrap_err(),
                "end 12:00 is not after start 13:00",
            ),
            (
                p.edit(9, t("12:00"), t("13:00")).unwrap_err(),
                "no private interval with id 9",
            ),
            (p.remove(9).unwrap_err(), "no private interval with id 9"),
        ] {
            assert_eq!(err.to_string(), want);
        }
        let mut long = Private::default();
        long.start(at("2026-11-04 15:00"), None, None).unwrap();
        long.stop(at("2026-11-06 09:00")).unwrap();
        let err = long.edit(1, t("15:00"), t("14:00")).unwrap_err();
        assert!(err.to_string().contains("over 24 hours"), "{err}");

        assert_eq!(p.remove(1).unwrap().label, "Bank");
        assert_eq!(p.intervals.iter().map(|i| i.id).collect::<Vec<_>>(), [2]);
        p.start(at("2026-11-07 10:00"), None, None).unwrap();
        assert_eq!(p.stop(at("2026-11-07 11:00")).unwrap().id, 3);
    }

    #[test]
    fn forgotten_timer() {
        let r = RunningTimer {
            start: at("2026-11-03 08:00"),
            label: DEFAULT_LABEL.into(),
            replaces: None,
        };
        for (now, forgotten) in [
            ("2026-11-03 20:00", false),
            ("2026-11-03 20:01", true),
            ("2026-11-04 00:00", true),
        ] {
            assert_eq!(r.forgotten(at(now)), forgotten, "{now}");
        }
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
