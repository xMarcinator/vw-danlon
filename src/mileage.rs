//! Tax-free mileage allowance (skattefri kørselsgodtgørelse).
//!
//! Follows Skattestyrelsen's requirements: each reimbursed trip must state
//! date, destination (mål og delmål), business purpose and km, and the
//! allowance is calculated at the year's rates — the low rate up to
//! 20,000 km per year, the high rate above. Commuting between home and a
//! fixed workplace never qualifies.
//!
//! Calendar entries away from the fixed workplace are business visits: the
//! day's first trip arriving at the entry's address and the last trip
//! leaving it are business trips, with the entry's title as purpose. Trips
//! in between (a lunch errand) are not. The rules are Skat's (LL § 9 B) and
//! cannot be switched off:
//!
//! - home ↔ a client is business on at most 60 days per client within 12
//!   months (see [`SixtyDays`]); workplace ↔ client always;
//! - client → stops → home (or the workplace) is reimbursed at the direct
//!   km only: the shortest drive between the two recorded before;
//! - driving without a business purpose (errands) is private;
//! - trips you set to business or private win over the rules.
//!
//! Private appointments (see [`crate::month::EntryKind::Private`]) are never
//! business visits: a trip arriving at or departing from an appointment's
//! address, or starting during one, is private. Recorded private time (see
//! [`crate::month::recorded_entries`]) counts as an appointment without an
//! address.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Timelike};
use chrono_tz::Tz;

use crate::address::{self, Places};
use crate::config::{FradragRate, MileageConfig, MileageRate};
use crate::month::{EntryKind, Month, TimedEntry};
use crate::trips::Trip;

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Business,
    Commute,
    Private,
    /// Marked or looks like business but lacks required documentation.
    Incomplete(&'static str),
}

/// The rule that decided how a trip counts.
#[derive(Debug, Clone, PartialEq)]
pub enum Reason {
    /// To or from a client: a calendar work entry away from home and the
    /// workplace, or the trip's own business purpose.
    ClientVisit,
    /// Client → stops → home (or the workplace): only the direct km of a
    /// drive recorded earlier between the two places.
    DirectKm,
    /// You set the trip to business or private (`trip business|private`).
    SetByYou,
    /// Home ↔ the fixed workplace.
    Commute,
    /// Home ↔ a client from the 61st day there within 12 months.
    SixtyDays { day: usize },
    /// The stops of a client → stops → home drive.
    Detour,
    /// Driving without a business purpose on a working day.
    Errand,
    /// Other driving without a business purpose.
    Private,
    /// To, from or during a private appointment or recorded private time.
    Appointment,
    /// Marked private in the trips file.
    MarkedPrivate,
    /// Business, but documentation is missing.
    Incomplete(&'static str),
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Reason::ClientVisit => write!(f, "client visit (LL § 9 B)"),
            Reason::DirectKm => write!(f, "client visit, direct km only"),
            Reason::SetByYou => write!(f, "set by you"),
            Reason::Commute => write!(f, "commute (LL § 9 B)"),
            Reason::SixtyDays { day } => write!(f, "60-day rule (day {day} at this client)"),
            Reason::Detour => write!(f, "detour after client visit"),
            Reason::Errand => write!(f, "errand (private)"),
            Reason::Private => write!(f, "private driving"),
            Reason::Appointment => write!(f, "private appointment"),
            Reason::MarkedPrivate => write!(f, "marked private in the trips file"),
            Reason::Incomplete(why) => write!(f, "{why}"),
        }
    }
}

/// A trip with how the rules count it.
#[derive(Debug, Clone)]
pub struct Driven {
    pub trip: Trip,
    pub status: Status,
    pub reason: Reason,
    pub purpose: String,
    /// The reimbursed km of this trip.
    pub km: f64,
}

impl Driven {
    /// The km driven that are not reimbursed.
    pub fn private_km(&self) -> f64 {
        (self.trip.distance_km.unwrap_or(0.0) - self.km).max(0.0)
    }

    /// The rule the private km fall under.
    pub fn private_reason(&self) -> Reason {
        match (&self.status, &self.reason) {
            (Status::Business, Reason::SetByYou) => Reason::SetByYou,
            (Status::Business, _) => Reason::Detour,
            (_, reason) => reason.clone(),
        }
    }
}

/// A trip's start to the minute, as overrides are kept (see
/// [`crate::state::State::trip_overrides`]).
pub fn minute(t: DateTime<Tz>) -> DateTime<FixedOffset> {
    let t = t.fixed_offset();
    t - chrono::Duration::seconds(i64::from(t.second()))
        - chrono::Duration::nanoseconds(i64::from(t.nanosecond()))
}

/// Whether a trip arrives at or departs from the address of a private
/// appointment on the trip's date, or starts during one.
pub fn touches_appointment(trip: &Trip, calendar: &[TimedEntry], places: &Places) -> bool {
    calendar
        .iter()
        .filter(|e| e.kind == EntryKind::Private)
        .any(|e| {
            let at = |place: &str, pos| {
                e.start.date_naive() == trip.start.date_naive() && places.at(place, pos, e.location)
            };
            at(&trip.to, trip.end_pos)
                || at(&trip.from, trip.start_pos)
                || (e.start <= trip.start && trip.start < e.end)
        })
}

/// Each trip's link to the day's client visits (calendar work entries away
/// from the workplace): the visit address it is the day's first arrival
/// at, the one it is the day's last departure from, and its purpose (its
/// own, else the visit's title).
struct Links {
    purpose: Vec<String>,
    arrive: Vec<Option<String>>,
    leave: Vec<Option<String>>,
}

fn visit_links(
    trips: &[Trip],
    calendar: &[TimedEntry],
    workplace: Option<&str>,
    places: &Places,
) -> Links {
    let mut visits: BTreeMap<NaiveDate, Vec<(&str, &str)>> = BTreeMap::new();
    for e in calendar {
        if is_visit(e, workplace) {
            visits
                .entry(e.start.date_naive())
                .or_default()
                .push((e.location, e.summary));
        }
    }

    let mut links = Links {
        purpose: trips.iter().map(|t| t.purpose.clone()).collect(),
        arrive: vec![None; trips.len()],
        leave: vec![None; trips.len()],
    };
    let mut by_date: BTreeMap<NaiveDate, Vec<usize>> = BTreeMap::new();
    for (i, t) in trips.iter().enumerate() {
        by_date.entry(t.start.date_naive()).or_default().push(i);
    }
    for (date, mut idx) in by_date {
        idx.sort_by_key(|&i| trips[i].start);
        for (location, summary) in visits.get(&date).map(Vec::as_slice).unwrap_or(&[]) {
            let first_in = idx
                .iter()
                .find(|&&i| places.at(&trips[i].to, trips[i].end_pos, location));
            let last_out = idx
                .iter()
                .rev()
                .find(|&&i| places.at(&trips[i].from, trips[i].start_pos, location));
            if let Some(&i) = first_in {
                links.arrive[i].get_or_insert_with(|| location.to_string());
            }
            if let Some(&i) = last_out {
                links.leave[i].get_or_insert_with(|| location.to_string());
            }
            for &i in first_in.into_iter().chain(last_out) {
                if links.purpose[i].is_empty() {
                    links.purpose[i] = summary.to_string();
                }
            }
        }
    }
    links
}

