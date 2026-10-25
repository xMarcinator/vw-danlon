//! What to type into the Danløn app, day by day: Komme/gå registrations from
//! the day's work periods and one Kørsel registration from its business
//! trips. Neither form's Description is filled in.
//!
//! Private time splits the day into separate Komme/gå entries and is never
//! registered; a gap overlapping the lunch window (an errand) is bridged and
//! becomes the entry's break, like the lunch deduction itself. Any other
//! unworked gap splits the day too.

use std::collections::BTreeSet;
use std::fmt::Write;

use chrono::{Datelike, NaiveDate, Weekday};

use crate::address::{self, Places};
use crate::config::Config;
use crate::mileage::{Entry, MileageMonth};
use crate::month::{Day, EntryKind, MonthReport, Span, TimedEntry};
use crate::timesheet::weekday_da;
use crate::trips::{Position, Trip};

#[derive(Debug, Clone)]
pub struct ChecklistDay {
    pub date: NaiveDate,
    pub status: Status,
    pub komme_gaa: Vec<KommeGaa>,
    pub korsel: Option<Korsel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    ToEnter,
    /// Weekend, holiday, vacation or no work: the reason.
    NothingToEnter(String),
    /// The day's trips need fixing first: the report's remark.
    FixFirst(String),
}

impl ChecklistDay {
    /// Why the day cannot be ticked as entered, if it cannot.
    pub fn tick_refusal(&self) -> Option<String> {
        match &self.status {
            Status::ToEnter => None,
            Status::NothingToEnter(reason) => Some(format!("nothing to enter ({reason})")),
            Status::FixFirst(reason) => Some(format!("fix first: {reason}")),
        }
    }
}

/// The checklist as text: each day with its status and, on days to enter,
/// its registrations; then the progress. `entered` are the ticked days.
pub fn render(days: &[ChecklistDay], entered: &BTreeSet<NaiveDate>) -> String {
    let mut out = String::new();
    for day in days {
        let ticked = entered.contains(&day.date);
        let status = match (&day.status, ticked) {
            (Status::ToEnter, false) => "[ ] to enter".to_string(),
            (Status::ToEnter, true) => "[x] entered".to_string(),
            _ => day.tick_refusal().unwrap_or_default(),
        };
        let _ = writeln!(
            out,
            "{}  {}  {status}",
            day.date,
            weekday_da(day.date.weekday())
        );
        if day.status != Status::ToEnter {
            continue;
        }
        for k in &day.komme_gaa {
            let _ = writeln!(
                out,
                "    Komme/gå  {} · break {} · {}",
                k.time_text(),
                k.break_text(),
                k.total_text()
            );
        }
        if let Some(k) = &day.korsel {
            let _ = writeln!(
                out,
                "    Kørsel    Purpose: {} · {} · Add return trip: {} · {} km",
                k.purpose,
                k.route_text(),
                if k.return_trip { "on" } else { "off" },
                k.km_text()
            );
            for (from, to) in &k.gaps {
                let _ = writeln!(out, "              check: no trip from {from} to {to}");
            }
        }
    }
    let p = progress(days, entered);
    let _ = write!(
        out,
        "\nDays entered: {} of {} · Trips entered: {} of {}",
        p.days_done, p.days_total, p.trips_done, p.trips_total
    );
    if p.fix_first > 0 {
        let _ = write!(out, " · {} to fix first", p.fix_first);
    }
    out.push('\n');
    out
}

/// How far the month is entered: days to enter and their Kørsel
/// registrations, ticked or not, and the days to fix first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub days_done: usize,
    pub days_total: usize,
    pub trips_done: usize,
    pub trips_total: usize,
    pub fix_first: usize,
}

pub fn progress(days: &[ChecklistDay], entered: &BTreeSet<NaiveDate>) -> Progress {
    let mut p = Progress::default();
    for day in days {
        let ticked = usize::from(entered.contains(&day.date));
        match day.status {
            Status::ToEnter => {
                p.days_total += 1;
                p.days_done += ticked;
                if day.korsel.is_some() {
                    p.trips_total += 1;
                    p.trips_done += ticked;
                }
            }
            Status::FixFirst(_) => p.fix_first += 1,
            Status::NothingToEnter(_) => {}
        }
    }
    p
}

