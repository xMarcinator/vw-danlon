//! The steps the CLI and the app run to produce a month's data: loading the
//! calendar and the trips, building the month, its mileage and checklist,
//! the vacation overview, and the Min Volkswagen account access and sync.
//! Nothing here prints or prompts; warnings come back as values.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::calendar::{self, Occurrence};
use crate::checklist::{self, ChecklistDay};
use crate::config::{CalendarUse, Config, MileageConfig, PrivateConfig};
use crate::connected_cars::{self, TokenStore, Vehicle};
use crate::db;
use crate::mileage::{self, MileageMonth, Status};
use crate::month::{self, EntryKind, Month, MonthReport, Span, TimedEntry, Today};
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

/// Calendar occurrences from `from` to `to`: the work calendar's, then the
/// private appointments of the extra calendars (see [`extra_occurrences`]).
/// An extra calendar that can't be read is named in the warning.
pub fn calendar_between(
    cfg: &Config,
    from: NaiveDate,
    to: NaiveDate,
    mode: CalendarMode,
) -> Result<Calendar> {
    let mut out = match &cfg.calendar {
        Some(cal) => {
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
            Calendar {
                occurrences: calendar::occurrences(&text, cfg.timezone, from, to)?,
                warning,
                fetched_at,
                fetch_error,
            }
        }
        None => Calendar::default(),
    };
    for extra in &cfg.extra_calendars {
        let Some(source) = &extra.source else {
            continue;
        };
        let read = calendar_text(cfg, source, mode)
            .and_then(|(text, ..)| extra_occurrences(cfg, extra.usage, &text, from, to));
        match read {
            Ok(occ) => out.occurrences.extend(occ),
            Err(e) => {
                let name = extra.name.as_deref().unwrap_or("extra calendar");
                let text = format!("calendar {name:?} can't be read: {e:#}");
                out.warning = Some(match out.warning.take() {
                    Some(w) => format!("{w}; {text}"),
                    None => text,
                });
            }
        }
    }
    Ok(out)
}

/// An extra calendar's private appointments from `from` to `to`: every
/// timed event of a Private calendar, the timed events with a private word
/// of a Words only one. All-day events are ignored.
fn extra_occurrences(
    cfg: &Config,
    usage: CalendarUse,
    text: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<Occurrence>> {
    Ok(calendar::occurrences(text, cfg.timezone, from, to)?
        .into_iter()
        .filter_map(|occ| match occ {
            Occurrence::Timed {
                start,
                end,
                summary,
                ..
            }
            | Occurrence::Private {
                start,
                end,
                summary,
            } if usage == CalendarUse::Private
                || month::has_word(&summary, cfg.private_words()) =>
            {
                Some(Occurrence::Private {
                    start,
                    end,
                    summary,
                })
            }
            _ => None,
        })
        .collect())
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

/// Fetches the calendar links (the work calendar's and the extra ones) and
/// keeps the copies, for the app's background and on-demand refresh: when
/// they were fetched and whether any changed. None without a calendar link;
/// an error only when none could be fetched.
pub fn refresh_calendar(cfg: &Config) -> Result<Option<(DateTime<Utc>, bool)>> {
    let links = cfg
        .calendar
        .as_ref()
        .map(|c| c.source.as_str())
        .into_iter()
        .chain(
            cfg.extra_calendars
                .iter()
                .filter_map(|c| c.source.as_deref()),
        )
        .filter(|s| is_link(s));
    let (mut done, mut first_error) = (None::<(DateTime<Utc>, bool)>, None);
    for link in links {
        match fetch_and_keep(cfg, link) {
            Ok((_, at, changed)) => {
                let any = done.is_some_and(|(_, c)| c);
                done = Some((at, any || changed));
            }
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    match (done, first_error) {
        (Some(d), _) => Ok(Some(d)),
        (None, Some(e)) => Err(e),
        (None, None) => Ok(None),
    }
}

/// A calendar as Settings lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarStatus {
    /// Its name (X-WR-CALNAME), else the configured one.
    pub name: Option<String>,
    /// None: the work calendar (working hours).
    pub usage: Option<CalendarUse>,
    /// The extra calendar's place in `extra_calendars`.
    pub index: Option<usize>,
    /// Where it is read from.
    pub read: CalendarRead,
    /// Its events in the month: all for the work calendar, the private
    /// appointments for an extra one.
    pub events: usize,
}

/// How a calendar was last read.
#[derive(Debug, Clone, PartialEq)]
pub enum CalendarRead {
    /// A link, fetched at this time.
    Synced(DateTime<Utc>),
    /// A file, read now.
    File,
    /// A link not fetched yet.
    NotSynced,
    /// No link on this device (a backup from another one).
    NoLink,
    /// The file or the kept copy can't be read.
    Failed(String),
}

/// The calendars with their names, last sync and events in `month`, from
/// the kept copies (nothing is fetched).
pub fn calendar_statuses(cfg: &Config, month: Month) -> Vec<CalendarStatus> {
    let (from, to) = (month.first_day(), month.last_day());
    let read = |source: &str| -> Result<Option<(String, CalendarRead)>> {
        if !is_link(source) {
            return Ok(Some((calendar::fetch(source)?, CalendarRead::File)));
        }
        Ok(db::cached_calendar(&cfg.database, source)?
            .map(|(text, at)| (text, CalendarRead::Synced(at))))
    };
    let status = |source: Option<&str>,
                  name: Option<&String>,
                  usage: Option<CalendarUse>,
                  index: Option<usize>| {
        let mut s = CalendarStatus {
            name: name.cloned(),
            usage,
            index,
            read: CalendarRead::NoLink,
            events: 0,
        };
        let Some(source) = source else { return s };
        let counted = read(source).and_then(|kept| {
            let Some((text, how)) = kept else {
                return Ok((None, CalendarRead::NotSynced, 0));
            };
            let events = match usage {
                None => calendar::occurrences(&text, cfg.timezone, from, to)?.len(),
                Some(u) => extra_occurrences(cfg, u, &text, from, to)?.len(),
            };
            Ok((calendar::name(&text), how, events))
        });
        match counted {
            Ok((found, how, events)) => {
                s.name = found.or(s.name);
                s.read = how;
                s.events = events;
            }
            Err(e) => s.read = CalendarRead::Failed(format!("{e:#}")),
        }
        s
    };
    let mut out = Vec::new();
    if let Some(c) = &cfg.calendar {
        out.push(status(Some(&c.source), c.name.as_ref(), None, None));
    }
    for (i, c) in cfg.extra_calendars.iter().enumerate() {
        out.push(status(
            c.source.as_deref(),
            c.name.as_ref(),
            Some(c.usage),
            Some(i),
        ));
    }
    out
}

/// Reads the calendar at `source` (a link or a .ics file) once, to check it
/// is a calendar before it is added, and keeps the copy of a link: its name
/// (X-WR-CALNAME), if it has one.
pub fn check_calendar(cfg: &Config, source: &str) -> Result<Option<String>> {
    let text = if is_link(source) {
        fetch_and_keep(cfg, source)?.0
    } else {
        calendar::fetch(source)?
    };
    if !text.contains("BEGIN:VEVENT") && !text.contains("BEGIN:VCALENDAR") {
        bail!("{source} is not a calendar (no BEGIN:VCALENDAR)");
    }
    let today = Utc::now().with_timezone(&cfg.timezone).date_naive();
    calendar::occurrences(&text, cfg.timezone, today, today)?;
    Ok(calendar::name(&text))
}

/// The calendar's timed entries, classified with the configured private
/// words, lunch title and home-working words and address.
pub fn timed_entries<'a>(cfg: &Config, cal: &'a [Occurrence]) -> Vec<TimedEntry<'a>> {
    let private_words = cfg.private_words();
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

/// Where the fixed workplace comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkplaceSource {
    /// `workplace` in the config.
    Configured,
    /// Not configured: the calendar's most frequent work location (see
    /// [`month::detect_workplace`]).
    Detected,
}

/// The fixed workplace for `month`: the configured one, else the one
/// detected from the calendar in the 60 days up to the month's end. None:
/// neither, so every calendar work entry with an address is a client
/// visit.
pub fn effective_workplace(
    cfg: &Config,
    cal: &[Occurrence],
    month: Month,
) -> Option<(String, WorkplaceSource)> {
    match &cfg.workplace {
        Some(w) => Some((w.clone(), WorkplaceSource::Configured)),
        None => month::detect_workplace(cfg, cal, month.last_day())
            .map(|w| (w, WorkplaceSource::Detected)),
    }
}

/// `cfg` with the workplace detected for `month` when none is configured
/// (see [`effective_workplace`]).
fn with_workplace<'a>(cfg: &'a Config, cal: &[Occurrence], month: Month) -> Cow<'a, Config> {
    match effective_workplace(cfg, cal, month) {
        Some((w, WorkplaceSource::Detected)) => Cow::Owned(Config {
            workplace: Some(w),
            ..cfg.clone()
        }),
        _ => Cow::Borrowed(cfg),
    }
}

