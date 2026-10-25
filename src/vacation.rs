//! Vacation balance under Danish samtidighedsferie (ferieloven).
//!
//! Vacation is earned per holiday year (ferieår, 1 September – 31 August) at
//! `days_per_month` (2,08) per month employed; a partial month earns 0,07
//! per calendar day employed. Days earned in a month can be taken from the
//! following month until 31 December of the following calendar year.
//! Vacation days are drawn from the oldest holiday year that still has days.
//! Feriefridage are a separate count per holiday year.
//!
//! All amounts are hundredths of a day, so sums stay exact (4 × 2,08 = 8,32).

use std::collections::BTreeMap;

use chrono::{Datelike, Duration, NaiveDate};

use crate::calendar::Occurrence;
use crate::config::Config;
use crate::month::{self, AllDayKind, Day, LeaveKind, Month, OffReason};

/// Earned per calendar day employed in a partial month: 0,07.
const PER_DAY: i64 = 7;
/// One vacation day.
const ONE_DAY: i64 = 100;

/// An amount of vacation in hundredths of a day; shown with a decimal comma.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Days(pub i64);

impl std::fmt::Display for Days {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let n = self.0.abs();
        write!(f, "{sign}{},{:02}", n / 100, n % 100)
    }
}

/// A holiday year (ferieår) by the calendar year it starts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HolidayYear(pub i32);

impl HolidayYear {
    pub fn containing(date: NaiveDate) -> Self {
        HolidayYear(if date.month() >= 9 {
            date.year()
        } else {
            date.year() - 1
        })
    }

    /// 1 September, the first day of earning.
    pub fn first_day(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.0, 9, 1).unwrap()
    }

    /// 31 August, the last day of earning.
    pub fn last_day(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.0 + 1, 8, 31).unwrap()
    }

    /// 31 December of the following year, the last day the days can be used.
    pub fn use_by(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.0 + 1, 12, 31).unwrap()
    }

    fn months(self) -> impl Iterator<Item = Month> {
        (8..20u32).map(move |i| Month {
            year: self.0 + (i / 12) as i32,
            month: i % 12 + 1,
        })
    }
}

impl std::fmt::Display for HolidayYear {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{:02}", self.0, (self.0 + 1).rem_euclid(100))
    }
}

/// How vacation is earned.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// `None`: earned from the start of each holiday year.
    pub employment_start: Option<NaiveDate>,
    /// Hundredths of a day per full month (208).
    pub per_month: i64,
    /// Feriefridage per holiday year.
    pub feriefridage: u32,
}

impl Rules {
    pub fn from_config(cfg: &Config) -> Self {
        Rules {
            employment_start: cfg.vacation.employment_start,
            per_month: cfg.vacation.hundredths_per_month(),
            feriefridage: cfg.vacation.feriefridage,
        }
    }

    /// The first day `year` earns anything; `None` if employment starts after it.
    pub fn earning_start(&self, year: HolidayYear) -> Option<NaiveDate> {
        let start = match self.employment_start {
            Some(s) => s.max(year.first_day()),
            None => year.first_day(),
        };
        (start <= year.last_day()).then_some(start)
    }

    fn month_earning(&self, m: Month) -> i64 {
        match self.employment_start {
            Some(s) if s > m.last_day() => 0,
            Some(s) if s > m.first_day() => PER_DAY * ((m.last_day() - s).num_days() + 1),
            _ => self.per_month,
        }
    }

    /// Earned in `year` in the months that ended on or before `date`.
    pub fn earned_through(&self, year: HolidayYear, date: NaiveDate) -> Days {
        Days(
            year.months()
                .filter(|m| m.last_day() <= date)
                .map(|m| self.month_earning(m))
                .sum(),
        )
    }

    /// Everything `year` earns by 31 August.
    pub fn earned_total(&self, year: HolidayYear) -> Days {
        self.earned_through(year, year.last_day())
    }

    /// What can be used on `date`: earned in the months before its month.
    fn usable_on(&self, year: HolidayYear, date: NaiveDate) -> Days {
        self.earned_through(year, date.with_day(1).unwrap() - Duration::days(1))
    }
}