fn is_visit(e: &TimedEntry, workplace: Option<&str>) -> bool {
    e.kind == EntryKind::Work
        && !e.location.trim().is_empty()
        && !workplace.is_some_and(|w| address::matches(e.location, w))
}

/// Purpose for each trip (same order as `trips`): its own, or the title of
/// the work entry away from the workplace that it is the day's first
/// arrival at or last departure from. Empty if neither.
pub fn calendar_purposes(
    trips: &[Trip],
    calendar: &[TimedEntry],
    workplace: Option<&str>,
    places: &Places,
) -> Vec<String> {
    visit_links(trips, calendar, workplace, places).purpose
}

/// The 60-day rule (LL § 9 B): driving between home and a client is
/// business on at most 60 working days per client address within 12
/// months. Several trips a day count as one day; only trips that start or
/// end at home count; the count starts over after 60 working days in a row
/// at other workplaces.
struct SixtyDays {
    /// Client addresses as first seen; an address matching one of them
    /// (see [`address::matches`]) is the same client.
    clients: Vec<String>,
    /// Per client: the days counted since the count last started over.
    days: Vec<Vec<NaiveDate>>,
    /// Per client: the days the calendar has work there.
    calendar_days: Vec<BTreeSet<NaiveDate>>,
    /// Days with work anywhere in the calendar.
    working_days: BTreeSet<NaiveDate>,
    numbers: BTreeMap<(usize, NaiveDate), usize>,
}

impl SixtyDays {
    fn new(calendar: &[TimedEntry], workplace: Option<&str>) -> Self {
        let mut s = SixtyDays {
            clients: Vec::new(),
            days: Vec::new(),
            calendar_days: Vec::new(),
            working_days: BTreeSet::new(),
            numbers: BTreeMap::new(),
        };
        for e in calendar.iter().filter(|e| e.kind == EntryKind::Work) {
            let date = e.start.date_naive();
            s.working_days.insert(date);
            if is_visit(e, workplace) {
                let c = s.client(e.location);
                s.calendar_days[c].insert(date);
            }
        }
        s
    }

    fn client(&mut self, place: &str) -> usize {
        if let Some(c) = self.clients.iter().position(|c| address::matches(c, place)) {
            return c;
        }
        self.clients.push(place.to_string());
        self.days.push(Vec::new());
        self.calendar_days.push(BTreeSet::new());
        self.clients.len() - 1
    }

    /// Counts `date` (days come in order) as a day driving between home and
    /// `place`, and returns which day within 12 months it is there.
    fn day(&mut self, place: &str, date: NaiveDate) -> usize {
        use std::ops::Bound::Excluded;
        let c = self.client(place);
        if let Some(&n) = self.numbers.get(&(c, date)) {
            return n;
        }
        let last_there = self.calendar_days[c]
            .range(..date)
            .next_back()
            .copied()
            .max(self.days[c].last().copied());
        if let Some(last) = last_there
            && self
                .working_days
                .range((Excluded(last), Excluded(date)))
                .count()
                >= 60
        {
            self.days[c].clear();
        }
        self.days[c].push(date);
        let year_ago = date - chrono::Months::new(12);
        let n = self.days[c].iter().filter(|d| **d > year_ago).count();
        self.numbers.insert((c, date), n);
        n
    }
}

/// A reimbursed drive: one trip, or a client → stops → home drive at the
/// direct km.
#[derive(Debug, Clone)]
struct Leg {
    start: DateTime<Tz>,
    from: String,
    to: String,
    via: Vec<String>,
    purpose: String,
    km: f64,
}

