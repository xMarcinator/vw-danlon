//! Local state kept in the database beside the config file (see
//! [`crate::db`]): per-day lunch overrides, the private-time and
//! home-working timers, the days ticked off in the Danløn app checklist and
//! the connected Min Volkswagen account.

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::month::{EntryKind, TimedEntry};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct State {
    /// Lunch break per day, overriding the default and the calendar.
    pub lunch: BTreeMap<NaiveDate, LunchOverride>,
    /// The private-time timer and the intervals it recorded.
    pub private: Private,
    /// The home-working timer and the intervals it recorded.
    pub home: Home,
    /// Days ticked off as entered in the Danløn app.
    pub entered: BTreeSet<NaiveDate>,
    /// The connected Min Volkswagen account. Its device token is in the
    /// keychain, never here.
    pub account: Option<Account>,
    /// Trips you set to business (`true`) or private (`false`), by their
    /// start to the minute; they win over the mileage rules.
    pub trip_overrides: BTreeMap<DateTime<FixedOffset>, bool>,
    /// Private calendar appointments whose time you deleted: you did not
    /// go, so they count no private time and are not recorded again.
    pub dismissed: BTreeSet<PlannedWindow>,
    /// Per day, when you left work without a trip ("Stop work (left
    /// early)"): an arrival at a work address without a departure ends
    /// then.
    pub left_work: BTreeMap<NaiveDate, NaiveDateTime>,
}

/// The account `account connect` registered, and when trips were last synced.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub email: String,
    #[serde(default)]
    pub last_sync: Option<DateTime<FixedOffset>>,
}

/// A day's lunch break. 0 minutes means no lunch that day.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LunchOverride {
    pub start: NaiveTime,
    pub minutes: u32,
}

/// Time measured with a start/stop timer. Times are local, in the
/// configured timezone, to the minute.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, bound = "")]
pub struct Timer<K> {
    pub running: Option<RunningTimer>,
    pub intervals: Vec<PrivateInterval>,
    /// The id the next recorded interval gets; ids are never reused.
    pub next_id: u32,
    #[serde(skip)]
    kind: PhantomData<K>,
}

impl<K> Default for Timer<K> {
    fn default() -> Self {
        Timer {
            running: None,
            intervals: Vec::new(),
            next_id: 0,
            kind: PhantomData,
        }
    }
}

/// What a [`Timer`] measures.
pub trait TimerKind {
    /// The label of a timer started without one (and without a linked event).
    const DEFAULT_LABEL: &'static str;
    /// What its intervals are called in errors.
    const NAME: &'static str;
}

/// Private time: not company time.
#[derive(Debug, PartialEq)]
pub struct PrivateKind;

impl TimerKind for PrivateKind {
    const DEFAULT_LABEL: &'static str = DEFAULT_LABEL;
    const NAME: &'static str = "private";
}

/// Working from home: counts as worked.
#[derive(Debug, PartialEq)]
pub struct HomeKind;

impl TimerKind for HomeKind {
    const DEFAULT_LABEL: &'static str = HOME_LABEL;
    const NAME: &'static str = "home-working";
}

/// The private-time timer.
pub type Private = Timer<PrivateKind>;
/// The home-working timer; its intervals never replace an appointment.
pub type Home = Timer<HomeKind>;

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
    #[serde(default)]
    pub kind: IntervalKind,
}

/// How a recorded interval came about.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntervalKind {
    /// Measured with the timer, or times you set.
    #[default]
    Recorded,
    /// The planned window of a private appointment nobody started, stopped
    /// or changed by the grace period after its end.
    CalendarTime,
}

impl IntervalKind {
    /// How `private list` names it.
    pub fn name(self) -> &'static str {
        match self {
            IntervalKind::Recorded => "recorded",
            IntervalKind::CalendarTime => "calendar time",
        }
    }
}

/// A private calendar appointment as planned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlannedWindow {
    pub title: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

/// The label of a private timer started without one and without a linked
/// event.
pub const DEFAULT_LABEL: &str = "Private";
/// The label of a home-working timer started without one.
pub const HOME_LABEL: &str = "Home";

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