/// One Komme/gå registration: Time = start to end, Break = `break_minutes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KommeGaa {
    pub time: Span,
    pub break_minutes: i64,
}

impl KommeGaa {
    /// Worked minutes: the span minus the break.
    pub fn minutes(&self) -> i64 {
        (self.time.1 - self.time.0).num_minutes() - self.break_minutes
    }

    /// E.g. "07:58 to 10:00".
    pub fn time_text(&self) -> String {
        format!(
            "{} to {}",
            self.time.0.format("%H:%M"),
            self.time.1.format("%H:%M")
        )
    }

    /// E.g. "30m", or "none".
    pub fn break_text(&self) -> String {
        match self.break_minutes {
            0 => "none".into(),
            m if m < 60 => format!("{m}m"),
            m => duration_text(m),
        }
    }

    /// E.g. "4h 15m".
    pub fn total_text(&self) -> String {
        duration_text(self.minutes())
    }
}

fn duration_text(minutes: i64) -> String {
    format!("{}h {:02}m", minutes / 60, minutes % 60)
}

/// One Kørsel registration for a day's business trips.
#[derive(Debug, Clone, PartialEq)]
pub struct Korsel {
    /// The visited calendar entries' names (the trips' purposes).
    pub purpose: String,
    /// Ordered stops by name; only the way out when `return_trip` is on.
    pub route: Vec<String>,
    /// "Add return trip": the business driving ends where it started.
    pub return_trip: bool,
    /// The day's business km, for checking against the app's route.
    pub km: f64,
    /// Where one trip ends and the next starts elsewhere (from, to).
    pub gaps: Vec<(String, String)>,
    /// Each stop's name and address, in route order, once each.
    pub addresses: Vec<(String, String)>,
}

impl Korsel {
    /// E.g. "Hjem → Kunde A".
    pub fn route_text(&self) -> String {
        self.route.join(" → ")
    }

    /// E.g. "148,0".
    pub fn km_text(&self) -> String {
        format!("{:.1}", self.km).replace('.', ",")
    }
}

/// The month's registrations. `trips` are those the mileage was computed
/// from (for their coordinates); `calendar` names the visited addresses.
pub fn build(
    cfg: &Config,
    report: &MonthReport,
    trips: &[Trip],
    mileage: Option<&MileageMonth>,
    calendar: &[TimedEntry],
) -> Vec<ChecklistDay> {
    let names = Names {
        cfg,
        places: &cfg.known_places,
        calendar,
    };
    let entries: &[Entry] = mileage.map(|m| m.entries.as_slice()).unwrap_or_default();
    report
        .days
        .iter()
        .map(|day| {
            let day_entries: Vec<&Entry> = entries
                .iter()
                .filter(|e| e.start.date_naive() == day.date)
                .collect();
            let korsel = korsel(day.date, &day_entries, trips, &names);
            ChecklistDay {
                date: day.date,
                status: status(day, korsel.is_some()),
                komme_gaa: komme_gaa(day),
                korsel,
            }
        })
        .collect()
}

fn status(day: &Day, driving: bool) -> Status {
    let weekend = matches!(day.date.weekday(), Weekday::Sat | Weekday::Sun);
    let working = !day.segments.is_empty()
        || driving
        || day.norm_hours > 0.0
        || (day.off_reason.is_none() && !weekend);
    let nothing = || {
        let reason = match &day.off_reason {
            Some(reason) => reason.to_string(),
            None if weekend => "weekend".into(),
            None => "ingen arbejdstid".into(),
        };
        Status::NothingToEnter(reason)
    };
    if !working {
        return nothing();
    }
    // Private time and home working are in the remark too, but are no
    // reason to fix anything.
    let flags: Vec<&str> = day
        .check
        .as_deref()
        .unwrap_or_default()
        .split("; ")
        .filter(|part| !part.is_empty())
        .collect();
    if !flags.is_empty() {
        Status::FixFirst(flags.join("; "))
    } else if day.segments.is_empty() && !driving {
        nothing()
    } else {
        Status::ToEnter
    }
}

