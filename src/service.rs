//! The steps the CLI and the app run to produce a month's data: loading the
//! calendar and the trips, building the month, its mileage and checklist,
//! the vacation overview, and the Min Volkswagen account access and sync.
//! Nothing here prints or prompts; warnings come back as values.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use chrono_tz::Tz;

use crate::calendar::{self, Occurrence};
use crate::checklist::{self, ChecklistDay};
use crate::config::{Config, MileageConfig};
use crate::connected_cars::{self, TokenStore, Vehicle};
use crate::mileage::{self, MileageMonth, Status};
use crate::month::{self, Month, MonthReport, TimedEntry};
use crate::state::{self, EventChoice, Private, RunningTimer, State};
use crate::trip_store::{self, Synced};
use crate::trips::{self, ExportTrip, Trip};
use crate::vacation::{self, Overview, Rules};

/// Trips from `file`, else from the trip store.
pub fn load_trips(cfg: &Config, file: Option<&Path>) -> Result<Vec<Trip>> {
    if let Some(file) = file {
        return trips::load(file, cfg.timezone);
    }
    trip_store::stored_trips(&cfg.trip_store, cfg.timezone)
}

/// Calendar occurrences, with the warning to show when the calendar has no
/// name to check against `calendar.name`.
#[derive(Debug, Default)]
pub struct Calendar {
    pub occurrences: Vec<Occurrence>,
    pub warning: Option<String>,
}

/// Calendar occurrences from 1 January to the end of `month`; the earlier
/// months are needed for the year's mileage total.
pub fn calendar(cfg: &Config, month: Month) -> Result<Calendar> {
    let year_start = NaiveDate::from_ymd_opt(month.year, 1, 1).unwrap();
    calendar_between(cfg, year_start, month.last_day())
}

/// Calendar occurrences from `from` to `to`.
pub fn calendar_between(cfg: &Config, from: NaiveDate, to: NaiveDate) -> Result<Calendar> {
    let Some(cal) = &cfg.calendar else {
        return Ok(Calendar::default());
    };
    let text = calendar::fetch(&cal.source)?;
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
    })
}

/// The calendar's timed entries, classified with the configured private
/// words and lunch title.
pub fn timed_entries<'a>(cfg: &Config, cal: &'a [Occurrence]) -> Vec<TimedEntry<'a>> {
    let private_words = cfg
        .calendar
        .as_ref()
        .map(|c| c.private_keywords.as_slice())
        .unwrap_or_default();
    month::timed_entries(cal, private_words, &cfg.lunch.title)
}

/// The month's work calendar and norm hours, without trips or the state
/// file.
pub fn norm_month(cfg: &Config, month: Month, cal: &[Occurrence]) -> MonthReport {
    month::build(cfg, month, &[], cal, &BTreeMap::new(), &[])
}