/// Every trip (sorted by start) with the rule that decided it, and the
/// reimbursed drives. `overrides` are the trips you set to business or
/// private (see [`minute`]); they win over the rules.
fn classify_all(
    cfg: &MileageConfig,
    workplace: Option<&str>,
    places: &Places,
    trips: &[Trip],
    calendar: &[TimedEntry],
    overrides: &BTreeMap<DateTime<FixedOffset>, bool>,
) -> (Vec<Driven>, Vec<Leg>) {
    let mut trips = trips.to_vec();
    trips.sort_by_key(|t| t.start);
    let links = visit_links(&trips, calendar, workplace, places);
    let mut sixty = SixtyDays::new(calendar, workplace);
    let home = cfg.home.as_deref();
    let at = |addr: &str, pos, place: Option<&str>| place.is_some_and(|p| places.at(addr, pos, p));
    let at_home_or_work = |addr: &str, pos| at(addr, pos, home) || at(addr, pos, workplace);
    let km = |t: &Trip| t.distance_km.unwrap_or(0.0);
    let overridden = |t: &Trip| overrides.get(&minute(t.start)).copied();

    let mut decided: Vec<Option<Driven>> = vec![None; trips.len()];
    let mut legs = Vec::new();
    for i in 0..trips.len() {
        if decided[i].is_some() {
            continue;
        }
        let t = &trips[i];
        let purpose = &links.purpose[i];
        let driven = |status, reason, purpose: &str, reimbursed| Driven {
            trip: t.clone(),
            status,
            reason,
            purpose: purpose.to_string(),
            km: reimbursed,
        };
        let complete = |t: &Trip| {
            if t.from.is_empty() || t.to.is_empty() {
                Err("mangler fra/til")
            } else if km(t) <= 0.0 {
                Err("mangler km")
            } else {
                Ok(())
            }
        };
        let leg = |purpose: &str| Leg {
            start: t.start,
            from: t.from.clone(),
            to: t.to.clone(),
            via: Vec::new(),
            purpose: purpose.to_string(),
            km: km(t),
        };
        let incomplete = |why| {
            driven(
                Status::Incomplete(why),
                Reason::Incomplete(why),
                purpose,
                0.0,
            )
        };

        if let Some(business) = overridden(t) {
            decided[i] = Some(if !business {
                driven(Status::Private, Reason::SetByYou, purpose, 0.0)
            } else if let Err(why) = complete(t) {
                incomplete(why)
            } else {
                let purpose = if purpose.is_empty() {
                    "Erhvervskørsel"
                } else {
                    purpose
                };
                legs.push(leg(purpose));
                driven(Status::Business, Reason::SetByYou, purpose, km(t))
            });
            continue;
        }
        if touches_appointment(t, calendar, places) {
            decided[i] = Some(driven(Status::Private, Reason::Appointment, purpose, 0.0));
            continue;
        }
        let from_home = at(&t.from, t.start_pos, home);
        let to_home = at(&t.to, t.end_pos, home);
        let commute = (from_home && at(&t.to, t.end_pos, workplace))
            || (at(&t.from, t.start_pos, workplace) && to_home);
        if commute {
            decided[i] = Some(driven(Status::Commute, Reason::Commute, purpose, 0.0));
            continue;
        }
        if t.business == Some(false) {
            decided[i] = Some(driven(Status::Private, Reason::MarkedPrivate, purpose, 0.0));
            continue;
        }
        if !t.business.unwrap_or(!purpose.is_empty()) {
            let working_day = sixty.working_days.contains(&t.start.date_naive());
            let reason = if working_day {
                Reason::Errand
            } else {
                Reason::Private
            };
            decided[i] = Some(driven(Status::Private, reason, purpose, 0.0));
            continue;
        }
        if purpose.is_empty() {
            decided[i] = Some(incomplete("mangler formål"));
            continue;
        }
        if let Err(why) = complete(t) {
            decided[i] = Some(incomplete(why));
            continue;
        }

        // Leaving a client for somewhere else than home, the workplace or
        // another client: the stops up to home or the workplace that day.
        let date = t.start.date_naive();
        let chain: Option<(usize, String)> = match &links.leave[i] {
            Some(client) if links.arrive[i].is_none() && !at_home_or_work(&t.to, t.end_pos) => {
                let mut end = None;
                for (j, s) in trips.iter().enumerate().skip(i + 1) {
                    if s.start.date_naive() != date
                        || overridden(s).is_some()
                        || links.arrive[j].is_some()
                        || links.leave[j].is_some()
                        || !s.purpose.is_empty()
                    {
                        break;
                    }
                    if at_home_or_work(&s.to, s.end_pos) {
                        end = Some((j, client.clone()));
                        break;
                    }
                }
                end
            }
            _ => None,
        };
        let chain_home = chain
            .as_ref()
            .is_some_and(|&(j, _)| at(&trips[j].to, trips[j].end_pos, home));

        // The client end of a drive between home and a client.
        let client = if from_home {
            Some(links.arrive[i].clone().unwrap_or_else(|| t.to.clone()))
        } else if to_home || chain_home {
            Some(links.leave[i].clone().unwrap_or_else(|| t.from.clone()))
        } else {
            None
        };
        if let Some(client) = client {
            let day = sixty.day(&client, date);
            if day > 60 {
                decided[i] = Some(driven(
                    Status::Private,
                    Reason::SixtyDays { day },
                    purpose,
                    0.0,
                ));
                if let Some((end, _)) = chain {
                    for j in i + 1..=end {
                        decided[j] = Some(Driven {
                            trip: trips[j].clone(),
                            status: Status::Private,
                            reason: Reason::Detour,
                            purpose: String::new(),
                            km: 0.0,
                        });
                    }
                }
                continue;
            }
        }

        let Some((end, client)) = chain else {
            legs.push(leg(purpose));
            decided[i] = Some(driven(
                Status::Business,
                Reason::ClientVisit,
                purpose,
                km(t),
            ));
            continue;
        };
        // Only the direct km: the shortest drive recorded earlier between
        // the client and where the stops end.
        let end_trip = &trips[end];
        let end_place = if at(&end_trip.to, end_trip.end_pos, home) {
            home
        } else {
            workplace
        };
        let direct = trips[..i]
            .iter()
            .filter(|d| {
                (places.at(&d.from, d.start_pos, &client) && at(&d.to, d.end_pos, end_place))
                    || (at(&d.from, d.start_pos, end_place) && places.at(&d.to, d.end_pos, &client))
            })
            .map(km)
            .filter(|&k| k > 0.0)
            .fold(None, |m: Option<f64>, k| Some(m.map_or(k, |m| m.min(k))));
        let Some(direct) = direct else {
            decided[i] = Some(incomplete(
                "mangler direkte km til hjem/arbejde (sæt turen med `trip business|private`)",
            ));
            continue;
        };
        let chain_km: f64 = trips[i..=end].iter().map(km).sum();
        let mut left = direct.min(chain_km);
        legs.push(Leg {
            start: t.start,
            from: t.from.clone(),
            to: end_trip.to.clone(),
            via: trips[i..end].iter().map(|s| s.to.clone()).collect(),
            purpose: purpose.clone(),
            km: left,
        });
        for (j, d) in decided.iter_mut().enumerate().take(end + 1).skip(i) {
            let share = left.min(km(&trips[j]));
            left -= share;
            let (reason, purpose) = if j == i {
                (Reason::DirectKm, purpose.clone())
            } else {
                (Reason::Detour, String::new())
            };
            *d = Some(Driven {
                trip: trips[j].clone(),
                status: if share > 0.0 {
                    Status::Business
                } else {
                    Status::Private
                },
                reason,
                purpose,
                km: share,
            });
        }
    }
    let driven = decided
        .into_iter()
        .map(|d| d.expect("every trip decided"))
        .collect();
    (driven, legs)
}

/// One reimbursed part of a trip, at a single rate.
#[derive(Debug, Clone)]
pub struct Entry {
    pub start: DateTime<Tz>,
    pub from: String,
    pub to: String,
    /// Stops between `from` and `to` (a client → stops → home drive at the
    /// direct km).
    pub via: Vec<String>,
    pub purpose: String,
    pub km: f64,
    pub rate: f64,
}

impl Entry {
    pub fn amount(&self) -> f64 {
        round2(self.km * self.rate)
    }

    /// The destination with its stops, e.g. "Hjem (via Netto)".
    pub fn destination(&self) -> String {
        if self.via.is_empty() {
            self.to.clone()
        } else {
            format!("{} (via {})", self.to, self.via.join(", "))
        }
    }
}

#[derive(Debug)]
pub struct MileageMonth {
    pub month: Month,
    pub rate: MileageRate,
    /// Business km earlier in the same year, before this month.
    pub km_before: f64,
    pub entries: Vec<Entry>,
    /// Trips in the month that were not reimbursed and why.
    pub skipped: Vec<(Trip, Status)>,
    /// Every trip in the month with the rule that decided it.
    pub driving: Vec<Driven>,
}

impl MileageMonth {
    pub fn km_at(&self, rate: f64) -> f64 {
        self.entries
            .iter()
            .filter(|e| e.rate == rate)
            .map(|e| e.km)
            .fold(0.0, |a, b| a + b)
    }
    pub fn total_km(&self) -> f64 {
        self.entries.iter().map(|e| e.km).fold(0.0, |a, b| a + b)
    }
    pub fn total_amount(&self) -> f64 {
        self.entries
            .iter()
            .map(Entry::amount)
            .fold(0.0, |a, b| a + b)
    }
    /// Business km this year up to and including the month.
    pub fn km_year_to_date(&self) -> f64 {
        self.km_before + self.total_km()
    }
    /// The km driven in the month.
    pub fn driven_km(&self) -> f64 {
        self.driving
            .iter()
            .map(|d| d.trip.distance_km.unwrap_or(0.0))
            .sum()
    }
    /// The km not reimbursed, by the rule that decided them: (rule, trips,
    /// km), in the order first met.
    pub fn not_reimbursed(&self) -> Vec<(String, usize, f64)> {
        let mut out: Vec<(String, usize, f64)> = Vec::new();
        for d in self.driving.iter().filter(|d| d.private_km() > 0.0) {
            let reason = d.private_reason().to_string();
            match out.iter_mut().find(|(r, _, _)| *r == reason) {
                Some((_, n, km)) => {
                    *n += 1;
                    *km += d.private_km();
                }
                None => out.push((reason, 1, d.private_km())),
            }
        }
        out
    }
}