/// The month's work calendar and norm hours, without trips or the state
/// file; the totals are the whole month's.
pub fn norm_month(cfg: &Config, month: Month, cal: &[Occurrence]) -> MonthReport {
    month::build(cfg, month, &[], cal, &State::default(), NaiveDate::MAX)
}

/// The month's days with totals up to and including `today` (with the
/// current time: today so far, see [`Today`]), with the lunch overrides
/// and recorded private and home-working time from the database, and the
/// workplace detected from the calendar when none is configured.
pub fn build_month(
    cfg: &Config,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
    today: impl Into<Today>,
) -> Result<MonthReport> {
    let state = State::load(&cfg.database)?;
    let cfg = with_workplace(cfg, cal, month);
    Ok(month::build(&cfg, month, trips, cal, &state, today))
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
/// to business or private count so. Without a configured workplace, the
/// one detected from the calendar is home ↔ work commuting.
pub fn compute_mileage(
    cfg: &Config,
    mcfg: &MileageConfig,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
) -> Result<MileageMonth> {
    let state = State::load(&cfg.database)?;
    let cfg = &*with_workplace(cfg, cal, month);
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
    let december = Month { year, month: 12 };
    let cal = calendar(cfg, december, mode)?.occurrences;
    let cfg = &*with_workplace(cfg, &cal, december);
    if mcfg.home.is_none() || cfg.workplace.is_none() {
        anyhow::bail!("the befordringsfradrag needs mileage.home and workplace in the config");
    }
    let trips = load_trips(cfg, None, december)?;
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
    let cfg = &*with_workplace(cfg, cal, report.month);
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
    /// The fixed workplace and where it comes from; none: neither
    /// configured nor detected.
    pub workplace: Option<(String, WorkplaceSource)>,
    /// Where today's work stands, when the month is the current one.
    pub work: Option<WorkToday>,
}

/// The month's days (today so far at `now`) and mileage as `report`
/// builds them (calendar time recorded first, see [`settle_private`]), the
/// private time from `now`'s day through [`PRIVATE_DAYS_AHEAD`] days later,
/// the workplace, and today's work state in the current month. `cal` may
/// reach past the month (see [`month_calendar`]); the days and mileage use
/// only the occurrences up to the month's end, like `report`.
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
    let report = build_month(cfg, month, trips, &up_to, now)?;
    let mileage = mileage_for(cfg, month, trips, &up_to)?;
    let state = State::load(&cfg.database)?;
    let private = private_time(
        &timed_entries(cfg, cal),
        &state,
        &cfg.private,
        today,
        today + Duration::days(PRIVATE_DAYS_AHEAD),
        now.naive_local(),
    );
    Ok(MonthView {
        report,
        mileage,
        private,
        workplace: effective_workplace(cfg, &up_to, month),
        work: month
            .contains(today)
            .then(|| work_today(cfg, &state, cal, trips, now)),
    })
}

