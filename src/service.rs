//! The steps the CLI and the app run to produce a month's data: loading the
//! calendar and the trips, building the month, its mileage and checklist,
//! the vacation overview, and the Min Volkswagen account access and sync.
//! Nothing here prints or prompts; warnings come back as values.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use chrono_tz::Tz;

use crate::calendar::{self, Occurrence};
use crate::checklist::{self, ChecklistDay};
use crate::config::{Config, MileageConfig, PrivateConfig};
use crate::connected_cars::{self, TokenStore, Vehicle};
use crate::db;
use crate::mileage::{self, MileageMonth, Status};
use crate::month::{self, EntryKind, Month, MonthReport, TimedEntry};
use crate::state::{
    self, Appointment, EventChoice, IntervalKind, PlannedWindow, PrivateInterval, RunningTimer,
    State,
};
use crate::trip_store::{self, Synced};
use crate::trips::{self, ExportTrip, Trip};
use crate::vacation::{self, Overview, Rules};

/// The first day of the data `month` needs: 1 January of the year before,
/// so the year's mileage and the 60-day rule's 12 months are counted.
pub fn history_start(month: Month) -> NaiveDate {
    NaiveDate::from_ymd_opt(month.year - 1, 1, 1).unwrap()
}

/// Trips from `file`, else the stored trips `month` needs: those from
/// [`history_start`] through the month's end.
pub fn load_trips(cfg: &Config, file: Option<&Path>, month: Month) -> Result<Vec<Trip>> {
    if let Some(file) = file {
        return trips::load(file, cfg.timezone);
    }
    trip_store::stored_trips_between(
        &cfg.database,
        cfg.timezone,
        history_start(month),
        month.last_day(),
    )
}

/// Where the calendar comes from when it is a link (a calendar file is
/// always read as it is).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarMode {
    /// Fetch it and keep the copy; when fetching fails, use the copy kept
    /// from the last fetch.
    Fresh,
    /// Use the kept copy, fetching only when there is none (the app, which
    /// refreshes it in the background with [`refresh_calendar`]).
    Cached,
    /// Use the kept copy; never fetch.
    Offline,
}

/// Calendar occurrences, with the warning to show when the calendar has no
/// name to check against `calendar.name`.
#[derive(Debug, Default)]
pub struct Calendar {
    pub occurrences: Vec<Occurrence>,
    pub warning: Option<String>,
    /// When the calendar used was fetched; none for a calendar file.
    pub fetched_at: Option<DateTime<Utc>>,
    /// Why fetching failed, when the kept copy was used instead.
    pub fetch_error: Option<String>,
}

/// Calendar occurrences from [`history_start`] to the end of `month`; the
/// earlier months are needed for the year's mileage total and the 60-day
/// rule.
pub fn calendar(cfg: &Config, month: Month, mode: CalendarMode) -> Result<Calendar> {
    calendar_between(cfg, history_start(month), month.last_day(), mode)
}

/// Calendar occurrences from `from` to `to`.
pub fn calendar_between(
    cfg: &Config,
    from: NaiveDate,
    to: NaiveDate,
    mode: CalendarMode,
) -> Result<Calendar> {
    let Some(cal) = &cfg.calendar else {
        return Ok(Calendar::default());
    };
    let (text, fetched_at, fetch_error) = calendar_text(cfg, &cal.source, mode)?;
    let mut warning = None;
    if let Some(expected) = &cal.name {
        match calendar::name(&text) {
            Some(actual) if actual.trim().eq_ignore_ascii_case(expected.trim()) => {}
            Some(actual) => anyhow::bail!(
                "calendar is {actual:?}, expected {expected:?} — check calendar.source"
            ),
            None => {
                warning = Some(format!(
                    "calendar has no name; cannot check it is {expected:?}"
                ))
            }
        }
    }
    Ok(Calendar {
        occurrences: calendar::occurrences(&text, cfg.timezone, from, to)?,
        warning,
        fetched_at,
        fetch_error,
    })
}

fn is_link(source: &str) -> bool {
    source.contains("://")
}

/// The calendar's text, when it was fetched, and why fetching failed when
/// the kept copy is used instead.
fn calendar_text(
    cfg: &Config,
    source: &str,
    mode: CalendarMode,
) -> Result<(String, Option<DateTime<Utc>>, Option<String>)> {
    if !is_link(source) {
        return Ok((calendar::fetch(source)?, None, None));
    }
    let kept = || db::cached_calendar(&cfg.database, source);
    match mode {
        CalendarMode::Offline => {
            let (text, at) = kept()?
                .context("no copy of the calendar is kept yet; run once without --offline")?;
            Ok((text, Some(at), None))
        }
        CalendarMode::Cached => match kept()? {
            Some((text, at)) => Ok((text, Some(at), None)),
            None => fetch_and_keep(cfg, source).map(|(text, at, _)| (text, Some(at), None)),
        },
        CalendarMode::Fresh => match fetch_and_keep(cfg, source) {
            Ok((text, at, _)) => Ok((text, Some(at), None)),
            Err(e) => match kept()? {
                Some((text, at)) => Ok((text, Some(at), Some(format!("{e:#}")))),
                None => Err(e),
            },
        },
    }
}

/// Fetches the calendar and keeps the copy; true when it changed.
fn fetch_and_keep(cfg: &Config, source: &str) -> Result<(String, DateTime<Utc>, bool)> {
    let text = calendar::fetch(source)?;
    let at = Utc::now();
    let changed = db::store_calendar(&cfg.database, source, &text, at)?;
    Ok((text, at, changed))
}

/// Fetches the calendar link and keeps the copy, for the app's background
/// and on-demand refresh: when it was fetched and whether it changed. None
/// without a calendar link.
pub fn refresh_calendar(cfg: &Config) -> Result<Option<(DateTime<Utc>, bool)>> {
    let Some(cal) = cfg.calendar.as_ref().filter(|c| is_link(&c.source)) else {
        return Ok(None);
    };
    let (_, at, changed) = fetch_and_keep(cfg, &cal.source)?;
    Ok(Some((at, changed)))
}

/// The calendar's timed entries, classified with the configured private
/// words, lunch title and home-working words and address.
pub fn timed_entries<'a>(cfg: &Config, cal: &'a [Occurrence]) -> Vec<TimedEntry<'a>> {
    let private_words = cfg
        .calendar
        .as_ref()
        .map(|c| c.private_keywords.as_slice())
        .unwrap_or_default();
    let mut entries = month::timed_entries(cal, private_words, &cfg.lunch.title);
    for e in &mut entries {
        if e.kind == EntryKind::Work && month::is_home_working(cfg, e.summary, e.location) {
            e.kind = EntryKind::Home;
        }
    }
    entries
}

/// The calendar's entries with the recorded private time, for classifying
/// trips: a private appointment whose time you deleted is left out.
fn trip_entries<'a>(cfg: &Config, cal: &'a [Occurrence], state: &'a State) -> Vec<TimedEntry<'a>> {
    let mut entries = timed_entries(cfg, cal);
    entries.retain(|e| e.kind != EntryKind::Private || !state.dismissed.contains(&window_of(e)));
    entries.extend(month::recorded_entries(
        &state.private.intervals,
        cfg.timezone,
    ));
    entries
}

/// A private calendar entry's planned window.
fn window_of(e: &TimedEntry) -> PlannedWindow {
    PlannedWindow {
        title: e.summary.to_string(),
        start: e.start.naive_local(),
        end: e.end.naive_local(),
    }
}

