//! Builds the per-day picture for one month: norm hours from the work
//! calendar, work periods from trips, and the difference between them.
//!
//! Working time is the time spent at a work address: from arriving at the
//! address of that day's calendar entry until leaving it again, and the time
//! working from home.
//!
//! Totals count the days up to and including today; later days show their
//! planned norm only.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Weekday};
use chrono_tz::Tz;

use crate::address::{self, Places};
use crate::calendar::Occurrence;
use crate::config::Config;
use crate::holidays;
use crate::state::{PlannedWindow, PrivateInterval, State};
use crate::trips::Trip;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Month {
    pub year: i32,
    pub month: u32,
}

impl Month {
    pub fn parse(s: &str) -> Result<Self> {
        let first = NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d")
            .with_context(|| format!("month must be YYYY-MM, got {s:?}"))?;
        Ok(Month {
            year: first.year(),
            month: first.month(),
        })
    }

    pub fn first_day(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.year, self.month, 1).unwrap()
    }

    pub fn last_day(self) -> NaiveDate {
        let next = if self.month == 12 {
            NaiveDate::from_ymd_opt(self.year + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(self.year, self.month + 1, 1)
        };
        next.unwrap() - Duration::days(1)
    }

    pub fn days(self) -> impl Iterator<Item = NaiveDate> {
        self.first_day()
            .iter_days()
            .take_while(move |d| *d <= self.last_day())
    }

    pub fn contains(self, date: NaiveDate) -> bool {
        date.year() == self.year && date.month() == self.month
    }
}

impl std::fmt::Display for Month {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{:02}", self.year, self.month)
    }
}

/// A start and end time on one day.
pub type Span = (DateTime<Tz>, DateTime<Tz>);

#[derive(Debug, Clone)]
pub struct Day {
    pub date: NaiveDate,
    pub norm_hours: f64,
    /// Planned working time from the calendar (first start, last end).
    pub planned: Option<Span>,
    /// Why this is not a working day.
    pub off_reason: Option<OffReason>,
    /// Work addresses for the day (calendar locations or the default).
    pub work_locations: Vec<String>,
    /// Private appointments and recorded private time (not company time),
    /// already left out of `segments`.
    pub appointments: Vec<Span>,
    /// Home-working periods: the planned window of a home-working calendar
    /// event (up to today) and recorded home-working time. Part of
    /// `segments`.
    pub home: Vec<Span>,
    /// Periods at a work address or working from home, minus private
    /// appointments; overlaps count once.
    pub segments: Vec<Span>,
    /// Start of the first and end of the last period at work.
    pub first_start: Option<DateTime<Tz>>,
    pub last_end: Option<DateTime<Tz>>,
    /// Anything that needs checking (e.g. arrival without departure),
    /// followed by the day's private appointments.
    pub note: Option<String>,
    /// What in `note` needs checking, without the private appointments.
    pub check: Option<String>,
    /// The day's lunch break, deducted where it overlaps `segments`.
    pub lunch: Lunch,
    pub worked_hours: f64,
}

/// Why a day is not a working day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OffReason {
    /// A public holiday or contractual day off, by name.
    Holiday(&'static str),
    /// Inside a vacation or feriefridag event in the calendar.
    Leave {
        kind: LeaveKind,
        /// The calendar event's title.
        title: String,
        /// Whether the day would have been a working day, i.e. counts as a
        /// vacation day: a weekday with planned hours (or `[work_week]` hours
        /// without the calendar norm) that is not a public holiday.
        working_day: bool,
    },
    /// Another all-day calendar event, by title.
    Other(String),
}

impl std::fmt::Display for OffReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OffReason::Holiday(name) => f.write_str(name),
            OffReason::Leave { kind, .. } => kind.fmt(f),
            OffReason::Other(title) => f.write_str(title),
        }
    }
}

/// The kinds of leave taken from a balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LeaveKind {
    Ferie,
    Feriefridag,
}

impl std::fmt::Display for LeaveKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LeaveKind::Ferie => "Ferie",
            LeaveKind::Feriefridag => "Feriefridag",
        })
    }
}

/// What an all-day calendar event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllDayKind {
    Leave(LeaveKind),
    /// A day off for another reason (matches `days_off_keywords`).
    Other,
}

/// Classifies an all-day event by its title: a feriefridag word, then a
/// vacation word, then the calendar's days-off keywords (empty: every other
/// all-day event is a day off). Words match anywhere in the title,
/// case-insensitively ("Ferie" matches "Juleferie"). `None`: not a day off.
pub fn classify_all_day(cfg: &Config, summary: &str) -> Option<AllDayKind> {
    let title = summary.to_lowercase();
    let has = |words: &[String]| {
        words
            .iter()
            .map(|w| w.trim().to_lowercase())
            .any(|w| !w.is_empty() && title.contains(&w))
    };
    let v = &cfg.vacation;
    if has(&v.feriefridag_keywords) {
        return Some(AllDayKind::Leave(LeaveKind::Feriefridag));
    }
    if has(&v.keywords) {
        return Some(AllDayKind::Leave(LeaveKind::Ferie));
    }
    let keywords = cfg
        .calendar
        .as_ref()
        .map(|c| c.days_off_keywords.as_slice())
        .unwrap_or_default();
    (keywords.is_empty() || has(keywords)).then_some(AllDayKind::Other)
}

/// A day's lunch break and where it comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lunch {
    /// `None` when the day has no lunch (an override of 0 minutes).
    pub window: Option<Span>,
    pub source: LunchSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LunchSource {
    /// `[lunch]` start and minutes.
    Default,
    /// A timed calendar event with the lunch title.
    Calendar,
    /// Set with `lunch set`.
    Override,
}

impl std::fmt::Display for LunchSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LunchSource::Default => "default",
            LunchSource::Calendar => "calendar",
            LunchSource::Override => "override",
        })
    }
}

impl Day {
    pub fn difference(&self) -> f64 {
        self.worked_hours - self.norm_hours
    }