/// `calendar` and `trips` should cover the 12 months before the year of
/// `month` through its end, so the 60-day rule and the business km earlier
/// in the year are counted the same way. A missing rate for the year is an
/// error only when the year has business km.
pub fn compute(
    cfg: &MileageConfig,
    workplace: Option<&str>,
    places: &Places,
    month: Month,
    trips: &[Trip],
    calendar: &[TimedEntry],
    overrides: &BTreeMap<DateTime<FixedOffset>, bool>,
) -> Result<MileageMonth> {
    let year_start = NaiveDate::from_ymd_opt(month.year, 1, 1).unwrap();
    let in_year = |d: NaiveDate| year_start <= d && d <= month.last_day();
    let (driven, legs) = classify_all(cfg, workplace, places, trips, calendar, overrides);
    let legs: Vec<Leg> = legs
        .into_iter()
        .filter(|l| in_year(l.start.date_naive()))
        .collect();
    let rate = if legs.iter().any(|l| l.km > 0.0) {
        cfg.rate(month.year)?
    } else {
        cfg.rate(month.year).unwrap_or(MileageRate {
            year: month.year,
            low: 0.0,
            high: 0.0,
            threshold_km: 20_000.0,
        })
    };

    let mut ytd = 0.0;
    let mut km_before = 0.0;
    let mut entries = Vec::new();
    for l in legs {
        if !month.contains(l.start.date_naive()) {
            km_before += l.km;
            ytd += l.km;
            continue;
        }
        let low = (rate.threshold_km - ytd).clamp(0.0, l.km);
        let entry = |km, rate| Entry {
            start: l.start,
            from: l.from.clone(),
            to: l.to.clone(),
            via: l.via.clone(),
            purpose: l.purpose.clone(),
            km,
            rate,
        };
        if low > 0.0 {
            entries.push(entry(low, rate.low));
        }
        if l.km - low > 0.0 {
            entries.push(entry(l.km - low, rate.high));
        }
        ytd += l.km;
    }
    debug_assert!(entries.iter().all(|e| e.start.year() == month.year));
    let driving: Vec<Driven> = driven
        .into_iter()
        .filter(|d| month.contains(d.trip.start.date_naive()))
        .collect();
    let skipped = driving
        .iter()
        .filter(|d| d.status != Status::Business)
        .map(|d| (d.trip.clone(), d.status.clone()))
        .collect();
    Ok(MileageMonth {
        month,
        rate,
        km_before,
        entries,
        skipped,
        driving,
    })
}

/// The befordringsfradrag estimate for a year (LL § 9 C), for information:
/// you enter it yourself in TastSelv (forskudsopgørelse felt 417,
/// årsopgørelse rubrik 51).
#[derive(Debug)]
pub struct Fradrag {
    pub year: i32,
    pub rate: FradragRate,
    pub yderkommune: bool,
    /// Home → workplace km: the median of the year's commuting trips.
    pub one_way_km: Option<f64>,
    /// The days driven to the workplace that count.
    pub days: Vec<NaiveDate>,
    /// Days driven to the workplace that do not count, and why.
    pub excluded: Vec<(NaiveDate, &'static str)>,
}

impl Fradrag {
    pub fn round_trip_km(&self) -> f64 {
        self.one_way_km.unwrap_or(0.0) * 2.0
    }

    /// The deduction per day: the round trip's km from 25 to 120 at the
    /// low rate, above 120 at the high (or yderkommune) rate.
    pub fn per_day(&self) -> f64 {
        let rt = self.round_trip_km();
        let band = (rt.min(120.0) - 24.0).max(0.0);
        let above = (rt - 120.0).max(0.0);
        let high = if self.yderkommune {
            self.rate.yderkommune_high
        } else {
            self.rate.high
        };
        round2(band * self.rate.low + above * high)
    }