/// Where today's work stands (see [`work_today`]).
#[derive(Debug, Clone, PartialEq)]
pub enum WorkState {
    /// Not at work, not working from home and no private time running;
    /// `late_by` how long since the planned start when it has passed,
    /// the planned end has not, and nothing was worked yet today.
    NotWorking { late_by: Option<Duration> },
    /// At a work address since arriving there (from the trips), without a
    /// departure yet.
    AtWork { place: String, since: DateTime<Tz> },
    /// Working from home: the home-working timer, or a home-working
    /// calendar event now.
    Home { since: DateTime<Tz> },
    /// Private time running: the timer (`auto` false), or a private
    /// appointment started automatically (`auto` true).
    Private {
        label: String,
        since: DateTime<Tz>,
        auto: bool,
    },
}

/// What the work card offers in a [`WorkState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkAction {
    /// Start the home-working timer.
    StartWork,
    /// Start private time for today's current or next private appointment.
    PrivateForAppointment,
    /// Start private time without a calendar event.
    PrivateWithoutEvent,
    /// Stop the home-working timer.
    StopWork,
    /// At a work address without a departure: you left now without a trip
    /// (see [`leave_work`]).
    LeaveWork,
    /// Stop the private timer, or the appointment started automatically.
    StopPrivate,
}

/// Today at some moment, for the work card.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkToday {
    pub state: WorkState,
    /// Hours worked so far today (see [`month::Day::worked_hours`]).
    pub worked: f64,
    /// Hours expected so far today (see [`month::Day::expected_hours`]).
    pub expected: f64,
    /// Today's planned working time (first start, last end).
    pub planned: Option<Span>,
    /// The first private appointment that is planned and has not ended,
    /// from today through [`PRIVATE_DAYS_AHEAD`] days later.
    pub next_private: Option<PrivateItem>,
    pub actions: Vec<WorkAction>,
}

impl WorkToday {
    /// Worked minus expected hours so far.
    pub fn difference(&self) -> f64 {
        self.worked - self.expected
    }
}

/// Where today's work stands at `now`: the private time running, else the
/// home-working timer, else the open period at a work address, else a
/// home-working calendar event now, else not working. `cal` and `trips`
/// cover today's month (`cal` may reach further, for the coming private
/// appointments); without a configured workplace the detected one counts.
pub fn work_today(
    cfg: &Config,
    state: &State,
    cal: &[Occurrence],
    trips: &[Trip],
    now: DateTime<Tz>,
) -> WorkToday {
    let today = now.date_naive();
    let month = Month {
        year: today.year(),
        month: today.month(),
    };
    let up_to: Vec<Occurrence> = cal
        .iter()
        .filter(|o| occurrence_date(o) <= month.last_day())
        .cloned()
        .collect();
    let cfg = &*with_workplace(cfg, &up_to, month);
    let report = month::build(cfg, month, trips, &up_to, state, now);
    let day = report
        .days
        .iter()
        .find(|d| d.date == today)
        .expect("today is in its month");
    let private = private_time(
        &timed_entries(cfg, cal),
        state,
        &cfg.private,
        today,
        today + Duration::days(PRIVATE_DAYS_AHEAD),
        now.naive_local(),
    );
    let local = |t: NaiveDateTime| {
        cfg.timezone
            .from_local_datetime(&t)
            .earliest()
            .unwrap_or(now)
    };
    let running_private = private.items.iter().find_map(|i| match i.state {
        PrivateState::Running { since } => Some((i.label.clone(), local(since), false)),
        PrivateState::StartedAutomatically { since } => Some((i.label.clone(), local(since), true)),
        _ => None,
    });
    // A home span running up to now: the timer, or a home-working event.
    let home_now = day.home.iter().find(|(_, end)| *end == now).map(|s| s.0);
    let state_now = if let Some((label, since, auto)) = running_private {
        WorkState::Private { label, since, auto }
    } else if let Some(r) = &state.home.running {
        WorkState::Home {
            since: local(r.start),
        }
    } else if let Some((since, place)) = &day.open {
        WorkState::AtWork {
            place: place.clone(),
            since: *since,
        }
    } else if let Some(since) = home_now {
        WorkState::Home { since }
    } else {
        let late_by = day
            .planned
            .filter(|(start, end)| *start < now && now < *end && day.first_start.is_none())
            .map(|(start, _)| now - start);
        WorkState::NotWorking { late_by }
    };
    let next_private = private
        .items
        .iter()
        .find(|i| {
            i.state == PrivateState::Planned
                && i.planned.is_some_and(|(_, end)| end > now.naive_local())
        })
        .cloned();
    let appointment_today = next_private
        .as_ref()
        .and_then(|i| i.planned)
        .is_some_and(|(start, _)| start.date() == today);
    let private_actions = || {
        appointment_today
            .then_some(WorkAction::PrivateForAppointment)
            .into_iter()
            .chain([WorkAction::PrivateWithoutEvent])
    };
    let actions = match &state_now {
        WorkState::NotWorking { .. } => vec![WorkAction::StartWork],
        WorkState::AtWork { .. } => private_actions().chain([WorkAction::LeaveWork]).collect(),
        WorkState::Home { .. } => private_actions()
            .chain(state.home.running.is_some().then_some(WorkAction::StopWork))
            .collect(),
        WorkState::Private { .. } => vec![WorkAction::StopPrivate],
    };
    WorkToday {
        state: state_now,
        worked: day.worked_hours,
        expected: day.expected_hours,
        planned: day.planned,
        next_private,
        actions,
    }
}

/// You left work at `at` without a trip ("Stop work (left early)"): an
/// arrival at a work address that day without a departure ends then. None
/// takes it back. A departure trip after the arrival still wins.
pub fn set_left_work(cfg: &Config, date: NaiveDate, at: Option<NaiveDateTime>) -> Result<()> {
    State::update(&cfg.database, |s| match at {
        Some(at) => {
            s.left_work.insert(date, at);
        }
        None => {
            s.left_work.remove(&date);
        }
    })
}