/// The private appointments among `entries`.
fn private_windows(entries: &[TimedEntry]) -> Vec<PlannedWindow> {
    entries
        .iter()
        .filter(|e| e.kind == EntryKind::Private)
        .map(window_of)
        .collect()
}

/// The month's work calendar and norm hours, without trips or the state
/// file; the totals are the whole month's.
pub fn norm_month(cfg: &Config, month: Month, cal: &[Occurrence]) -> MonthReport {
    month::build(cfg, month, &[], cal, &State::default(), NaiveDate::MAX)
}

/// The month's days with totals up to and including `today`, with the
/// lunch overrides and recorded private and home-working time from the
/// database.
pub fn build_month(
    cfg: &Config,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
    today: NaiveDate,
) -> Result<MonthReport> {
    let state = State::load(&cfg.database)?;
    Ok(month::build(cfg, month, trips, cal, &state, today))
}

/// The month's mileage, when the config has a `[mileage]` section.
pub fn mileage_for(
    cfg: &Config,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
) -> Result<Option<MileageMonth>> {
    cfg.mileage
        .as_ref()
        .map(|m| compute_mileage(cfg, m, month, trips, cal))
        .transpose()
}

/// The month's mileage; trips starting during recorded private time are
/// private, like those during a private appointment, and the trips you set
/// to business or private count so.
pub fn compute_mileage(
    cfg: &Config,
    mcfg: &MileageConfig,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
) -> Result<MileageMonth> {
    let state = State::load(&cfg.database)?;
    let entries = trip_entries(cfg, cal, &state);
    mileage::compute(
        mcfg,
        cfg.workplace.as_deref(),
        &cfg.known_places,
        month,
        trips,
        &entries,
        &state.trip_overrides,
    )
}

/// Sets the stored trip starting at `start` (to the minute) to business
/// (`Some(true)`), private (`Some(false)`), or back to the rules (`None`);
/// returns the trip. A trip that is not stored is refused, naming it.
pub fn set_trip_override(
    cfg: &Config,
    start: DateTime<Tz>,
    business: Option<bool>,
) -> Result<Trip> {
    let key = mileage::minute(start);
    let date = start.date_naive();
    let trip = trip_store::stored_trips_between(&cfg.database, cfg.timezone, date, date)?
        .into_iter()
        .find(|t| mileage::minute(t.start) == key)
        .with_context(|| {
            format!(
                "no stored trip starts at {} (see `logbook <month> --all`)",
                start.format("%Y-%m-%d %H:%M")
            )
        })?;
    State::update(&cfg.database, |s| match business {
        Some(b) => {
            s.trip_overrides.insert(key, b);
        }
        None => {
            s.trip_overrides.remove(&key);
        }
    })?;
    Ok(trip)
}

/// The befordringsfradrag estimate for `year` (LL § 9 C); home-working
/// days and days off do not count.
pub fn fradrag(
    cfg: &Config,
    year: i32,
    today: NaiveDate,
    mode: CalendarMode,
) -> Result<mileage::Fradrag> {
    let mcfg = cfg
        .mileage
        .as_ref()
        .context("no [mileage] section in config")?;
    if mcfg.home.is_none() || cfg.workplace.is_none() {
        anyhow::bail!("the befordringsfradrag needs mileage.home and workplace in the config");
    }
    let december = Month { year, month: 12 };
    let trips = load_trips(cfg, None, december)?;
    let cal = calendar(cfg, december, mode)?.occurrences;
    let state = State::load(&cfg.database)?;
    let mut excluded: std::collections::BTreeMap<NaiveDate, &'static str> = Default::default();
    for m in 1..=12 {
        let month = Month { year, month: m };
        if month.first_day() > today {
            break;
        }
        for day in month::build(cfg, month, &trips, &cal, &state, today).days {
            if !day.home.is_empty() {
                excluded.insert(day.date, "home working");
            } else if day.off_reason.is_some() {
                excluded.insert(day.date, "day off");
            }
        }
    }
    let entries = trip_entries(cfg, &cal, &state);
    mileage::fradrag(
        mcfg,
        cfg.workplace.as_deref(),
        &cfg.known_places,
        year,
        &trips,
        &entries,
        &state.trip_overrides,
        |d| excluded.get(&d).copied(),
    )
}

/// The month's Danløn app registrations per day.
pub fn checklist_days(
    cfg: &Config,
    report: &MonthReport,
    trips: &[Trip],
    mileage: Option<&MileageMonth>,
    cal: &[Occurrence],
) -> Vec<ChecklistDay> {
    checklist::build(cfg, report, trips, mileage, &timed_entries(cfg, cal))
}

/// Ok when `date` is a checklist day that can be ticked off.
pub fn check_tickable(days: &[ChecklistDay], date: NaiveDate) -> Result<()> {
    let day = days
        .iter()
        .find(|d| d.date == date)
        .with_context(|| format!("{date} is not in the checklist"))?;
    if let Some(why) = day.tick_refusal() {
        anyhow::bail!("{date} cannot be ticked: {why}");
    }
    Ok(())
}

/// A month's checklist as the Danløn screen shows it.
#[derive(Debug)]
pub struct ChecklistView {
    pub days: Vec<ChecklistDay>,
    /// The ticked days, from the state file.
    pub entered: BTreeSet<NaiveDate>,
}

/// The month's registrations and ticks, as `checklist <month>` prints them
/// on `today`. `cal` runs from 1 January through the month (see
/// [`calendar`]).
pub fn checklist_view(
    cfg: &Config,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
    today: NaiveDate,
) -> Result<ChecklistView> {
    let report = build_month(cfg, month, trips, cal, today)?;
    let mileage = mileage_for(cfg, month, trips, cal)?;
    Ok(ChecklistView {
        days: checklist_days(cfg, &report, trips, mileage.as_ref(), cal),
        entered: State::load(&cfg.database)?.entered,
    })
}

/// Ticks `date` off as entered in the Danløn app, or removes its tick;
/// a tick is refused on a day that cannot be ticked (see
/// [`check_tickable`]). True when the state file changed.
pub fn set_entered(
    cfg: &Config,
    days: &[ChecklistDay],
    date: NaiveDate,
    entered: bool,
) -> Result<bool> {
    if entered {
        check_tickable(days, date)?;
        State::update(&cfg.database, |s| s.entered.insert(date))
    } else {
        State::update(&cfg.database, |s| s.entered.remove(&date))
    }
}

/// The private and the home-working timer, each when it has been running
/// so long at `now` that it was likely forgotten.
pub fn forgotten_timers(
    cfg: &Config,
    now: NaiveDateTime,
) -> Result<(Option<RunningTimer>, Option<RunningTimer>)> {
    let state = State::load(&cfg.database)?;
    let forgotten = |r: Option<RunningTimer>| r.filter(|r| r.forgotten(now));
    Ok((
        forgotten(state.private.running),
        forgotten(state.home.running),
    ))
}

/// How many days after today the Month screen lists coming private
/// appointments.
pub const PRIVATE_DAYS_AHEAD: i64 = 14;

/// The calendar for the Month screen: what [`calendar`] fetches, through
/// [`PRIVATE_DAYS_AHEAD`] days after `today` for the coming private
/// appointments.
pub fn month_calendar(
    cfg: &Config,
    month: Month,
    today: NaiveDate,
    mode: CalendarMode,
) -> Result<Calendar> {
    let to = month
        .last_day()
        .max(today + Duration::days(PRIVATE_DAYS_AHEAD));
    calendar_between(cfg, history_start(month), to, mode)
}

/// A month as the Month and Day screens show it.
#[derive(Debug)]
pub struct MonthView {
    pub report: MonthReport,
    pub mileage: Option<MileageMonth>,
    /// Today's and the coming private time.
    pub private: PrivateTime,
}