    pub fn total(&self) -> f64 {
        round2(self.per_day() * self.days.len() as f64)
    }
}

/// The befordringsfradrag of `year`: the days with a commuting trip, less
/// those `excluded` (home working, days off) and those whose km from or to
/// home were reimbursed. `trips` and `calendar` as for [`compute`], through
/// the year's end.
#[allow(clippy::too_many_arguments)]
pub fn fradrag(
    cfg: &MileageConfig,
    workplace: Option<&str>,
    places: &Places,
    year: i32,
    trips: &[Trip],
    calendar: &[TimedEntry],
    overrides: &BTreeMap<DateTime<FixedOffset>, bool>,
    excluded: impl Fn(NaiveDate) -> Option<&'static str>,
) -> Result<Fradrag> {
    let rate = cfg.fradrag_rate(year)?;
    let (driven, _) = classify_all(cfg, workplace, places, trips, calendar, overrides);
    let driven: Vec<&Driven> = driven
        .iter()
        .filter(|d| d.trip.start.year() == year)
        .collect();
    let home = cfg.home.as_deref();
    let at_home = |addr: &str, pos| home.is_some_and(|h| places.at(addr, pos, h));
    let reimbursed_home: BTreeSet<NaiveDate> = driven
        .iter()
        .filter(|d| {
            d.km > 0.0
                && (at_home(&d.trip.from, d.trip.start_pos) || at_home(&d.trip.to, d.trip.end_pos))
        })
        .map(|d| d.trip.start.date_naive())
        .collect();
    let commutes: Vec<&&Driven> = driven
        .iter()
        .filter(|d| d.reason == Reason::Commute)
        .collect();
    let mut km: Vec<f64> = commutes
        .iter()
        .filter_map(|d| d.trip.distance_km)
        .filter(|k| *k > 0.0)
        .collect();
    km.sort_by(f64::total_cmp);
    let one_way_km = (!km.is_empty()).then(|| {
        let mid = km.len() / 2;
        if km.len().is_multiple_of(2) {
            (km[mid - 1] + km[mid]) / 2.0
        } else {
            km[mid]
        }
    });
    let mut days = Vec::new();
    let mut out = Vec::new();
    let dates: BTreeSet<NaiveDate> = commutes.iter().map(|d| d.trip.start.date_naive()).collect();
    for date in dates {
        if let Some(why) = excluded(date) {
            out.push((date, why));
        } else if reimbursed_home.contains(&date) {
            out.push((date, "km reimbursed as business"));
        } else {
            days.push(date);
        }
    }
    Ok(Fradrag {
        year,
        rate,
        yderkommune: cfg.yderkommune,
        one_way_km,
        days,
        excluded: out,
    })
}

/// Writes the kørselsregnskab for the month as semicolon-separated text,
/// ready to print and sign as employer control.
pub fn write_logbook(cfg: &MileageConfig, m: &MileageMonth, mut w: impl Write) -> Result<()> {
    let n = |v: f64| format!("{v:.2}").replace('.', ",");
    writeln!(w, "Kørselsregnskab;{}", m.month)?;
    writeln!(w, "Navn;{}", csv_field(&cfg.driver_name))?;
    writeln!(w, "Adresse;{}", csv_field(address_of(cfg)))?;
    writeln!(
        w,
        "CPR-nr.;{}",
        csv_field(cfg.cpr.as_deref().unwrap_or_default())
    )?;
    writeln!(w, "Reg.nr.;{}", cfg.registration_number)?;
    writeln!(w, "Erhvervskørte km i år før perioden;{}", n(m.km_before))?;
    writeln!(
        w,
        "Erhvervskørte km i år til og med perioden;{}",
        n(m.km_year_to_date())
    )?;
    writeln!(
        w,
        "Satser {};{} kr/km op til {} km;{} kr/km derover",
        m.rate.year,
        n(m.rate.low),
        m.rate.threshold_km,
        n(m.rate.high)
    )?;
    writeln!(w)?;
    writeln!(w, "Dato;Afgang;Fra;Til;Formål;Km;Sats;Beløb")?;
    for e in &m.entries {
        writeln!(
            w,
            "{};{};{};{};{};{};{};{}",
            e.start.format("%d-%m-%Y"),
            e.start.format("%H:%M"),
            csv_field(&e.from),
            csv_field(&e.destination()),
            csv_field(&e.purpose),
            n(e.km),
            n(e.rate),
            n(e.amount())
        )?;
    }
    writeln!(w)?;
    writeln!(w, "I alt;;;;;{};;{}", n(m.total_km()), n(m.total_amount()))?;
    writeln!(w)?;
    writeln!(
        w,
        "Kontrolleret og godkendt af arbejdsgiver;Dato:;;Underskrift:"
    )?;
    Ok(())
}

/// The month's logbook (kørselsregnskab) as an HTML fragment for printing
/// and signing: the same lines as [`write_logbook`], then signature lines
/// for the driver and the employer. Classes start with `lb-`; see
/// [`LOGBOOK_CSS`].
pub fn logbook_html_body(cfg: &MileageConfig, m: &MileageMonth) -> String {
    let n = |v: f64| format!("{v:.2}").replace('.', ",");
    let mut rows = String::new();
    for e in &m.entries {
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"lb-r\">{}</td><td class=\"lb-r\">{}</td><td class=\"lb-r\">{}</td></tr>\n",
            e.start.format("%d-%m-%Y"),
            e.start.format("%H:%M"),
            html_escape(&e.from),
            html_escape(&e.destination()),
            html_escape(&e.purpose),
            n(e.km),
            n(e.rate),
            n(e.amount())
        ));
    }
    if m.entries.is_empty() {
        rows.push_str("<tr><td colspan=\"8\">Ingen erhvervskørsel i perioden</td></tr>\n");
    }
    let sign = |who: &str| {
        format!(
            "<div class=\"lb-sign\"><h2>{who}</h2><div class=\"lb-lines\">\
             <div><span class=\"lb-line\"></span>Navn</div>\
             <div><span class=\"lb-line\"></span>Dato</div>\
             <div><span class=\"lb-line\"></span>Underskrift</div></div></div>\n"
        )
    };
    format!(
        "<article class=\"lb-sheet\">\n<h1>Kørselsregnskab {month}</h1>\n\
         <dl class=\"lb-facts\"><dt>Navn</dt><dd>{name}</dd><dt>Adresse</dt><dd>{address}</dd>\
         <dt>CPR-nr.</dt><dd>{cpr}</dd><dt>Reg.nr.</dt><dd>{plate}</dd>\
         <dt>Erhvervskørte km i år før perioden</dt><dd>{before}</dd>\
         <dt>Erhvervskørte km i år til og med perioden</dt><dd>{ytd}</dd>\
         <dt>Satser {year}</dt><dd>{low} kr/km op til {threshold} km, {high} kr/km derover</dd></dl>\n\
         <table class=\"lb-table\"><thead><tr><th>Dato</th><th>Afgang</th><th>Fra</th><th>Til</th><th>Formål</th>\
         <th class=\"lb-r\">Km</th><th class=\"lb-r\">Sats</th><th class=\"lb-r\">Beløb</th></tr></thead>\n<tbody>\n{rows}</tbody>\n\
         <tfoot><tr><td colspan=\"5\">I alt</td><td class=\"lb-r\">{km}</td><td></td><td class=\"lb-r\">{amount}</td></tr></tfoot></table>\n\
         {driver}{employer}</article>\n",
        month = m.month,
        name = html_escape(&cfg.driver_name),
        address = html_escape(address_of(cfg)),
        cpr = html_escape(cfg.cpr.as_deref().unwrap_or_default()),
        ytd = n(m.km_year_to_date()),
        plate = html_escape(&cfg.registration_number),
        before = n(m.km_before),
        year = m.rate.year,
        low = n(m.rate.low),
        threshold = m.rate.threshold_km,
        high = n(m.rate.high),
        km = n(m.total_km()),
        amount = n(m.total_amount()),
        driver = sign("Medarbejder"),
        employer = sign("Kontrolleret og godkendt af arbejdsgiver"),
    )
}

/// The styles of [`logbook_html_body`], for screen and print.
pub const LOGBOOK_CSS: &str = "\
.lb-sheet{font-family:'IBM Plex Sans',Arial,sans-serif;color:#111;background:#fff;font-size:12px;line-height:1.4}\
.lb-sheet h1{font-size:18px;margin:0 0 8px}\
.lb-sheet h2{font-size:12px;margin:0 0 18px;font-weight:600}\
.lb-facts{display:grid;grid-template-columns:max-content 1fr;gap:2px 12px;margin:0 0 12px}\
.lb-facts dt{color:#555}.lb-facts dd{margin:0}\
.lb-table{width:100%;border-collapse:collapse;margin-bottom:24px}\
.lb-table th,.lb-table td{border-bottom:1px solid #ccc;padding:4px 6px;text-align:left;vertical-align:top}\
.lb-table thead th{border-bottom:1.5px solid #111}\
.lb-table tfoot td{font-weight:600;border-top:1.5px solid #111;border-bottom:0}\
.lb-r,.lb-table td:nth-child(-n+2){white-space:nowrap}.lb-r{text-align:right!important}\
.lb-sign{margin-top:18px;break-inside:avoid}\
.lb-lines{display:grid;grid-template-columns:2fr 1fr 2fr;gap:16px}\
.lb-lines div{color:#555;font-size:11px}\
.lb-line{display:block;border-bottom:1px solid #111;height:28px;margin-bottom:3px}";

/// The logbook as a standalone HTML page, to share or print.
pub fn logbook_html(cfg: &MileageConfig, m: &MileageMonth) -> String {
    format!(
        "<!doctype html>\n<html lang=\"da\"><head><meta charset=\"utf-8\"><title>Kørselsregnskab {}</title>\
         <style>body{{margin:24px}}{LOGBOOK_CSS}</style></head><body>\n{}</body></html>\n",
        m.month,
        logbook_html_body(cfg, m)
    )
}