    /// Periods at work, e.g. "08:02–11:30, 12:15–15:40".
    pub fn periods_text(&self) -> String {
        self.segments
            .iter()
            .map(|(s, e)| format!("{}–{}", s.format("%H:%M"), e.format("%H:%M")))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Lunch window and source, e.g. "12:00–12:30 (default)".
    pub fn lunch_text(&self) -> String {
        let window = match self.lunch.window {
            Some((s, e)) => format!("{}–{}", s.format("%H:%M"), e.format("%H:%M")),
            None => "none".into(),
        };
        format!("{window} ({})", self.lunch.source)
    }

    /// Day-off reason and anything that needs checking.
    pub fn remark(&self) -> String {
        [
            self.off_reason.as_ref().map(ToString::to_string),
            self.note.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("; ")
    }
}

#[derive(Debug)]
pub struct MonthReport {
    pub month: Month,
    pub days: Vec<Day>,
    /// The totals count the days up to and including this one.
    pub today: NaiveDate,
}

impl MonthReport {
    /// The days the totals count: up to and including today.
    pub fn to_date(&self) -> impl Iterator<Item = &Day> {
        self.days.iter().filter(|d| d.date <= self.today)
    }
    /// Norm hours up to and including today.
    pub fn norm_total(&self) -> f64 {
        self.to_date().map(|d| d.norm_hours).fold(0.0, |a, b| a + b)
    }
    /// Worked hours up to and including today.
    pub fn worked_total(&self) -> f64 {
        self.to_date()
            .map(|d| d.worked_hours)
            .fold(0.0, |a, b| a + b)
    }
    /// Norm hours of the whole month, including the days after today.
    pub fn month_norm_total(&self) -> f64 {
        self.days
            .iter()
            .map(|d| d.norm_hours)
            .fold(0.0, |a, b| a + b)
    }
}

/// The month's days, with totals up to and including `today`
/// (`NaiveDate::MAX`: the whole month). The lunch overrides in `state` beat
/// a lunch calendar event, which beats the `[lunch]` default. Recorded
/// private time counts like a private appointment; one linked to an
/// appointment replaces that appointment's planned window, and an
/// appointment whose time you deleted counts nothing. Recorded
/// home-working time, and the planned window of a home-working event on a
/// day up to today, count as worked.
pub fn build(
    cfg: &Config,
    month: Month,
    trips: &[Trip],
    calendar: &[Occurrence],
    state: &State,
    today: NaiveDate,
) -> MonthReport {
    let tz = cfg.timezone;
    let lunch_overrides = &state.lunch;
    let recorded = state.private.intervals.as_slice();

    // Public holidays first: a vacation event over a holiday does not make
    // the holiday a vacation day.
    let mut off: HashMap<NaiveDate, OffReason> = holidays::days_off(month.year, &cfg.holidays)
        .into_iter()
        .map(|(d, name)| (d, OffReason::Holiday(name)))
        .collect();
    let norm_from_calendar = cfg.calendar.as_ref().is_some_and(|c| c.norm_from_calendar);
    let default_location = cfg
        .calendar
        .as_ref()
        .and_then(|c| c.default_location.clone());
    let match_addresses = cfg.calendar.is_some();
    let private_words: &[String] = cfg
        .calendar
        .as_ref()
        .map(|c| c.private_keywords.as_slice())
        .unwrap_or_default();

    // Planned working time per date, from timed calendar events.
    let mut planned: BTreeMap<NaiveDate, Vec<Span>> = BTreeMap::new();
    let mut appointments: BTreeMap<NaiveDate, Vec<(Span, &str)>> = BTreeMap::new();
    let mut locations: BTreeMap<NaiveDate, Vec<String>> = BTreeMap::new();
    let mut missing_location: BTreeMap<NaiveDate, String> = BTreeMap::new();
    let mut lunch_events: BTreeMap<NaiveDate, Span> = BTreeMap::new();
    // Dates with a work event (home working included), and home working.
    let mut work_days: BTreeSet<NaiveDate> = BTreeSet::new();
    let mut home: BTreeMap<NaiveDate, Vec<(Span, &str)>> = BTreeMap::new();
    for occ in calendar {
        match occ {
            Occurrence::AllDay { date, summary } => {
                let reason = match classify_all_day(cfg, summary) {
                    Some(AllDayKind::Leave(kind)) => OffReason::Leave {
                        kind,
                        title: summary.clone(),
                        working_day: false,
                    },
                    Some(AllDayKind::Other) => OffReason::Other(summary.clone()),
                    None => continue,
                };
                off.entry(*date).or_insert(reason);
            }
            Occurrence::Timed {
                start,
                end,
                summary,
                location,
            } => {
                match classify(summary, private_words, &cfg.lunch.title) {
                    EntryKind::Private => {
                        let window = PlannedWindow {
                            title: summary.clone(),
                            start: start.naive_local(),
                            end: end.naive_local(),
                        };
                        // Measured (or calendar time, the same window), or
                        // deleted because you did not go.
                        let measured = recorded
                            .iter()
                            .any(|i| i.replaces.as_ref() == Some(&window));
                        if measured || state.dismissed.contains(&window) {
                            continue;
                        }
                        for span in split_at_midnight(*start, *end, tz) {
                            appointments
                                .entry(span.0.date_naive())
                                .or_default()
                                .push((span, summary));
                        }
                        continue;
                    }
                    EntryKind::Lunch => {
                        // One break per day: the earliest lunch event.
                        if let Some(span) = split_at_midnight(*start, *end, tz).first() {
                            let day = lunch_events.entry(span.0.date_naive()).or_insert(*span);
                            if span.0 < day.0 {
                                *day = *span;
                            }
                        }
                        continue;
                    }
                    EntryKind::Work | EntryKind::Home => {}
                }
                work_days.insert(start.date_naive());
                if is_home_working(cfg, summary, location) {
                    for span in split_at_midnight(*start, *end, tz) {
                        if span.0.date_naive() <= today {
                            home.entry(span.0.date_naive())
                                .or_default()
                                .push((span, summary));
                        }
                        if norm_from_calendar {
                            planned.entry(span.0.date_naive()).or_default().push(span);
                        }
                    }
                    continue;
                }
                let day_locations = locations.entry(start.date_naive()).or_default();
                if location.trim().is_empty() {
                    missing_location.insert(start.date_naive(), summary.clone());
                } else if !day_locations.contains(location) {
                    day_locations.push(location.clone());
                }
                if !norm_from_calendar {
                    continue;
                }
                for span in split_at_midnight(*start, *end, tz) {
                    planned.entry(span.0.date_naive()).or_default().push(span);
                }
            }
        }
    }

    for (span, label) in recorded_spans(recorded, tz) {
        for span in split_at_midnight(span.0, span.1, tz) {
            appointments
                .entry(span.0.date_naive())
                .or_default()
                .push((span, label));
        }
    }
    for day in appointments.values_mut() {
        day.sort_by_key(|(span, _)| *span);
    }
    for (span, label) in recorded_spans(&state.home.intervals, tz) {
        for span in split_at_midnight(span.0, span.1, tz) {
            home.entry(span.0.date_naive())
                .or_default()
                .push((span, label));
        }
    }
    for day in home.values_mut() {
        day.sort_by_key(|(span, _)| *span);
    }

    // Trips are assigned to the local date they start on.
    let mut by_date: BTreeMap<NaiveDate, Vec<&Trip>> = BTreeMap::new();
    for trip in trips {
        by_date
            .entry(trip.start.date_naive())
            .or_default()
            .push(trip);
    }

    let days = month
        .days()
        .map(|date| {
            let day_plan = planned.get(&date).map(Vec::as_slice).unwrap_or(&[]);
            let off_reason = off.get(&date).cloned().map(|reason| match reason {
                OffReason::Leave { kind, title, .. } => OffReason::Leave {
                    kind,
                    title,
                    working_day: !matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
                        && if norm_from_calendar {
                            !day_plan.is_empty()
                        } else {
                            cfg.work_week.hours(date.weekday()) > 0.0
                        },
                },
                other => other,
            });
            let lunch = match (lunch_overrides.get(&date), lunch_events.get(&date)) {
                (Some(o), _) => Lunch {
                    window: window(date, o.start, o.minutes, tz),
                    source: LunchSource::Override,
                },
                (None, Some(span)) => Lunch {
                    window: Some(*span),
                    source: LunchSource::Calendar,
                },
                (None, None) => Lunch {
                    window: window(date, cfg.lunch.start, cfg.lunch.minutes, tz),
                    source: LunchSource::Default,
                },
            };
            // Planned time in the calendar includes the lunch break; the norm
            // does not, so a normal day with lunch has no difference.
            let norm_hours = match (&off_reason, norm_from_calendar) {
                (Some(_), _) => 0.0,
                (None, true) => {
                    let lunch_window: Vec<Span> = lunch.window.into_iter().collect();
                    subtract(day_plan, &lunch_window)
                        .iter()
                        .map(|(s, e)| (*e - *s).num_minutes() as f64 / 60.0)
                        .fold(0.0, |a, b| a + b)
                }
                (None, false) => cfg.work_week.hours(date.weekday()),
            };
            // Planned time is not shown on days off.
            let planned = day_plan
                .iter()
                .filter(|_| off_reason.is_none())
                .map(|p| p.0)
                .min()
                .zip(day_plan.iter().map(|p| p.1).max());

            let day_trips = by_date.get(&date).map(Vec::as_slice).unwrap_or(&[]);
            let mut work_locations = locations.get(&date).cloned().unwrap_or_default();
            if work_locations.is_empty()
                && let Some(default) = &default_location
            {
                work_locations.push(default.clone());
            }

            // The fixed workplace always counts, e.g. when a colleague drives
            // on from there to the day's calendar address.
            let mut match_locations = work_locations.clone();
            if let Some(work) = &cfg.workplace
                && !match_locations.iter().any(|l| address::matches(l, work))
            {
                match_locations.push(work.clone());
            }

            let day_home = home.get(&date).map(Vec::as_slice).unwrap_or(&[]);
            let (segments, mut note) = if !match_addresses {
                // No calendar: first departure to last arrival.
                let span = day_trips
                    .iter()
                    .map(|t| t.start)
                    .min()
                    .zip(day_trips.iter().map(|t| t.end).max());
                (span.into_iter().collect(), None)
            } else if match_locations.is_empty() {
                let note = (!day_trips.is_empty() && day_home.is_empty())
                    .then(|| "kørsel, men ingen arbejdsadresse i kalenderen".to_string());
                (Vec::new(), note)
            } else {
                work_segments(
                    day_trips,
                    &match_locations,
                    &cfg.known_places,
                    cfg.periods.count_travel_time,
                    day_home.is_empty(),
                )
            };
            if note.is_none()
                && let Some(summary) = missing_location.get(&date)
            {
                note = Some(format!("kalenderpost uden adresse: {summary}"));
            }

            // Nothing needs fixing on a day without work: driving then is
            // private (e.g. at the weekend).
            if !(work_days.contains(&date) || !day_home.is_empty() || norm_hours > 0.0) {
                note = None;
            }
            let check = note.clone();
            for ((s, e), label) in day_home {
                let text = format!("{label} {}–{} · home", s.format("%H:%M"), e.format("%H:%M"));
                note = Some(match note {
                    Some(n) => format!("{n}; {text}"),
                    None => text,
                });
            }
            let home_spans: Vec<Span> = day_home.iter().map(|(span, _)| *span).collect();
            let segments = merge(segments.into_iter().chain(home_spans.iter().copied()));
            let day_appointments = appointments.get(&date).map(Vec::as_slice).unwrap_or(&[]);
            for ((s, e), summary) in day_appointments {
                let text = format!(
                    "{summary} {}–{} · private",
                    s.format("%H:%M"),
                    e.format("%H:%M")
                );
                note = Some(match note {
                    Some(n) => format!("{n}; {text}"),
                    None => text,
                });
            }
            let appointments: Vec<Span> = day_appointments.iter().map(|(span, _)| *span).collect();
            let segments = subtract(&segments, &appointments);

            let worked_hours = worked(&segments, lunch.window, cfg.periods.round_to_minutes);
            Day {
                date,
                norm_hours,
                planned,
                off_reason,
                work_locations,
                first_start: segments.first().map(|s| s.0),
                last_end: segments.last().map(|s| s.1),
                appointments,
                home: home_spans,
                segments,
                note,
                check,
                lunch,
                worked_hours,
            }
        })
        .collect();

    MonthReport { month, days, today }
}

/// Whether a work event is home working: its title has a `[home_working]`
/// word (whole word, case-insensitive), or with `at_home_address` its
/// location is the home address (`mileage.home`).
pub fn is_home_working(cfg: &Config, summary: &str, location: &str) -> bool {
    let h = &cfg.home_working;
    has_word(summary, &h.keywords)
        || (h.at_home_address
            && !location.trim().is_empty()
            && cfg
                .mileage
                .as_ref()
                .and_then(|m| m.home.as_deref())
                .is_some_and(|home| address::matches(location, home)))
}

/// What a timed calendar entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Planned working time at the entry's address.
    Work,
    /// A private appointment (e.g. the doctor): not company time.
    Private,
    /// The day's lunch break: no work address and no planned hours.
    Lunch,
    /// Planned working time from home (see [`is_home_working`]): no address
    /// to drive to.
    Home,
}

/// A timed calendar entry and what its title makes it.
#[derive(Debug, Clone)]
pub struct TimedEntry<'a> {
    pub kind: EntryKind,
    pub start: DateTime<Tz>,
    pub end: DateTime<Tz>,
    pub summary: &'a str,
    pub location: &'a str,
}