/// A date inside a vacation or feriefridag event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaveDay {
    pub date: NaiveDate,
    pub kind: LeaveKind,
    /// The event's title.
    pub title: String,
    /// Counts as a vacation day (see [`OffReason::Leave`]).
    pub working_day: bool,
}

/// Every date inside a vacation or feriefridag event of `calendar`, with
/// whether `days` (built from the same calendar) makes it a working day. A
/// public holiday inside the event is listed but does not count.
pub fn leave_days(cfg: &Config, calendar: &[Occurrence], days: &[Day]) -> Vec<LeaveDay> {
    let counted: BTreeMap<NaiveDate, LeaveKind> = days
        .iter()
        .filter_map(|d| match &d.off_reason {
            Some(OffReason::Leave {
                kind,
                working_day: true,
                ..
            }) => Some((d.date, *kind)),
            _ => None,
        })
        .collect();
    let mut out: Vec<LeaveDay> = calendar
        .iter()
        .filter_map(|occ| match occ {
            Occurrence::AllDay { date, summary } => match month::classify_all_day(cfg, summary) {
                Some(AllDayKind::Leave(kind)) => Some(LeaveDay {
                    date: *date,
                    kind,
                    title: summary.clone(),
                    working_day: counted.get(date) == Some(&kind),
                }),
                _ => None,
            },
            Occurrence::Timed { .. } => None,
        })
        .collect();
    // One event per date: the first, as for the day's reason.
    out.sort_by_key(|d| d.date);
    out.dedup_by_key(|d| d.date);
    out
}

/// Vacation days of one period drawn from one holiday year.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draw {
    pub year: HolidayYear,
    pub days: Days,
    /// The year's days left after this period (of everything it earns).
    pub left_after: Days,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodStatus {
    Taken,
    Ongoing,
    Planned,
}

/// One calendar event: consecutive dates with the same kind and title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Period {
    pub kind: LeaveKind,
    pub title: String,
    pub first: NaiveDate,
    pub last: NaiveDate,
    pub working_days: u32,
    pub status: PeriodStatus,
    /// Ferie: where the days come from, oldest year first.
    pub draws: Vec<Draw>,
    /// Ferie: some days were taken before they were earned (forskudsferie).
    pub forskud: bool,
    /// Ferie: days before the employment start, drawn from no year.
    pub before_employment: Days,
    /// Feriefridag: the holiday year and its feriefridage left after this period.
    pub feriefridage_left: Option<(HolidayYear, i64)>,
}

impl Period {
    pub fn calendar_days(&self) -> i64 {
        (self.last - self.first).num_days() + 1
    }
}

/// One holiday year's balance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YearBalance {
    pub year: HolidayYear,
    /// Earned in the months completed so far.
    pub earned: Days,
    /// Everything the year earns by 31 August.
    pub total: Days,
    pub taken: Days,
    pub planned: Days,
    /// `total` minus taken and planned.
    pub left: Days,
    /// Earning is over (after 31 August).
    pub earning_done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeriefridageYear {
    pub year: HolidayYear,
    pub allowance: u32,
    pub taken: u32,
    pub planned: u32,
}

impl FeriefridageYear {
    pub fn left(&self) -> i64 {
        i64::from(self.allowance) - i64::from(self.taken) - i64::from(self.planned)
    }
}

/// Days of a year neither taken nor planned by its use-by date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expiring {
    pub year: HolidayYear,
    pub days: Days,
    pub date: NaiveDate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    pub years: Vec<YearBalance>,
    pub periods: Vec<Period>,
    pub feriefridage: Vec<FeriefridageYear>,
    pub expiring: Vec<Expiring>,
}

/// The holiday years open on `today`: the one earning now, and the previous
/// one until its use-by date (September – December).
pub fn open_years(today: NaiveDate) -> Vec<HolidayYear> {
    let current = HolidayYear::containing(today);
    let previous = HolidayYear(current.0 - 1);
    if today <= previous.use_by() {
        vec![previous, current]
    } else {
        vec![current]
    }
}

