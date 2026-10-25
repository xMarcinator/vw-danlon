//! Builds the per-day picture for one month: norm hours from the work
//! calendar, work periods from trips, and the difference between them.
//!
//! Working time is the time spent at a work address: from arriving at the
//! address of that day's calendar entry until leaving it again.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone};
use chrono_tz::Tz;

use crate::calendar::Occurrence;
use crate::config::Config;
use crate::trips::Trip;
use crate::{address, holidays};

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
    /// Why this is not a working day (holiday name or calendar entry).
    pub off_reason: Option<String>,
    /// Work addresses for the day (calendar locations or the default).
    pub work_locations: Vec<String>,
    /// Periods at a work address.
    pub segments: Vec<Span>,
    /// Start of the first and end of the last period at work.
    pub first_start: Option<DateTime<Tz>>,
    pub last_end: Option<DateTime<Tz>>,
    /// Anything that needs checking (e.g. arrival without departure).
    pub note: Option<String>,
    pub break_minutes: u32,
    pub worked_hours: f64,
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

    /// Day-off reason and anything that needs checking.
    pub fn remark(&self) -> String {
        [self.off_reason.as_deref(), self.note.as_deref()]
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
}

impl MonthReport {
    pub fn norm_total(&self) -> f64 {
        self.days
            .iter()
            .map(|d| d.norm_hours)
            .fold(0.0, |a, b| a + b)
    }
    pub fn worked_total(&self) -> f64 {
        self.days
            .iter()
            .map(|d| d.worked_hours)
            .fold(0.0, |a, b| a + b)
    }
}