/// The calendar's timed entries, classified by title (see [`classify`]).
pub fn timed_entries<'a>(
    calendar: &'a [Occurrence],
    private_words: &[String],
    lunch_title: &str,
) -> Vec<TimedEntry<'a>> {
    calendar
        .iter()
        .filter_map(|occ| match occ {
            Occurrence::Timed {
                start,
                end,
                summary,
                location,
            } => Some(TimedEntry {
                kind: classify(summary, private_words, lunch_title),
                start: *start,
                end: *end,
                summary,
                location,
            }),
            Occurrence::AllDay { .. } => None,
        })
        .collect()
}

/// Recorded private time as private entries without an address, e.g. for
/// classifying the trips that start during it.
pub fn recorded_entries(recorded: &[PrivateInterval], tz: Tz) -> Vec<TimedEntry<'_>> {
    recorded_spans(recorded, tz)
        .into_iter()
        .map(|((start, end), label)| TimedEntry {
            kind: EntryKind::Private,
            start,
            end,
            summary: label,
            location: "",
        })
        .collect()
}

/// Recorded intervals in `tz`, with their labels.
fn recorded_spans(recorded: &[PrivateInterval], tz: Tz) -> Vec<(Span, &str)> {
    let local = |t| tz.from_local_datetime(&t).earliest();
    recorded
        .iter()
        .filter_map(|i| Some(((local(i.start)?, local(i.end)?), i.label.as_str())))
        .collect()
}