/// `t` to the minute.
pub(crate) fn minute(t: NaiveDateTime) -> NaiveDateTime {
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

impl<K: TimerKind> Timer<K> {
    /// Starts the timer at `now`. The label defaults to the linked event's
    /// title, then the kind's default label.
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
            .unwrap_or_else(|| K::DEFAULT_LABEL.to_string());
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
        let end = minute(now).max(r.start);
        Ok(self.record(r.start, end, r.label, r.replaces, IntervalKind::Recorded))
    }

    /// Records an interval with the next id.
    pub fn record(
        &mut self,
        start: NaiveDateTime,
        end: NaiveDateTime,
        label: String,
        replaces: Option<PlannedWindow>,
        kind: IntervalKind,
    ) -> &PrivateInterval {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.intervals.push(PrivateInterval {
            id,
            start,
            end,
            label,
            replaces,
            kind,
        });
        self.intervals.last().unwrap()
    }

    /// Sets an interval's start and end times. Both are on the interval's
    /// start day, except an end earlier than the start when the interval
    /// already ended on a later day (it ran past midnight): that end stays
    /// on its own day. Calendar time whose times change becomes recorded
    /// time.
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
        if (interval.start, interval.end) != (new_start, new_end) {
            interval.kind = IntervalKind::Recorded;
        }
        interval.start = new_start;
        interval.end = new_end;
        Ok(interval)
    }

    /// Changes an interval's label.
    pub fn relabel(&mut self, id: u32, label: &str) -> Result<&PrivateInterval> {
        let label = label.trim();
        if label.is_empty() {
            bail!("the label is empty");
        }
        let i = self.find(id)?;
        self.intervals[i].label = label.to_string();
        Ok(&self.intervals[i])
    }

    /// Deletes an interval.
    pub fn remove(&mut self, id: u32) -> Result<PrivateInterval> {
        let i = self.find(id)?;
        Ok(self.intervals.remove(i))
    }

    fn find(&self, id: u32) -> Result<usize> {
        match self.intervals.iter().position(|i| i.id == id) {
            Some(i) => Ok(i),
            None => bail!("no {} interval with id {id}", K::NAME),
        }
    }
}

/// What happened to a private calendar appointment at some moment, with
/// automatic start on (see [`State::appointment`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appointment {
    /// Not started yet, or automatic start is off.
    Planned,
    /// Its start has passed and nothing records it: it counts as running
    /// since its planned start.
    StartedAutomatically,
    /// Untouched until the grace period after its end: its planned window
    /// is to be recorded as calendar time.
    Due,
    /// A recorded interval or the running timer measures it.
    Measured,
    /// You deleted its time: it counts no private time.
    Dismissed,
}

impl State {
    /// Where the private appointment `w` stands at `now`.
    pub fn appointment(
        &self,
        w: &PlannedWindow,
        now: NaiveDateTime,
        cfg: &crate::config::PrivateConfig,
    ) -> Appointment {
        let linked = |r: Option<&PlannedWindow>| r == Some(w);
        if self.dismissed.contains(w) {
            Appointment::Dismissed
        } else if self
            .private
            .intervals
            .iter()
            .any(|i| linked(i.replaces.as_ref()))
            || self
                .private
                .running
                .as_ref()
                .is_some_and(|r| linked(r.replaces.as_ref()))
        {
            Appointment::Measured
        } else if !cfg.auto_start || now < w.start {
            Appointment::Planned
        } else if now < w.end + cfg.grace() {
            Appointment::StartedAutomatically
        } else {
            Appointment::Due
        }
    }

    /// Records the planned window of each appointment in `windows` that is
    /// [`Appointment::Due`] at `now`, marked calendar time; returns them.
    pub fn record_calendar_time<'a>(
        &mut self,
        windows: impl IntoIterator<Item = &'a PlannedWindow>,
        now: NaiveDateTime,
        cfg: &crate::config::PrivateConfig,
    ) -> Vec<PrivateInterval> {
        let mut recorded = Vec::new();
        for w in windows {
            if self.appointment(w, now, cfg) == Appointment::Due {
                recorded.push(
                    self.private
                        .record(
                            w.start,
                            w.end,
                            w.title.clone(),
                            Some(w.clone()),
                            IntervalKind::CalendarTime,
                        )
                        .clone(),
                );
            }
        }
        recorded
    }

    /// Deletes a private interval. When it was for a calendar appointment,
    /// you did not go: the appointment then counts no private time and is
    /// not recorded again.
    pub fn remove_private(&mut self, id: u32) -> Result<PrivateInterval> {
        let i = self.private.remove(id)?;
        if let Some(w) = &i.replaces {
            self.dismissed.insert(w.clone());
        }
        Ok(i)
    }

    /// Reads the state from the database `db`.
    pub fn load(db: &Path) -> Result<Self> {
        let conn = crate::db::open(db)?;
        Ok(read_rows(&conn)?)
    }

    /// Stores this state in the database `db`, replacing the stored one.
    pub fn save(&self, db: &Path) -> Result<()> {
        crate::db::write(db, |tx| Ok(write_rows(tx, self)?))
    }

    /// Loads the state, applies `change` and saves it, in one transaction.
    /// A database that cannot be read is never overwritten.
    pub fn update<T>(db: &Path, change: impl FnOnce(&mut State) -> T) -> Result<T> {
        crate::db::write(db, |tx| {
            let mut state = read_rows(tx)?;
            let out = change(&mut state);
            write_rows(tx, &state)?;
            Ok(out)
        })
    }
}