/// Your address on the logbook: `address`, else `home`.
fn address_of(cfg: &MileageConfig) -> &str {
    cfg.address
        .as_deref()
        .or(cfg.home.as_deref())
        .unwrap_or_default()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn csv_field(s: &str) -> String {
    if s.contains([';', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::Occurrence;
    use crate::month::timed_entries;
    use crate::trips::parse_time;

    fn cfg() -> MileageConfig {
        MileageConfig {
            driver_name: "Test".into(),
            registration_number: "AB12345".into(),
            home: Some("Hjem".into()),
            address: None,
            cpr: None,
            rates: vec![MileageRate {
                year: 2026,
                low: 3.94,
                high: 2.28,
                threshold_km: 20_000.0,
            }],
            yderkommune: false,
            fradrag_rates: Vec::new(),
        }
    }

    fn trip(start: &str, from: &str, to: &str, purpose: &str, km: f64) -> Trip {
        let tz = chrono_tz::Europe::Copenhagen;
        let start = parse_time(start, tz).unwrap();
        Trip {
            start,
            end: start,
            distance_km: Some(km),
            from: from.into(),
            to: to.into(),
            purpose: purpose.into(),
            business: None,
            start_pos: None,
            end_pos: None,
        }
    }

    const WORK: Option<&str> = Some("Kontor");

    fn classify_own(t: &Trip, c: &MileageConfig) -> Status {
        let none = BTreeMap::new();
        let (driven, _) = classify_all(
            c,
            WORK,
            &Places::default(),
            std::slice::from_ref(t),
            &[],
            &none,
        );
        driven[0].status.clone()
    }

    #[test]
    fn classification() {
        let c = cfg();
        assert_eq!(
            classify_own(
                &trip("2026-10-05 08:00", "Hjem", "kontor", "Arbejde", 20.0),
                &c
            ),
            Status::Commute
        );
        assert_eq!(
            classify_own(
                &trip("2026-10-05 08:00", "Hjem", "Kunde A", "Møde", 20.0),
                &c
            ),
            Status::Business
        );
        assert_eq!(
            classify_own(&trip("2026-10-05 08:00", "Hjem", "Netto", "", 3.0), &c),
            Status::Private
        );
        let mut t = trip("2026-10-05 08:00", "Hjem", "Kunde A", "", 20.0);
        t.business = Some(true);
        assert_eq!(classify_own(&t, &c), Status::Incomplete("mangler formål"));
    }

    fn visit(start: &str, summary: &str, location: &str) -> Occurrence {
        appointment(start, start, summary, location)
    }

    fn appointment(start: &str, end: &str, summary: &str, location: &str) -> Occurrence {
        let tz = chrono_tz::Europe::Copenhagen;
        let start = parse_time(start, tz).unwrap();
        Occurrence::Timed {
            start,
            end: parse_time(end, tz).unwrap(),
            summary: summary.into(),
            location: location.into(),
        }
    }

    #[test]
    fn calendar_visits_are_business() {
        let c = cfg();
        let odense = "Vestergade 12, 5000 Odense C";
        let cal = vec![
            visit("2026-10-05 08:00", "Kontor", "Kontor"),
            visit("2026-10-06 09:00", "Service Kunde A", odense),
            visit("2026-10-07 09:00", "Service Kunde A", odense),
        ];
        let trips = vec![
            // Monday at the workplace: commuting only.
            trip("2026-10-05 07:30", "Hjem", "Kontor", "", 25.0),
            trip("2026-10-05 16:00", "Kontor", "Hjem", "", 25.0),
            // Tuesday: home → customer, a lunch errand, customer → office.
            trip(
                "2026-10-06 07:45",
                "Hjem",
                "Vestergade 12, 5000 Odense",
                "",
                98.0,
            ),
            trip(
                "2026-10-06 11:30",
                "Vestergade 12, 5000 Odense",
                "Bager",
                "",
                3.0,
            ),
            trip(
                "2026-10-06 12:00",
                "Bager",
                "Vestergade 12, 5000 Odense",
                "",
                3.0,
            ),
            trip(
                "2026-10-06 15:00",
                "Vestergade 12, 5000 Odense",
                "Kontor",
                "",
                75.0,
            ),
            trip("2026-10-06 17:00", "Kontor", "Hjem", "", 25.0),
            // Wednesday: a colleague drives to the customer; the car stays at work.
            trip("2026-10-07 07:30", "Hjem", "Kontor", "", 25.0),
            trip("2026-10-07 16:00", "Kontor", "Hjem", "", 25.0),
        ];
        let cal = timed_entries(&cal, &[], "Frokost");
        let m = compute(
            &c,
            WORK,
            &Places::default(),
            Month::parse("2026-10").unwrap(),
            &trips,
            &cal,
            &BTreeMap::new(),
        )
        .unwrap();
        let reimbursed: Vec<_> = m
            .entries
            .iter()
            .map(|e| (e.km, e.purpose.as_str()))
            .collect();
        assert_eq!(
            reimbursed,
            [(98.0, "Service Kunde A"), (75.0, "Service Kunde A")]
        );
        let commutes = m.skipped.iter().filter(|s| s.1 == Status::Commute).count();
        assert_eq!(commutes, 5);
    }

    #[test]
    fn private_appointment_trips_are_private() {
        let c = cfg();
        let doctor = "Torvet 3, 8000 Aarhus C, Danmark";
        let occurrences = vec![
            visit("2026-10-08 08:00", "Kontor", "Kontor"),
            appointment("2026-10-08 10:00", "2026-10-08 11:15", "Læge", doctor),
        ];
        let cal = timed_entries(&occurrences, &["Læge".to_string()], "Frokost");
        let trips = vec![
            trip(
                "2026-10-08 09:45",
                "Kontoret",
                "Torvet 3, 8000 Aarhus",
                "",
                4.0,
            ),
            // Starts during the appointment: private even with its own purpose.
            trip("2026-10-08 10:05", "Kontoret", "Apotek", "Indkøb", 1.0),
            trip(
                "2026-10-08 11:20",
                "Torvet 3, 8000 Aarhus",
                "Kontoret",
                "",
                4.0,
            ),
            // Business trips the same day are unaffected.
            trip("2026-10-08 13:00", "Kontoret", "Kunde B", "Service", 10.0),
        ];

        // A located appointment never becomes a business visit purpose.
        let purposes = calendar_purposes(&trips, &cal, WORK, &Places::default());
        assert_eq!(purposes, ["", "Indkøb", "", "Service"]);

        let m = compute(
            &c,
            WORK,
            &Places::default(),
            Month::parse("2026-10").unwrap(),
            &trips,
            &cal,
            &BTreeMap::new(),
        )
        .unwrap();
        let statuses: Vec<_> = m.skipped.iter().map(|s| s.1.clone()).collect();
        assert_eq!(statuses, [const { Status::Private }; 3]);
        let mut logbook = Vec::new();
        write_logbook(&c, &m, &mut logbook).unwrap();
        let logbook = String::from_utf8(logbook).unwrap();
        assert!(!logbook.contains("Torvet") && !logbook.contains("Apotek"));
        assert!(logbook.contains("Kunde B"));

        // The printable logbook has the same trips, escaped, and the
        // signature lines.
        let mut c = c;
        c.driver_name = "Test <Testesen>".into();
        let html = logbook_html(&c, &m);
        assert!(
            html.contains("<td>Kunde B</td><td>Service</td><td class=\"lb-r\">10,00</td>"),
            "{html}"
        );
        assert!(html.contains("Test &lt;Testesen&gt;") && !html.contains("Torvet"));
        assert!(html.contains("Kørselsregnskab 2026-10"));
        assert_eq!(
            html.matches("<span class=\"lb-line\"></span>Underskrift")
                .count(),
            2
        );
        assert!(html.contains("Kontrolleret og godkendt af arbejdsgiver"));
    }

    #[test]
    fn trips_during_recorded_private_time_are_private() {
        let at = |s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        let recorded = [crate::state::PrivateInterval {
            id: 1,
            start: at("2026-10-08 10:02"),
            end: at("2026-10-08 11:14"),
            label: "Privat".into(),
            kind: Default::default(),
            replaces: None,
        }];
        let cal = crate::month::recorded_entries(&recorded, chrono_tz::Europe::Copenhagen);
        let trips = vec![
            trip("2026-10-08 10:05", "Kontoret", "Kunde A", "Service", 10.0),
            trip("2026-10-08 11:14", "Kunde A", "Kontoret", "Service", 10.0),
        ];
        let m = compute(
            &cfg(),
            WORK,
            &Places::default(),
            Month::parse("2026-10").unwrap(),
            &trips,
            &cal,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(m.skipped.len(), 1);
        assert_eq!(m.skipped[0].1, Status::Private);
        assert_eq!(m.skipped[0].0.start, trips[0].start);
        assert_eq!(m.total_km(), 10.0);
    }

    #[test]
    fn threshold_split_across_year() {
        let c = cfg();
        let trips = vec![
            trip("2026-09-01 08:00", "Hjem", "Kunde A", "Montage", 19_990.0),
            trip("2026-10-05 08:00", "Hjem", "Kunde B", "Service", 30.0),
            trip("2026-10-06 08:00", "Hjem", "Kontor", "Arbejde", 25.0),
        ];
        let m = compute(
            &c,
            WORK,
            &Places::default(),
            Month::parse("2026-10").unwrap(),
            &trips,
            &[],
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(m.km_before, 19_990.0);
        assert_eq!(m.km_at(3.94), 10.0);
        assert_eq!(m.km_at(2.28), 20.0);
        assert!((m.total_amount() - (39.40 + 45.60)).abs() < 1e-9);
        assert_eq!(m.skipped.len(), 1);
        assert_eq!(m.skipped[0].1, Status::Commute);
    }

    const ODENSE: &str = "Vestergade 12, 5000 Odense C";

    fn run(
        trips: &[Trip],
        occurrences: &[Occurrence],
        overrides: &BTreeMap<DateTime<FixedOffset>, bool>,
        month: &str,
    ) -> MileageMonth {
        let cal = timed_entries(occurrences, &[], "Frokost");
        compute(
            &cfg(),
            WORK,
            &Places::default(),
            Month::parse(month).unwrap(),
            trips,
            &cal,
            overrides,
        )
        .unwrap()
    }

    fn reasons(m: &MileageMonth) -> Vec<(String, f64)> {
        m.driving
            .iter()
            .map(|d| (d.reason.to_string(), d.km))
            .collect()
    }

    #[test]
    fn commute_is_driving_and_a_client_visit_is_reimbursed() {
        let cal = vec![visit("2026-10-06 09:00", "Service Kunde A", ODENSE)];
        let trips = vec![
            trip("2026-10-05 07:30", "Hjem", "Kontor", "", 25.0),
            trip("2026-10-05 16:00", "Kontor", "Hjem", "", 25.0),
            trip("2026-10-06 07:45", "Hjem", ODENSE, "", 98.0),
            trip("2026-10-06 15:00", ODENSE, "Hjem", "", 98.0),
        ];
        let m = run(&trips, &cal, &BTreeMap::new(), "2026-10");
        let commute = "commute (LL § 9 B)".to_string();
        let visit = "client visit (LL § 9 B)".to_string();
        assert_eq!(
            reasons(&m),
            [
                (commute.clone(), 0.0),
                (commute.clone(), 0.0),
                (visit.clone(), 98.0),
                (visit, 98.0)
            ]
        );
        assert_eq!(m.total_km(), 196.0);
        assert_eq!(m.driven_km(), 246.0);
        assert_eq!(m.not_reimbursed(), [(commute, 2, 50.0)]);
    }

    /// A home ↔ client trip on each of `days` days from 1 January 2026.
    fn client_days(days: i64) -> (Vec<Trip>, Vec<Occurrence>) {
        let first = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let mut trips = Vec::new();
        let mut cal = Vec::new();
        for n in 0..days {
            let d = first + chrono::Duration::days(n);
            cal.push(visit(&format!("{d} 09:00"), "Montage", ODENSE));
            trips.push(trip(&format!("{d} 07:45"), "Hjem", ODENSE, "", 98.0));
            trips.push(trip(&format!("{d} 15:00"), ODENSE, "Hjem", "", 98.0));
        }
        (trips, cal)
    }

    #[test]
    fn home_to_a_client_is_private_from_the_61st_day() {
        // 1 January – 1 March is 60 days; 2 March is the 61st.
        let (mut trips, mut cal) = client_days(61);
        // On the 61st day the drive from the workplace stays business.
        trips.pop();
        trips.pop();
        cal.push(visit("2026-03-02 08:00", "Kontor", "Kontor"));
        trips.push(trip("2026-03-02 07:30", "Hjem", "Kontor", "", 25.0));
        trips.push(trip("2026-03-02 10:00", "Kontor", ODENSE, "", 75.0));
        trips.push(trip("2026-03-02 15:00", ODENSE, "Hjem", "", 98.0));
        let m = run(&trips, &cal, &BTreeMap::new(), "2026-03");
        let day = |d: &str| {
            reasons(&m)
                .into_iter()
                .zip(&m.driving)
                .filter(|(_, x)| x.trip.start.date_naive().to_string() == d)
                .map(|(r, _)| r)
                .collect::<Vec<_>>()
        };
        let visit = "client visit (LL § 9 B)".to_string();
        assert_eq!(
            day("2026-03-01"),
            [(visit.clone(), 98.0), (visit.clone(), 98.0)]
        );
        assert_eq!(
            day("2026-03-02"),
            [
                ("commute (LL § 9 B)".to_string(), 0.0),
                (visit, 75.0),
                ("60-day rule (day 61 at this client)".to_string(), 0.0),
            ]
        );
    }

    #[test]
    fn the_60_day_count_starts_over_after_60_working_days_elsewhere() {
        let at = |d: &str, location: &str| visit(&format!("{d} 08:00"), "Arbejde", location);
        let first = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let day = |n: i64| (first + chrono::Duration::days(n)).to_string();
        for (elsewhere, expected) in [(59, 12), (60, 2)] {
            // 10 days at the client, then days at the workplace (and weekend
            // days without work, which do not count), then back.
            let mut occ: Vec<Occurrence> = (0..10).map(|n| at(&day(n), ODENSE)).collect();
            let mut n = 10;
            for _ in 0..elsewhere {
                occ.push(at(&day(n), "Kontor"));
                n += 2; // a day without work in between
            }
            occ.push(at(&day(n), ODENSE));
            occ.push(at(&day(n + 1), ODENSE));
            let cal = timed_entries(&occ, &[], "Frokost");
            let mut sixty = SixtyDays::new(&cal, WORK);
            for k in 0..10 {
                sixty.day(ODENSE, first + chrono::Duration::days(k));
            }
            let back = first + chrono::Duration::days(n);
            assert_eq!(
                sixty.day(ODENSE, back),
                expected - 1,
                "{elsewhere} days elsewhere"
            );
            assert_eq!(
                sixty.day(
                    "Vestergade 12, 5000 Odense",
                    back + chrono::Duration::days(1)
                ),
                expected
            );
        }
    }

    #[test]
    fn errands_on_the_way_home_are_private() {
        let cal = vec![visit("2026-10-05 08:00", "Kontor", "Kontor")];
        let trips = vec![
            trip("2026-10-05 07:30", "Hjem", "Kontor", "", 25.0),
            trip("2026-10-05 16:00", "Kontor", "Netto", "", 20.0),
            trip("2026-10-05 16:30", "Netto", "Hjem", "", 8.0),
            // A Saturday trip is private driving.
            trip("2026-10-10 10:00", "Hjem", "Netto", "", 8.0),
        ];
        let m = run(&trips, &cal, &BTreeMap::new(), "2026-10");
        assert!(m.entries.is_empty());
        assert_eq!(
            m.not_reimbursed(),
            [
                ("commute (LL § 9 B)".to_string(), 1, 25.0),
                ("errand (private)".to_string(), 2, 28.0),
                ("private driving".to_string(), 1, 8.0),
            ]
        );
    }

    #[test]
    fn client_then_shops_then_home_gets_the_direct_km_only() {
        let cal = vec![
            visit("2026-10-06 09:00", "Service Kunde A", ODENSE),
            visit("2026-10-07 09:00", "Service Kunde A", ODENSE),
        ];
        let mut trips = vec![
            // Recorded earlier: the direct drive home from the client.
            trip("2026-09-01 15:00", ODENSE, "Hjem", "", 95.0),
            trip("2026-10-06 07:45", "Hjem", ODENSE, "", 98.0),
            trip("2026-10-06 15:00", ODENSE, "Bilka", "", 10.0),
            trip("2026-10-06 15:40", "Bilka", "Netto", "", 3.0),
            trip("2026-10-06 16:00", "Netto", "Hjem", "", 92.0),
        ];
        let m = run(&trips, &cal, &BTreeMap::new(), "2026-10");
        let e = &m.entries[1];
        assert_eq!((e.km, e.from.as_str()), (95.0, ODENSE));
        assert_eq!(e.destination(), "Hjem (via Bilka, Netto)");
        assert_eq!(m.total_km(), 98.0 + 95.0);
        assert_eq!(
            m.not_reimbursed(),
            [("detour after client visit".to_string(), 1, 10.0)]
        );

        // Without a direct drive recorded before (either way), the km must
        // be set by hand.
        trips.remove(0);
        trips[0] = trip("2026-10-06 07:45", "Kontor", ODENSE, "", 75.0);
        let m = run(&trips, &cal, &BTreeMap::new(), "2026-10");
        assert_eq!(m.total_km(), 75.0);
        assert!(matches!(
            m.driving[1].status,
            Status::Incomplete(why) if why.starts_with("mangler direkte km")
        ));
    }

    #[test]
    fn a_trip_set_by_you_wins_over_the_rules() {
        let cal = vec![visit("2026-10-06 08:00", "Kontor", "Kontor")];
        let trips = vec![
            trip("2026-10-06 07:30", "Hjem", "Kontor", "", 25.0),
            // Between workplaces: the tool cannot see it.
            trip("2026-10-06 10:00:31", "Kontor", "Lager", "", 12.0),
            trip("2026-10-06 16:00", "Kontor", "Hjem", "", 25.0),
        ];
        let set = |at: &str, business| {
            let tz = chrono_tz::Europe::Copenhagen;
            (minute(parse_time(at, tz).unwrap()), business)
        };
        let overrides = BTreeMap::from([
            set("2026-10-06 10:00", true),
            set("2026-10-06 16:00", false),
        ]);
        let m = run(&trips, &cal, &overrides, "2026-10");
        assert_eq!(
            reasons(&m),
            [
                ("commute (LL § 9 B)".to_string(), 0.0),
                ("set by you".to_string(), 12.0),
                ("set by you".to_string(), 0.0),
            ]
        );
        assert_eq!(m.entries[0].purpose, "Erhvervskørsel");
        // Reset: the rules decide again.
        let m = run(&trips, &cal, &BTreeMap::new(), "2026-10");
        assert_eq!(m.total_km(), 0.0);
    }

    #[test]
    fn a_missing_rate_is_an_error_only_with_business_km() {
        let cal = timed_entries(&[], &[], "Frokost");
        let compute_2027 = |trips: &[Trip]| {
            compute(
                &cfg(),
                WORK,
                &Places::default(),
                Month::parse("2027-01").unwrap(),
                trips,
                &cal,
                &BTreeMap::new(),
            )
        };
        let commute = trip("2027-01-04 07:30", "Hjem", "Kontor", "", 25.0);
        assert!(compute_2027(std::slice::from_ref(&commute)).is_ok());
        let visit = trip("2027-01-05 07:30", "Hjem", "Kunde", "Service", 25.0);
        let err = compute_2027(&[commute, visit]).unwrap_err().to_string();
        assert!(err.contains("2027"), "{err}");
    }

    #[test]
    fn logbook_carries_skats_fields() {
        let mut c = cfg();
        c.cpr = Some("010101-1234".into());
        let trips = vec![trip("2026-10-05 08:00", "Hjem", "Kunde B", "Service", 30.0)];
        let m = run(&trips, &[], &BTreeMap::new(), "2026-10");
        let mut out = Vec::new();
        write_logbook(&c, &m, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        for line in [
            "Navn;Test",
            "Adresse;Hjem",
            "CPR-nr.;010101-1234",
            "Erhvervskørte km i år til og med perioden;30,00",
            "05-10-2026;08:00;Hjem;Kunde B;Service;30,00;3,94;118,20",
        ] {
            assert!(text.contains(line), "{line} in {text}");
        }
        c.cpr = None;
        let html = logbook_html(&c, &m);
        assert!(html.contains("<dt>CPR-nr.</dt><dd></dd>"), "{html}");
    }

    fn fradrag_for(one_way_km: f64, yderkommune: bool) -> Fradrag {
        let mut c = cfg();
        c.yderkommune = yderkommune;
        let mut trips = Vec::new();
        for d in 5..=9 {
            trips.push(trip(
                &format!("2026-10-0{d} 07:30"),
                "Hjem",
                "Kontor",
                "",
                one_way_km,
            ));
            trips.push(trip(
                &format!("2026-10-0{d} 16:00"),
                "Kontor",
                "Hjem",
                "",
                one_way_km,
            ));
        }
        // A client visit from home on the 9th: those km are reimbursed.
        trips.push(trip("2026-10-09 17:00", "Hjem", "Kunde", "Service", 10.0));
        let home_working = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        fradrag(
            &c,
            WORK,
            &Places::default(),
            2026,
            &trips,
            &[],
            &BTreeMap::new(),
            |d| (d == home_working).then_some("home working"),
        )
        .unwrap()
    }

    #[test]
    fn befordringsfradrag_starts_above_24_km_a_day() {
        let f = fradrag_for(9.0, false);
        assert_eq!(
            (f.round_trip_km(), f.per_day(), f.total()),
            (18.0, 0.0, 0.0)
        );
        assert_eq!(f.days.len(), 3);
        assert_eq!(f.excluded.len(), 2);

        // 80 km a day: 56 km at 3,17.
        let f = fradrag_for(40.0, false);
        assert_eq!((f.per_day(), f.total()), (177.52, 532.56));
        // 140 km a day: 96 km at 3,17 and 20 km at 1,59 (3,51 in a yderkommune).
        assert_eq!(fradrag_for(70.0, false).per_day(), 336.12);
        assert_eq!(fradrag_for(70.0, true).per_day(), 374.52);
    }
}