/// Classifies a timed calendar entry by its title. A configured word matches
/// a whole word of the title, case-insensitively ("Lægemiddelfirma" is not
/// "Læge"). A private word wins over the lunch title.
fn classify(summary: &str, private_words: &[String], lunch_title: &str) -> EntryKind {
    if has_word(summary, private_words) {
        EntryKind::Private
    } else if has_word(summary, &[lunch_title.to_string()]) {
        EntryKind::Lunch
    } else {
        EntryKind::Work
    }
}

/// Whether `summary` contains one of `keywords` as whole words,
/// case-insensitively.
fn has_word(summary: &str, keywords: &[String]) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let title = words(summary);
    keywords.iter().any(|k| {
        let k = words(k);
        !k.is_empty() && title.windows(k.len()).any(|w| w == k.as_slice())
    })
}

/// Splits a span crossing midnight into one span per date it covers.
fn split_at_midnight(start: DateTime<Tz>, end: DateTime<Tz>, tz: Tz) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut s = start;
    while s < end {
        let next_midnight = tz
            .from_local_datetime(
                &(s.date_naive() + Duration::days(1))
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )
            .earliest()
            .unwrap_or(end);
        let e = end.min(next_midnight);
        spans.push((s, e));
        s = e;
    }
    spans
}

/// `spans` with every part covered by one of `windows` removed. Overlapping
/// windows remove a time only once.
fn subtract(spans: &[Span], windows: &[Span]) -> Vec<Span> {
    let mut out = spans.to_vec();
    for (ws, we) in windows {
        out = out
            .into_iter()
            .flat_map(|(s, e)| {
                if *we <= s || *ws >= e {
                    return vec![(s, e)];
                }
                [(s, *ws), (*we, e)]
                    .into_iter()
                    .filter(|(a, b)| a < b)
                    .collect()
            })
            .collect();
    }
    out
}

/// Periods at a work address: from a trip arriving at one of `locations`
/// until a trip departing from that address. A drive from one work address
/// to another is working time; with `count_travel` the drives to and from
/// work are included too. `places` locates home and the workplace by
/// coordinates too. Without `expect_driving` (working from home), trips
/// that never reach work are no reason for a note.
fn work_segments(
    trips: &[&Trip],
    locations: &[String],
    places: &Places,
    count_travel: bool,
    expect_driving: bool,
) -> (Vec<Span>, Option<String>) {
    let mut segments: Vec<Span> = Vec::new();
    let mut at: Option<(DateTime<Tz>, &str)> = None;
    for t in trips {
        let mut left_work = false;
        if let Some((since, loc)) = at
            && places.at(&t.from, t.start_pos, loc)
        {
            let until = if count_travel { t.end } else { t.start };
            segments.push((since, until));
            at = None;
            left_work = true;
        }
        if at.is_none()
            && let Some(loc) = locations.iter().find(|l| places.at(&t.to, t.end_pos, l))
        {
            let since = if count_travel || left_work {
                t.start
            } else {
                t.end
            };
            at = Some((since, loc));
        }
    }
    let note = match at {
        Some((_, loc)) => Some(format!("ankomst til {loc} uden registreret afgang")),
        None if segments.is_empty() && !trips.is_empty() && expect_driving => {
            Some("ingen kørsel til eller fra arbejdsadressen".to_string())
        }
        None => None,
    };

    // Merge overlaps (travel between two work addresses with count_travel).
    (merge(segments), note)
}