/// The day's periods at work, joined across gaps that overlap the lunch
/// window unless private time falls in the gap.
fn komme_gaa(day: &Day) -> Vec<KommeGaa> {
    let bridges = |gap: Span| {
        day.lunch.window.is_some_and(|l| overlaps(gap, l))
            && !day.appointments.iter().any(|p| overlaps(gap, *p))
    };
    let mut stretches: Vec<Span> = Vec::new();
    for &(start, end) in &day.segments {
        match stretches.last_mut() {
            Some(last) if bridges((last.1, start)) => last.1 = end,
            _ => stretches.push((start, end)),
        }
    }
    stretches
        .into_iter()
        .map(|time| {
            let worked: i64 = day
                .segments
                .iter()
                .filter(|s| time.0 <= s.0 && s.1 <= time.1)
                .map(|&s| minutes(s) - day.lunch.window.map_or(0, |l| overlap_minutes(s, l)))
                .sum();
            KommeGaa {
                time,
                break_minutes: minutes(time) - worked,
            }
        })
        .collect()
}

fn minutes((start, end): Span) -> i64 {
    (end - start).num_minutes()
}

fn overlaps(a: Span, b: Span) -> bool {
    a.0 < b.1 && b.0 < a.1
}

fn overlap_minutes(a: Span, b: Span) -> i64 {
    (a.1.min(b.1) - a.0.max(b.0)).num_minutes().max(0)
}

/// Stop names as used in the Danløn app.
struct Names<'a> {
    cfg: &'a Config,
    places: &'a Places,
    calendar: &'a [TimedEntry<'a>],
}

impl Names<'_> {
    /// The configured name of home or the workplace, else the title of the
    /// day's work entry at that address, else the address itself; with the
    /// configured or calendar address of the place.
    fn name(&self, date: NaiveDate, address: &str, pos: Option<Position>) -> (String, String) {
        let at = |place: &str| self.places.at(address, pos, place);
        let home = self.cfg.mileage.as_ref().and_then(|m| m.home.as_deref());
        if let Some(home) = home.filter(|h| at(h)) {
            return (self.cfg.checklist.home_name.clone(), home.to_string());
        }
        if let Some(work) = self.cfg.workplace.as_deref().filter(|w| at(w)) {
            return (self.cfg.checklist.workplace_name.clone(), work.to_string());
        }
        self.calendar
            .iter()
            .find(|e| {
                e.kind == EntryKind::Work
                    && e.start.date_naive() == date
                    && !e.location.trim().is_empty()
                    && at(e.location)
            })
            .map_or_else(
                || (address.to_string(), address.to_string()),
                |e| (e.summary.to_string(), e.location.to_string()),
            )
    }
}

/// One registration for the day's reimbursed trips; a trip split across two
/// rates counts once.
fn korsel(date: NaiveDate, entries: &[&Entry], trips: &[Trip], names: &Names) -> Option<Korsel> {
    let mut legs: Vec<(&Entry, f64)> = Vec::new();
    for e in entries {
        match legs.last_mut() {
            Some((prev, km)) if prev.start == e.start && prev.from == e.from && prev.to == e.to => {
                *km += e.km;
            }
            _ => legs.push((e, e.km)),
        }
    }
    if legs.is_empty() {
        return None;
    }

    let mut purposes: Vec<&str> = Vec::new();
    let mut route: Vec<String> = Vec::new();
    let mut gaps = Vec::new();
    let mut addresses: Vec<(String, String)> = Vec::new();
    let mut prev: Option<(&Entry, String)> = None;
    for (e, _) in &legs {
        let trip = trips
            .iter()
            .find(|t| t.start == e.start && t.from == e.from && t.to == e.to);
        let from_stop = names.name(date, &e.from, trip.and_then(|t| t.start_pos));
        let to_stop = names.name(date, &e.to, trip.and_then(|t| t.end_pos));
        let (from, to) = (from_stop.0.clone(), to_stop.0.clone());
        if let Some((p, p_to)) = &prev
            && *p_to != from
            && !address::matches(&p.to, &e.from)
        {
            gaps.push((p_to.clone(), from.clone()));
        }
        for stop in [from_stop, to_stop] {
            if route.last() != Some(&stop.0) {
                route.push(stop.0.clone());
            }
            if !addresses.iter().any(|(name, _)| *name == stop.0) {
                addresses.push(stop);
            }
        }
        if !e.purpose.is_empty() && !purposes.contains(&e.purpose.as_str()) {
            purposes.push(&e.purpose);
        }
        prev = Some((e, to));
    }

    let return_trip = route.len() > 2 && route.first() == route.last();
    if return_trip {
        route.pop();
    }
    Some(Korsel {
        purpose: purposes.join(", "),
        route,
        return_trip,
        km: legs.iter().map(|(_, km)| km).sum(),
        gaps,
        addresses,
    })
}