/// The month's days and mileage as `report` builds them (calendar time
/// recorded first, see [`settle_private`]), and the private time from `now`'s day through [`PRIVATE_DAYS_AHEAD`] days later. `cal`
/// may reach past the month (see [`month_calendar`]); the days and mileage
/// use only the occurrences up to the month's end, like `report`.
pub fn month_view(
    cfg: &Config,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
    now: DateTime<Tz>,
) -> Result<MonthView> {
    let last = month.last_day();
    let up_to: Vec<Occurrence> = cal
        .iter()
        .filter(|o| occurrence_date(o) <= last)
        .cloned()
        .collect();
    let today = now.date_naive();
    settle_private(cfg, cal, now.naive_local())?;
    let report = build_month(cfg, month, trips, &up_to, today)?;
    let mileage = mileage_for(cfg, month, trips, &up_to)?;
    let private = private_time(
        &timed_entries(cfg, cal),
        &State::load(&cfg.database)?,
        &cfg.private,
        today,
        today + Duration::days(PRIVATE_DAYS_AHEAD),
        now.naive_local(),
    );
    Ok(MonthView {
        report,
        mileage,
        private,
    })
}

fn occurrence_date(o: &Occurrence) -> NaiveDate {
    match o {
        Occurrence::Timed { start, .. } => start.date_naive(),
        Occurrence::AllDay { date, .. } => *date,
    }
}

/// Where a private-time item stands.
#[derive(Debug, Clone, PartialEq)]
pub enum PrivateState {
    /// A private calendar appointment, not measured with the timer.
    Planned,
    /// The timer measures it now.
    Running { since: NaiveDateTime },
    /// A private appointment whose start has passed without a recording:
    /// it counts as running since its planned start (see
    /// [`Appointment::StartedAutomatically`]).
    StartedAutomatically { since: NaiveDateTime },
    /// Recorded; `id` is the interval's.
    Recorded {
        id: u32,
        start: NaiveDateTime,
        end: NaiveDateTime,
        kind: IntervalKind,
    },
}

/// A private calendar appointment, recorded private time, or both when
/// the recording replaces the appointment's planned window.
#[derive(Debug, Clone, PartialEq)]
pub struct PrivateItem {
    pub label: String,
    /// The calendar appointment's planned window.
    pub planned: Option<(NaiveDateTime, NaiveDateTime)>,
    pub state: PrivateState,
}

impl PrivateItem {
    /// When it starts: as measured, else as planned.
    pub fn start(&self) -> NaiveDateTime {
        match (&self.state, self.planned) {
            (PrivateState::Running { since }, _) => *since,
            (PrivateState::StartedAutomatically { since }, _) => *since,
            (PrivateState::Recorded { start, .. }, _) => *start,
            (PrivateState::Planned, Some((start, _))) => start,
            (PrivateState::Planned, None) => NaiveDateTime::MIN,
        }
    }
}

/// The private time of some days, in start order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrivateTime {
    pub running: Option<RunningTimer>,
    pub items: Vec<PrivateItem>,
    /// The item to show first: the running one, else the first one started
    /// automatically, else the first planned appointment that has not ended
    /// at `now`.
    pub current: Option<usize>,
}

/// The private appointments and recorded intervals starting `from`
/// through `to`, and the running timer when it started then or `now` is
/// in that range. A recording or timer linked to an appointment is shown
/// as that appointment; an appointment whose start has passed without one
/// shows as started automatically; one whose time you deleted is left out.
pub fn private_time(
    entries: &[TimedEntry],
    state: &State,
    pcfg: &PrivateConfig,
    from: NaiveDate,
    to: NaiveDate,
    now: NaiveDateTime,
) -> PrivateTime {
    let private = &state.private;
    let in_range = |d: NaiveDate| from <= d && d <= to;
    let mut items: Vec<PrivateItem> = entries
        .iter()
        .filter(|e| e.kind == month::EntryKind::Private && in_range(e.start.date_naive()))
        .filter_map(|e| {
            let w = window_of(e);
            let state = match state.appointment(&w, now, pcfg) {
                Appointment::Dismissed => return None,
                Appointment::StartedAutomatically => {
                    PrivateState::StartedAutomatically { since: w.start }
                }
                _ => PrivateState::Planned,
            };
            Some(PrivateItem {
                label: w.title,
                planned: Some((w.start, w.end)),
                state,
            })
        })
        .collect();
    let mut place = |label: &str, replaces: Option<&state::PlannedWindow>, state: PrivateState| {
        let linked = replaces.and_then(|w| {
            items.iter_mut().find(|i| {
                i.state == PrivateState::Planned
                    && i.planned == Some((w.start, w.end))
                    && i.label == w.title
            })
        });
        match linked {
            Some(i) => {
                i.label = label.to_string();
                i.state = state;
            }
            None => items.push(PrivateItem {
                label: label.to_string(),
                planned: replaces.map(|w| (w.start, w.end)),
                state,
            }),
        }
    };
    for i in private
        .intervals
        .iter()
        .filter(|i| in_range(i.start.date()))
    {
        place(
            &i.label,
            i.replaces.as_ref(),
            PrivateState::Recorded {
                id: i.id,
                start: i.start,
                end: i.end,
                kind: i.kind,
            },
        );
    }
    let running = private
        .running
        .clone()
        .filter(|r| in_range(r.start.date()) || in_range(now.date()));
    if let Some(r) = &running {
        place(
            &r.label,
            r.replaces.as_ref(),
            PrivateState::Running { since: r.start },
        );
    }
    items.sort_by_key(PrivateItem::start);
    let current = items
        .iter()
        .position(|i| matches!(i.state, PrivateState::Running { .. }))
        .or_else(|| {
            items
                .iter()
                .position(|i| matches!(i.state, PrivateState::StartedAutomatically { .. }))
        })
        .or_else(|| {
            items.iter().position(|i| {
                i.state == PrivateState::Planned && i.planned.is_some_and(|(_, end)| end > now)
            })
        });
    PrivateTime {
        running,
        items,
        current,
    }
}

/// Records calendar time: the planned window of each private appointment
/// in `cal` left untouched until the grace period after its end (see
/// [`Appointment::Due`]). Runs whenever the CLI or the app loads the
/// private time; there is no background service. Returns what it recorded.
pub fn settle_private(
    cfg: &Config,
    cal: &[Occurrence],
    now: NaiveDateTime,
) -> Result<Vec<PrivateInterval>> {
    if !cfg.private.auto_start {
        return Ok(Vec::new());
    }
    let windows = private_windows(&timed_entries(cfg, cal));
    let state = State::load(&cfg.database)?;
    let due = |s: &State| {
        windows
            .iter()
            .any(|w| s.appointment(w, now, &cfg.private) == Appointment::Due)
    };
    if !due(&state) {
        return Ok(Vec::new());
    }
    State::update(&cfg.database, |s| {
        s.record_calendar_time(&windows, now, &cfg.private)
    })
}

/// The private appointments in `cal` started automatically at `now`, in
/// start order.
fn started_automatically(
    cfg: &Config,
    cal: &[Occurrence],
    state: &State,
    now: NaiveDateTime,
) -> Vec<PlannedWindow> {
    let mut found: Vec<PlannedWindow> = private_windows(&timed_entries(cfg, cal))
        .into_iter()
        .filter(|w| state.appointment(w, now, &cfg.private) == Appointment::StartedAutomatically)
        .collect();
    found.sort();
    found.dedup();
    found.sort_by_key(|w| w.start);
    found
}