/// `spans` sorted, with overlapping ones joined.
fn merge(spans: impl IntoIterator<Item = Span>) -> Vec<Span> {
    let mut spans: Vec<Span> = spans.into_iter().collect();
    spans.sort();
    let mut merged: Vec<Span> = Vec::new();
    for (s, e) in spans {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

/// A lunch window of `minutes` from `start` on `date`; `None` for 0 minutes.
fn window(date: NaiveDate, start: chrono::NaiveTime, minutes: u32, tz: Tz) -> Option<Span> {
    if minutes == 0 {
        return None;
    }
    let s = tz.from_local_datetime(&date.and_time(start)).earliest()?;
    Some((s, s + Duration::minutes(i64::from(minutes))))
}

/// Hours at work minus the part of the lunch window that overlaps it, rounded
/// to `round_to` minutes. Lunch outside work time (e.g. inside a private
/// appointment or a lunch errand) is not deducted.
fn worked(segments: &[Span], lunch: Option<Span>, round_to: u32) -> f64 {
    let left = subtract(segments, lunch.as_slice());
    let mut minutes: i64 = left.iter().map(|(s, e)| (*e - *s).num_minutes()).sum();
    if minutes <= 0 {
        return 0.0;
    }
    let step = i64::from(round_to);
    if step > 0 {
        minutes = (minutes + step / 2) / step * step;
    }
    minutes as f64 / 60.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar;
    use crate::state::LunchOverride;
    use crate::trips::parse_time;
    use chrono::NaiveTime;

    fn cfg(extra: &str) -> Config {
        toml::from_str(&format!(
            r#"
            employee_number = "1"
            [danlon]
            {extra}
            "#
        ))
        .unwrap()
    }

    fn trip(cfg: &Config, s: &str, e: &str) -> Trip {
        Trip {
            start: parse_time(s, cfg.timezone).unwrap(),
            end: parse_time(e, cfg.timezone).unwrap(),
            distance_km: Some(20.0),
            from: String::new(),
            to: String::new(),
            purpose: String::new(),
            business: None,
            start_pos: None,
            end_pos: None,
        }
    }

    #[test]
    fn norm_hours_october_2026() {
        let cfg = cfg("");
        let r = build(
            &cfg,
            Month::parse("2026-10").unwrap(),
            &[],
            &[],
            &State::default(),
            NaiveDate::MAX,
        );
        // 22 weekdays, no holidays, 7.4 h each.
        assert!((r.norm_total() - 22.0 * 7.4).abs() < 1e-9);
    }

    #[test]
    fn holidays_reduce_norm() {
        let cfg = cfg("");
        // December 2026: 23 weekdays, minus 24th, 25th, 31st (26th is a Saturday).
        let r = build(
            &cfg,
            Month::parse("2026-12").unwrap(),
            &[],
            &[],
            &State::default(),
            NaiveDate::MAX,
        );
        assert!((r.norm_total() - 20.0 * 7.4).abs() < 1e-9);
    }

    #[test]
    fn work_period_from_trips() {
        let cfg = cfg("");
        let trips = vec![
            trip(&cfg, "2026-10-05 07:30", "2026-10-05 08:00"),
            trip(&cfg, "2026-10-05 16:30", "2026-10-05 17:00"),
            trip(&cfg, "2026-10-10 09:00", "2026-10-10 12:30"),
        ];
        let r = build(
            &cfg,
            Month::parse("2026-10").unwrap(),
            &trips,
            &[],
            &State::default(),
            NaiveDate::MAX,
        );
        let mon = &r.days[4];
        assert_eq!(mon.worked_hours, 9.0); // 07:30–17:00 minus 30 min
        assert!((mon.difference() - 1.6).abs() < 1e-9);
        let sat = &r.days[9];
        assert_eq!(sat.difference(), 3.0);
    }

    #[test]
    fn calendar_sets_norm_and_days_off() {
        let mut cfg = cfg("");
        cfg.calendar = Some(crate::config::CalendarConfig {
            source: String::new(),
            norm_from_calendar: true,
            days_off_keywords: vec!["ferie".into()],
            private_keywords: vec![],
            name: None,
            default_location: None,
        });
        let ics = "BEGIN:VEVENT
UID:w
DTSTART;TZID=Europe/Copenhagen:20261005T080000
DTEND;TZID=Europe/Copenhagen:20261005T160000
RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH
END:VEVENT
BEGIN:VEVENT
UID:f
SUMMARY:Efterårsferie
DTSTART;VALUE=DATE:20261019
DTEND;VALUE=DATE:20261024
END:VEVENT
BEGIN:VEVENT
UID:m
SUMMARY:Tandlæge
DTSTART;VALUE=DATE:20261026
END:VEVENT
";
        let m = Month::parse("2026-10").unwrap();
        let occ = calendar::occurrences(ics, cfg.timezone, m.first_day(), m.last_day()).unwrap();
        let r = build(&cfg, m, &[], &occ, &State::default(), NaiveDate::MAX);
        // Mon–Thu 08:00–16:00 from 5 Oct minus the default lunch = 7,5 h: weeks of
        // 5, 12, 26 Oct = 12 days; 19–22 Oct are ferie.
        assert_eq!(r.norm_total(), 12.0 * 7.5);
        // "Efterårsferie" contains the vacation word "Ferie".
        assert_eq!(
            r.days[18].off_reason,
            Some(OffReason::Leave {
                kind: LeaveKind::Ferie,
                title: "Efterårsferie".into(),
                working_day: true,
            })
        );
        assert_eq!(r.days[18].off_reason.as_ref().unwrap().to_string(), "Ferie");
        // Friday 23 Oct has no planned hours, so it is no vacation day.
        assert!(matches!(
            r.days[22].off_reason,
            Some(OffReason::Leave {
                working_day: false,
                ..
            })
        ));
        // "Tandlæge" does not match the keywords, so the 26th stays a workday.
        assert_eq!(r.days[25].norm_hours, 7.5);
        // Fridays have no planned time.
        assert_eq!(r.days[8].norm_hours, 0.0);
    }

    #[test]
    fn vacation_days_have_norm_0_and_reason_ferie() {
        let mut cfg = cfg("[calendar]\nsource = \"\"\ndays_off_keywords = [\"fri\"]");
        cfg.lunch.minutes = 0;
        let ics = "BEGIN:VEVENT
UID:w
SUMMARY:Kontor
DTSTART;TZID=Europe/Copenhagen:20261005T080000
DTEND;TZID=Europe/Copenhagen:20261005T160000
RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR
END:VEVENT
BEGIN:VEVENT
UID:f
SUMMARY:ferie
DTSTART;VALUE=DATE:20261009
DTEND;VALUE=DATE:20261013
END:VEVENT
BEGIN:VEVENT
UID:d
SUMMARY:Feriefridag
DTSTART;VALUE=DATE:20261016
END:VEVENT
BEGIN:VEVENT
UID:o
SUMMARY:Kursus
DTSTART;VALUE=DATE:20261020
END:VEVENT
";
        let m = Month::parse("2026-10").unwrap();
        let occ = calendar::occurrences(ics, cfg.timezone, m.first_day(), m.last_day()).unwrap();
        let r = build(&cfg, m, &[], &occ, &State::default(), NaiveDate::MAX);
        let day = |n: usize| &r.days[n - 1];
        // Fri 9 – Mon 12 Oct, also outside the days-off keywords.
        for n in [9, 10, 11, 12] {
            assert_eq!(day(n).norm_hours, 0.0);
            assert_eq!(day(n).off_reason.as_ref().unwrap().to_string(), "Ferie");
            assert_eq!(day(n).planned, None);
        }
        let working = |n: usize| match &day(n).off_reason {
            Some(OffReason::Leave { working_day, .. }) => *working_day,
            other => panic!("{other:?}"),
        };
        assert!(working(9) && working(12));
        assert!(!working(10) && !working(11));
        assert_eq!(
            day(16).off_reason.as_ref().unwrap().to_string(),
            "Feriefridag"
        );
        assert_eq!(day(16).norm_hours, 0.0);
        // Not a vacation word and not a days-off keyword: a working day.
        assert_eq!(day(20).off_reason, None);
        assert_eq!(day(20).norm_hours, 8.0);
    }

    fn located_trip(cfg: &Config, s: &str, e: &str, from: &str, to: &str) -> Trip {
        Trip {
            from: from.into(),
            to: to.into(),
            ..trip(cfg, s, e)
        }
    }

    #[test]
    fn working_time_is_time_at_calendar_address() {
        let mut cfg = cfg("");
        cfg.lunch.minutes = 0;
        cfg.calendar = Some(crate::config::CalendarConfig {
            source: String::new(),
            norm_from_calendar: true,
            days_off_keywords: vec![],
            private_keywords: vec![],
            name: None,
            default_location: Some("Kontorvej 1, 8000 Aarhus C".into()),
        });
        let ics = "BEGIN:VEVENT
UID:a
SUMMARY:Kunde A
LOCATION:Vestergade 12\\, 5000 Odense C\\, Danmark
DTSTART;TZID=Europe/Copenhagen:20261006T080000
DTEND;TZID=Europe/Copenhagen:20261006T153000
END:VEVENT
BEGIN:VEVENT
UID:k
SUMMARY:Kontor
DTSTART;TZID=Europe/Copenhagen:20261008T080000
DTEND;TZID=Europe/Copenhagen:20261008T160000
END:VEVENT
";
        let m = Month::parse("2026-10").unwrap();
        let occ = calendar::occurrences(ics, cfg.timezone, m.first_day(), m.last_day()).unwrap();
        let home = "Hjemvej 3, 8200 Aarhus N";
        let odense = "Vestergade 12, 5000 Odense";
        let trips = vec![
            // Tuesday: at the customer's 08:40–14:30, lunch errand not at the address.
            located_trip(&cfg, "2026-10-06 07:30", "2026-10-06 08:40", home, odense),
            located_trip(
                &cfg,
                "2026-10-06 11:30",
                "2026-10-06 11:40",
                odense,
                "Bager",
            ),
            located_trip(
                &cfg,
                "2026-10-06 11:55",
                "2026-10-06 12:05",
                "Bager",
                odense,
            ),
            located_trip(&cfg, "2026-10-06 14:30", "2026-10-06 15:40", odense, home),
            // Wednesday: no calendar entry, so the default (office) address applies.
            located_trip(
                &cfg,
                "2026-10-07 07:40",
                "2026-10-07 08:00",
                home,
                "Kontorvej 1, 8000 Aarhus",
            ),
            located_trip(
                &cfg,
                "2026-10-07 16:00",
                "2026-10-07 16:20",
                "Kontorvej 1, 8000 Aarhus",
                home,
            ),
            // Thursday (office in the calendar): arrives but no departure recorded.
            located_trip(
                &cfg,
                "2026-10-08 07:40",
                "2026-10-08 08:00",
                home,
                "Kontorvej 1, 8000 Aarhus",
            ),
            // Saturday: private trip only.
            located_trip(
                &cfg,
                "2026-10-10 10:00",
                "2026-10-10 10:20",
                home,
                "Bauhaus",
            ),
        ];
        let r = build(&cfg, m, &trips, &occ, &State::default(), NaiveDate::MAX);

        let tue = &r.days[5];
        assert_eq!(tue.periods_text(), "08:40–11:30, 12:05–14:30");
        assert_eq!(tue.worked_hours, (170.0 + 145.0) / 60.0);
        assert_eq!(tue.norm_hours, 7.5);

        let wed = &r.days[6];
        assert_eq!(wed.periods_text(), "08:00–16:00");

        let thu = &r.days[7];
        assert_eq!(thu.worked_hours, 0.0);
        assert!(
            thu.check
                .as_deref()
                .unwrap()
                .contains("uden registreret afgang")
        );

        // Saturday has no work event: its driving is private, nothing to fix.
        let sat = &r.days[9];
        assert_eq!(sat.worked_hours, 0.0);
        assert_eq!((sat.note.as_deref(), sat.check.as_deref()), (None, None));

        // With travel time, Tuesday runs from leaving home to arriving home.
        cfg.periods.count_travel_time = true;
        let r = build(&cfg, m, &trips, &occ, &State::default(), NaiveDate::MAX);
        assert_eq!(r.days[5].periods_text(), "07:30–11:40, 11:55–15:40");
    }

    #[test]
    fn workplace_counts_on_calendar_days_elsewhere() {
        let mut cfg = cfg("");
        cfg.lunch.minutes = 0;
        cfg.workplace = Some("Kontorvej 1, 8000 Aarhus C".into());
        cfg.calendar = Some(crate::config::CalendarConfig {
            source: String::new(),
            norm_from_calendar: true,
            days_off_keywords: vec![],
            private_keywords: vec![],
            name: None,
            default_location: None,
        });
        let ics = "BEGIN:VEVENT
UID:a
SUMMARY:Kunde A
LOCATION:Vestergade 12\\, 5000 Odense C
DTSTART;TZID=Europe/Copenhagen:20261006T080000
DTEND;TZID=Europe/Copenhagen:20261006T160000
RRULE:FREQ=DAILY;COUNT=2
END:VEVENT
";
        let m = Month::parse("2026-10").unwrap();
        let occ = calendar::occurrences(ics, cfg.timezone, m.first_day(), m.last_day()).unwrap();
        let (home, office, odense) = (
            "Hjem",
            "Kontorvej 1, 8000 Aarhus",
            "Vestergade 12, 5000 Odense",
        );
        let trips = vec![
            // Tuesday: office first, then on to the customer and home.
            located_trip(&cfg, "2026-10-06 07:40", "2026-10-06 08:00", home, office),
            located_trip(&cfg, "2026-10-06 09:00", "2026-10-06 10:15", office, odense),
            located_trip(&cfg, "2026-10-06 15:00", "2026-10-06 16:30", odense, home),
            // Wednesday: a colleague drives from the office; the car stays there.
            located_trip(&cfg, "2026-10-07 07:40", "2026-10-07 08:00", home, office),
            located_trip(&cfg, "2026-10-07 16:30", "2026-10-07 16:50", office, home),
        ];
        let r = build(&cfg, m, &trips, &occ, &State::default(), NaiveDate::MAX);
        assert_eq!(r.days[5].periods_text(), "08:00–15:00");
        assert_eq!(r.days[6].periods_text(), "08:00–16:30");
        assert_eq!(r.days[6].note, None);
    }

    #[test]
    fn private_appointments_are_not_company_time() {
        // `private_keywords` defaults to ["Læge", "Tandlæge", "Fysioterapeut"].
        let mut cfg = cfg("[calendar]\nsource = \"\"");
        cfg.lunch.minutes = 0;
        let ics = "BEGIN:VEVENT
UID:w
SUMMARY:Kontor
LOCATION:Kontorvej 1\\, 8000 Aarhus C
DTSTART;TZID=Europe/Copenhagen:20261005T080000
DTEND;TZID=Europe/Copenhagen:20261005T160000
RRULE:FREQ=DAILY;COUNT=4
END:VEVENT
BEGIN:VEVENT
UID:l
SUMMARY:Læge
LOCATION:Lægehuset\\, Bygade 2\\, 8000 Aarhus C
DTSTART;TZID=Europe/Copenhagen:20261005T100000
DTEND;TZID=Europe/Copenhagen:20261005T111500
END:VEVENT
BEGIN:VEVENT
UID:m
SUMMARY:Lægemiddelfirma
LOCATION:Industrivej 9\\, 8000 Aarhus C
DTSTART;TZID=Europe/Copenhagen:20261006T090000
DTEND;TZID=Europe/Copenhagen:20261006T100000
END:VEVENT
BEGIN:VEVENT
UID:n
SUMMARY:Læge
DTSTART;TZID=Europe/Copenhagen:20261007T100000
DTEND;TZID=Europe/Copenhagen:20261007T110000
END:VEVENT
BEGIN:VEVENT
UID:t
SUMMARY:tandlæge
DTSTART;TZID=Europe/Copenhagen:20261008T100000
DTEND;TZID=Europe/Copenhagen:20261008T110000
END:VEVENT
BEGIN:VEVENT
UID:u
SUMMARY:Læge
DTSTART;TZID=Europe/Copenhagen:20261008T103000
DTEND;TZID=Europe/Copenhagen:20261008T113000
END:VEVENT
BEGIN:VEVENT
UID:f
SUMMARY:Fysioterapeut
DTSTART;TZID=Europe/Copenhagen:20261009T230000
DTEND;TZID=Europe/Copenhagen:20261010T010000
END:VEVENT
";
        let m = Month::parse("2026-10").unwrap();
        let occ = calendar::occurrences(ics, cfg.timezone, m.first_day(), m.last_day()).unwrap();
        let (home, office) = ("Hjem", "Kontorvej 1, 8000 Aarhus");
        let trips = vec![
            located_trip(&cfg, "2026-10-05 07:40", "2026-10-05 07:58", home, office),
            located_trip(&cfg, "2026-10-05 16:00", "2026-10-05 16:20", office, home),
            located_trip(&cfg, "2026-10-08 07:40", "2026-10-08 08:00", home, office),
            located_trip(&cfg, "2026-10-08 16:00", "2026-10-08 16:20", office, home),
        ];
        let r = build(&cfg, m, &trips, &occ, &State::default(), NaiveDate::MAX);

        // Monday: 07:58–16:00 minus Læge 10:00–11:15; norm stays 8 h.
        let mon = &r.days[4];
        assert_eq!(mon.periods_text(), "07:58–10:00, 11:15–16:00");
        assert_eq!(mon.worked_hours, 407.0 / 60.0);
        assert_eq!(format!("{:.2}", mon.worked_hours), "6.78");
        assert_eq!(mon.norm_hours, 8.0);
        assert_eq!(mon.work_locations, vec!["Kontorvej 1, 8000 Aarhus C"]);
        assert_eq!(mon.remark(), "Læge 10:00–11:15 · private");

        // "Lægemiddelfirma" is a work entry: its address and hours count.
        let tue = &r.days[5];
        assert_eq!(tue.norm_hours, 9.0);
        assert!(
            tue.work_locations
                .contains(&"Industrivej 9, 8000 Aarhus C".into())
        );
        assert!(tue.appointments.is_empty());

        // An appointment without address gives no missing-address warning.
        assert_eq!(
            r.days[6].note.as_deref(),
            Some("Læge 10:00–11:00 · private")
        );

        // Overlapping appointments 10:00–11:00 and 10:30–11:30 deduct 90 min once.
        let thu = &r.days[7];
        assert_eq!(thu.worked_hours, 6.5);
        assert_eq!(thu.norm_hours, 8.0);

        // Across midnight: split onto each date.
        let at = |d: &Day| -> Vec<String> {
            d.appointments
                .iter()
                .map(|(s, e)| format!("{}–{}", s.format("%d %H:%M"), e.format("%d %H:%M")))
                .collect()
        };
        assert_eq!(at(&r.days[8]), vec!["09 23:00–10 00:00"]);
        assert_eq!(at(&r.days[9]), vec!["10 00:00–10 01:00"]);
        assert_eq!(r.days[9].norm_hours, 0.0);

        // An empty word list disables the feature.
        cfg.calendar.as_mut().unwrap().private_keywords.clear();
        let r = build(&cfg, m, &trips, &occ, &State::default(), NaiveDate::MAX);
        assert_eq!(r.days[4].worked_hours, 482.0 / 60.0);
        assert!(r.days[4].appointments.is_empty());
    }

    #[test]
    fn recorded_private_time_counts_like_an_appointment() {
        let mut cfg = cfg("[calendar]\nsource = \"\"");
        cfg.lunch.minutes = 0;
        let ics = "BEGIN:VEVENT
UID:w
SUMMARY:Kontor
LOCATION:Kontorvej 1\\, 8000 Aarhus C
DTSTART;TZID=Europe/Copenhagen:20261102T080000
DTEND;TZID=Europe/Copenhagen:20261102T160000
RRULE:FREQ=DAILY;COUNT=2
END:VEVENT
BEGIN:VEVENT
UID:l
SUMMARY:Læge
DTSTART;TZID=Europe/Copenhagen:20261103T130000
DTEND;TZID=Europe/Copenhagen:20261103T140000
END:VEVENT
";
        let m = Month::parse("2026-11").unwrap();
        let occ = calendar::occurrences(ics, cfg.timezone, m.first_day(), m.last_day()).unwrap();
        let (home, office) = ("Hjem", "Kontorvej 1, 8000 Aarhus");
        let mut trips = Vec::new();
        for day in ["02", "03"] {
            let at = |t: &str| format!("2026-11-{day} {t}");
            trips.push(located_trip(&cfg, &at("07:40"), &at("08:00"), home, office));
            trips.push(located_trip(&cfg, &at("16:00"), &at("16:20"), office, home));
        }
        let at = |s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        let recorded = [
            PrivateInterval {
                id: 1,
                start: at("2026-11-02 10:02"),
                end: at("2026-11-02 11:14"),
                label: "Læge".into(),
                kind: Default::default(),
                replaces: None,
            },
            PrivateInterval {
                id: 2,
                start: at("2026-11-03 12:30"),
                end: at("2026-11-03 14:10"),
                label: "Læge".into(),
                kind: Default::default(),
                replaces: Some(crate::state::PlannedWindow {
                    title: "Læge".into(),
                    start: at("2026-11-03 13:00"),
                    end: at("2026-11-03 14:00"),
                }),
            },
        ];
        let mut state = State::default();
        state.private.intervals = recorded.to_vec();
        let r = build(&cfg, m, &trips, &occ, &state, NaiveDate::MAX);

        // Recorded 10:02–11:14 is deducted and noted like a calendar Læge.
        let mon = &r.days[1];
        assert_eq!(mon.periods_text(), "08:00–10:02, 11:14–16:00");
        assert_eq!(mon.worked_hours, (480.0 - 72.0) / 60.0);
        assert_eq!(mon.remark(), "Læge 10:02–11:14 · private");

        // The linked 12:30–14:10 replaces the planned 13:00–14:00: 1:40 once.
        let tue = &r.days[2];
        assert_eq!(tue.periods_text(), "08:00–12:30, 14:10–16:00");
        assert_eq!(tue.worked_hours, (480.0 - 100.0) / 60.0);
        assert_eq!(tue.remark(), "Læge 12:30–14:10 · private");
    }

    #[test]
    fn lunch_overlap_with_work_time_is_deducted() {
        let cfg = cfg("[calendar]\nsource = \"\"");
        let ics = "BEGIN:VEVENT
UID:w
SUMMARY:Kontor
LOCATION:Kontorvej 1\\, 8000 Aarhus C
DTSTART;TZID=Europe/Copenhagen:20261005T080000
DTEND;TZID=Europe/Copenhagen:20261005T160000
RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;COUNT=7
END:VEVENT
BEGIN:VEVENT
UID:l
SUMMARY:Læge
DTSTART;TZID=Europe/Copenhagen:20261008T113000
DTEND;TZID=Europe/Copenhagen:20261008T123000
END:VEVENT
BEGIN:VEVENT
UID:f
SUMMARY:Frokost
LOCATION:Café Kanten\\, Åboulevarden 1\\, 8000 Aarhus C
DTSTART;TZID=Europe/Copenhagen:20261009T123000
DTEND;TZID=Europe/Copenhagen:20261009T130000
RRULE:FREQ=WEEKLY;COUNT=2
END:VEVENT
";
        let m = Month::parse("2026-10").unwrap();
        let occ = calendar::occurrences(ics, cfg.timezone, m.first_day(), m.last_day()).unwrap();
        let (home, office) = ("Hjem", "Kontorvej 1, 8000 Aarhus");
        let mut trips = Vec::new();
        for (day, arrive, leave) in [
            ("05", "07:52", "16:05"),
            ("06", "08:00", "11:30"),
            ("07", "08:00", "16:00"),
            ("08", "08:00", "16:00"),
            ("09", "08:00", "16:00"),
            ("12", "08:00", "16:00"),
            ("13", "08:00", "16:00"),
        ] {
            let at = |t: &str| format!("2026-10-{day} {t}");
            trips.push(located_trip(&cfg, &at("07:30"), &at(arrive), home, office));
            if day == "07" {
                // Lunch errand: away 11:45–12:30.
                trips.push(located_trip(
                    &cfg,
                    &at("11:45"),
                    &at("11:55"),
                    office,
                    "Bager",
                ));
                trips.push(located_trip(
                    &cfg,
                    &at("12:20"),
                    &at("12:30"),
                    "Bager",
                    office,
                ));
            }
            trips.push(located_trip(&cfg, &at(leave), &at("16:30"), office, home));
        }
        let at = |h, min| NaiveTime::from_hms_opt(h, min, 0).unwrap();
        let overrides = BTreeMap::from([
            (
                m.first_day() + Duration::days(11),
                LunchOverride {
                    start: at(11, 0),
                    minutes: 45,
                },
            ),
            (
                m.first_day() + Duration::days(12),
                LunchOverride {
                    start: at(12, 0),
                    minutes: 0,
                },
            ),
        ]);
        let state = State {
            lunch: overrides,
            ..State::default()
        };
        let r = build(&cfg, m, &trips, &occ, &state, NaiveDate::MAX);
        let day = |i: usize| (r.days[i].worked_hours * 60.0).round() as i64;

        // Normal day 07:52–16:05 (493 min): 30 min deducted.
        assert_eq!(day(4), 493 - 30);
        assert_eq!(r.days[4].lunch_text(), "12:00–12:30 (default)");
        // Work ends 11:30: no deduction.
        assert_eq!(day(5), 210);
        // Lunch errand gap 11:45–12:30 already covers lunch: nothing more.
        assert_eq!(r.days[6].periods_text(), "08:00–11:45, 12:30–16:00");
        assert_eq!(day(6), 435);
        // Doctor 11:30–12:30 covers lunch: deducted once.
        assert_eq!(day(7), 480 - 60);
        // Frokost event sets the window; not a work address, no planned hours.
        assert_eq!(day(8), 480 - 30);
        assert_eq!(r.days[8].lunch_text(), "12:30–13:00 (calendar)");
        assert_eq!(r.days[8].norm_hours, 7.5);
        assert_eq!(r.days[8].work_locations, vec!["Kontorvej 1, 8000 Aarhus C"]);
        // Override beats the event; 0 minutes is no lunch.
        assert_eq!(day(11), 480 - 45);
        assert_eq!(r.days[11].lunch_text(), "11:00–11:45 (override)");
        assert_eq!(day(12), 480);
        assert_eq!(r.days[12].lunch_text(), "none (override)");

        // The norm is the planned 08:00–16:00 minus the day's lunch, so a
        // normal day with lunch has no difference.
        assert_eq!(r.days[4].norm_hours, 7.5);
        assert_eq!(r.days[11].norm_hours, 8.0 - 0.75);
        assert_eq!(r.days[12].norm_hours, 8.0);
    }

    #[test]
    fn home_working_counts_as_worked_and_totals_run_to_today() {
        let mut cfg = cfg(r#"
            [calendar]
            source = ""
            [mileage]
            driver_name = "T"
            registration_number = "AB12345"
            home = "Hjemvej 3, 8200 Aarhus N"
            rates = []
            "#);
        let event = |uid: &str, summary: &str, location: &str, day: &str, from: &str, to: &str| {
            format!(
                "BEGIN:VEVENT\nUID:{uid}\nSUMMARY:{summary}\nLOCATION:{location}\n\
                 DTSTART;TZID=Europe/Copenhagen:202611{day}T{from}00\n\
                 DTEND;TZID=Europe/Copenhagen:202611{day}T{to}00\nEND:VEVENT\n"
            )
        };
        let ics = [
            event("h", "Hjemmearbejde", "", "02", "0800", "1600"),
            event("l", "Læge", "", "02", "1000", "1100"),
            event(
                "a",
                "Kunde",
                "Hjemvej 3\\, 8200 Aarhus N",
                "03",
                "0800",
                "1200",
            ),
            event(
                "w",
                "Hjemmeside møde",
                "Kontorvej 1\\, 8000 Aarhus C",
                "04",
                "0800",
                "1600",
            ),
            event("f", "hjemmearbejde", "", "05", "0800", "1600"),
        ]
        .concat();
        let m = Month::parse("2026-11").unwrap();
        let occ = calendar::occurrences(&ics, cfg.timezone, m.first_day(), m.last_day()).unwrap();
        let (home, office) = ("Hjemvej 3, 8200 Aarhus N", "Kontorvej 1, 8000 Aarhus");
        let trips = vec![
            located_trip(&cfg, "2026-11-02 17:00", "2026-11-02 17:10", home, "Netto"),
            located_trip(&cfg, "2026-11-04 07:40", "2026-11-04 08:00", home, office),
            located_trip(&cfg, "2026-11-04 13:00", "2026-11-04 13:20", office, home),
            // Saturday: private driving without a work event.
            located_trip(
                &cfg,
                "2026-11-07 10:00",
                "2026-11-07 10:20",
                home,
                "Bauhaus",
            ),
        ];
        let at = |s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        let mut state = State::default();
        state.home.intervals.push(PrivateInterval {
            id: 1,
            start: at("2026-11-04 12:30"),
            end: at("2026-11-04 15:00"),
            label: "Hjemmearbejde".into(),
            kind: Default::default(),
            replaces: None,
        });
        let today = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
        let r = build(&cfg, m, &trips, &occ, &state, today);
        let day = |n: usize| &r.days[n - 1];

        // The planned window minus Læge and lunch; the trip is no reason to fix.
        assert_eq!(day(2).periods_text(), "08:00–10:00, 11:00–16:00");
        assert_eq!(day(2).worked_hours, 6.5);
        assert_eq!(day(2).norm_hours, 7.5);
        assert_eq!(day(2).check, None);
        assert_eq!(
            day(2).remark(),
            "Hjemmearbejde 08:00–16:00 · home; Læge 10:00–11:00 · private"
        );
        // An event at the home address is home working too.
        assert_eq!(day(3).home.len(), 1);
        assert_eq!(day(3).worked_hours, 4.0);
        // "Hjemmeside" is not the word "Hjemme": office, then the timer;
        // 12:30–13:00 at both counts once.
        assert_eq!(day(4).periods_text(), "08:00–15:00");
        assert_eq!(day(4).worked_hours, 6.5);
        // After today: planned norm only.
        assert_eq!((day(5).norm_hours, day(5).worked_hours), (7.5, 0.0));
        assert!(day(5).home.is_empty());
        // No work event at the weekend: nothing to fix.
        assert_eq!(
            (day(7).note.as_deref(), day(7).check.as_deref()),
            (None, None)
        );

        assert_eq!(r.norm_total(), 7.5 + 4.0 + 7.5);
        assert_eq!(r.worked_total(), 6.5 + 4.0 + 6.5);
        assert_eq!(r.month_norm_total(), 7.5 + 4.0 + 7.5 + 7.5);
        // A past month counts every day, a coming one none.
        let after = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
        assert_eq!(
            build(&cfg, m, &trips, &occ, &state, after).norm_total(),
            26.5
        );
        let before = NaiveDate::from_ymd_opt(2026, 10, 31).unwrap();
        let r = build(&cfg, m, &trips, &occ, &state, before);
        assert_eq!((r.norm_total(), r.worked_total()), (0.0, 0.0));

        // Switched off: the event at home is a work address again.
        cfg.home_working.at_home_address = false;
        cfg.home_working.keywords.clear();
        let r = build(&cfg, m, &trips, &occ, &state, today);
        assert!(r.days[2].home.is_empty() && r.days[1].home.is_empty());
        assert_eq!(r.days[2].worked_hours, 0.0);
    }

    #[test]
    fn calendar_name() {
        let ics = "BEGIN:VCALENDAR\nX-WR-CALNAME:Arbejdstid\nEND:VCALENDAR\n";
        assert_eq!(calendar::name(ics).as_deref(), Some("Arbejdstid"));
    }
}