/// The month's days, with the lunch overrides and recorded private time
/// from the state file.
pub fn build_month(
    cfg: &Config,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
) -> Result<MonthReport> {
    let state = State::load(&cfg.state_file)?;
    Ok(month::build(
        cfg,
        month,
        trips,
        cal,
        &state.lunch,
        &state.private.intervals,
    ))
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
/// private, like those during a private appointment.
pub fn compute_mileage(
    cfg: &Config,
    mcfg: &MileageConfig,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
) -> Result<MileageMonth> {
    let recorded = State::load(&cfg.state_file)?.private.intervals;
    let mut entries = timed_entries(cfg, cal);
    entries.extend(month::recorded_entries(&recorded, cfg.timezone));
    mileage::compute(
        mcfg,
        cfg.workplace.as_deref(),
        &cfg.known_places,
        month,
        trips,
        &entries,
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

/// The month's registrations and ticks, as `checklist <month>` prints them.
/// `cal` runs from 1 January through the month (see [`calendar`]).
pub fn checklist_view(
    cfg: &Config,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
) -> Result<ChecklistView> {
    let report = build_month(cfg, month, trips, cal)?;
    let mileage = mileage_for(cfg, month, trips, cal)?;
    Ok(ChecklistView {
        days: checklist_days(cfg, &report, trips, mileage.as_ref(), cal),
        entered: State::load(&cfg.state_file)?.entered,
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
        State::update(&cfg.state_file, |s| s.entered.insert(date))
    } else {
        State::update(&cfg.state_file, |s| s.entered.remove(&date))
    }
}

/// The private timer when it has been running so long at `now` that it was
/// likely forgotten.
pub fn forgotten_timer(cfg: &Config, now: NaiveDateTime) -> Result<Option<RunningTimer>> {
    Ok(State::load(&cfg.state_file)?
        .private
        .running
        .filter(|r| r.forgotten(now)))
}

/// How many days after today the Month screen lists coming private
/// appointments.
pub const PRIVATE_DAYS_AHEAD: i64 = 14;

/// The calendar for the Month screen: what [`calendar`] fetches, through
/// [`PRIVATE_DAYS_AHEAD`] days after `today` for the coming private
/// appointments.
pub fn month_calendar(cfg: &Config, month: Month, today: NaiveDate) -> Result<Calendar> {
    let year_start = NaiveDate::from_ymd_opt(month.year, 1, 1).unwrap();
    let to = month
        .last_day()
        .max(today + Duration::days(PRIVATE_DAYS_AHEAD));
    calendar_between(cfg, year_start, to)
}

/// A month as the Month and Day screens show it.
#[derive(Debug)]
pub struct MonthView {
    pub report: MonthReport,
    pub mileage: Option<MileageMonth>,
    /// Today's and the coming private time.
    pub private: PrivateTime,
}

/// The month's days and mileage as `report` builds them, and the private
/// time from `now`'s day through [`PRIVATE_DAYS_AHEAD`] days later. `cal`
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
    let report = build_month(cfg, month, trips, &up_to)?;
    let mileage = mileage_for(cfg, month, trips, &up_to)?;
    let today = now.date_naive();
    let private = private_time(
        &timed_entries(cfg, cal),
        &State::load(&cfg.state_file)?.private,
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
    /// Measured with the timer; `id` is the recorded interval's.
    Recorded {
        id: u32,
        start: NaiveDateTime,
        end: NaiveDateTime,
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
    /// The item to show first: the running one, else the first planned
    /// appointment that has not ended at `now`.
    pub current: Option<usize>,
}

/// The private appointments and recorded intervals starting `from`
/// through `to`, and the running timer when it started then or `now` is
/// in that range. A recording or timer linked to an appointment is shown
/// as that appointment.
pub fn private_time(
    entries: &[TimedEntry],
    private: &Private,
    from: NaiveDate,
    to: NaiveDate,
    now: NaiveDateTime,
) -> PrivateTime {
    let in_range = |d: NaiveDate| from <= d && d <= to;
    let mut items: Vec<PrivateItem> = entries
        .iter()
        .filter(|e| e.kind == month::EntryKind::Private && in_range(e.start.date_naive()))
        .map(|e| PrivateItem {
            label: e.summary.to_string(),
            planned: Some((e.start.naive_local(), e.end.naive_local())),
            state: PrivateState::Planned,
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

/// Starts the private timer at `now`, linked to the private calendar
/// appointment that day that `choice` names, and saves it.
pub fn start_private(
    cfg: &Config,
    cal: &[Occurrence],
    now: DateTime<Tz>,
    choice: &EventChoice,
) -> Result<RunningTimer> {
    let window = state::link_event(&timed_entries(cfg, cal), now, choice)?;
    State::update(&cfg.state_file, |s| {
        s.private.start(now.naive_local(), None, window).cloned()
    })?
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
    let mut s = State::load(&cfg.state_file)?;
    s.private.edit(id, start, end)?;
    s.private.relabel(id, label)?;
    s.save(&cfg.state_file)
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
    let path = &cfg.state_file;
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
    trip_store::add_manual(&cfg.trip_store, &trip)?;
    Ok(trip)
}

/// Deletes the trip added by hand that starts at `start`.
pub fn delete_manual_trip(cfg: &Config, start: DateTime<Tz>) -> Result<()> {
    trip_store::delete_manual(&cfg.trip_store, start.fixed_offset()).map(|_| ())
}

/// The start times of the trips added by hand, in the config's timezone.
pub fn manual_trip_starts(cfg: &Config) -> Result<BTreeSet<DateTime<Tz>>> {
    Ok(trip_store::TripStore::load(&cfg.trip_store)?
        .manual_starts()
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
}

/// The vacation overview: the calendar from the oldest open holiday year's
/// start to the newest one's use-by date.
pub fn vacation(cfg: &Config, today: NaiveDate) -> Result<VacationView> {
    let open = vacation::open_years(today);
    let from = open[0].first_day();
    let to = open[open.len() - 1].use_by();
    let cal = calendar_between(cfg, from, to)?;
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
    State::update(&cfg.state_file, |s| {
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
    State::update(&cfg.state_file, |s| s.account = None)
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
    let state = State::load(&cfg.state_file)?;
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
        None => trip_store::sync_start(&cfg.trip_store, cfg.timezone, today)?,
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
    let synced = trip_store::sync(&cfg.trip_store, from, today, cfg.timezone, |first, last| {
        connected_cars::retry_429(&mut sleep, || {
            session.trips_on_days(&vehicle.id, first, last, cfg.timezone)
        })
    })?;
    let email = access.email.clone().unwrap_or_default();
    State::update(&cfg.state_file, |s| {
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
        let mut p = Private::default();
        p.start(at("2026-11-24 10:02"), None, None).unwrap();
        p.stop(at("2026-11-24 11:14")).unwrap();
        let day = |s: &str| state::parse_date(s).unwrap();
        let view = |p: &Private, now: &str| {
            private_time(&entries, p, day("2026-11-24"), day("2026-12-08"), at(now))
        };

        // Nothing running: the next appointment comes first.
        let t = view(&p, "2026-11-24 12:00");
        let labels: Vec<&str> = t.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Privat", "Læge", "Tandlæge"]);
        assert_eq!(t.current, Some(1));
        assert!(matches!(
            t.items[0].state,
            PrivateState::Recorded { id: 1, .. }
        ));

        // A timer linked to Læge shows as Læge, running.
        p.start(
            at("2026-11-24 12:30"),
            None,
            window("Læge", "2026-11-24 13:00", "2026-11-24 14:00"),
        )
        .unwrap();
        let t = view(&p, "2026-11-24 12:40");
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
        p.stop(at("2026-11-24 14:10")).unwrap();
        p.relabel(2, "Læge (blodprøve)").unwrap();
        let t = view(&p, "2026-11-24 14:20");
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
        let mut p = Private::default();
        p.start(at("2026-11-23 16:00"), None, None).unwrap();
        let t = view(&p, "2026-11-24 08:00");
        assert_eq!(t.items[0].label, state::DEFAULT_LABEL);
        assert_eq!(t.current, Some(0));
        assert!(t.running.is_some());
    }

    #[test]
    fn a_refused_change_saves_nothing() {
        let dir = std::env::temp_dir().join(format!("vw-danlon-change-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let cfg = Config::from_text("employee_number = \"1\"\n[danlon]\n", &path).unwrap();
        State::update(&cfg.state_file, |s| {
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
        let i = &State::load(&cfg.state_file).unwrap().private.intervals[0];
        assert_eq!((i.label.as_str(), i.end), ("Bank", at("2026-11-03 13:00")));

        change_private(&cfg, 1, "Post", t("12:15"), t("12:40")).unwrap();
        let i = &State::load(&cfg.state_file).unwrap().private.intervals[0];
        assert_eq!(
            (i.label.as_str(), i.start, i.end),
            ("Post", at("2026-11-03 12:15"), at("2026-11-03 12:40"))
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
            let trips = load_trips(&cfg, None).unwrap();
            let report = build_month(&cfg, month, &trips, &[]).unwrap();
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
        assert_eq!(load_trips(&cfg, None).unwrap()[0].distance_km, Some(12.5));

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
}