/// Starts the private timer at `at` today (default `now`; not later than
/// `now`), linked to the private calendar appointment that `choice` names:
/// with [`EventChoice::Auto`] the one started automatically, else the
/// current or next one (see [`state::link_event`]). The label defaults to
/// the appointment's title. Setting the real start of an appointment
/// started automatically is this, with `at` its real start.
pub fn start_private(
    cfg: &Config,
    cal: &[Occurrence],
    now: DateTime<Tz>,
    choice: &EventChoice,
    at: Option<NaiveTime>,
    label: Option<String>,
) -> Result<RunningTimer> {
    let start = match at {
        Some(t) => {
            let start = now.date_naive().and_time(t);
            if start > now.naive_local() {
                bail!("{} is in the future", t.format("%H:%M"));
            }
            start
        }
        None => now.naive_local(),
    };
    let entries = timed_entries(cfg, cal);
    State::update(&cfg.database, |s| {
        let auto = match choice {
            EventChoice::Auto => started_automatically(cfg, cal, s, now.naive_local()).pop(),
            _ => None,
        };
        let window = match auto {
            Some(w) => Some(w),
            None => state::link_event(&entries, now, choice)?,
        };
        s.private.start(start, label, window).cloned()
    })?
}

/// Stops the private timer at `now` and records the interval. Without a
/// running timer, stops the appointment started automatically (the one
/// that started last): its planned start through `now` is recorded,
/// replacing its planned window.
pub fn stop_private(
    cfg: &Config,
    cal: &[Occurrence],
    now: NaiveDateTime,
) -> Result<PrivateInterval> {
    State::update(&cfg.database, |s| {
        if s.private.running.is_some() {
            return s.private.stop(now).cloned();
        }
        let Some(w) = started_automatically(cfg, cal, s, now).pop() else {
            bail!("no timer is running and no private appointment has started");
        };
        let end = state::minute(now).max(w.start);
        Ok(s.private
            .record(
                w.start,
                end,
                w.title.clone(),
                Some(w),
                IntervalKind::Recorded,
            )
            .clone())
    })?
}

/// Marks the private appointment on `date` titled `title` (default: the
/// one started automatically at `now`) as not taken: you did not go, so it
/// counts no private time and is not recorded as calendar time. An
/// appointment with recorded time is refused, naming the interval to
/// delete instead.
pub fn skip_private(
    cfg: &Config,
    cal: &[Occurrence],
    date: NaiveDate,
    title: Option<&str>,
    now: NaiveDateTime,
) -> Result<PlannedWindow> {
    let windows: Vec<PlannedWindow> = private_windows(&timed_entries(cfg, cal))
        .into_iter()
        .filter(|w| w.start.date() == date)
        .collect();
    State::update(&cfg.database, |s| {
        let w = match title {
            Some(title) => {
                let wanted = title.trim().to_lowercase();
                windows
                    .iter()
                    .find(|w| w.title.trim().to_lowercase() == wanted)
                    .cloned()
                    .with_context(|| {
                        let names: Vec<&str> = windows.iter().map(|w| w.title.as_str()).collect();
                        format!(
                            "no private calendar event {title:?} on {date}; that day has: {}",
                            if names.is_empty() {
                                "none".to_string()
                            } else {
                                names.join(", ")
                            }
                        )
                    })?
            }
            None => {
                let started: Vec<&PlannedWindow> = windows
                    .iter()
                    .filter(|w| {
                        s.appointment(w, now, &cfg.private) == Appointment::StartedAutomatically
                    })
                    .collect();
                match started.as_slice() {
                    [w] => (*w).clone(),
                    [] => bail!(
                        "no private appointment on {date} has started automatically; name it with --event"
                    ),
                    _ => bail!(
                        "several private appointments on {date} have started; name one with --event"
                    ),
                }
            }
        };
        if let Some(i) = s
            .private
            .intervals
            .iter()
            .find(|i| i.replaces.as_ref() == Some(&w))
        {
            bail!(
                "{} {}–{} has recorded time; delete it with `private rm {}`",
                w.title,
                w.start.format("%H:%M"),
                w.end.format("%H:%M"),
                i.id
            );
        }
        s.dismissed.insert(w.clone());
        Ok(w)
    })?
}

/// Deletes a recorded interval; when it was for a calendar appointment,
/// that appointment then counts no private time (you did not go) and is
/// not recorded again (see [`State::remove_private`]).
pub fn delete_private(cfg: &Config, id: u32) -> Result<PrivateInterval> {
    State::update(&cfg.database, |s| s.remove_private(id))?
}

/// Sets a recorded interval's label and start and end times; nothing is
/// saved when either change is refused.
pub fn change_private(
    cfg: &Config,
    id: u32,
    label: &str,
    start: NaiveTime,
    end: NaiveTime,
) -> Result<()> {
    let mut s = State::load(&cfg.database)?;
    s.private.edit(id, start, end)?;
    s.private.relabel(id, label)?;
    s.save(&cfg.database)
}

/// A day's lunch break as the Day screen sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LunchChoice {
    /// The default, or the calendar's lunch event: the override is removed.
    Default,
    /// No lunch that day.
    None,
    At {
        start: NaiveTime,
        minutes: u32,
    },
}

/// Sets `date`'s lunch override as `lunch set|clear` does; the text says
/// what was done.
pub fn set_lunch(cfg: &Config, date: NaiveDate, choice: LunchChoice) -> Result<String> {
    let path = &cfg.database;
    Ok(match choice {
        LunchChoice::Default => {
            if State::update(path, |s| s.lunch.remove(&date))?.is_some() {
                format!("{date}: lunch override cleared")
            } else {
                format!("{date}: no lunch override")
            }
        }
        LunchChoice::None | LunchChoice::At { minutes: 0, .. } => {
            let start = match choice {
                LunchChoice::At { start, .. } => start,
                _ => cfg.lunch.start,
            };
            State::update(path, |s| {
                s.lunch
                    .insert(date, state::LunchOverride { start, minutes: 0 })
            })?;
            format!("{date}: no lunch")
        }
        LunchChoice::At { start, minutes } => {
            if start + Duration::minutes(minutes.into()) <= start {
                anyhow::bail!(
                    "a {minutes} min lunch from {} ends after midnight",
                    start.format("%H:%M")
                );
            }
            State::update(path, |s| {
                s.lunch
                    .insert(date, state::LunchOverride { start, minutes })
            })?;
            format!("{date}: lunch {} for {minutes} min", start.format("%H:%M"))
        }
    })
}

/// A trip typed in by hand, as the Day screen's form gives it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ManualTripForm {
    pub start: String,
    pub end: String,
    pub from: String,
    pub to: String,
    /// Km with a decimal comma or point.
    pub km: String,
    pub purpose: String,
}

/// Adds the trip on `date` from `form` to the trip store, marked as added
/// by hand (see [`trip_store::add_manual`]).
pub fn add_manual_trip(cfg: &Config, date: NaiveDate, form: &ManualTripForm) -> Result<Trip> {
    let at = |s: &str, what: &str| -> Result<DateTime<Tz>> {
        let t = state::parse_time(s.trim()).with_context(|| format!("{what} time"))?;
        date.and_time(t)
            .and_local_timezone(cfg.timezone)
            .earliest()
            .with_context(|| {
                format!(
                    "{date} {} does not exist in {}",
                    t.format("%H:%M"),
                    cfg.timezone
                )
            })
    };
    let km = form.km.trim().replace(',', ".");
    let distance_km = km
        .parse::<f64>()
        .map_err(|_| anyhow::anyhow!("invalid km '{}'", form.km.trim()))?;
    let trip = Trip {
        start: at(&form.start, "start")?,
        end: at(&form.end, "end")?,
        distance_km: Some(distance_km),
        from: form.from.trim().to_string(),
        to: form.to.trim().to_string(),
        purpose: form.purpose.trim().to_string(),
        business: None,
        start_pos: None,
        end_pos: None,
    };
    trip_store::add_manual(&cfg.database, &trip)?;
    Ok(trip)
}