#[cfg(test)]
mod tests {
    use crate::state::State;

    use super::*;
    use crate::calendar;
    use crate::mileage;
    use crate::month::{self, Month, timed_entries};
    use crate::trips::parse_time;

    const BASE: &str = r#"
employee_number = "1"
workplace = "Kontorvej 1, 8000 Aarhus C"
[calendar]
source = ""
[mileage]
driver_name = "Test"
registration_number = "AB12345"
home = "Hjemvej 3, 8200 Aarhus N"
[[mileage.rates]]
year = 2026
low = 3.94
high = 2.28
threshold_km = 100
[danlon]
"#;

    const HOME: &str = "Hjemvej 3, 8200 Aarhus N";
    const OFFICE: &str = "Kontorvej 1, 8000 Aarhus";
    const KUNDE_A: &str = "Vestergade 12, 5000 Odense";
    const KUNDE_C: &str = "Industrivej 9, 8000 Aarhus C";

    fn trip(day: &str, start: &str, end: &str, from: &str, to: &str, km: f64) -> Trip {
        let tz = chrono_tz::Europe::Copenhagen;
        let at = |t: &str| parse_time(&format!("2026-11-{day} {t}"), tz).unwrap();
        Trip {
            start: at(start),
            end: at(end),
            distance_km: Some(km),
            from: from.into(),
            to: to.into(),
            purpose: String::new(),
            business: None,
            start_pos: None,
            end_pos: None,
        }
    }

    /// Kontor every weekday at the workplace, plus the given entries.
    fn checklist(cfg: &Config, ics: &str, trips: &[Trip]) -> (MonthReport, Vec<ChecklistDay>) {
        checklist_with(cfg, ics, trips, &State::default())
    }

    /// [`checklist`] with the recorded time in `state`.
    fn checklist_with(
        cfg: &Config,
        ics: &str,
        trips: &[Trip],
        state: &State,
    ) -> (MonthReport, Vec<ChecklistDay>) {
        let ics = format!(
            "BEGIN:VEVENT
UID:kontor
SUMMARY:Kontor
LOCATION:Kontorvej 1\\, 8000 Aarhus C
DTSTART;TZID=Europe/Copenhagen:20261102T080000
DTEND;TZID=Europe/Copenhagen:20261102T160000
RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR
END:VEVENT
{ics}"
        );
        let m = Month::parse("2026-11").unwrap();
        let occ = calendar::occurrences(&ics, cfg.timezone, m.first_day(), m.last_day()).unwrap();
        let private = &cfg.calendar.as_ref().unwrap().private_keywords;
        let cal = timed_entries(&occ, private, &cfg.lunch.title);
        let report = month::build(cfg, m, trips, &occ, state, NaiveDate::MAX);
        let mileage = mileage::compute(
            cfg.mileage.as_ref().unwrap(),
            cfg.workplace.as_deref(),
            &cfg.known_places,
            m,
            trips,
            &cal,
        )
        .unwrap();
        let days = build(cfg, &report, trips, Some(&mileage), &cal);
        (report, days)
    }

    fn entry(summary: &str, location: &str, day: &str) -> String {
        format!(
            "BEGIN:VEVENT
UID:{summary}{day}
SUMMARY:{summary}
LOCATION:{location}
DTSTART;TZID=Europe/Copenhagen:202611{day}T090000
DTEND;TZID=Europe/Copenhagen:202611{day}T150000
END:VEVENT
"
        )
    }

    fn texts(day: &ChecklistDay) -> Vec<(String, String, String)> {
        day.komme_gaa
            .iter()
            .map(|k| (k.time_text(), k.break_text(), k.total_text()))
            .collect()
    }

    fn texts_of(entries: &[(&str, &str, &str)]) -> Vec<(String, String, String)> {
        entries
            .iter()
            .map(|(a, b, c)| (a.to_string(), b.to_string(), c.to_string()))
            .collect()
    }

