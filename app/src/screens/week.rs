//! The Month screen's week view: one ISO week, Monday to Sunday, which can
//! span two months. Its title and range, the months it needs and its totals.

use chrono::{Datelike, Duration, NaiveDate};
use vw_danlon::mileage::Entry;
use vw_danlon::month::{Day, Month};

/// Whether the Month screen shows a month or a week.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    Month,
    Week,
}

/// The Monday of the ISO week of `d`.
pub fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(i64::from(d.weekday().num_days_from_monday()))
}

pub fn sunday_of(monday: NaiveDate) -> NaiveDate {
    monday + Duration::days(6)
}

pub fn month_of(d: NaiveDate) -> Month {
    Month {
        year: d.year(),
        month: d.month(),
    }
}

/// The week Month → Week shows for a month other than the current one: the
/// first week starting in it, so Week → Month comes back to the same month.
pub fn first_week(m: Month) -> NaiveDate {
    let first = m.first_day();
    let monday = monday_of(first);
    if monday == first {
        first
    } else {
        monday + Duration::days(7)
    }
}

/// "Week 47": the ISO week number.
pub fn week_title(monday: NaiveDate) -> String {
    format!("Week {}", monday.iso_week().week())
}

/// "16–22 Nov 2026", "30 Nov – 6 Dec 2026", "28 Dec 2026 – 3 Jan 2027".
pub fn week_range(monday: NaiveDate) -> String {
    let sunday = sunday_of(monday);
    if monday.year() != sunday.year() {
        format!(
            "{} – {}",
            monday.format("%-d %b %Y"),
            sunday.format("%-d %b %Y")
        )
    } else if monday.month() != sunday.month() {
        format!(
            "{} – {}",
            monday.format("%-d %b"),
            sunday.format("%-d %b %Y")
        )
    } else {
        format!("{}–{}", monday.day(), sunday.format("%-d %b %Y"))
    }
}

/// The months the week's days come from: the Monday's, and the Sunday's
/// when it is another one.
pub fn week_months(monday: NaiveDate) -> (Month, Option<Month>) {
    let first = month_of(monday);
    let last = month_of(sunday_of(monday));
    (first, (last != first).then_some(last))
}

pub fn in_week(monday: NaiveDate, d: NaiveDate) -> bool {
    d >= monday && d <= sunday_of(monday)
}

/// Totals of a set of days, as the Month KPIs count them: norm and worked
/// up to and including `today` (today so far), the norm of all the days.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Totals {
    pub norm: f64,
    pub worked: f64,
    pub whole_norm: f64,
}

impl Totals {
    pub fn difference(&self) -> f64 {
        self.worked - self.norm
    }
}

pub fn totals(days: &[&Day], today: NaiveDate) -> Totals {
    let to_date = || days.iter().filter(|d| d.date <= today);
    Totals {
        norm: to_date().map(|d| d.expected_hours).fold(0.0, |a, b| a + b),
        worked: to_date().map(|d| d.worked_hours).fold(0.0, |a, b| a + b),
        whole_norm: days.iter().map(|d| d.norm_hours).fold(0.0, |a, b| a + b),
    }
}

/// The days the Hours screen lists: newest first, today on top, none after
/// today.
pub fn listed<'a>(days: &[&'a Day], today: NaiveDate) -> Vec<&'a Day> {
    days.iter()
        .rev()
        .copied()
        .filter(|d| d.date <= today)
        .collect()
}