/// The balance on `today` from the leave days of the calendar (loaded from
/// the oldest open year's start). Days are drawn from the oldest year that
/// still has days and whose use window holds the date; when every year is
/// used up, the newest one goes negative.
pub fn overview(rules: &Rules, days: &[LeaveDay], today: NaiveDate) -> Overview {
    let oldest = open_years(today)[0];
    let mut drawn: BTreeMap<HolidayYear, i64> = BTreeMap::new();
    let mut taken: BTreeMap<HolidayYear, i64> = BTreeMap::new();
    let mut planned: BTreeMap<HolidayYear, i64> = BTreeMap::new();
    let mut fridage: BTreeMap<HolidayYear, (u32, u32)> = BTreeMap::new();
    let mut periods = Vec::new();

    for group in periods_of(days) {
        let first = group[0];
        let last = group[group.len() - 1];
        let mut period = Period {
            kind: first.kind,
            title: first.title.clone(),
            first: first.date,
            last: last.date,
            working_days: 0,
            status: if last.date <= today {
                PeriodStatus::Taken
            } else if first.date > today {
                PeriodStatus::Planned
            } else {
                PeriodStatus::Ongoing
            },
            draws: Vec::new(),
            forskud: false,
            before_employment: Days(0),
            feriefridage_left: None,
        };
        let mut from: BTreeMap<HolidayYear, i64> = BTreeMap::new();
        for day in group.iter().filter(|d| d.working_day) {
            period.working_days += 1;
            let done = day.date <= today;
            if day.kind == LeaveKind::Feriefridag {
                let year = HolidayYear::containing(day.date);
                let count = fridage.entry(year).or_default();
                if done {
                    count.0 += 1;
                } else {
                    count.1 += 1;
                }
                let left = i64::from(rules.feriefridage) - i64::from(count.0 + count.1);
                period.feriefridage_left = Some((year, left));
                continue;
            }
            // Only this year and the previous one can hold the date.
            let here = HolidayYear::containing(day.date);
            let candidates: Vec<HolidayYear> = [HolidayYear(here.0 - 1), here]
                .into_iter()
                .filter(|y| {
                    *y >= oldest
                        && rules
                            .earning_start(*y)
                            .is_some_and(|s| s <= day.date && day.date <= y.use_by())
                })
                .collect();
            let mut need = ONE_DAY;
            for (i, year) in candidates.iter().enumerate() {
                let used = drawn.entry(*year).or_default();
                let rest = rules.earned_total(*year).0 - *used;
                // The newest candidate takes what the others cannot.
                let take = if i + 1 == candidates.len() {
                    need
                } else {
                    need.min(rest.max(0))
                };
                if take == 0 {
                    continue;
                }
                *used += take;
                need -= take;
                *from.entry(*year).or_default() += take;
                let book = if done { &mut taken } else { &mut planned };
                *book.entry(*year).or_default() += take;
                if *used > rules.usable_on(*year, day.date).0 {
                    period.forskud = true;
                }
                if need == 0 {
                    break;
                }
            }
            period.before_employment.0 += need;
        }
        period.draws = from
            .into_iter()
            .map(|(year, days)| Draw {
                year,
                days: Days(days),
                left_after: Days(rules.earned_total(year).0 - drawn[&year]),
            })
            .collect();
        periods.push(period);
    }

    let current = HolidayYear::containing(today);
    let mut shown: Vec<HolidayYear> = open_years(today)
        .into_iter()
        .filter(|y| rules.earning_start(*y).is_some())
        .chain(drawn.keys().copied())
        .collect();
    shown.sort();
    shown.dedup();
    let yesterday = today - Duration::days(1);
    let years: Vec<YearBalance> = shown
        .into_iter()
        .map(|year| {
            let total = rules.earned_total(year);
            let taken = taken.get(&year).copied().unwrap_or_default();
            let planned = planned.get(&year).copied().unwrap_or_default();
            YearBalance {
                year,
                earned: rules.earned_through(year, yesterday),
                total,
                taken: Days(taken),
                planned: Days(planned),
                left: Days(total.0 - taken - planned),
                earning_done: today > year.last_day(),
            }
        })
        .collect();
    let expiring = years
        .iter()
        .filter(|y| y.earning_done && y.left.0 > 0 && today <= y.year.use_by())
        .map(|y| Expiring {
            year: y.year,
            days: y.left,
            date: y.year.use_by(),
        })
        .collect();

    fridage.entry(current).or_default();
    let feriefridage = fridage
        .into_iter()
        .filter(|(year, _)| *year >= oldest)
        .map(|(year, (taken, planned))| FeriefridageYear {
            year,
            allowance: rules.feriefridage,
            taken,
            planned,
        })
        .collect();

    Overview {
        years,
        periods,
        feriefridage,
        expiring,
    }
}