    fn november() -> (MonthReport, Vec<ChecklistDay>) {
        let cfg = Config::parse(BASE).unwrap();
        let ics = [
            "BEGIN:VEVENT
UID:laege
SUMMARY:Læge
DTSTART;TZID=Europe/Copenhagen:20261119T100000
DTEND;TZID=Europe/Copenhagen:20261119T111500
END:VEVENT
BEGIN:VEVENT
UID:ferie
SUMMARY:Ferie
DTSTART;VALUE=DATE:20261126
END:VEVENT
"
            .to_string(),
            entry("Kunde A", "Vestergade 12\\, 5000 Odense C", "18"),
            entry("Kunde C", "Industrivej 9\\, 8000 Aarhus C", "20"),
            entry("Kunde A", "Vestergade 12\\, 5000 Odense C", "23"),
            entry("Kunde C", "Industrivej 9\\, 8000 Aarhus C", "23"),
            entry("Kunde A", "Vestergade 12\\, 5000 Odense C", "27"),
            entry("Kunde C", "Industrivej 9\\, 8000 Aarhus C", "27"),
        ]
        .concat();
        let trips = vec![
            // Lunch errand 11:50–12:35.
            trip("04", "07:20", "07:41", HOME, OFFICE, 20.0),
            trip("04", "11:50", "12:00", OFFICE, "Bager", 1.0),
            trip("04", "12:25", "12:35", "Bager", OFFICE, 1.0),
            trip("04", "15:55", "16:15", OFFICE, HOME, 20.0),
            // Customer visit, the second trip split across the two rates.
            trip("18", "07:30", "08:30", HOME, KUNDE_A, 74.0),
            trip("18", "15:30", "16:30", KUNDE_A, HOME, 74.0),
            // Læge 10:00–11:15 from the calendar.
            trip("19", "07:40", "07:58", HOME, OFFICE, 20.0),
            trip("19", "16:00", "16:20", OFFICE, HOME, 20.0),
            // Office → customer → office.
            trip("20", "07:40", "08:00", HOME, OFFICE, 20.0),
            trip("20", "09:00", "09:30", OFFICE, KUNDE_C, 30.0),
            trip("20", "14:00", "14:30", KUNDE_C, OFFICE, 30.0),
            trip("20", "16:00", "16:20", OFFICE, HOME, 20.0),
            // Not returning: home → A → C → office.
            trip("23", "07:00", "08:00", HOME, KUNDE_A, 70.0),
            trip("23", "12:40", "13:30", KUNDE_A, KUNDE_C, 60.0),
            trip("23", "15:00", "15:15", KUNDE_C, OFFICE, 5.0),
            trip("23", "16:00", "16:20", OFFICE, HOME, 20.0),
            // A private errand 09:00–10:00, outside the lunch window.
            trip("24", "07:40", "08:00", HOME, OFFICE, 20.0),
            trip("24", "09:00", "09:10", OFFICE, "Bauhaus", 3.0),
            trip("24", "09:50", "10:00", "Bauhaus", OFFICE, 3.0),
            trip("24", "16:00", "16:20", OFFICE, HOME, 20.0),
            // Arrival without departure.
            trip("25", "07:40", "08:00", HOME, OFFICE, 20.0),
            // The drive from A to C is missing.
            trip("27", "07:00", "08:00", HOME, KUNDE_A, 70.0),
            trip("27", "14:00", "15:00", KUNDE_C, OFFICE, 5.0),
            trip("27", "16:00", "16:20", OFFICE, HOME, 20.0),
        ];
        checklist(&cfg, &ics, &trips)
    }

    fn day(days: &[ChecklistDay], d: u32) -> &ChecklistDay {
        days.iter().find(|c| c.date.day() == d).unwrap()
    }