/// The mileage allowance of the week's business trips: amount, km and the
/// number of trips.
pub fn week_mileage<'a>(
    entries: impl IntoIterator<Item = &'a Entry>,
    monday: NaiveDate,
) -> (f64, f64, usize) {
    entries
        .into_iter()
        .filter(|e| in_week(monday, e.start.date_naive()))
        .fold((0.0, 0.0, 0), |(amount, km, n), e| {
            (amount + e.amount(), km + e.km, n + 1)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use vw_danlon::month::{Lunch, LunchSource};

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn day(date: &str, norm: f64, expected: f64, worked: f64) -> Day {
        Day {
            date: d(date),
            norm_hours: norm,
            expected_hours: expected,
            so_far: false,
            open: None,
            planned: None,
            off_reason: None,
            work_locations: Vec::new(),
            appointments: Vec::new(),
            home: Vec::new(),
            segments: Vec::new(),
            clients: Vec::new(),
            first_start: None,
            last_end: None,
            note: None,
            check: None,
            lunch: Lunch {
                window: None,
                source: LunchSource::Default,
            },
            worked_hours: worked,
        }
    }

    #[test]
    fn a_week_inside_a_month() {
        let monday = monday_of(d("2026-11-19"));
        assert_eq!(monday, d("2026-11-16"));
        assert_eq!(monday_of(monday), monday);
        assert_eq!(monday_of(d("2026-11-22")), monday);
        assert_eq!(week_title(monday), "Week 47");
        assert_eq!(week_range(monday), "16–22 Nov 2026");
        assert_eq!(
            week_months(monday),
            (
                Month {
                    year: 2026,
                    month: 11
                },
                None
            )
        );
    }

    #[test]
    fn a_week_across_a_month() {
        let monday = monday_of(d("2026-12-02"));
        assert_eq!(monday, d("2026-11-30"));
        assert_eq!(week_title(monday), "Week 49");
        assert_eq!(week_range(monday), "30 Nov – 6 Dec 2026");
        assert_eq!(
            week_months(monday),
            (
                Month {
                    year: 2026,
                    month: 11
                },
                Some(Month {
                    year: 2026,
                    month: 12
                })
            )
        );
    }

    #[test]
    fn a_week_across_a_year() {
        let monday = monday_of(d("2027-01-01"));
        assert_eq!(monday, d("2026-12-28"));
        // Thursday 31 Dec is in 2026: ISO week 53 of 2026.
        assert_eq!(week_title(monday), "Week 53");
        assert_eq!(week_range(monday), "28 Dec 2026 – 3 Jan 2027");
        assert_eq!(
            week_months(monday).1,
            Some(Month {
                year: 2027,
                month: 1
            })
        );
        // 4 Jan 2027 starts week 1.
        assert_eq!(week_title(d("2027-01-04")), "Week 1");
    }

    #[test]
    fn the_first_week_of_a_month_starts_in_it() {
        let oct = Month {
            year: 2026,
            month: 10,
        };
        // 1 Oct 2026 is a Thursday.
        assert_eq!(first_week(oct), d("2026-10-05"));
        assert_eq!(month_of(first_week(oct)), oct);
        // 1 Jun 2026 is a Monday.
        let jun = Month {
            year: 2026,
            month: 6,
        };
        assert_eq!(first_week(jun), d("2026-06-01"));
    }

    #[test]
    fn the_week_totals_count_up_to_today() {
        let days = [
            day("2026-11-23", 7.5, 7.5, 7.67),
            // Today so far: the planned time up to now.
            day("2026-11-24", 7.5, 1.17, 0.0),
            day("2026-11-25", 7.5, 7.5, 0.0),
            day("2026-11-26", 7.5, 7.5, 0.0),
            day("2026-11-27", 7.5, 7.5, 0.0),
            day("2026-11-28", 0.0, 0.0, 0.0),
            day("2026-11-29", 0.0, 0.0, 0.0),
        ];
        let refs: Vec<&Day> = days.iter().collect();
        let t = totals(&refs, d("2026-11-24"));
        assert!((t.norm - 8.67).abs() < 1e-9);
        assert!((t.worked - 7.67).abs() < 1e-9);
        assert!((t.difference() + 1.0).abs() < 1e-9);
        assert!((t.whole_norm - 37.5).abs() < 1e-9);
        // A week that has passed counts every day.
        let t = totals(&refs, d("2026-12-01"));
        assert!((t.norm - 31.17).abs() < 1e-9);
        // One that has not started counts none.
        let t = totals(&refs, d("2026-11-20"));
        assert_eq!((t.norm, t.worked), (0.0, 0.0));
    }

    #[test]
    fn the_days_are_listed_newest_first_up_to_today() {
        let days = [
            day("2026-11-23", 7.5, 7.5, 7.67),
            day("2026-11-24", 7.5, 1.17, 0.0),
            day("2026-11-25", 7.5, 7.5, 0.0),
        ];
        let refs: Vec<&Day> = days.iter().collect();
        let dates: Vec<NaiveDate> = listed(&refs, d("2026-11-24"))
            .iter()
            .map(|x| x.date)
            .collect();
        assert_eq!(dates, [d("2026-11-24"), d("2026-11-23")]);
        assert!(listed(&refs, d("2026-11-22")).is_empty());
    }

    #[test]
    fn the_week_mileage_counts_its_own_trips() {
        let tz = chrono_tz::Europe::Copenhagen;
        let entry = |date: &str, km: f64| Entry {
            start: tz
                .from_local_datetime(&d(date).and_hms_opt(8, 0, 0).unwrap())
                .unwrap(),
            from: "Hjem".into(),
            to: "Kunde".into(),
            via: Vec::new(),
            purpose: "Kunde".into(),
            km,
            rate: 3.94,
        };
        let entries = [
            entry("2026-11-15", 10.0),
            entry("2026-11-18", 74.0),
            entry("2026-11-18", 74.0),
            entry("2026-11-23", 50.0),
        ];
        let (amount, km, n) = week_mileage(&entries, d("2026-11-16"));
        assert_eq!(n, 2);
        assert!((km - 148.0).abs() < 1e-9);
        assert!((amount - 583.12).abs() < 1e-9);
    }
}