fn occurrence_date(o: &Occurrence) -> NaiveDate {
    match o {
        Occurrence::Timed { start, .. } | Occurrence::Private { start, .. } => start.date_naive(),
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
    let start = start_time(now, at)?;
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

/// A timer's start: `at` today, not later than `now`; else `now`.
fn start_time(now: DateTime<Tz>, at: Option<NaiveTime>) -> Result<NaiveDateTime> {
    match at {
        Some(t) => {
            let start = now.date_naive().and_time(t);
            if start > now.naive_local() {
                bail!("{} is in the future", t.format("%H:%M"));
            }
            Ok(start)
        }
        None => Ok(now.naive_local()),
    }
}

/// Starts the home-working timer at `now`, or at `at` earlier today (you
/// started working from home and forgot to start it).
pub fn start_home(cfg: &Config, now: DateTime<Tz>, at: Option<NaiveTime>) -> Result<RunningTimer> {
    let start = start_time(now, at)?;
    State::update(&cfg.database, |s| s.home.start(start, None, None).cloned())?
}

/// Stops the home-working timer at `now` and records the interval.
pub fn stop_home(cfg: &Config, now: NaiveDateTime) -> Result<PrivateInterval> {
    State::update(&cfg.database, |s| s.home.stop(now).cloned())?
}

/// `start`–`end` on `date` as an interval to record: the end after the
/// start, at most 24 hours, and not after `now`.
fn interval_on(
    date: NaiveDate,
    start: NaiveTime,
    end: NaiveTime,
    now: NaiveDateTime,
) -> Result<(NaiveDateTime, NaiveDateTime)> {
    let (s, e) = (
        state::minute(date.and_time(start)),
        state::minute(date.and_time(end)),
    );
    if e <= s {
        bail!(
            "end {} is not after start {}",
            end.format("%H:%M"),
            start.format("%H:%M")
        );
    }
    if e - s > Duration::hours(24) {
        bail!(
            "{date} {}–{} is over 24 hours",
            start.format("%H:%M"),
            end.format("%H:%M")
        );
    }
    if e > state::minute(now) {
        bail!("{date} {} is in the future", end.format("%H:%M"));
    }
    Ok((s, e))
}

/// The label typed, or the kind's default when it is empty.
fn label_or(label: &str, default: &str) -> String {
    match label.trim() {
        "" => default.to_string(),
        l => l.to_string(),
    }
}

/// Records private time you forgot to time: `start`–`end` on `date` (today
/// or earlier, not after `now`), with `label` (default
/// [`state::DEFAULT_LABEL`]). It replaces no calendar appointment.
pub fn add_private(
    cfg: &Config,
    date: NaiveDate,
    start: NaiveTime,
    end: NaiveTime,
    label: &str,
    now: NaiveDateTime,
) -> Result<PrivateInterval> {
    let (s, e) = interval_on(date, start, end, now)?;
    let label = label_or(label, state::DEFAULT_LABEL);
    State::update(&cfg.database, |st| {
        st.private
            .record(s, e, label, None, IntervalKind::Recorded)
            .clone()
    })
}

/// Records time worked from home that was not timed: `start`–`end` on
/// `date` (today or earlier, not after `now`), with `label` (default
/// [`state::HOME_LABEL`]).
pub fn add_home(
    cfg: &Config,
    date: NaiveDate,
    start: NaiveTime,
    end: NaiveTime,
    label: &str,
    now: NaiveDateTime,
) -> Result<PrivateInterval> {
    let (s, e) = interval_on(date, start, end, now)?;
    let label = label_or(label, state::HOME_LABEL);
    State::update(&cfg.database, |st| {
        st.home
            .record(s, e, label, None, IntervalKind::Recorded)
            .clone()
    })
}

/// Sets a home-working interval's label and start and end times; nothing
/// is saved when either change is refused.
pub fn change_home(
    cfg: &Config,
    id: u32,
    label: &str,
    start: NaiveTime,
    end: NaiveTime,
) -> Result<()> {
    let mut s = State::load(&cfg.database)?;
    s.home.edit(id, start, end)?;
    s.home.relabel(id, label)?;
    s.save(&cfg.database)
}

/// Deletes a home-working interval.
pub fn delete_home(cfg: &Config, id: u32) -> Result<PrivateInterval> {
    State::update(&cfg.database, |s| s.home.remove(id))?
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

/// A time span on one day, in local time.
pub type LocalSpan = (NaiveDateTime, NaiveDateTime);

/// What a part of the Day timeline is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineKind {
    Planned,
    /// At the workplace or working from home.
    Work,
    /// At a client (see [`month::Day::clients`]).
    Client,
    Private,
    Lunch,
    Drive,
}

/// The Day timeline on one line: the planned day first, then the rest in
/// time order without overlaps. Driving and private time cut what they
/// fall in, the lunch break cuts the work around it, and the time at a
/// client (part of `work`) shows as a client visit.
pub fn day_timeline(
    planned: Option<LocalSpan>,
    work: &[LocalSpan],
    clients: &[LocalSpan],
    lunch: Option<LocalSpan>,
    private: &[LocalSpan],
    drives: &[LocalSpan],
) -> Vec<(NaiveDateTime, NaiveDateTime, TimelineKind)> {
    let away: Vec<LocalSpan> = private.iter().chain(drives).copied().collect();
    let lunch = month::subtract(lunch.as_slice(), &away);
    let away: Vec<LocalSpan> = away.into_iter().chain(lunch.iter().copied()).collect();
    let client = month::subtract(clients, &away);
    let at_work = month::subtract(&month::subtract(work, clients), &away);
    let mut rest: Vec<(NaiveDateTime, NaiveDateTime, TimelineKind)> = [
        (at_work, TimelineKind::Work),
        (client, TimelineKind::Client),
        (private.to_vec(), TimelineKind::Private),
        (lunch, TimelineKind::Lunch),
        (drives.to_vec(), TimelineKind::Drive),
    ]
    .into_iter()
    .flat_map(|(spans, kind)| spans.into_iter().map(move |(s, e)| (s, e, kind)))
    .filter(|(s, e, _)| s < e)
    .collect();
    rest.sort_by_key(|(s, _, _)| *s);
    planned
        .map(|(s, e)| (s, e, TimelineKind::Planned))
        .into_iter()
        .chain(rest)
        .collect()
}

/// The time at work as the Day list shows it: the periods at work with
/// the time at a client (part of `work`) split out as client visits, in
/// time order. Unlike [`day_timeline`], lunch, private time and driving do
/// not cut it. Home-working time (part of `work`) is left out: the Day list
/// shows it as its own "Worked from home" entry, not as time at work.
pub fn day_work_steps(
    work: &[LocalSpan],
    clients: &[LocalSpan],
    home: &[LocalSpan],
) -> Vec<(NaiveDateTime, NaiveDateTime, TimelineKind)> {
    let away = month::subtract(work, home);
    let mut out: Vec<(NaiveDateTime, NaiveDateTime, TimelineKind)> =
        month::subtract(&away, clients)
            .into_iter()
            .map(|(s, e)| (s, e, TimelineKind::Work))
            .chain(clients.iter().map(|(s, e)| (*s, *e, TimelineKind::Client)))
            .filter(|(s, e, _)| s < e)
            .collect();
    out.sort_by_key(|(s, _, _)| *s);
    out
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

/// The account's vehicles, to pick the one to sync.
pub fn account_vehicles(access: &AccountAccess, token: &str) -> Result<Vec<Vehicle>> {
    let mut sleep = std::thread::sleep;
    let session = connected_cars::retry_429(&mut sleep, || {
        connected_cars::Session::login(access.namespace, token)
    })?;
    connected_cars::retry_429(&mut sleep, || session.vehicles())
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
    fn day_timeline_is_one_line_without_overlaps() {
        use TimelineKind::*;
        let span = |s: &str, e: &str| {
            (
                at(&format!("2026-11-19 {s}")),
                at(&format!("2026-11-19 {e}")),
            )
        };
        let hm = |t: NaiveDateTime| t.format("%H:%M").to_string();
        let rows = |line: Vec<(NaiveDateTime, NaiveDateTime, TimelineKind)>| {
            line.into_iter()
                .map(|(s, e, k)| (hm(s), hm(e), k))
                .collect::<Vec<_>>()
        };
        let want = |rows: &[(&str, &str, TimelineKind)]| {
            rows.iter()
                .map(|(s, e, k)| (s.to_string(), e.to_string(), *k))
                .collect::<Vec<_>>()
        };
        // At work 08:15–15:40 less private time 13:10–13:40; lunch 12:00–12:30.
        let work = [span("08:15", "13:10"), span("13:40", "15:40")];
        let private = [span("13:10", "13:40")];
        let drives = [span("07:52", "08:15"), span("15:40", "16:05")];
        let line = |clients: &[LocalSpan]| {
            rows(day_timeline(
                Some(span("08:00", "16:00")),
                &work,
                clients,
                Some(span("12:00", "12:30")),
                &private,
                &drives,
            ))
        };
        assert_eq!(
            line(&[]),
            want(&[
                ("08:00", "16:00", Planned),
                ("07:52", "08:15", Drive),
                ("08:15", "12:00", Work),
                ("12:00", "12:30", Lunch),
                ("12:30", "13:10", Work),
                ("13:10", "13:40", Private),
                ("13:40", "15:40", Work),
                ("15:40", "16:05", Drive),
            ])
        );
        // The afternoon at a client.
        assert_eq!(
            line(&[span("13:40", "15:40")])[6..],
            want(&[("13:40", "15:40", Client), ("15:40", "16:05", Drive)])
        );
        // Driving through the lunch break cuts it; lunch never covers private time.
        assert_eq!(
            rows(day_timeline(
                None,
                &[span("08:00", "16:00")],
                &[],
                Some(span("12:00", "12:30")),
                &[span("12:20", "12:40")],
                &[span("11:50", "12:10")],
            )),
            want(&[
                ("08:00", "11:50", Work),
                ("11:50", "12:10", Drive),
                ("12:10", "12:20", Lunch),
                ("12:20", "12:40", Private),
                ("12:40", "16:00", Work),
            ])
        );
    }

    #[test]
    fn day_work_steps_split_the_client_visit_out() {
        use TimelineKind::*;
        let span = |s: &str, e: &str| {
            (
                at(&format!("2026-10-22 {s}")),
                at(&format!("2026-10-22 {e}")),
            )
        };
        let hm = |t: NaiveDateTime| t.format("%H:%M").to_string();
        let rows = |work: &[LocalSpan], clients: &[LocalSpan]| {
            day_work_steps(work, clients, &[])
                .into_iter()
                .map(|(s, e, k)| (hm(s), hm(e), k))
                .collect::<Vec<_>>()
        };
        let want = |rows: &[(&str, &str, TimelineKind)]| {
            rows.iter()
                .map(|(s, e, k)| (s.to_string(), e.to_string(), *k))
                .collect::<Vec<_>>()
        };
        // At work 08:58–16:10 with a client 13:44–14:13 in the middle.
        assert_eq!(
            rows(&[span("08:58", "16:10")], &[span("13:44", "14:13")]),
            want(&[
                ("08:58", "13:44", Work),
                ("13:44", "14:13", Client),
                ("14:13", "16:10", Work),
            ])
        );
        // Ending the day at the client; no client is one step.
        assert_eq!(
            rows(&[span("08:58", "14:13")], &[span("13:44", "14:13")]),
            want(&[("08:58", "13:44", Work), ("13:44", "14:13", Client)])
        );
        assert_eq!(
            rows(&[span("08:58", "16:10")], &[]),
            want(&[("08:58", "16:10", Work)])
        );
        // Home working added afterwards is part of the worked time but not
        // time at work: it gets its own entry.
        let worked = [span("08:58", "13:35"), span("15:18", "18:14")];
        assert_eq!(
            day_work_steps(&worked, &[], &[span("15:18", "18:14")])
                .into_iter()
                .map(|(s, e, k)| (hm(s), hm(e), k))
                .collect::<Vec<_>>(),
            want(&[("08:58", "13:35", Work)])
        );
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
        assert_eq!(labels, ["Private", "Læge", "Tandlæge"]);
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

    /// A work calendar and two extra calendars as files: Personal (Private)
    /// with a gym session and an all-day "Ferie", Family (Words only) with
    /// a doctor's visit and a birthday.
    fn extra_setup(name: &str) -> Config {
        let dir = crate::trips::tests::temp_dir(name);
        let event = |summary: &str, start: &str, end: &str| {
            format!(
                "BEGIN:VEVENT\nUID:{summary}\nSUMMARY:{summary}\nLOCATION:Kunde A\n\
                 DTSTART;TZID=Europe/Copenhagen:{start}\nDTEND;TZID=Europe/Copenhagen:{end}\nEND:VEVENT\n"
            )
        };
        let ics = |name: &str, events: &[String]| {
            format!(
                "BEGIN:VCALENDAR\nX-WR-CALNAME:{name}\n{}END:VCALENDAR\n",
                events.concat()
            )
        };
        std::fs::write(
            dir.join("work.ics"),
            ics(
                "Work",
                &[event("Arbejde", "20261103T080000", "20261103T160000")],
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("personal.ics"),
            ics(
                "Personal",
                &[
                    event("Gym", "20261103T100000", "20261103T110000"),
                    "BEGIN:VEVENT\nUID:f\nSUMMARY:Ferie\nDTSTART;VALUE=DATE:20261104\nDTEND;VALUE=DATE:20261105\nEND:VEVENT\n".into(),
                ],
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("family.ics"),
            ics(
                "Family",
                &[
                    event("Læge", "20261103T130000", "20261103T140000"),
                    event("Fødselsdag", "20261103T180000", "20261103T200000"),
                ],
            ),
        )
        .unwrap();
        Config::from_text(
            "employee_number = \"1\"\n[danlon]\n[calendar]\nsource = \"work.ics\"\n\
             [[extra_calendars]]\nsource = \"personal.ics\"\nuse = \"private\"\n\
             [[extra_calendars]]\nsource = \"family.ics\"\nuse = \"words_only\"\n\
             [[extra_calendars]]\nid = \"elsewhere\"\nuse = \"private\"\nname = \"Old\"\n",
            &dir.join("config.toml"),
        )
        .unwrap()
    }

    #[test]
    fn extra_calendars_add_only_private_appointments() {
        let cfg = extra_setup("svc-extra");
        let month = Month {
            year: 2026,
            month: 11,
        };
        let cal = calendar(&cfg, month, CalendarMode::Offline).unwrap();
        assert_eq!(cal.warning, None);
        let kinds: Vec<(&str, EntryKind)> = timed_entries(&cfg, &cal.occurrences)
            .iter()
            .map(|e| (e.summary, e.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("Arbejde", EntryKind::Work),
                ("Gym", EntryKind::Private),
                ("Læge", EntryKind::Private),
            ]
        );

        // Hours and days off are the work calendar's alone.
        let mut work_only = cfg.clone();
        work_only.extra_calendars.clear();
        let alone = calendar(&work_only, month, CalendarMode::Offline).unwrap();
        let day = state::parse_date("2026-11-03").unwrap();
        let with = build_month(&cfg, month, &[], &cal.occurrences, day).unwrap();
        let without = build_month(&work_only, month, &[], &alone.occurrences, day).unwrap();
        for (a, b) in with.days.iter().zip(&without.days) {
            assert_eq!(
                (a.norm_hours, a.planned, &a.off_reason, &a.work_locations),
                (b.norm_hours, b.planned, &b.off_reason, &b.work_locations),
                "{}",
                a.date
            );
        }
        let tuesday = with.days.iter().find(|d| d.date == day).unwrap();
        assert_eq!(tuesday.appointments.len(), 2);

        // Settings: names from the files, events in the month, the one
        // without a link on this device.
        let statuses: Vec<_> = calendar_statuses(&cfg, month)
            .into_iter()
            .map(|s| (s.name, s.usage, s.read, s.events))
            .collect();
        assert_eq!(
            statuses,
            [
                (Some("Work".into()), None, CalendarRead::File, 1),
                (
                    Some("Personal".into()),
                    Some(CalendarUse::Private),
                    CalendarRead::File,
                    1
                ),
                (
                    Some("Family".into()),
                    Some(CalendarUse::WordsOnly),
                    CalendarRead::File,
                    1
                ),
                (
                    Some("Old".into()),
                    Some(CalendarUse::Private),
                    CalendarRead::NoLink,
                    0
                ),
            ]
        );
    }

    #[test]
    fn an_extra_calendar_link_uses_the_kept_copy_and_a_missing_one_is_named() {
        let mut cfg = extra_setup("svc-extra-link");
        let link = "http://127.0.0.1:1/personal.ics";
        cfg.extra_calendars[0].source = Some(link.into());
        cfg.extra_calendars[0].name = Some("Personal".into());
        let month = Month {
            year: 2026,
            month: 11,
        };
        let cal = calendar(&cfg, month, CalendarMode::Fresh).unwrap();
        let warning = cal.warning.unwrap();
        assert!(warning.contains("\"Personal\" can't be read"), "{warning}");
        assert!(check_calendar(&cfg, link).is_err());
        assert!(refresh_calendar(&cfg).is_err());

        let text = std::fs::read_to_string(cfg.database.with_file_name("personal.ics")).unwrap();
        db::store_calendar(&cfg.database, link, &text, Utc::now()).unwrap();
        for mode in [CalendarMode::Fresh, CalendarMode::Offline] {
            let cal = calendar(&cfg, month, mode).unwrap();
            assert_eq!(cal.warning, None);
            assert!(
                timed_entries(&cfg, &cal.occurrences)
                    .iter()
                    .any(|e| e.summary == "Gym" && e.kind == EntryKind::Private)
            );
        }
        assert_eq!(
            check_calendar(&cfg, cfg.extra_calendars[1].source.as_deref().unwrap()).unwrap(),
            Some("Family".into())
        );
        let not_ics = cfg.database.with_file_name("config.toml");
        std::fs::write(&not_ics, "employee_number = 1").unwrap();
        let err = check_calendar(&cfg, not_ics.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("is not a calendar"), "{err}");
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

    /// The user's setup without `workplace`: home in Silkeborg, `days` office
    /// days at Lysbrohøjen 3 in the calendar from 2026-10-19.
    fn no_workplace(name: &str, days: u32) -> (std::path::PathBuf, Config, Vec<Occurrence>) {
        let dir = crate::trips::tests::temp_dir(name);
        let cfg = Config::from_text(
            "employee_number = \"1\"\n[danlon]\nmileage_lonart = \"39\"\n\
             [calendar]\nsource = \"cal.ics\"\n\
             [mileage]\ndriver_name = \"T\"\nregistration_number = \"AB1\"\n\
             home = \"Arendalsvej 192, 2. tv., 8600 Silkeborg\"\n\
             [[mileage.rates]]\nyear = 2026\nlow = 3.94\nhigh = 2.28\n",
            &dir.join("config.toml"),
        )
        .unwrap();
        let tz = cfg.timezone;
        let cal = (0..days)
            .map(|i| {
                let day = state::parse_date("2026-10-19").unwrap() + Duration::days(i.into());
                let t = |h| {
                    day.and_hms_opt(h, 0, 0)
                        .unwrap()
                        .and_local_timezone(tz)
                        .unwrap()
                };
                Occurrence::Timed {
                    start: t(8),
                    end: t(16),
                    summary: "Kontor".into(),
                    location: "Lysbrohøjen 3, 8600 Silkeborg".into(),
                }
            })
            .collect();
        (dir, cfg, cal)
    }

    #[test]
    fn without_a_workplace_the_one_in_the_calendar_is_commuting() {
        let month = Month::parse("2026-10").unwrap();
        let trip = |s: &str, e: &str, from: &str, to: &str| Trip {
            start: trips::parse_time(s, chrono_tz::Europe::Copenhagen).unwrap(),
            end: trips::parse_time(e, chrono_tz::Europe::Copenhagen).unwrap(),
            distance_km: Some(6.5),
            from: from.into(),
            to: to.into(),
            purpose: String::new(),
            business: None,
            start_pos: None,
            end_pos: None,
        };
        let (home, work) = ("Arendalsvej, 8600 Silkeborg", "Lysbrohøjen, Lysbro");
        let trips = [
            trip("2026-10-20 07:40", "2026-10-20 07:55", home, work),
            trip("2026-10-20 16:00", "2026-10-20 16:15", work, home),
        ];

        let (dir, cfg, cal) = no_workplace("svc-workplace-detected", 5);
        assert_eq!(
            effective_workplace(&cfg, &cal, month),
            Some((
                "Lysbrohøjen 3, 8600 Silkeborg".into(),
                WorkplaceSource::Detected
            ))
        );
        let m = mileage_for(&cfg, month, &trips, &cal).unwrap().unwrap();
        assert_eq!(m.total_km(), 0.0);
        let reasons: Vec<_> = m.driving.iter().map(|d| d.reason.clone()).collect();
        assert_eq!(
            reasons,
            [mileage::Reason::Commute, mileage::Reason::Commute]
        );
        // The month counts the detected workplace as at work too.
        let r = build_month(&cfg, month, &trips, &cal, NaiveDate::MAX).unwrap();
        assert_eq!(r.days[19].periods_text(), "07:55–16:00");
        std::fs::remove_dir_all(&dir).unwrap();

        // Two days are no clear candidate: none, and the drives are client
        // visits (`report` and `logbook` warn).
        let (dir, cfg, cal) = no_workplace("svc-workplace-none", 2);
        assert_eq!(effective_workplace(&cfg, &cal, month), None);
        let m = mileage_for(&cfg, month, &trips, &cal).unwrap().unwrap();
        assert_eq!(m.total_km(), 13.0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn work_today_states() {
        // Office at Lysbrohøjen 08:00–16:00 on 19–23 Oct (detected
        // workplace), Læge 13:00–14:00 on the 20th.
        let (dir, cfg, mut cal) = no_workplace("svc-work-today", 5);
        let tz = cfg.timezone;
        let now = |s: &str| at(s).and_local_timezone(tz).unwrap();
        cal.push(Occurrence::Timed {
            start: now("2026-10-20 13:00"),
            end: now("2026-10-20 14:00"),
            summary: "Læge".into(),
            location: String::new(),
        });
        let arrive = Trip {
            start: now("2026-10-20 08:50"),
            end: now("2026-10-20 09:10"),
            distance_km: Some(6.5),
            from: "Arendalsvej, 8600 Silkeborg".into(),
            to: "Lysbrohøjen, Lysbro".into(),
            purpose: String::new(),
            business: None,
            start_pos: None,
            end_pos: None,
        };
        let st = State::default();
        let w = |st: &State, trips: &[Trip], t: &str| work_today(&cfg, st, &cal, trips, now(t));

        // Not working, 40 min late; the Læge appointment is next.
        let t = w(&st, &[], "2026-10-20 08:40");
        assert_eq!(
            t.state,
            WorkState::NotWorking {
                late_by: Some(Duration::minutes(40))
            }
        );
        assert_eq!(t.actions, [WorkAction::StartWork]);
        assert_eq!((t.expected, t.worked), (40.0 / 60.0, 0.0));
        assert_eq!(t.next_private.as_ref().unwrap().label, "Læge");
        // Before the planned start: not late.
        let t = w(&st, &[], "2026-10-20 07:00");
        assert_eq!(t.state, WorkState::NotWorking { late_by: None });

        // At the office by trip since 09:10.
        let t = w(&st, std::slice::from_ref(&arrive), "2026-10-20 10:10");
        assert_eq!(
            t.state,
            WorkState::AtWork {
                place: "Lysbrohøjen 3, 8600 Silkeborg".into(),
                since: now("2026-10-20 09:10")
            }
        );
        let min = |h: f64| (h * 60.0).round() as i64;
        assert_eq!(
            (min(t.expected), min(t.worked), min(t.difference())),
            (130, 60, -70)
        );
        assert_eq!(
            t.actions,
            [
                WorkAction::PrivateForAppointment,
                WorkAction::PrivateWithoutEvent,
                WorkAction::LeaveWork
            ]
        );

        // Left work at 10:40 without a trip: the period ends then and the
        // day is not working any more; a later departure trip still wins.
        let mut left = State::default();
        left.left_work
            .insert(at("2026-10-20 10:40").date(), at("2026-10-20 10:40"));
        let t = w(&left, std::slice::from_ref(&arrive), "2026-10-20 11:00");
        assert_eq!(t.state, WorkState::NotWorking { late_by: None });
        assert_eq!(min(t.worked), 90);
        let depart = Trip {
            start: now("2026-10-20 10:50"),
            end: now("2026-10-20 11:05"),
            from: arrive.to.clone(),
            to: arrive.from.clone(),
            ..arrive.clone()
        };
        let t = w(&left, &[arrive.clone(), depart], "2026-10-20 12:00");
        assert_eq!(min(t.worked), 100);

        // The Læge appointment started automatically at 13:00.
        let t = w(&st, std::slice::from_ref(&arrive), "2026-10-20 13:10");
        assert_eq!(
            t.state,
            WorkState::Private {
                label: "Læge".into(),
                since: now("2026-10-20 13:00"),
                auto: true
            }
        );
        assert_eq!(t.actions, [WorkAction::StopPrivate]);
        assert_eq!(t.next_private, None);

        // Home timer since 08:05.
        let mut home = State::default();
        home.home.start(at("2026-10-20 08:05"), None, None).unwrap();
        let t = w(&home, &[], "2026-10-20 09:05");
        assert_eq!(
            t.state,
            WorkState::Home {
                since: now("2026-10-20 08:05")
            }
        );
        assert_eq!((t.worked, t.expected), (1.0, 65.0 / 60.0));
        assert_eq!(
            t.actions,
            [
                WorkAction::PrivateForAppointment,
                WorkAction::PrivateWithoutEvent,
                WorkAction::StopWork
            ]
        );

        // The private timer running wins over the home timer.
        home.private
            .start(at("2026-10-20 09:00"), Some("Bank".into()), None)
            .unwrap();
        let t = w(&home, &[], "2026-10-20 09:05");
        assert_eq!(
            t.state,
            WorkState::Private {
                label: "Bank".into(),
                since: now("2026-10-20 09:00"),
                auto: false
            }
        );
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

    #[test]
    fn forgotten_private_and_home_time_is_added_changed_and_deleted() {
        let (dir, cfg, _) = private_setup("svc-add-time", "");
        let date = state::parse_date("2026-11-03").unwrap();
        let t = |s: &str| state::parse_time(s).unwrap();
        let now = at("2026-11-03 15:00");
        let p = add_private(&cfg, date, t("10:00"), t("11:15"), " Doctor ", now).unwrap();
        assert_eq!(
            (p.start, p.end, p.label.as_str(), p.kind),
            (
                at("2026-11-03 10:00"),
                at("2026-11-03 11:15"),
                "Doctor",
                IntervalKind::Recorded
            )
        );
        assert!(p.replaces.is_none());
        let h = add_home(&cfg, date, t("13:00"), t("14:30"), "", now).unwrap();
        assert_eq!(h.label, state::HOME_LABEL);
        assert_eq!(
            add_private(&cfg, date, t("10:00"), t("10:00"), "", now)
                .unwrap_err()
                .to_string(),
            "end 10:00 is not after start 10:00"
        );
        assert!(
            add_home(&cfg, date, t("14:00"), t("15:30"), "", now)
                .unwrap_err()
                .to_string()
                .contains("in the future")
        );
        let tomorrow = date + Duration::days(1);
        assert!(add_private(&cfg, tomorrow, t("08:00"), t("09:00"), "", now).is_err());
        let s = State::load(&cfg.database).unwrap();
        assert_eq!((s.private.intervals.len(), s.home.intervals.len()), (1, 1));

        // Changed: both or nothing.
        change_home(&cfg, h.id, "Home office", t("12:30"), t("14:00")).unwrap();
        assert!(change_home(&cfg, h.id, " ", t("08:00"), t("09:00")).is_err());
        assert!(change_home(&cfg, h.id, "x", t("09:00"), t("08:00")).is_err());
        assert!(change_home(&cfg, 99, "x", t("08:00"), t("09:00")).is_err());
        let i = State::load(&cfg.database).unwrap().home.intervals[0].clone();
        assert_eq!(
            (i.start, i.end, i.label.as_str()),
            (
                at("2026-11-03 12:30"),
                at("2026-11-03 14:00"),
                "Home office"
            )
        );

        assert_eq!(delete_home(&cfg, h.id).unwrap().label, "Home office");
        assert!(delete_home(&cfg, h.id).is_err());
        let s = State::load(&cfg.database).unwrap();
        assert!(s.home.intervals.is_empty());
        assert_eq!(s.private.intervals.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_home_timer_starts_at_an_earlier_time_today() {
        let (dir, cfg, _) = private_setup("svc-home-earlier", "");
        let now = cfg
            .timezone
            .from_local_datetime(&at("2026-11-03 09:10"))
            .unwrap();
        let t = |s: &str| state::parse_time(s).unwrap();
        assert!(
            start_home(&cfg, now, Some(t("09:30")))
                .unwrap_err()
                .to_string()
                .contains("in the future")
        );
        let r = start_home(&cfg, now, Some(t("07:45"))).unwrap();
        assert_eq!(r.start, at("2026-11-03 07:45"));
        assert!(start_home(&cfg, now, None).is_err());
        let i = stop_home(&cfg, at("2026-11-03 12:00")).unwrap();
        assert_eq!(
            (i.start, i.end),
            (at("2026-11-03 07:45"), at("2026-11-03 12:00"))
        );
        assert!(stop_home(&cfg, at("2026-11-03 12:05")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