    #[test]
    fn home_working_is_entered_and_weekend_driving_is_nothing() {
        let cfg = Config::parse(BASE).unwrap();
        let mut state = State::default();
        let at = |s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        // Saturday morning at home with the timer.
        state.home.intervals.push(crate::state::PrivateInterval {
            id: 1,
            start: at("2026-11-07 09:00"),
            end: at("2026-11-07 11:00"),
            label: "Hjemmearbejde".into(),
            replaces: None,
        });
        let trips = [
            trip("07", "13:00", "13:20", HOME, "Bauhaus", 12.0),
            trip("08", "10:00", "10:20", HOME, "Bauhaus", 12.0),
        ];
        let (_, days) = checklist_with(&cfg, &entry("Hjemmearbejde", "", "09"), &trips, &state);
        let day = |n: usize| &days[n - 1];
        assert_eq!(day(7).status, Status::ToEnter);
        assert_eq!(
            texts(day(7)),
            texts_of(&[("09:00 to 11:00", "none", "2h 00m")])
        );
        assert_eq!(day(8).status, Status::NothingToEnter("weekend".into()));
        // Monday at home all day, no driving: the planned 09:00–15:00.
        assert_eq!(day(9).status, Status::ToEnter);
        assert_eq!(
            texts(day(9)),
            texts_of(&[("09:00 to 15:00", "30m", "5h 30m")])
        );
    }

    #[test]
    fn private_time_splits_the_day() {
        let (_, days) = november();
        let nov19 = day(&days, 19);
        assert_eq!(nov19.status, Status::ToEnter);
        assert_eq!(
            texts(nov19),
            texts_of(&[
                ("07:58 to 10:00", "none", "2h 02m"),
                ("11:15 to 16:00", "30m", "4h 15m"),
            ])
        );
    }

    #[test]
    fn lunch_errand_is_the_break() {
        let (_, days) = november();
        assert_eq!(
            texts(day(&days, 4)),
            texts_of(&[("07:41 to 15:55", "45m", "7h 29m")])
        );
    }

    #[test]
    fn unworked_gap_outside_lunch_splits_the_day() {
        let (_, days) = november();
        assert_eq!(
            texts(day(&days, 24)),
            texts_of(&[
                ("08:00 to 09:00", "none", "1h 00m"),
                ("10:00 to 16:00", "30m", "5h 30m"),
            ])
        );
    }

    #[test]
    fn entries_add_up_to_worked_hours() {
        let (report, days) = november();
        for (d, c) in report.days.iter().zip(&days) {
            let entered: i64 = c.komme_gaa.iter().map(KommeGaa::minutes).sum();
            assert_eq!(
                entered,
                (d.worked_hours * 60.0).round() as i64,
                "{}",
                d.date
            );
        }
    }