/// Deletes the trip added by hand that starts at `start`.
pub fn delete_manual_trip(cfg: &Config, start: DateTime<Tz>) -> Result<()> {
    trip_store::delete_manual(&cfg.database, start.fixed_offset()).map(|_| ())
}

/// The start times of the trips added by hand, in the config's timezone.
pub fn manual_trip_starts(cfg: &Config) -> Result<BTreeSet<DateTime<Tz>>> {
    Ok(trip_store::manual_starts(&cfg.database)?
        .into_iter()
        .map(|t| t.with_timezone(&cfg.timezone))
        .collect())
}

/// The trips starting on `date`, each with how the mileage counts it
/// (`None` without a `[mileage]` section).
pub fn day_trips<'a>(
    trips: &'a [Trip],
    mileage: Option<&MileageMonth>,
    date: NaiveDate,
) -> Vec<(&'a Trip, Option<Status>)> {
    trips
        .iter()
        .filter(|t| t.start.date_naive() == date)
        .map(|t| {
            let status = mileage.map(|m| {
                m.skipped
                    .iter()
                    .find(|(s, _)| s == t)
                    .map_or(Status::Business, |(_, status)| status.clone())
            });
            (t, status)
        })
        .collect()
}

/// The vacation overview of the open holiday years on `today`.
#[derive(Debug)]
pub struct VacationView {
    pub rules: Rules,
    pub overview: Overview,
    pub calendar_warning: Option<String>,
    /// When the calendar was fetched, and why fetching failed (see
    /// [`Calendar`]).
    pub calendar_fetched_at: Option<DateTime<Utc>>,
    pub calendar_fetch_error: Option<String>,
}

/// The vacation overview: the calendar from the oldest open holiday year's
/// start to the newest one's use-by date.
pub fn vacation(cfg: &Config, today: NaiveDate, mode: CalendarMode) -> Result<VacationView> {
    let open = vacation::open_years(today);
    let from = open[0].first_day();
    let to = open[open.len() - 1].use_by();
    let cal = calendar_between(cfg, from, to, mode)?;
    let mut days = Vec::new();
    let mut month = Month {
        year: from.year(),
        month: from.month(),
    };
    while month.first_day() <= to {
        days.extend(norm_month(cfg, month, &cal.occurrences).days);
        let next = month.last_day() + chrono::Duration::days(1);
        month = Month {
            year: next.year(),
            month: next.month(),
        };
    }
    let rules = Rules::from_config(cfg);
    let leave = vacation::leave_days(cfg, &cal.occurrences, &days);
    let overview = vacation::overview(&rules, &leave, today);
    Ok(VacationView {
        rules,
        overview,
        calendar_warning: cal.warning,
        calendar_fetched_at: cal.fetched_at,
        calendar_fetch_error: cal.fetch_error,
    })
}

/// How to reach the account: namespace, where the device token is kept,
/// and the connected (else configured) email.
pub struct AccountAccess<'a> {
    pub namespace: &'a str,
    pub tokens: TokenStore,
    pub email: Option<String>,
    pub vehicle: Option<&'a str>,
}

impl AccountAccess<'_> {
    pub fn new<'a>(cfg: &'a Config, state: &State) -> AccountAccess<'a> {
        let cc = cfg.connected_cars.as_ref();
        AccountAccess {
            namespace: cc.map_or("semler:minvolkswagen", |c| c.namespace.as_str()),
            tokens: match cc.and_then(|c| c.token_file.as_deref()) {
                Some(f) => TokenStore::File(f.into()),
                None => TokenStore::Keychain,
            },
            email: state
                .account
                .as_ref()
                .map(|a| a.email.clone())
                .or_else(|| cc.and_then(|c| c.email.clone())),
            vehicle: cc.and_then(|c| c.vehicle.as_deref()),
        }
    }

    /// The stored device token, or an error naming `account connect` and
    /// `trips import`.
    pub fn token(&self) -> Result<String> {
        self.email
            .as_deref()
            .map(|e| self.tokens.get(e))
            .transpose()?
            .flatten()
            .context("no device token: run `account connect`, or use `trips import`")
    }

    /// Whether the email has a device token.
    pub fn connected(&self) -> Result<bool> {
        Ok(match &self.email {
            Some(e) => self.tokens.get(e)?.is_some(),
            None => false,
        })
    }

    /// Registers a device for `email` with its password, keeps the device
    /// token and records the account in the state file, keeping its last
    /// sync when the email is unchanged.
    pub fn connect(&self, cfg: &Config, email: &str, password: &str) -> Result<()> {
        let token = connected_cars::register_device(self.namespace, email, password)?;
        self.tokens.set(email, &token)?;
        remember_account(cfg, email)
    }

    /// Deletes `email`'s device token and forgets the account; true when a
    /// token was deleted.
    pub fn disconnect(&self, cfg: &Config, email: &str) -> Result<bool> {
        let deleted = self.tokens.delete(email)?;
        forget_account(cfg)?;
        Ok(deleted)
    }
}

/// Records the connected account in the state file, keeping its last sync
/// when the email is unchanged.
pub fn remember_account(cfg: &Config, email: &str) -> Result<()> {
    State::update(&cfg.database, |s| {
        let last_sync = s
            .account
            .take()
            .filter(|a| a.email == email)
            .and_then(|a| a.last_sync);
        s.account = Some(state::Account {
            email: email.to_string(),
            last_sync,
        });
    })
}

/// Forgets the connected account in the state file.
pub fn forget_account(cfg: &Config) -> Result<()> {
    State::update(&cfg.database, |s| s.account = None)
}

/// What a trip sync did, over the days `from` through `to`.
#[derive(Debug)]
pub struct SyncOutcome {
    pub synced: Synced,
    pub from: NaiveDate,
    pub to: NaiveDate,
}

/// Fetches trips from the account month by month into the trip store,
/// from `from` (default: where the store ends) through `today`.
pub fn sync_trips(cfg: &Config, from: Option<&str>, today: NaiveDate) -> Result<SyncOutcome> {
    let state = State::load(&cfg.database)?;
    let access = AccountAccess::new(cfg, &state);
    let token = access.token()?;
    sync_trips_with(cfg, &access, &token, from, today)
}

/// [`sync_trips`] with a device token the caller keeps (the app's secret
/// store).
pub fn sync_trips_with(
    cfg: &Config,
    access: &AccountAccess,
    token: &str,
    from: Option<&str>,
    today: NaiveDate,
) -> Result<SyncOutcome> {
    let from = match from {
        Some(f) => state::parse_date(f)?,
        None => trip_store::sync_start(&cfg.database, cfg.timezone, today)?,
    };
    if from > today {
        anyhow::bail!("--from {from} is after today");
    }
    let mut sleep = std::thread::sleep;
    let session = connected_cars::retry_429(&mut sleep, || {
        connected_cars::Session::login(access.namespace, token)
    })?;
    let vehicles = connected_cars::retry_429(&mut sleep, || session.vehicles())?;
    let vehicle = connected_cars::pick_vehicle(&vehicles, access.vehicle)?;
    let synced = trip_store::sync(&cfg.database, from, today, cfg.timezone, |first, last| {
        connected_cars::retry_429(&mut sleep, || {
            session.trips_on_days(&vehicle.id, first, last, cfg.timezone)
        })
    })?;
    let email = access.email.clone().unwrap_or_default();
    State::update(&cfg.database, |s| {
        s.account
            .get_or_insert(state::Account {
                email,
                last_sync: None,
            })
            .last_sync = Some(chrono::Utc::now().fixed_offset());
    })?;
    Ok(SyncOutcome {
        synced,
        from,
        to: today,
    })
}