/// The state in a JSON file an earlier version wrote; `None` when there is
/// no file.
pub(crate) fn read_json(path: &Path) -> Result<Option<State>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading state {}", path.display())),
    };
    serde_json::from_str(&text).map(Some).with_context(|| {
        format!(
            "parsing state {} (fix or remove the file; it was not changed)",
            path.display()
        )
    })
}

fn window(
    title: Option<String>,
    start: Option<NaiveDateTime>,
    end: Option<NaiveDateTime>,
) -> Option<PlannedWindow> {
    Some(PlannedWindow {
        title: title?,
        start: start?,
        end: end?,
    })
}

/// The state's rows in the database.
pub(crate) fn read_rows(conn: &Connection) -> rusqlite::Result<State> {
    let mut lunch = BTreeMap::new();
    let mut st = conn.prepare("SELECT day, start, minutes FROM lunch")?;
    for r in st.query_map([], |r| {
        Ok((
            r.get::<_, NaiveDate>(0)?,
            LunchOverride {
                start: r.get(1)?,
                minutes: r.get(2)?,
            },
        ))
    })? {
        let (day, l) = r?;
        lunch.insert(day, l);
    }
    let entered = conn
        .prepare("SELECT day FROM entered")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<BTreeSet<NaiveDate>>>()?;
    // A database from before calendar time (opened read-only, so not
    // upgraded) has only recorded intervals.
    let calendar_time = if crate::db::has_column(conn, "private_intervals", "calendar_time")? {
        "calendar_time"
    } else {
        "0"
    };
    let intervals = conn
        .prepare(&format!(
            "SELECT id, start, end, label, replaces_title, replaces_start, replaces_end, \
             {calendar_time} FROM private_intervals ORDER BY id"
        ))?
        .query_map([], |r| {
            Ok(PrivateInterval {
                id: r.get(0)?,
                start: r.get(1)?,
                end: r.get(2)?,
                label: r.get(3)?,
                replaces: window(r.get(4)?, r.get(5)?, r.get(6)?),
                kind: if r.get(7)? {
                    IntervalKind::CalendarTime
                } else {
                    IntervalKind::Recorded
                },
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let running = conn
        .query_row(
            "SELECT start, label, replaces_title, replaces_start, replaces_end FROM private_running",
            [],
            |r| {
                Ok(RunningTimer {
                    start: r.get(0)?,
                    label: r.get(1)?,
                    replaces: window(r.get(2)?, r.get(3)?, r.get(4)?),
                })
            },
        )
        .optional()?;
    let next_id = read_next_id(conn, "private_next_id")?;
    let account = conn
        .query_row("SELECT email, last_sync FROM account", [], |r| {
            Ok(Account {
                email: r.get(0)?,
                last_sync: r.get(1)?,
            })
        })
        .optional()?;
    Ok(State {
        lunch,
        private: Private {
            running,
            intervals,
            next_id,
            kind: PhantomData,
        },
        home: read_home(conn)?,
        entered,
        account,
        trip_overrides: read_overrides(conn)?,
        dismissed: read_dismissed(conn)?,
        left_work: read_left_work(conn)?,
    })
}

/// The private appointments whose time you deleted; none in a database
/// from before them (opened read-only, so not upgraded).
fn read_dismissed(conn: &Connection) -> rusqlite::Result<BTreeSet<PlannedWindow>> {
    if !crate::db::has_table(conn, "private_dismissed")? {
        return Ok(BTreeSet::new());
    }
    conn.prepare("SELECT title, start, end FROM private_dismissed")?
        .query_map([], |r| {
            Ok(PlannedWindow {
                title: r.get(0)?,
                start: r.get(1)?,
                end: r.get(2)?,
            })
        })?
        .collect()
}

/// When you left work without a trip, per day; none in a database from
/// before it (opened read-only, so not upgraded).
fn read_left_work(conn: &Connection) -> rusqlite::Result<BTreeMap<NaiveDate, NaiveDateTime>> {
    if !crate::db::has_table(conn, "work_end")? {
        return Ok(BTreeMap::new());
    }
    conn.prepare("SELECT day, at FROM work_end")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

/// The trip overrides; none in a database from before them (opened
/// read-only, so not upgraded).
fn read_overrides(conn: &Connection) -> rusqlite::Result<BTreeMap<DateTime<FixedOffset>, bool>> {
    if !crate::db::has_table(conn, "trip_overrides")? {
        return Ok(BTreeMap::new());
    }
    conn.prepare("SELECT start, business FROM trip_overrides")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

fn read_next_id(conn: &Connection, key: &str) -> rusqlite::Result<u32> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| {
            r.get::<_, String>(0)
        })
        .optional()?
        .and_then(|v| v.parse().ok())
        .unwrap_or_default())
}

/// The home-working timer's rows; none in a database from before it
/// (opened read-only, so not upgraded).
fn read_home(conn: &Connection) -> rusqlite::Result<Home> {
    if !crate::db::has_table(conn, "home_intervals")? {
        return Ok(Home::default());
    }
    let intervals = conn
        .prepare("SELECT id, start, end, label FROM home_intervals ORDER BY id")?
        .query_map([], |r| {
            Ok(PrivateInterval {
                id: r.get(0)?,
                start: r.get(1)?,
                end: r.get(2)?,
                label: r.get(3)?,
                replaces: None,
                kind: IntervalKind::Recorded,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let running = conn
        .query_row("SELECT start, label FROM home_running", [], |r| {
            Ok(RunningTimer {
                start: r.get(0)?,
                label: r.get(1)?,
                replaces: None,
            })
        })
        .optional()?;
    Ok(Home {
        running,
        intervals,
        next_id: read_next_id(conn, "home_next_id")?,
        kind: PhantomData,
    })
}

/// Replaces the state's rows in the database with `state`.
pub(crate) fn write_rows(conn: &Connection, state: &State) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DELETE FROM lunch; DELETE FROM entered; DELETE FROM private_intervals; \
         DELETE FROM private_running; DELETE FROM account; \
         DELETE FROM home_intervals; DELETE FROM home_running; DELETE FROM trip_overrides; \
         DELETE FROM private_dismissed; DELETE FROM work_end; \
         DELETE FROM meta WHERE key IN ('private_next_id', 'home_next_id');",
    )?;
    let mut st = conn.prepare("INSERT INTO lunch (day, start, minutes) VALUES (?1, ?2, ?3)")?;
    for (day, l) in &state.lunch {
        st.execute(params![day, l.start, l.minutes])?;
    }
    let mut st = conn.prepare("INSERT INTO entered (day) VALUES (?1)")?;
    for day in &state.entered {
        st.execute([day])?;
    }
    let mut st = conn.prepare(
        "INSERT INTO private_intervals \
         (id, start, end, label, replaces_title, replaces_start, replaces_end, calendar_time) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for i in &state.private.intervals {
        let w = i.replaces.as_ref();
        st.execute(params![
            i.id,
            i.start,
            i.end,
            i.label,
            w.map(|w| &w.title),
            w.map(|w| w.start),
            w.map(|w| w.end),
            i.kind == IntervalKind::CalendarTime
        ])?;
    }
    if let Some(r) = &state.private.running {
        let w = r.replaces.as_ref();
        conn.execute(
            "INSERT INTO private_running (one, start, label, replaces_title, replaces_start, replaces_end) \
             VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            params![
                r.start,
                r.label,
                w.map(|w| &w.title),
                w.map(|w| w.start),
                w.map(|w| w.end)
            ],
        )?;
    }
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('private_next_id', ?1)",
        [state.private.next_id.to_string()],
    )?;
    let mut st =
        conn.prepare("INSERT INTO home_intervals (id, start, end, label) VALUES (?1, ?2, ?3, ?4)")?;
    for i in &state.home.intervals {
        st.execute(params![i.id, i.start, i.end, i.label])?;
    }
    if let Some(r) = &state.home.running {
        conn.execute(
            "INSERT INTO home_running (one, start, label) VALUES (1, ?1, ?2)",
            params![r.start, r.label],
        )?;
    }
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('home_next_id', ?1)",
        [state.home.next_id.to_string()],
    )?;
    if let Some(a) = &state.account {
        conn.execute(
            "INSERT INTO account (one, email, last_sync) VALUES (1, ?1, ?2)",
            params![a.email, a.last_sync],
        )?;
    }
    let mut st = conn.prepare("INSERT INTO trip_overrides (start, business) VALUES (?1, ?2)")?;
    for (start, business) in &state.trip_overrides {
        st.execute(params![start, business])?;
    }
    let mut st =
        conn.prepare("INSERT INTO private_dismissed (title, start, end) VALUES (?1, ?2, ?3)")?;
    for w in &state.dismissed {
        st.execute(params![w.title, w.start, w.end])?;
    }
    let mut st = conn.prepare("INSERT INTO work_end (day, at) VALUES (?1, ?2)")?;
    for (day, at) in &state.left_work {
        st.execute(params![day, at])?;
    }
    Ok(())
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

    fn temp_dir(name: &str) -> std::path::PathBuf {
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
        let path = dir.join(crate::db::FILE_NAME);
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
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn entered_days_round_trip() {
        let dir = temp_dir("state-entered");
        let path = dir.join(crate::db::FILE_NAME);
        // A state file from before the checklist has no ticks.
        std::fs::write(dir.join("state.json"), r#"{"lunch": {}}"#).unwrap();
        assert!(State::load(&path).unwrap().entered.is_empty());

        assert!(State::update(&path, |s| s.entered.insert(d("2026-11-18"))).unwrap());
        assert!(!State::update(&path, |s| s.entered.insert(d("2026-11-18"))).unwrap());
        State::update(&path, |s| s.entered.insert(d("2026-11-19"))).unwrap();
        assert!(State::update(&path, |s| s.entered.remove(&d("2026-11-18"))).unwrap());
        assert_eq!(
            State::load(&path).unwrap().entered,
            BTreeSet::from([d("2026-11-19")])
        );
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
        let path = dir.join(crate::db::FILE_NAME);
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
                kind: Default::default(),
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
            (
                p.relabel(9, "Bank").unwrap_err(),
                "no private interval with id 9",
            ),
            (p.relabel(1, "  ").unwrap_err(), "the label is empty"),
        ] {
            assert_eq!(err.to_string(), want);
        }
        let mut long = Private::default();
        long.start(at("2026-11-04 15:00"), None, None).unwrap();
        long.stop(at("2026-11-06 09:00")).unwrap();
        let err = long.edit(1, t("15:00"), t("14:00")).unwrap_err();
        assert!(err.to_string().contains("over 24 hours"), "{err}");

        assert_eq!(p.relabel(2, " Tandlæge ").unwrap().label, "Tandlæge");
        assert_eq!(p.remove(1).unwrap().label, "Bank");
        assert_eq!(p.intervals.iter().map(|i| i.id).collect::<Vec<_>>(), [2]);
        p.start(at("2026-11-07 10:00"), None, None).unwrap();
        assert_eq!(p.stop(at("2026-11-07 11:00")).unwrap().id, 3);
    }

    #[test]
    fn home_timer_round_trips_and_names_its_intervals() {
        let dir = temp_dir("state-home");
        let path = dir.join(crate::db::FILE_NAME);
        let t = |s| parse_time(s).unwrap();
        State::update(&path, |s| {
            s.home.start(at("2026-11-03 08:01"), None, None)?;
            s.home.stop(at("2026-11-03 12:00"))?;
            s.home
                .start(at("2026-11-04 13:00"), Some("Rapport".into()), None)?;
            s.home.edit(1, t("08:00"), t("11:30")).map(|_| ())
        })
        .unwrap()
        .unwrap();
        let loaded = State::load(&path).unwrap();
        assert_eq!(
            loaded.home.intervals,
            vec![PrivateInterval {
                id: 1,
                start: at("2026-11-03 08:00"),
                end: at("2026-11-03 11:30"),
                label: HOME_LABEL.into(),
                kind: Default::default(),
                replaces: None,
            }]
        );
        assert_eq!(loaded.home.running.as_ref().unwrap().label, "Rapport");
        assert!(loaded.private.intervals.is_empty() && loaded.private.running.is_none());

        let mut home = loaded.home;
        assert_eq!(
            home.remove(9).unwrap_err().to_string(),
            "no home-working interval with id 9"
        );
        assert!(home.start(at("2026-11-04 14:00"), None, None).is_err());
        assert_eq!(home.stop(at("2026-11-04 15:00")).unwrap().id, 2);
        assert_eq!(home.remove(1).unwrap().id, 1);
        std::fs::remove_dir_all(&dir).unwrap();
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