    #[test]
    fn statuses() {
        let (_, days) = november();
        let nothing = |reason: &str| Status::NothingToEnter(reason.into());
        assert_eq!(day(&days, 21).status, nothing("weekend"));
        assert_eq!(day(&days, 26).status, nothing("Ferie"));
        // A workday without trips.
        assert_eq!(day(&days, 3).status, nothing("ingen arbejdstid"));
        match &day(&days, 25).status {
            Status::FixFirst(reason) => assert!(reason.contains("uden registreret afgang")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn return_trip_to_the_start() {
        let (_, days) = november();
        let korsel = |d| day(&days, d).korsel.clone().unwrap();
        let nov18 = korsel(18);
        assert_eq!(nov18.purpose, "Kunde A");
        assert_eq!(nov18.route_text(), "Hjem → Kunde A");
        assert!(nov18.return_trip);
        assert_eq!(nov18.km_text(), "148,0");
        assert!(nov18.gaps.is_empty());
        // The addresses to copy: the configured home, the calendar entry's.
        assert_eq!(
            nov18.addresses,
            vec![
                ("Hjem".to_string(), HOME.to_string()),
                (
                    "Kunde A".to_string(),
                    "Vestergade 12, 5000 Odense C".to_string()
                ),
            ]
        );

        let nov20 = korsel(20);
        assert_eq!(nov20.route_text(), "Kontoret → Kunde C");
        assert!(nov20.return_trip);
        assert_eq!(nov20.km_text(), "60,0");

        let nov23 = korsel(23);
        assert_eq!(nov23.purpose, "Kunde A, Kunde C");
        assert_eq!(nov23.route_text(), "Hjem → Kunde A → Kunde C → Kontoret");
        assert!(!nov23.return_trip);
        assert_eq!(nov23.km_text(), "135,0");

        // Commuting and errands are no Kørsel.
        assert_eq!(day(&days, 4).korsel, None);
        assert_eq!(day(&days, 19).korsel, None);
    }

    #[test]
    fn trips_that_do_not_chain_flag_the_gap() {
        let (_, days) = november();
        let nov27 = day(&days, 27).korsel.clone().unwrap();
        assert_eq!(nov27.route_text(), "Hjem → Kunde A → Kunde C → Kontoret");
        assert!(!nov27.return_trip);
        assert_eq!(
            nov27.gaps,
            vec![("Kunde A".to_string(), "Kunde C".to_string())]
        );
    }

    #[test]
    fn only_days_to_enter_can_be_ticked() {
        let (_, days) = november();
        assert_eq!(day(&days, 18).tick_refusal(), None);
        assert_eq!(
            day(&days, 21).tick_refusal().as_deref(),
            Some("nothing to enter (weekend)")
        );
        let fix = day(&days, 25).tick_refusal().unwrap();
        assert!(
            fix.starts_with("fix first: ") && fix.contains("uden registreret afgang"),
            "{fix}"
        );
    }

    #[test]
    fn rendered_checklist_shows_registrations_and_progress() {
        let (_, days) = november();
        let d = |n| NaiveDate::from_ymd_opt(2026, 11, n).unwrap();
        let text = render(&days, &BTreeSet::from([d(18), d(19)]));
        let block = |n: u32| {
            let start = text.find(&d(n).to_string()).unwrap();
            let end = text[start + 1..]
                .find("\n2026-")
                .map_or(text.len(), |i| start + 1 + i);
            text[start..end].to_string()
        };
        assert_eq!(
            block(18),
            "2026-11-18  ons  [x] entered\n    Komme/gå  08:30 to 15:30 · break 30m · 6h 30m\n    Kørsel    Purpose: Kunde A · Hjem → Kunde A · Add return trip: on · 148,0 km"
        );
        assert_eq!(
            block(19),
            "2026-11-19  tor  [x] entered\n    Komme/gå  07:58 to 10:00 · break none · 2h 02m\n    Komme/gå  11:15 to 16:00 · break 30m · 4h 15m"
        );
        assert!(block(20).starts_with("2026-11-20  fre  [ ] to enter\n"));
        assert_eq!(block(21), "2026-11-21  lør  nothing to enter (weekend)");
        assert!(block(25).starts_with("2026-11-25  ons  fix first: "));
        assert!(!text.contains("Description"));
        assert_eq!(
            progress(&days, &BTreeSet::from([d(18), d(19)])),
            Progress {
                days_done: 2,
                days_total: 6,
                trips_done: 1,
                trips_total: 3,
                fix_first: 2,
            }
        );
        assert!(
            text.ends_with("\nDays entered: 2 of 6 · Trips entered: 1 of 3 · 2 to fix first\n"),
            "{text}"
        );
    }

    #[test]
    fn stops_are_named_as_in_the_app() {
        let cfg = Config::parse(&format!(
            "{BASE}[places]\nhome = \"56.1890, 10.1820\"\n[checklist]\nhome_name = \"Hjemme\"\nworkplace_name = \"Arbejde\"\n"
        ))
        .unwrap();
        let ics = entry("Kunde A", "Vestergade 12\\, 5000 Odense C", "03");
        let fabrik = "Fabriksvej 4, 8600 Silkeborg";
        let mut trips = vec![
            // The export names the road; the coordinates say home.
            trip(
                "03",
                "07:00",
                "08:00",
                "Gødvadvej, 8200 Aarhus N",
                KUNDE_A,
                74.0,
            ),
            trip("03", "12:00", "13:00", KUNDE_A, fabrik, 80.0),
            trip("03", "14:00", "15:00", fabrik, OFFICE, 40.0),
        ];
        trips[0].start_pos = Some(Position {
            lat: 56.1890,
            lon: 10.1820,
        });
        trips[2].purpose = "Levering".into();
        let (_, days) = checklist(&cfg, &ics, &trips);
        let korsel = day(&days, 3).korsel.clone().unwrap();
        assert_eq!(
            korsel.route_text(),
            "Hjemme → Kunde A → Fabriksvej 4, 8600 Silkeborg → Arbejde"
        );
        assert_eq!(korsel.purpose, "Kunde A, Levering");
    }
}