/// Consecutive dates with the same kind and title, in date order.
fn periods_of(days: &[LeaveDay]) -> Vec<Vec<&LeaveDay>> {
    let mut sorted: Vec<&LeaveDay> = days.iter().collect();
    sorted.sort_by_key(|d| d.date);
    let mut out: Vec<Vec<&LeaveDay>> = Vec::new();
    for day in sorted {
        match out.last_mut() {
            Some(group)
                if group.last().is_some_and(|p| {
                    p.date + Duration::days(1) == day.date
                        && p.kind == day.kind
                        && p.title == day.title
                }) =>
            {
                group.push(day)
            }
            _ => out.push(vec![day]),
        }
    }
    out
}

fn date_text(d: NaiveDate) -> String {
    d.format("%-d %b %Y").to_string()
}

/// The overview as text: one line per holiday year, the periods, the
/// feriefridage and any expiring days.
pub fn render(rules: &Rules, o: &Overview) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let per_month = Days(rules.per_month);
    match rules.employment_start {
        Some(start) => writeln!(
            out,
            "Vacation · employed since {} · {per_month} days a month",
            date_text(start)
        ),
        None => writeln!(
            out,
            "Vacation · {per_month} days a month\n\
             Employment start not set (vacation.employment_start): earned counts from 1 Sep of each holiday year"
        ),
    }
    .unwrap();

    out.push('\n');
    for y in &o.years {
        let earned = if y.earning_done {
            format!("earned {}", y.earned)
        } else {
            format!(
                "earned {} ({} by {})",
                y.earned,
                y.total,
                date_text(y.year.last_day())
            )
        };
        writeln!(
            out,
            "{} · {earned} · taken {} · planned {} · left {} · use by {}",
            y.year,
            y.taken,
            y.planned,
            y.left,
            date_text(y.year.use_by())
        )
        .unwrap();
    }

    out.push('\n');
    if o.periods.is_empty() {
        out.push_str("No vacation in the calendar\n");
    } else {
        out.push_str("Periods (oldest days are used first)\n");
    }
    for p in &o.periods {
        let dates = if p.first == p.last {
            date_text(p.first)
        } else {
            format!("{} – {}", date_text(p.first), date_text(p.last))
        };
        let mut days = match p.working_days {
            0 => "no working days".to_string(),
            1 => "1 working day".to_string(),
            n => format!("{n} working days"),
        };
        if p.calendar_days() != i64::from(p.working_days) && p.working_days > 0 {
            write!(days, " of {}", p.calendar_days()).unwrap();
        }
        let status = match p.status {
            PeriodStatus::Taken => "taken",
            PeriodStatus::Ongoing => "ongoing",
            PeriodStatus::Planned => "planned",
        };
        let mut source: Vec<String> = p
            .draws
            .iter()
            .map(|d| format!("{} from {} ({} left)", d.days, d.year, d.left_after))
            .collect();
        if let Some((year, left)) = p.feriefridage_left {
            source.push(format!(
                "feriefridage {year} ({left} of {} left)",
                rules.feriefridage
            ));
        }
        if p.forskud {
            source.push("forskudsferie: taken before earned".into());
        }
        if p.before_employment.0 > 0 {
            source.push(format!(
                "{} before the employment start",
                p.before_employment
            ));
        }
        writeln!(
            out,
            "  {dates} · {} · {days} · {status}{}",
            p.title,
            source.iter().map(|s| format!(" · {s}")).collect::<String>()
        )
        .unwrap();
    }

    out.push('\n');
    for f in &o.feriefridage {
        writeln!(
            out,
            "Feriefridage {} · taken {} · planned {} · {} of {} left",
            f.year,
            f.taken,
            f.planned,
            f.left(),
            f.allowance
        )
        .unwrap();
    }

    for e in &o.expiring {
        writeln!(
            out,
            "\nwarning: {} days from {} expire on {}",
            e.days,
            e.year,
            date_text(e.date)
        )
        .unwrap();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar;
    use chrono::Weekday;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn rules(start: Option<&str>) -> Rules {
        Rules {
            employment_start: start.map(d),
            per_month: 208,
            feriefridage: 5,
        }
    }

    /// Every date of an event; weekdays not in `off` are working days.
    fn event(kind: LeaveKind, title: &str, first: &str, last: &str, off: &[&str]) -> Vec<LeaveDay> {
        d(first)
            .iter_days()
            .take_while(|day| *day <= d(last))
            .map(|date| LeaveDay {
                date,
                kind,
                title: title.into(),
                working_day: !matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
                    && !off.iter().any(|o| d(o) == date),
            })
            .collect()
    }

    #[test]
    fn earned_from_employment_start() {
        let r = rules(Some("2026-05-01"));
        assert_eq!(r.earned_total(HolidayYear(2025)).to_string(), "8,32");
        assert_eq!(HolidayYear(2025).use_by(), d("2026-12-31"));
        assert_eq!(HolidayYear(2025).to_string(), "2025/26");
        // 2026/27 after Sep–Nov; November counts once it has ended.
        let y = HolidayYear(2026);
        assert_eq!(r.earned_through(y, d("2026-11-30")).to_string(), "6,24");
        assert_eq!(r.earned_through(y, d("2026-11-29")).to_string(), "4,16");
        // Without a start, earning runs from 1 September.
        assert_eq!(
            rules(None).earned_total(HolidayYear(2025)).to_string(),
            "24,96"
        );
    }

    #[test]
    fn partial_month_earns_per_calendar_day() {
        // 20–31 May: 12 calendar days × 0,07.
        let r = rules(Some("2026-05-20"));
        assert_eq!(
            r.earned_through(HolidayYear(2025), d("2026-05-31")),
            Days(84)
        );
        assert_eq!(r.earned_total(HolidayYear(2025)), Days(84 + 3 * 208));
    }

    #[test]
    fn only_working_days_in_calendar_events_count() {
        let cfg = Config::parse("employee_number = \"1\"\n[danlon]\n").unwrap();
        let ics = "BEGIN:VEVENT
UID:a
SUMMARY:Ferie
DTSTART;VALUE=DATE:20261002
DTEND;VALUE=DATE:20261006
END:VEVENT
BEGIN:VEVENT
UID:b
SUMMARY:Ferie
DTSTART;VALUE=DATE:20260513
DTEND;VALUE=DATE:20260516
END:VEVENT
BEGIN:VEVENT
UID:c
SUMMARY:Weekendferie
DTSTART;VALUE=DATE:20261010
DTEND;VALUE=DATE:20261012
END:VEVENT
";
        let occ =
            calendar::occurrences(ics, cfg.timezone, d("2026-05-01"), d("2026-10-31")).unwrap();
        let days: Vec<Day> = (5..=10)
            .flat_map(|m| {
                let month = Month {
                    year: 2026,
                    month: m,
                };
                month::build(&cfg, month, &[], &occ, &BTreeMap::new(), &[]).days
            })
            .collect();
        let leave = leave_days(&cfg, &occ, &days);
        let r = rules(Some("2026-05-01"));
        let o = overview(&r, &leave, d("2026-10-25"));
        let counts: Vec<(NaiveDate, u32)> = o
            .periods
            .iter()
            .map(|p| (p.first, p.working_days))
            .collect();
        assert_eq!(
            counts,
            [
                // Wed–Fri over Kristi himmelfartsdag (Thu 14 May).
                (d("2026-05-13"), 2),
                // Fri–Mon.
                (d("2026-10-02"), 2),
                (d("2026-10-10"), 0),
            ]
        );
        assert!(
            !leave
                .iter()
                .any(|l| l.date == d("2026-05-14") && l.working_day)
        );
        let text = render(&r, &o);
        assert!(text.contains("Weekendferie · no working days"), "{text}");
        // Taken in May, before anything was earned.
        assert!(o.periods[0].forskud);
        assert!(text.contains("forskudsferie"), "{text}");
    }

    #[test]
    fn periods_draw_from_the_oldest_year_first() {
        let mut leave = event(LeaveKind::Ferie, "Ferie", "2026-11-13", "2026-11-13", &[]);
        let holidays = ["2026-12-24", "2026-12-25", "2026-12-31", "2027-01-01"];
        leave.extend(event(
            LeaveKind::Ferie,
            "Juleferie",
            "2026-12-19",
            "2027-01-03",
            &holidays,
        ));
        leave.extend(event(
            LeaveKind::Ferie,
            "Vinterferie",
            "2027-02-15",
            "2027-02-19",
            &[],
        ));
        leave.extend(event(
            LeaveKind::Feriefridag,
            "Feriefridag",
            "2027-04-30",
            "2027-04-30",
            &[],
        ));
        let r = rules(Some("2026-05-01"));
        let o = overview(&r, &leave, d("2026-11-20"));

        let years: Vec<String> = o
            .years
            .iter()
            .map(|y| {
                format!(
                    "{} {} {} {} {}",
                    y.year, y.earned, y.taken, y.planned, y.left
                )
            })
            .collect();
        assert_eq!(
            years,
            [
                "2025/26 8,32 1,00 6,00 1,32",
                "2026/27 4,16 0,00 5,00 19,96"
            ]
        );
        let draws: Vec<Vec<String>> = o
            .periods
            .iter()
            .map(|p| {
                p.draws
                    .iter()
                    .map(|d| format!("{} {} {}", d.year, d.days, d.left_after))
                    .collect()
            })
            .collect();
        assert_eq!(
            draws,
            [
                vec!["2025/26 1,00 7,32"],
                vec!["2025/26 6,00 1,32"],
                vec!["2026/27 5,00 19,96"],
                vec![],
            ]
        );
        assert!(o.periods.iter().all(|p| !p.forskud));
        assert_eq!(o.periods[1].working_days, 6);
        assert_eq!(o.periods[3].feriefridage_left, Some((HolidayYear(2026), 4)));
        assert_eq!(
            o.expiring,
            [Expiring {
                year: HolidayYear(2025),
                days: Days(132),
                date: d("2026-12-31"),
            }]
        );

        let text = render(&r, &o);
        for line in [
            "2025/26 · earned 8,32 · taken 1,00 · planned 6,00 · left 1,32 · use by 31 Dec 2026",
            "19 Dec 2026 – 3 Jan 2027 · Juleferie · 6 working days of 16 · planned · 6,00 from 2025/26 (1,32 left)",
            "Feriefridage 2026/27 · taken 0 · planned 1 · 4 of 5 left",
            "warning: 1,32 days from 2025/26 expire on 31 Dec 2026",
        ] {
            assert!(text.contains(line), "{line}\n{text}");
        }
        assert!(!text.contains("Employment start not set"));
        assert!(render(&rules(None), &o).contains("Employment start not set"));
    }

    #[test]
    fn a_period_splits_when_the_oldest_year_runs_out() {
        // 9 working days 1–11 Dec: 8,32 from 2025/26, the rest from 2026/27,
        // which has 6,24 usable in December.
        let leave = event(LeaveKind::Ferie, "Ferie", "2026-12-01", "2026-12-11", &[]);
        let o = overview(&rules(Some("2026-05-01")), &leave, d("2026-11-20"));
        let p = &o.periods[0];
        assert_eq!(
            p.draws,
            [
                Draw {
                    year: HolidayYear(2025),
                    days: Days(832),
                    left_after: Days(0),
                },
                Draw {
                    year: HolidayYear(2026),
                    days: Days(68),
                    left_after: Days(2496 - 68),
                },
            ]
        );
        assert!(!p.forskud);
        assert!(o.expiring.is_empty());
    }
}
