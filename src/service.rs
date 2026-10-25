//! The steps the CLI and the app run to produce a month's data: loading the
//! calendar and the trips, building the month, its mileage and checklist,
//! the vacation overview, and the Min Volkswagen account access and sync.
//! Nothing here prints or prompts; warnings come back as values.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, NaiveDateTime};

use crate::calendar::{self, Occurrence};
use crate::checklist::{self, ChecklistDay};
use crate::config::{Config, MileageConfig};
use crate::connected_cars::{self, TokenStore, Vehicle};
use crate::mileage::{self, MileageMonth};
use crate::month::{self, Month, MonthReport, TimedEntry};
use crate::state::{self, RunningTimer, State};
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

/// The private timer when it has been running so long at `now` that it was
/// likely forgotten.
pub fn forgotten_timer(cfg: &Config, now: NaiveDateTime) -> Result<Option<RunningTimer>> {
    Ok(State::load(&cfg.state_file)?
        .private
        .running
        .filter(|r| r.forgotten(now)))
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

    /// Deletes `email`'s device token and forgets the account; true when a
    /// token was deleted.
    pub fn disconnect(&self, cfg: &Config, email: &str) -> Result<bool> {
        let deleted = self.tokens.delete(email)?;
        State::update(&cfg.state_file, |s| s.account = None)?;
        Ok(deleted)
    }
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
    let from = match from {
        Some(f) => state::parse_date(f)?,
        None => trip_store::sync_start(&cfg.trip_store, cfg.timezone, today)?,
    };
    if from > today {
        anyhow::bail!("--from {from} is after today");
    }
    let mut sleep = std::thread::sleep;
    let session = connected_cars::retry_429(&mut sleep, || {
        connected_cars::Session::login(access.namespace, &token)
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