pub fn build(cfg: &Config, month: Month, trips: &[Trip], calendar: &[Occurrence]) -> MonthReport {
    let tz = cfg.timezone;

    let mut off: HashMap<NaiveDate, String> = holidays::days_off(month.year, &cfg.holidays)
        .into_iter()
        .map(|(d, name)| (d, name.to_string()))
        .collect();
    let keywords: Vec<String> = cfg
        .calendar
        .as_ref()
        .map(|c| {
            c.days_off_keywords
                .iter()
                .map(|k| k.to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let norm_from_calendar = cfg.calendar.as_ref().is_some_and(|c| c.norm_from_calendar);
    let default_location = cfg
        .calendar
        .as_ref()
        .and_then(|c| c.default_location.clone());
    let match_addresses = cfg.calendar.is_some();

    // Planned working time per date, from timed calendar events.
    let mut planned: BTreeMap<NaiveDate, Vec<Span>> = BTreeMap::new();
    let mut locations: BTreeMap<NaiveDate, Vec<String>> = BTreeMap::new();
    let mut missing_location: BTreeMap<NaiveDate, String> = BTreeMap::new();
    for occ in calendar {
        match occ {
            Occurrence::AllDay { date, summary } => {
                let lower = summary.to_lowercase();
                if keywords.is_empty() || keywords.iter().any(|k| lower.contains(k)) {
                    off.entry(*date).or_insert_with(|| summary.clone());
                }
            }
            Occurrence::Timed {
                start,
                end,
                summary,
                location,
            } => {
                let day_locations = locations.entry(start.date_naive()).or_default();
                if location.trim().is_empty() {
                    missing_location.insert(start.date_naive(), summary.clone());
                } else if !day_locations.contains(location) {
                    day_locations.push(location.clone());
                }
                if !norm_from_calendar {
                    continue;
                }
                // Split events crossing midnight onto each date they cover.
                let mut s = *start;
                while s < *end {
                    let next_midnight = tz
                        .from_local_datetime(
                            &(s.date_naive() + Duration::days(1))
                                .and_hms_opt(0, 0, 0)
                                .unwrap(),
                        )
                        .earliest()
                        .unwrap_or(*end);
                    let e = (*end).min(next_midnight);
                    planned.entry(s.date_naive()).or_default().push((s, e));
                    s = e;
                }
            }
        }
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
            let off_reason = off.get(&date).cloned();
            let day_plan = planned.get(&date).map(Vec::as_slice).unwrap_or(&[]);
            let planned_hours: f64 = day_plan
                .iter()
                .map(|(s, e)| (*e - *s).num_minutes() as f64 / 60.0)
                .fold(0.0, |a, b| a + b);
            let norm_hours = match (&off_reason, norm_from_calendar) {
                (Some(_), _) => 0.0,
                (None, true) => planned_hours,
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

            let (segments, mut note) = if !match_addresses {
                // No calendar: first departure to last arrival.
                let span = day_trips
                    .iter()
                    .map(|t| t.start)
                    .min()
                    .zip(day_trips.iter().map(|t| t.end).max());
                (span.into_iter().collect(), None)
            } else if match_locations.is_empty() {
                let note = (!day_trips.is_empty())
                    .then(|| "kørsel, men ingen arbejdsadresse i kalenderen".to_string());
                (Vec::new(), note)
            } else {
                work_segments(day_trips, &match_locations, cfg.periods.count_travel_time)
            };
            if note.is_none()
                && let Some(summary) = missing_location.get(&date)
            {
                note = Some(format!("kalenderpost uden adresse: {summary}"));
            }

            let minutes: i64 = segments.iter().map(|(s, e)| (*e - *s).num_minutes()).sum();
            let (worked_hours, break_minutes) = worked(minutes, cfg);
            Day {
                date,
                norm_hours,
                planned,
                off_reason,
                work_locations,
                first_start: segments.first().map(|s| s.0),
                last_end: segments.last().map(|s| s.1),
                segments,
                note,
                break_minutes,
                worked_hours,
            }
        })
        .collect();

    MonthReport { month, days }
}

/// Periods at a work address: from a trip arriving at one of `locations`
/// until a trip departing from that address. A drive from one work address
/// to another is working time; with `count_travel` the drives to and from
/// work are included too.
fn work_segments(
    trips: &[&Trip],
    locations: &[String],
    count_travel: bool,
) -> (Vec<Span>, Option<String>) {
    let mut segments: Vec<Span> = Vec::new();
    let mut at: Option<(DateTime<Tz>, &str)> = None;
    for t in trips {
        let mut left_work = false;
        if let Some((since, loc)) = at
            && address::matches(&t.from, loc)
        {
            let until = if count_travel { t.end } else { t.start };
            segments.push((since, until));
            at = None;
            left_work = true;
        }
        if at.is_none()
            && let Some(loc) = locations.iter().find(|l| address::matches(&t.to, l))
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
        None if segments.is_empty() && !trips.is_empty() => {
            Some("ingen kørsel til eller fra arbejdsadressen".to_string())
        }
        None => None,
    };

    // Merge overlaps (travel between two work addresses with count_travel).
    segments.sort();
    let mut merged: Vec<Span> = Vec::new();
    for (s, e) in segments {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    (merged, note)
}

/// Worked minutes minus break, rounded. Returns hours and the break deducted.
fn worked(total_minutes: i64, cfg: &Config) -> (f64, u32) {
    if total_minutes <= 0 {
        return (0.0, 0);
    }
    let brk = i64::from(cfg.periods.unpaid_break_minutes).min(total_minutes);
    let mut minutes = total_minutes - brk;
    if minutes <= 0 {
        return (0.0, brk as u32);
    }
    let step = i64::from(cfg.periods.round_to_minutes);
    if step > 0 {
        minutes = (minutes + step / 2) / step * step;
    }
    (minutes as f64 / 60.0, brk as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar;
    use crate::trips::parse_time;

    fn cfg(extra: &str) -> Config {
        toml::from_str(&format!(
            r#"
            employee_number = "1"
            [periods]
            unpaid_break_minutes = 30
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
        }
    }

    #[test]
    fn norm_hours_october_2026() {
        let cfg = cfg("");
        let r = build(&cfg, Month::parse("2026-10").unwrap(), &[], &[]);
        // 22 weekdays, no holidays, 7.4 h each.
        assert!((r.norm_total() - 22.0 * 7.4).abs() < 1e-9);
    }

    #[test]
    fn holidays_reduce_norm() {
        let cfg = cfg("");
        // December 2026: 23 weekdays, minus 24th, 25th, 31st (26th is a Saturday).
        let r = build(&cfg, Month::parse("2026-12").unwrap(), &[], &[]);
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
        let r = build(&cfg, Month::parse("2026-10").unwrap(), &trips, &[]);
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
        let r = build(&cfg, m, &[], &occ);
        // Mon–Thu 8 h from 5 Oct: weeks of 5, 12, 26 Oct = 12 days; 19–22 Oct are ferie.
        assert_eq!(r.norm_total(), 12.0 * 8.0);
        assert_eq!(r.days[18].off_reason.as_deref(), Some("Efterårsferie"));
        // "Tandlæge" does not match the keywords, so the 26th stays a workday.
        assert_eq!(r.days[25].norm_hours, 8.0);
        // Fridays have no planned time.
        assert_eq!(r.days[8].norm_hours, 0.0);
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
        cfg.periods.unpaid_break_minutes = 0;
        cfg.calendar = Some(crate::config::CalendarConfig {
            source: String::new(),
            norm_from_calendar: true,
            days_off_keywords: vec![],
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
            // Thursday: arrives but no departure recorded.
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
        let r = build(&cfg, m, &trips, &occ);

        let tue = &r.days[5];
        assert_eq!(tue.periods_text(), "08:40–11:30, 12:05–14:30");
        assert_eq!(tue.worked_hours, (170.0 + 145.0) / 60.0);
        assert_eq!(tue.norm_hours, 7.5);

        let wed = &r.days[6];
        assert_eq!(wed.periods_text(), "08:00–16:00");

        let thu = &r.days[7];
        assert_eq!(thu.worked_hours, 0.0);
        assert!(
            thu.note
                .as_deref()
                .unwrap()
                .contains("uden registreret afgang")
        );

        let sat = &r.days[9];
        assert_eq!(sat.worked_hours, 0.0);

        // With travel time, Tuesday runs from leaving home to arriving home.
        cfg.periods.count_travel_time = true;
        let r = build(&cfg, m, &trips, &occ);
        assert_eq!(r.days[5].periods_text(), "07:30–11:40, 11:55–15:40");
    }

    #[test]
    fn workplace_counts_on_calendar_days_elsewhere() {
        let mut cfg = cfg("");
        cfg.periods.unpaid_break_minutes = 0;
        cfg.workplace = Some("Kontorvej 1, 8000 Aarhus C".into());
        cfg.calendar = Some(crate::config::CalendarConfig {
            source: String::new(),
            norm_from_calendar: true,
            days_off_keywords: vec![],
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
        let r = build(&cfg, m, &trips, &occ);
        assert_eq!(r.days[5].periods_text(), "08:00–15:00");
        assert_eq!(r.days[6].periods_text(), "08:00–16:30");
        assert_eq!(r.days[6].note, None);
    }

    #[test]
    fn calendar_name() {
        let ics = "BEGIN:VCALENDAR\nX-WR-CALNAME:Arbejdstid\nEND:VCALENDAR\n";
        assert_eq!(calendar::name(ics).as_deref(), Some("Arbejdstid"));
    }
}