/// The trips of a period fetched from the account, not stored.
#[derive(Debug)]
pub struct AccountCheck {
    pub vehicle: Vehicle,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub trips: Vec<ExportTrip>,
}

/// Fetches the trips of `from` (default: 30 days before `to`) through `to`
/// (default: `today`) without storing them, to check the access.
pub fn check_account(
    cfg: &Config,
    access: &AccountAccess,
    from: Option<&str>,
    to: Option<&str>,
    today: NaiveDate,
) -> Result<AccountCheck> {
    let token = access.token()?;
    let to = to.map(state::parse_date).transpose()?.unwrap_or(today);
    let from = match from {
        Some(f) => state::parse_date(f)?,
        None => to - chrono::Duration::days(30),
    };
    let session = connected_cars::Session::login(access.namespace, &token)?;
    let vehicles = session.vehicles()?;
    let vehicle = connected_cars::pick_vehicle(&vehicles, access.vehicle)?.clone();
    let trips = session.trips_on_days(&vehicle.id, from, to, cfg.timezone)?;
    Ok(AccountCheck {
        vehicle,
        from,
        to,
        trips,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::month::EntryKind;
    use crate::state::PlannedWindow;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn private(summary: &'static str, start: &str, end: &str) -> TimedEntry<'static> {
        let tz = chrono_tz::Europe::Copenhagen;
        TimedEntry {
            kind: EntryKind::Private,
            start: at(start).and_local_timezone(tz).unwrap(),
            end: at(end).and_local_timezone(tz).unwrap(),
            summary,
            location: "",
        }
    }

    fn window(title: &str, start: &str, end: &str) -> Option<PlannedWindow> {
        Some(PlannedWindow {
            title: title.into(),
            start: at(start),
            end: at(end),
        })
    }

    #[test]
    fn private_time_merges_recordings_into_their_appointments() {
        let entries = [
            private("Læge", "2026-11-24 13:00", "2026-11-24 14:00"),
            private("Tandlæge", "2026-11-27 08:30", "2026-11-27 09:15"),
            private("Fysioterapeut", "2026-12-20 15:00", "2026-12-20 16:00"),
            private("Gammel", "2026-11-20 09:00", "2026-11-20 10:00"),
        ];
        let mut st = State::default();
        st.private
            .start(at("2026-11-24 10:02"), None, None)
            .unwrap();
        st.private.stop(at("2026-11-24 11:14")).unwrap();
        let day = |s: &str| state::parse_date(s).unwrap();
        let view = |s: &State, now: &str| {
            let pcfg = PrivateConfig::default();
            private_time(
                &entries,
                s,
                &pcfg,
                day("2026-11-24"),
                day("2026-12-08"),
                at(now),
            )
        };

        // Nothing running: the next appointment comes first.
        let t = view(&st, "2026-11-24 12:00");
        let labels: Vec<&str> = t.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Privat", "Læge", "Tandlæge"]);
        assert_eq!(t.current, Some(1));
        assert!(matches!(
            t.items[0].state,
            PrivateState::Recorded { id: 1, .. }
        ));

        // A timer linked to Læge shows as Læge, running.
        st.private
            .start(
                at("2026-11-24 12:30"),
                None,
                window("Læge", "2026-11-24 13:00", "2026-11-24 14:00"),
            )
            .unwrap();
        let t = view(&st, "2026-11-24 12:40");
        assert_eq!(t.items.len(), 3);
        assert_eq!(t.current, Some(1));
        assert_eq!(
            t.items[1].state,
            PrivateState::Running {
                since: at("2026-11-24 12:30")
            }
        );

        // Stopped and relabelled: recorded, keeping the planned window; the
        // next appointment is now Tandlæge.
        st.private.stop(at("2026-11-24 14:10")).unwrap();
        st.private.relabel(2, "Læge (blodprøve)").unwrap();
        let t = view(&st, "2026-11-24 14:20");
        assert_eq!(t.items[1].label, "Læge (blodprøve)");
        assert_eq!(
            t.items[1].planned,
            Some((at("2026-11-24 13:00"), at("2026-11-24 14:00")))
        );
        assert!(matches!(
            t.items[1].state,
            PrivateState::Recorded { id: 2, .. }
        ));
        assert_eq!(t.current, Some(2));

        // A timer left running since yesterday still shows today.
        let mut st = State::default();
        st.private
            .start(at("2026-11-23 16:00"), None, None)
            .unwrap();
        let t = view(&st, "2026-11-24 08:00");
        assert_eq!(t.items[0].label, state::DEFAULT_LABEL);
        assert_eq!(t.current, Some(0));
        assert!(t.running.is_some());
    }

    /// A config with a calendar (for the private words), its database in a
    /// fresh folder, and a day with home working 08:00–16:00 and Læge
    /// 13:00–14:00 on 2026-11-03.
    fn private_setup(name: &str, extra: &str) -> (std::path::PathBuf, Config, Vec<Occurrence>) {
        let dir = crate::trips::tests::temp_dir(name);
        let cfg = Config::from_text(
            &format!(
                "employee_number = \"1\"\n[danlon]\n[calendar]\nsource = \"cal.ics\"\n{extra}"
            ),
            &dir.join("config.toml"),
        )
        .unwrap();
        let tz = cfg.timezone;
        let timed = |summary: &str, start: &str, end: &str| Occurrence::Timed {
            start: at(start).and_local_timezone(tz).unwrap(),
            end: at(end).and_local_timezone(tz).unwrap(),
            summary: summary.into(),
            location: String::new(),
        };
        let cal = vec![
            timed("Hjemmearbejde", "2026-11-03 08:00", "2026-11-03 16:00"),
            timed("Læge", "2026-11-03 13:00", "2026-11-03 14:00"),
        ];
        (dir, cfg, cal)
    }

    fn view_at(cfg: &Config, cal: &[Occurrence], now: &str) -> PrivateTime {
        let s = State::load(&cfg.database).unwrap();
        let day = at(now).date();
        private_time(
            &timed_entries(cfg, cal),
            &s,
            &cfg.private,
            day,
            day,
            at(now),
        )
    }

    fn worked(cfg: &Config, cal: &[Occurrence]) -> f64 {
        let month = Month {
            year: 2026,
            month: 11,
        };
        let day = state::parse_date("2026-11-03").unwrap();
        let r = build_month(cfg, month, &[], cal, day).unwrap();
        r.days.iter().find(|d| d.date == day).unwrap().worked_hours
    }

    #[test]
    fn a_private_appointment_starts_automatically_then_becomes_calendar_time() {
        let (dir, cfg, cal) = private_setup("svc-auto-start", "");
        let state_of = |now: &str| view_at(&cfg, &cal, now).items[0].state.clone();
        assert_eq!(state_of("2026-11-03 12:59"), PrivateState::Planned);
        let since = at("2026-11-03 13:00");
        assert_eq!(
            state_of("2026-11-03 13:05"),
            PrivateState::StartedAutomatically { since }
        );
        let t = view_at(&cfg, &cal, "2026-11-03 13:05");
        assert_eq!(t.current, Some(0));
        let before = worked(&cfg, &cal);

        // Within the grace period (60 min after the planned end): nothing
        // is recorded yet.
        assert!(
            settle_private(&cfg, &cal, at("2026-11-03 14:59"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            state_of("2026-11-03 14:59"),
            PrivateState::StartedAutomatically { since }
        );

        // Grace over: the planned window is recorded, marked calendar time,
        // once; the month's numbers do not change.
        let recorded = settle_private(&cfg, &cal, at("2026-11-03 15:00")).unwrap();
        assert_eq!(recorded.len(), 1);
        let i = &recorded[0];
        assert_eq!(
            (i.start, i.end, i.label.as_str(), i.kind),
            (
                at("2026-11-03 13:00"),
                at("2026-11-03 14:00"),
                "Læge",
                IntervalKind::CalendarTime
            )
        );
        assert!(
            settle_private(&cfg, &cal, at("2026-11-04 09:00"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            State::load(&cfg.database).unwrap().private.intervals.len(),
            1
        );
        assert_eq!(worked(&cfg, &cal), before);
        assert!(matches!(
            state_of("2026-11-03 15:00"),
            PrivateState::Recorded {
                id: 1,
                kind: IntervalKind::CalendarTime,
                ..
            }
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn calendar_time_can_be_edited_relabelled_and_deleted() {
        let (dir, cfg, cal) = private_setup("svc-calendar-time", "");
        settle_private(&cfg, &cal, at("2026-11-03 18:00")).unwrap();
        let before = worked(&cfg, &cal);
        let t = |s| state::parse_time(s).unwrap();

        change_private(&cfg, 1, "Læge (blodprøve)", t("13:00"), t("14:00")).unwrap();
        let i = &State::load(&cfg.database).unwrap().private.intervals[0];
        assert_eq!(
            (i.label.as_str(), i.kind),
            ("Læge (blodprøve)", IntervalKind::CalendarTime)
        );
        change_private(&cfg, 1, "Læge (blodprøve)", t("13:00"), t("14:30")).unwrap();
        let i = &State::load(&cfg.database).unwrap().private.intervals[0];
        assert_eq!(
            (i.end, i.kind),
            (at("2026-11-03 14:30"), IntervalKind::Recorded)
        );
        assert_eq!(worked(&cfg, &cal), before - 0.5);

        // Deleting it means you did not go: no private time for Læge, and
        // it is not recorded again.
        delete_private(&cfg, 1).unwrap();
        assert!(
            settle_private(&cfg, &cal, at("2026-11-04 09:00"))
                .unwrap()
                .is_empty()
        );
        assert!(
            State::load(&cfg.database)
                .unwrap()
                .private
                .intervals
                .is_empty()
        );
        assert_eq!(worked(&cfg, &cal), before + 1.0);
        assert!(view_at(&cfg, &cal, "2026-11-03 18:00").items.is_empty());
        let err = delete_private(&cfg, 1).unwrap_err();
        assert_eq!(err.to_string(), "no private interval with id 1");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_appointment_started_automatically_gets_its_real_start_or_is_stopped() {
        let (dir, cfg, cal) = private_setup("svc-auto-set", "");
        let now = |s: &str| at(s).and_local_timezone(cfg.timezone).unwrap();
        let t = |s| state::parse_time(s).unwrap();
        let w = window("Læge", "2026-11-03 13:00", "2026-11-03 14:00");

        // An earlier real start, linked to the appointment.
        let err = start_private(
            &cfg,
            &cal,
            now("2026-11-03 13:20"),
            &EventChoice::Auto,
            Some(t("13:30")),
            None,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "13:30 is in the future");
        let r = start_private(
            &cfg,
            &cal,
            now("2026-11-03 13:20"),
            &EventChoice::Auto,
            Some(t("12:50")),
            None,
        )
        .unwrap();
        assert_eq!((r.start, &r.replaces), (at("2026-11-03 12:50"), &w));
        // After the planned end, within the grace period: stop records it.
        let i = stop_private(&cfg, &cal, at("2026-11-03 14:20")).unwrap();
        assert_eq!(
            (i.start, i.end, &i.replaces, i.kind),
            (
                at("2026-11-03 12:50"),
                at("2026-11-03 14:20"),
                &w,
                IntervalKind::Recorded
            )
        );
        assert!(
            settle_private(&cfg, &cal, at("2026-11-03 18:00"))
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir_all(&dir).unwrap();

        // No timer: stop records the planned start until now.
        let (dir, cfg, cal) = private_setup("svc-auto-stop", "");
        let err = stop_private(&cfg, &cal, at("2026-11-03 12:30")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "no timer is running and no private appointment has started"
        );
        let i = stop_private(&cfg, &cal, at("2026-11-03 13:45")).unwrap();
        assert_eq!(
            (i.start, i.end, &i.replaces, i.kind),
            (
                at("2026-11-03 13:00"),
                at("2026-11-03 13:45"),
                &w,
                IntervalKind::Recorded
            )
        );
        assert!(
            settle_private(&cfg, &cal, at("2026-11-03 18:00"))
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir_all(&dir).unwrap();

        // Skipped: you did not go; nothing is recorded later.
        let (dir, cfg, cal) = private_setup("svc-auto-skip", "");
        let day = at("2026-11-03 13:10").date();
        let skipped = skip_private(&cfg, &cal, day, None, at("2026-11-03 13:10")).unwrap();
        assert_eq!(Some(skipped), w);
        assert!(view_at(&cfg, &cal, "2026-11-03 13:10").items.is_empty());
        assert!(
            settle_private(&cfg, &cal, at("2026-11-03 18:00"))
                .unwrap()
                .is_empty()
        );
        let err =
            skip_private(&cfg, &cal, day, Some("Tandlæge"), at("2026-11-03 13:10")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "no private calendar event \"Tandlæge\" on 2026-11-03; that day has: Læge"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_auto_start_an_appointment_stays_planned() {
        let (dir, cfg, cal) = private_setup("svc-auto-off", "[private]\nauto_start = false\n");
        assert_eq!(
            view_at(&cfg, &cal, "2026-11-03 13:30").items[0].state,
            PrivateState::Planned
        );
        assert!(
            settle_private(&cfg, &cal, at("2026-11-04 09:00"))
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_refused_change_saves_nothing() {
        let dir = std::env::temp_dir().join(format!("vw-danlon-change-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let cfg = Config::from_text("employee_number = \"1\"\n[danlon]\n", &path).unwrap();
        State::update(&cfg.database, |s| {
            s.private
                .start(at("2026-11-03 12:30"), Some("Bank".into()), None)?;
            s.private.stop(at("2026-11-03 13:00")).map(|_| ())
        })
        .unwrap()
        .unwrap();
        let t = |s| state::parse_time(s).unwrap();

        let err = change_private(&cfg, 1, "Post", t("13:00"), t("12:00")).unwrap_err();
        assert_eq!(err.to_string(), "end 12:00 is not after start 13:00");
        let err = change_private(&cfg, 1, " ", t("12:00"), t("12:40")).unwrap_err();
        assert_eq!(err.to_string(), "the label is empty");
        let i = &State::load(&cfg.database).unwrap().private.intervals[0];
        assert_eq!((i.label.as_str(), i.end), ("Bank", at("2026-11-03 13:00")));

        change_private(&cfg, 1, "Post", t("12:15"), t("12:40")).unwrap();
        let i = &State::load(&cfg.database).unwrap().private.intervals[0];
        assert_eq!(
            (i.label.as_str(), i.start, i.end),
            ("Post", at("2026-11-03 12:15"), at("2026-11-03 12:40"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_trip_set_to_business_or_private_and_reset() {
        let dir = crate::trips::tests::temp_dir("svc-trip-override");
        let cfg = Config::from_text(
            "employee_number = \"1\"\nworkplace = \"Kontorvej 1\"\n\
             [danlon]\nmileage_lonart = \"39\"\n\
             [mileage]\ndriver_name = \"T\"\nregistration_number = \"AB1\"\nhome = \"Hjem\"\n\
             [[mileage.rates]]\nyear = 2026\nlow = 3.94\nhigh = 2.28\n",
            &dir.join("config.toml"),
        )
        .unwrap();
        let date = state::parse_date("2026-11-03").unwrap();
        let form = ManualTripForm {
            start: "10:00".into(),
            end: "10:20".into(),
            from: "Kontorvej 1".into(),
            to: "Lager".into(),
            km: "12".into(),
            purpose: String::new(),
        };
        add_manual_trip(&cfg, date, &form).unwrap();
        let month = Month {
            year: 2026,
            month: 11,
        };
        let at = |s| trips::parse_time(s, cfg.timezone).unwrap();
        let km = || {
            let trips = load_trips(&cfg, None, month).unwrap();
            let m = mileage_for(&cfg, month, &trips, &[]).unwrap().unwrap();
            (m.total_km(), m.driving[0].reason.to_string())
        };
        assert_eq!(km(), (0.0, "private driving".into()));
        set_trip_override(&cfg, at("2026-11-03 10:00"), Some(true)).unwrap();
        assert_eq!(km(), (12.0, "set by you".into()));
        set_trip_override(&cfg, at("2026-11-03 10:00"), Some(false)).unwrap();
        assert_eq!(km(), (0.0, "set by you".into()));
        set_trip_override(&cfg, at("2026-11-03 10:00"), None).unwrap();
        assert_eq!(km(), (0.0, "private driving".into()));
        let err = set_trip_override(&cfg, at("2026-11-03 11:00"), Some(true)).unwrap_err();
        assert!(
            err.to_string()
                .contains("no stored trip starts at 2026-11-03 11:00"),
            "{err}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lunch_changes_and_manual_trips_reach_the_month() {
        let dir = std::env::temp_dir().join(format!("vw-danlon-svc-lunch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config::from_text(
            "employee_number = \"1\"\nworkplace = \"Kontorvej 1\"\n[danlon]\n",
            &dir.join("config.toml"),
        )
        .unwrap();
        let date = state::parse_date("2026-11-03").unwrap();
        let month = Month {
            year: 2026,
            month: 11,
        };
        let form = |start: &str, end: &str, from: &str, to: &str| ManualTripForm {
            start: start.into(),
            end: end.into(),
            from: from.into(),
            to: to.into(),
            km: "12,5".into(),
            purpose: String::new(),
        };
        // Arrival only: the day has no departure yet.
        add_manual_trip(&cfg, date, &form("07:30", "08:00", "Hjem", "Kontorvej 1")).unwrap();
        let worked = || {
            let trips = load_trips(&cfg, None, month).unwrap();
            let report = build_month(&cfg, month, &trips, &[], date).unwrap();
            let d = report.days.iter().find(|d| d.date == date).unwrap();
            (d.worked_hours, d.lunch.source)
        };
        add_manual_trip(&cfg, date, &form("16:00", "16:30", "Kontorvej 1", "Hjem")).unwrap();
        // The default 30 min lunch is deducted.
        let (base, source) = worked();
        assert_eq!(source, month::LunchSource::Default);
        assert!(base > 7.0, "{base}");
        let starts = manual_trip_starts(&cfg).unwrap();
        assert_eq!(starts.len(), 2);
        assert_eq!(
            load_trips(&cfg, None, month).unwrap()[0].distance_km,
            Some(12.5)
        );

        let t = |s| state::parse_time(s).unwrap();
        assert_eq!(
            set_lunch(&cfg, date, LunchChoice::None).unwrap(),
            "2026-11-03: no lunch"
        );
        assert_eq!(worked(), (base + 0.5, month::LunchSource::Override));
        assert_eq!(
            set_lunch(
                &cfg,
                date,
                LunchChoice::At {
                    start: t("11:30"),
                    minutes: 60
                }
            )
            .unwrap(),
            "2026-11-03: lunch 11:30 for 60 min"
        );
        assert_eq!(worked(), (base - 0.5, month::LunchSource::Override));
        let err = set_lunch(
            &cfg,
            date,
            LunchChoice::At {
                start: t("23:30"),
                minutes: 60,
            },
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "a 60 min lunch from 23:30 ends after midnight"
        );
        assert_eq!(
            set_lunch(&cfg, date, LunchChoice::Default).unwrap(),
            "2026-11-03: lunch override cleared"
        );
        assert_eq!(worked(), (base, month::LunchSource::Default));

        for (bad, want) in [
            (
                form("7", "08:00", "A", "B"),
                "start time: invalid time '7', expected HH:MM",
            ),
            (
                ManualTripForm {
                    km: "x".into(),
                    ..form("09:00", "09:30", "A", "B")
                },
                "invalid km 'x'",
            ),
            (
                form("10:00", "09:00", "A", "B"),
                "the trip ends 09:00 before it starts 10:00",
            ),
        ] {
            let err = add_manual_trip(&cfg, date, &bad).unwrap_err();
            assert_eq!(format!("{err:#}"), want);
        }

        delete_manual_trip(&cfg, *starts.iter().next().unwrap()).unwrap();
        assert_eq!(manual_trip_starts(&cfg).unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_calendar_link_falls_back_to_the_kept_copy() {
        let dir = crate::trips::tests::temp_dir("svc-calendar-cache");
        // Nothing listens on port 1: fetching fails at once.
        let link = "http://127.0.0.1:1/calendar.ics";
        let text = format!("employee_number = \"1\"\n[calendar]\nsource = \"{link}\"\n[danlon]\n");
        let cfg = Config::from_text(&text, &dir.join("config.toml")).unwrap();
        let month = Month::parse("2026-10").unwrap();
        for mode in [
            CalendarMode::Fresh,
            CalendarMode::Cached,
            CalendarMode::Offline,
        ] {
            assert!(
                calendar(&cfg, month, mode).is_err(),
                "{mode:?} without a copy"
            );
        }
        let err = format!(
            "{:#}",
            calendar(&cfg, month, CalendarMode::Offline).unwrap_err()
        );
        assert!(err.contains("run once without --offline"), "{err}");

        let ics = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/arbejdstid.ics"),
        )
        .unwrap();
        let kept_at = DateTime::parse_from_rfc3339("2026-10-20T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        db::store_calendar(&cfg.database, link, &ics, kept_at).unwrap();

        let fresh = calendar(&cfg, month, CalendarMode::Fresh).unwrap();
        assert!(!fresh.occurrences.is_empty());
        assert_eq!(fresh.fetched_at, Some(kept_at));
        assert!(
            fresh
                .fetch_error
                .as_deref()
                .is_some_and(|e| e.contains("downloading calendar"))
        );
        for mode in [CalendarMode::Cached, CalendarMode::Offline] {
            let c = calendar(&cfg, month, mode).unwrap();
            assert_eq!(
                (c.fetched_at, c.fetch_error),
                (Some(kept_at), None),
                "{mode:?}"
            );
            assert_eq!(c.occurrences.len(), fresh.occurrences.len());
        }
        // A failed refresh keeps the copy and its time.
        assert!(refresh_calendar(&cfg).is_err());
        assert_eq!(
            db::cached_calendar(&cfg.database, link).unwrap(),
            Some((ics, kept_at))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
