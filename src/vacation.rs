//! Vacation balance under Danish samtidighedsferie (ferieloven).
//!
//! Vacation is earned per holiday year (ferieår, 1 September – 31 August) at
//! `days_per_month` (2,08) per month employed; a partial month earns 0,07
//! per calendar day employed. Days earned in a month can be taken from the
//! following month until 31 December of the following calendar year.
//! Vacation days are drawn from the oldest holiday year that still has days.
//!
//! Feriefridage are not in ferieloven: from the contract, 5 are granted each
//! 1 May (`feriefridage_from`), used by 30 April, and a vacation day uses a
//! feriefridag while one is left.
//!
//! On 31 December of the year after a holiday year, what is left of it splits
//! in two: the part beyond 4 weeks (20 days) of what the year earned is paid
//! out (§§ 23–24), the rest goes to Arbejdsmarkedets Feriefond and is lost
//! (§ 34). Checked against LBK nr. 152 af 20.02.2024 on 31 Oct 2026.
//!
//! All amounts are hundredths of a day, so sums stay exact (4 × 2,08 = 8,32).

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, Duration, NaiveDate};

use crate::calendar::Occurrence;
use crate::config::Config;
use crate::holidays;
use crate::month::{self, AllDayKind, Day, LeaveKind, Month, OffReason};

/// Earned per calendar day employed in a partial month: 0,07.
const PER_DAY: i64 = 7;
/// One vacation day.
const ONE_DAY: i64 = 100;
/// 4 weeks of a 5-day week: what is left beyond this of a year's earning is
/// paid out, the rest is lost (ferieloven §§ 23–24, 34).
const FOUR_WEEKS: i64 = 2000;
/// How far the projection reaches, from the current month.
pub const PROJECTION_MONTHS: u32 = 24;

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

/// A feriefridage period by the calendar year it starts in: a year from
/// [`Rules::feriefridage_from`] (1 May; contract, not ferieloven).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FridagYear(pub i32);

impl std::fmt::Display for FridagYear {
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
    /// Feriefridage per period (contract).
    pub feriefridage: u32,
    /// (month, day) the feriefridage period starts (contract; 1 May).
    pub feriefridage_from: (u32, u32),
    /// Most days in hundredths taken before they are earned without a form
    /// signed by management (contract; ferieloven § 7 sets no maximum).
    pub max_forskud: i64,
}

impl Rules {
    pub fn from_config(cfg: &Config) -> Self {
        Rules {
            employment_start: cfg.vacation.employment_start,
            per_month: cfg.vacation.hundredths_per_month(),
            feriefridage: cfg.vacation.feriefridage,
            feriefridage_from: cfg.vacation.feriefridage_start().unwrap_or((5, 1)),
            max_forskud: cfg.vacation.max_forskud_hundredths(),
        }
    }

    /// The first day of a feriefridage period.
    pub fn feriefridage_first(&self, y: FridagYear) -> NaiveDate {
        let (m, d) = self.feriefridage_from;
        NaiveDate::from_ymd_opt(y.0, m, d).unwrap()
    }

    /// The last day a period's feriefridage can be used; what is left is lost.
    pub fn feriefridage_last(&self, y: FridagYear) -> NaiveDate {
        self.feriefridage_first(FridagYear(y.0 + 1)) - Duration::days(1)
    }

    /// The feriefridage period holding `date`.
    pub fn feriefridage_year(&self, date: NaiveDate) -> FridagYear {
        let y = FridagYear(date.year());
        if date >= self.feriefridage_first(y) {
            y
        } else {
            FridagYear(y.0 - 1)
        }
    }

    /// Feriefridage granted for a period: none if employment starts after it.
    pub fn feriefridage_allowance(&self, y: FridagYear) -> i64 {
        match self.employment_start {
            Some(s) if s > self.feriefridage_last(y) => 0,
            _ => i64::from(self.feriefridage),
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
            Occurrence::Timed { .. } | Occurrence::Private { .. } => None,
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
    /// Feriefridage used: the period and its feriefridage left after this one.
    pub feriefridage_left: Option<(FridagYear, i64)>,
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
    pub year: FridagYear,
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

/// Where one leave working day's vacation came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Year(HolidayYear),
    Feriefridag(FridagYear),
    /// Before the employment start: drawn from no year.
    BeforeEmployment,
}

/// Vacation used on one date, from one source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spent {
    pub date: NaiveDate,
    pub source: Source,
    pub days: Days,
}

/// The balance on `today` from the leave days of the calendar (loaded from
/// the start of the feriefridage period holding the oldest open year's
/// start). A working day uses a feriefridag while its period has one left;
/// an event with a feriefridag word always does. Otherwise days are drawn
/// from the oldest year that still has days and whose use window holds the
/// date; when every year is used up, the newest one goes negative.
pub fn overview(rules: &Rules, days: &[LeaveDay], today: NaiveDate) -> Overview {
    run(rules, days, today).0
}

fn run(rules: &Rules, days: &[LeaveDay], today: NaiveDate) -> (Overview, Vec<Spent>) {
    let oldest = open_years(today)[0];
    // Days before this only count against feriefridage.
    let start = oldest.first_day();
    let mut drawn: BTreeMap<HolidayYear, i64> = BTreeMap::new();
    let mut taken: BTreeMap<HolidayYear, i64> = BTreeMap::new();
    let mut planned: BTreeMap<HolidayYear, i64> = BTreeMap::new();
    let mut fridage: BTreeMap<FridagYear, (u32, u32)> = BTreeMap::new();
    let mut spent = Vec::new();
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
            let fy = rules.feriefridage_year(day.date);
            let count = fridage.entry(fy).or_default();
            let left = rules.feriefridage_allowance(fy) - i64::from(count.0 + count.1);
            if day.kind == LeaveKind::Feriefridag || left > 0 {
                if done {
                    count.0 += 1;
                } else {
                    count.1 += 1;
                }
                period.feriefridage_left = Some((fy, left - 1));
                spent.push(Spent {
                    date: day.date,
                    source: Source::Feriefridag(fy),
                    days: Days(ONE_DAY),
                });
                continue;
            }
            if day.date < start {
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
                spent.push(Spent {
                    date: day.date,
                    source: Source::Year(*year),
                    days: Days(take),
                });
                if *used > rules.usable_on(*year, day.date).0 {
                    period.forskud = true;
                }
                if need == 0 {
                    break;
                }
            }
            if need > 0 {
                period.before_employment.0 += need;
                spent.push(Spent {
                    date: day.date,
                    source: Source::BeforeEmployment,
                    days: Days(need),
                });
            }
        }
        period.draws = from
            .into_iter()
            .map(|(year, days)| Draw {
                year,
                days: Days(days),
                left_after: Days(rules.earned_total(year).0 - drawn[&year]),
            })
            .collect();
        if last.date >= start {
            periods.push(period);
        }
    }

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

    fridage.entry(rules.feriefridage_year(today)).or_default();
    let feriefridage = fridage
        .into_iter()
        .filter(|(year, _)| rules.feriefridage_last(*year) >= start)
        .map(|(year, (taken, planned))| FeriefridageYear {
            year,
            allowance: rules.feriefridage_allowance(year) as u32,
            taken,
            planned,
        })
        .collect();

    (
        Overview {
            years,
            periods,
            feriefridage,
            expiring,
        },
        spent,
    )
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

/// Dates to try as vacation and dates to take out, before changing the
/// calendar. Nothing is stored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    pub add: BTreeSet<NaiveDate>,
    pub remove: BTreeSet<NaiveDate>,
}

impl Draft {
    pub fn len(&self) -> usize {
        self.add.len() + self.remove.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What a deadline settles: a holiday year's days or a period's feriefridage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pool {
    Year(HolidayYear),
    Feriefridage(FridagYear),
}

impl std::fmt::Display for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Pool::Year(y) => write!(f, "{y}"),
            Pool::Feriefridage(y) => write!(f, "feriefridage {y}"),
        }
    }
}

/// Days left at a deadline: lost, or paid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settlement {
    pub pool: Pool,
    pub date: NaiveDate,
    pub days: Days,
}

/// One month of the projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthPoint {
    pub month: Month,
    /// Vacation days used in the month (not feriefridage).
    pub vacation_used: Days,
    pub feriefridage_used: u32,
    /// Vacation days earned and not used, at month end, of the years that
    /// can still be used then.
    pub balance: Days,
    /// Feriefridage left at month end in the period holding it.
    pub feriefridage_left: i64,
}

/// The first date more days are taken before they are earned than the
/// contract allows without a form signed by management.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Forskud {
    pub date: NaiveDate,
    pub days: Days,
}

/// The vacation from today to [`PROJECTION_MONTHS`] ahead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub today: NaiveDate,
    pub overview: Overview,
    pub months: Vec<MonthPoint>,
    /// Lost to Arbejdsmarkedets Feriefond (§ 34), and leftover feriefridage.
    pub losses: Vec<Settlement>,
    /// Beyond 4 weeks, paid out after 31 December (§§ 23–24).
    pub payouts: Vec<Settlement>,
    pub forskud: Option<Forskud>,
    pub max_forskud: Days,
    /// Planned days that come from `[vacation].repeats`, not the calendar.
    pub repeat_days: BTreeSet<NaiveDate>,
}

/// The last day of the projection: the end of its last month.
pub fn horizon(today: NaiveDate) -> NaiveDate {
    let months = today.year() * 12 + today.month0() as i32 + PROJECTION_MONTHS as i32 - 1;
    Month {
        year: months / 12,
        month: (months % 12) as u32 + 1,
    }
    .last_day()
}

/// The projection of `days` (calendar, repeats and draft) from `today`.
pub fn project(rules: &Rules, days: &[LeaveDay], today: NaiveDate) -> Plan {
    let (overview, spent) = run(rules, days, today);
    let oldest = open_years(today)[0];
    let end = horizon(today);
    let years = || {
        (oldest.0..=HolidayYear::containing(end).0)
            .map(HolidayYear)
            .filter(|y| rules.earning_start(*y).is_some())
    };
    let drawn = |year: HolidayYear, by: NaiveDate| -> i64 {
        spent
            .iter()
            .filter(|s| s.source == Source::Year(year) && s.date <= by)
            .map(|s| s.days.0)
            .sum()
    };
    let fridage_used = |year: FridagYear, by: NaiveDate| -> i64 {
        spent
            .iter()
            .filter(|s| s.source == Source::Feriefridag(year) && s.date <= by)
            .count() as i64
    };

    let mut months = Vec::new();
    let mut month = Month {
        year: today.year(),
        month: today.month(),
    };
    for _ in 0..PROJECTION_MONTHS {
        let last = month.last_day();
        let in_month = spent.iter().filter(|s| month.contains(s.date));
        let fridag = rules.feriefridage_year(last);
        months.push(MonthPoint {
            month,
            vacation_used: Days(
                in_month
                    .clone()
                    .filter(|s| !matches!(s.source, Source::Feriefridag(_)))
                    .map(|s| s.days.0)
                    .sum(),
            ),
            feriefridage_used: in_month
                .filter(|s| matches!(s.source, Source::Feriefridag(_)))
                .count() as u32,
            balance: Days(
                years()
                    .filter(|y| rules.earning_start(*y).is_some_and(|s| s <= last))
                    .filter(|y| last <= y.use_by())
                    .map(|y| rules.earned_through(y, last).0 - drawn(y, last))
                    .sum(),
            ),
            feriefridage_left: rules.feriefridage_allowance(fridag) - fridage_used(fridag, last),
        });
        month = Month {
            year: (last + Duration::days(1)).year(),
            month: (last + Duration::days(1)).month(),
        };
    }

    let mut losses = Vec::new();
    let mut payouts = Vec::new();
    for year in years() {
        let date = year.use_by();
        if date < today || date > end {
            continue;
        }
        let total = rules.earned_total(year).0;
        let left = total - drawn(year, date);
        if left <= 0 {
            continue;
        }
        let paid = left.min((total - FOUR_WEEKS).max(0));
        for (list, days) in [(&mut payouts, paid), (&mut losses, left - paid)] {
            if days > 0 {
                list.push(Settlement {
                    pool: Pool::Year(year),
                    date,
                    days: Days(days),
                });
            }
        }
    }
    for y in rules.feriefridage_year(today).0..=rules.feriefridage_year(end).0 {
        let year = FridagYear(y);
        let date = rules.feriefridage_last(year);
        if date < today || date > end {
            continue;
        }
        let left = rules.feriefridage_allowance(year) - fridage_used(year, date);
        if left > 0 {
            losses.push(Settlement {
                pool: Pool::Feriefridage(year),
                date,
                days: Days(left * ONE_DAY),
            });
        }
    }
    losses.sort_by_key(|s| (s.date, s.pool));
    payouts.sort_by_key(|s| (s.date, s.pool));

    // Taken before earned: what each year has drawn beyond what it had
    // earned by the month before.
    let forskud = spent
        .iter()
        .filter(|s| s.date > today && matches!(s.source, Source::Year(_)))
        .map(|s| s.date)
        .find_map(|date| {
            let ahead: i64 = years()
                .map(|y| (drawn(y, date) - rules.usable_on(y, date).0).max(0))
                .sum();
            (ahead > rules.max_forskud).then_some(Forskud {
                date,
                days: Days(ahead),
            })
        });

    Plan {
        today,
        overview,
        months,
        losses,
        payouts,
        forskud,
        max_forskud: Days(rules.max_forskud),
        repeat_days: BTreeSet::new(),
    }
}

/// A weekday with hours in `[work_week]` that is not a day off.
fn is_working_day(cfg: &Config, date: NaiveDate) -> bool {
    cfg.work_week.hours(date.weekday()) > 0.0
        && !holidays::days_off(date.year(), &cfg.holidays)
            .iter()
            .any(|(d, _)| *d == date)
}

/// The dates after `today` up to `end` that `[vacation].repeats` plans, for
/// each year in which no vacation event of `calendar` has the repeat's
/// title. ISO weeks are Monday – Sunday; Christmas is the Monday before 24
/// December through 23 December.
pub fn repeat_days(
    cfg: &Config,
    calendar: &[LeaveDay],
    today: NaiveDate,
    end: NaiveDate,
) -> Vec<LeaveDay> {
    let booked: BTreeSet<NaiveDate> = calendar.iter().map(|l| l.date).collect();
    let mut out = Vec::new();
    for r in &cfg.vacation.repeats {
        let word = r.title.to_lowercase();
        for year in today.year()..=end.year() {
            let replaced = calendar
                .iter()
                .any(|l| l.date.year() == year && l.title.to_lowercase().contains(&word));
            if replaced {
                continue;
            }
            let mut dates: Vec<NaiveDate> = r
                .weeks
                .iter()
                .filter_map(|w| NaiveDate::from_isoywd_opt(year, *w, chrono::Weekday::Mon))
                .flat_map(|mon| mon.iter_days().take(7))
                .collect();
            if r.christmas {
                let eve = NaiveDate::from_ymd_opt(year, 12, 24).unwrap();
                let mut mon = eve - Duration::days(1);
                while mon.weekday() != chrono::Weekday::Mon {
                    mon -= Duration::days(1);
                }
                dates.extend(mon.iter_days().take_while(|d| *d < eve));
            }
            out.extend(
                dates
                    .into_iter()
                    .filter(|d| *d > today && *d <= end && !booked.contains(d))
                    .map(|date| LeaveDay {
                        date,
                        kind: LeaveKind::Ferie,
                        title: r.title.clone(),
                        working_day: is_working_day(cfg, date),
                    }),
            );
        }
    }
    out.sort_by_key(|d| d.date);
    out.dedup_by_key(|d| d.date);
    out
}

/// `days` with the draft applied: removed dates dropped, added dates as
/// all-day "Vacation".
pub fn with_draft(cfg: &Config, days: &[LeaveDay], draft: &Draft) -> Vec<LeaveDay> {
    let mut out: Vec<LeaveDay> = days
        .iter()
        .filter(|d| !draft.remove.contains(&d.date))
        .cloned()
        .collect();
    let booked: BTreeSet<NaiveDate> = out.iter().map(|d| d.date).collect();
    out.extend(
        draft
            .add
            .iter()
            .filter(|d| !booked.contains(d))
            .map(|&date| LeaveDay {
                date,
                kind: LeaveKind::Ferie,
                title: "Vacation".into(),
                working_day: is_working_day(cfg, date),
            }),
    );
    out
}

/// The projection from the calendar's leave days, the repeats and a draft.
pub fn plan(cfg: &Config, calendar: &[LeaveDay], today: NaiveDate, draft: &Draft) -> Plan {
    let repeats = repeat_days(cfg, calendar, today, horizon(today));
    let repeat_dates: BTreeSet<NaiveDate> = repeats
        .iter()
        .map(|d| d.date)
        .filter(|d| !draft.remove.contains(d))
        .collect();
    let mut days = calendar.to_vec();
    days.extend(repeats);
    let mut plan = project(
        &Rules::from_config(cfg),
        &with_draft(cfg, &days, draft),
        today,
    );
    plan.repeat_days = repeat_dates;
    plan
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Loss,
    Forskud,
    Payout,
    Register,
    Clear,
}

/// Is anything lost, and the one next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub kind: StatusKind,
    pub headline: String,
    pub next_step: String,
}

impl Status {
    /// Shown warn-coloured.
    pub fn warn(&self) -> bool {
        matches!(
            self.kind,
            StatusKind::Loss | StatusKind::Forskud | StatusKind::Payout
        )
    }
}

/// How many days until a period counts as coming up for registration.
pub const REGISTER_WITHIN_DAYS: i64 = 60;

fn plural(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The status line, first match: a loss, a forskud over the limit, a
/// payout, a period in the next 60 days not yet registered (`registered`
/// holds title and first date), else nothing to do.
pub fn status(plan: &Plan, registered: &[(String, NaiveDate)]) -> Status {
    let clear = "Nothing is lost".to_string();
    if let Some(loss) = plan.losses.first() {
        let n = (loss.days.0 + ONE_DAY - 1) / ONE_DAY;
        let (headline, more) = match loss.pool {
            Pool::Year(_) => (
                format!(
                    "{} {} lost on {}",
                    if loss.days.0 == ONE_DAY {
                        "1 day".to_string()
                    } else {
                        format!("{} days", loss.days)
                    },
                    if loss.days.0 == ONE_DAY { "is" } else { "are" },
                    date_text(loss.date)
                ),
                plural(n, "more vacation day", "more vacation days"),
            ),
            Pool::Feriefridage(_) => (
                format!(
                    "{} {} lost on {}",
                    plural(n, "feriefridag", "feriefridage"),
                    if n == 1 { "is" } else { "are" },
                    date_text(loss.date)
                ),
                plural(n, "more feriefridag", "more feriefridage"),
            ),
        };
        return Status {
            kind: StatusKind::Loss,
            headline,
            next_step: format!("Plan {more} before {}", date_text(loss.date)),
        };
    }
    if let Some(f) = plan.forskud {
        return Status {
            kind: StatusKind::Forskud,
            headline: clear,
            next_step: format!(
                "Ask management for a forskud form before {}",
                date_text(f.date)
            ),
        };
    }
    if let Some(p) = plan.payouts.first() {
        return Status {
            kind: StatusKind::Payout,
            headline: clear,
            next_step: format!(
                "{} days beyond 4 weeks are paid out after {}: take them or agree a transfer by {}",
                p.days,
                date_text(p.date),
                date_text(p.date)
            ),
        };
    }
    let soon = plan.today + Duration::days(REGISTER_WITHIN_DAYS);
    let due = plan.overview.periods.iter().find(|p| {
        p.working_days > 0
            && p.first > plan.today
            && p.first <= soon
            && !registered
                .iter()
                .any(|(title, first)| *title == p.title && *first == p.first)
    });
    if let Some(p) = due {
        let dates = if p.first == p.last {
            date_text(p.first)
        } else {
            format!("{} – {}", date_text(p.first), date_text(p.last))
        };
        return Status {
            kind: StatusKind::Register,
            headline: clear,
            next_step: format!(
                "Register {} ({dates}) in Danløn, Calendar and Teams",
                p.title
            ),
        };
    }
    Status {
        kind: StatusKind::Clear,
        headline: clear,
        next_step: "Nothing to do now".into(),
    }
}

/// The status line, the losses, the payouts and any forskud as text.
pub fn render_plan(plan: &Plan, status: &Status) -> String {
    use std::fmt::Write;
    let mut out = format!("{} · {}\n", status.headline, status.next_step);
    for l in &plan.losses {
        let what = match l.pool {
            Pool::Year(y) => format!("{} days of {y}", l.days),
            Pool::Feriefridage(y) => format!(
                "{} of {y}",
                plural(l.days.0 / ONE_DAY, "feriefridag", "feriefridage")
            ),
        };
        writeln!(out, "lost: {what} on {}", date_text(l.date)).unwrap();
    }
    for p in &plan.payouts {
        writeln!(
            out,
            "paid out: {} days of {} beyond 4 weeks, after {}",
            p.days,
            p.pool,
            date_text(p.date)
        )
        .unwrap();
    }
    if let Some(f) = plan.forskud {
        writeln!(
            out,
            "forskud: {} days ahead on {}, over {}: needs a form signed by management",
            f.days,
            date_text(f.date),
            plan.max_forskud
        )
        .unwrap();
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
    use crate::state::State;
    use chrono::Weekday;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn rules(start: Option<&str>) -> Rules {
        Rules {
            employment_start: start.map(d),
            per_month: 208,
            feriefridage: 5,
            feriefridage_from: (5, 1),
            max_forskud: 500,
        }
    }

    /// Without feriefridage, so every vacation day draws from a year.
    fn vacation_only(start: Option<&str>) -> Rules {
        Rules {
            feriefridage: 0,
            ..rules(start)
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
                month::build(&cfg, month, &[], &occ, &State::default(), NaiveDate::MAX).days
            })
            .collect();
        let leave = leave_days(&cfg, &occ, &days);
        let r = vacation_only(Some("2026-05-01"));
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
        let r = vacation_only(Some("2026-05-01"));
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
        assert_eq!(o.periods[3].feriefridage_left, Some((FridagYear(2026), -1)));
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
            "Feriefridage 2026/27 · taken 0 · planned 1 · -1 of 0 left",
            "warning: 1,32 days from 2025/26 expire on 31 Dec 2026",
        ] {
            assert!(text.contains(line), "{line}\n{text}");
        }
        assert!(!text.contains("Employment start not set"));
        assert!(render(&vacation_only(None), &o).contains("Employment start not set"));
    }

    #[test]
    fn a_period_splits_when_the_oldest_year_runs_out() {
        // 9 working days 1–11 Dec: 8,32 from 2025/26, the rest from 2026/27,
        // which has 6,24 usable in December.
        let leave = event(LeaveKind::Ferie, "Ferie", "2026-12-01", "2026-12-11", &[]);
        let o = overview(&vacation_only(Some("2026-05-01")), &leave, d("2026-11-20"));
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

    fn ferie(first: &str, last: &str) -> Vec<LeaveDay> {
        event(LeaveKind::Ferie, "Ferie", first, last, &[])
    }

    fn cfg(vacation: &str) -> Config {
        Config::parse(&format!(
            "employee_number = \"1\"\n[danlon]\n[vacation]\n{vacation}\n"
        ))
        .unwrap()
    }

    fn of(list: &[Settlement], pool: Pool) -> Option<Days> {
        list.iter().find(|s| s.pool == pool).map(|s| s.days)
    }

    #[test]
    fn projection_draws_across_the_year_boundary_month_by_month() {
        // 9 working days 1–11 Dec 2026: 8,32 from 2025/26, 0,68 from 2026/27.
        let r = vacation_only(Some("2026-05-01"));
        let p = project(&r, &ferie("2026-12-01", "2026-12-11"), d("2026-10-31"));
        assert_eq!(p.months.len(), 24);
        assert_eq!(
            p.months[0].month,
            Month {
                year: 2026,
                month: 10
            }
        );
        assert_eq!(
            p.months[23].month,
            Month {
                year: 2028,
                month: 9
            }
        );
        assert_eq!(horizon(d("2026-10-31")), d("2028-09-30"));
        let show = |i: usize| {
            let m = p.months[i];
            format!("{} {} {}", m.vacation_used, m.balance, m.feriefridage_used)
        };
        // October: 8,32 of 2025/26 and Sep–Oct of 2026/27.
        assert_eq!(show(0), "0,00 12,48 0");
        // December: 2025/26 used up, 2026/27 has earned 8,32 and lent 0,68.
        assert_eq!(show(2), "9,00 7,64 0");
        // January: 2025/26 is closed.
        assert_eq!(show(3), "0,00 9,72 0");
        assert_eq!(of(&p.losses, Pool::Year(HolidayYear(2025))), None);
        assert_eq!(of(&p.payouts, Pool::Year(HolidayYear(2025))), None);
    }

    #[test]
    fn what_is_left_on_31_december_is_paid_out_beyond_4_weeks_and_lost_within() {
        let today = d("2026-10-31");
        let y = Pool::Year(HolidayYear(2025));
        // Earned 24,96 (no employment start): beyond 4 weeks is 4,96.
        // 21 days used: the 3,96 left are all beyond 4 weeks, paid out.
        let p = project(
            &vacation_only(None),
            &ferie("2026-06-01", "2026-06-29"),
            today,
        );
        assert_eq!(
            (of(&p.payouts, y), of(&p.losses, y)),
            (Some(Days(396)), None)
        );
        // 14 days used: 4,96 paid out, the 6,00 within 4 weeks lost.
        let p = project(
            &vacation_only(None),
            &ferie("2026-06-01", "2026-06-18"),
            today,
        );
        assert_eq!(
            (of(&p.payouts, y), of(&p.losses, y)),
            (Some(Days(496)), Some(Days(600)))
        );
        assert_eq!(p.losses[0].date, d("2026-12-31"));
        // Employed from 1 May 2026: 8,32 earned, all within 4 weeks, so
        // the 7,32 left are lost.
        let p = project(
            &vacation_only(Some("2026-05-01")),
            &ferie("2026-06-01", "2026-06-01"),
            today,
        );
        assert_eq!(
            (of(&p.payouts, y), of(&p.losses, y)),
            (None, Some(Days(732)))
        );
        // A year whose use-by date is past the projection is not settled.
        assert_eq!(of(&p.losses, Pool::Year(HolidayYear(2027))), None);
        let s = status(&p, &[]);
        assert_eq!(s.kind, StatusKind::Loss);
        assert_eq!(s.headline, "7,32 days are lost on 31 Dec 2026");
        assert_eq!(s.next_step, "Plan 8 more vacation days before 31 Dec 2026");
        assert!(s.warn());
        let text = render_plan(&p, &s);
        assert!(
            text.contains("lost: 7,32 days of 2025/26 on 31 Dec 2026"),
            "{text}"
        );
        let p = project(&rules(None), &ferie("2026-11-02", "2026-11-04"), today);
        let text = render_plan(&p, &status(&p, &[]));
        assert!(
            text.contains("lost: 2 feriefridage of 2026/27 on 30 Apr 2027"),
            "{text}"
        );
        assert!(
            text.contains("paid out: 4,96 days of 2026/27 beyond 4 weeks, after 31 Dec 2027"),
            "{text}"
        );
    }

    #[test]
    fn feriefridage_are_used_first_and_lost_on_30_april() {
        let today = d("2026-10-31");
        let r = rules(None);
        // Six days in November: five feriefridage, then one vacation day.
        let p = project(&r, &ferie("2026-11-02", "2026-11-09"), today);
        let periods = &p.overview.periods;
        assert_eq!(periods[0].feriefridage_left, Some((FridagYear(2026), 0)));
        assert_eq!(
            periods[0].draws,
            [Draw {
                year: HolidayYear(2025),
                days: Days(100),
                left_after: Days(2396),
            }]
        );
        assert_eq!(
            p.overview.feriefridage[0],
            FeriefridageYear {
                year: FridagYear(2026),
                allowance: 5,
                taken: 0,
                planned: 5,
            }
        );
        assert_eq!(p.months[1].feriefridage_used, 5);
        assert_eq!(p.months[1].vacation_used, Days(100));
        assert_eq!(p.months[1].feriefridage_left, 0);
        // None left of 2026/27; next period's five are lost on 30 Apr 2028.
        assert_eq!(of(&p.losses, Pool::Feriefridage(FridagYear(2026))), None);
        assert_eq!(
            of(&p.losses, Pool::Feriefridage(FridagYear(2027))),
            Some(Days(500))
        );
        // Three used: two are lost on 30 Apr 2027.
        let p = project(&r, &ferie("2026-11-02", "2026-11-04"), today);
        let loss = p
            .losses
            .iter()
            .find(|s| s.pool == Pool::Feriefridage(FridagYear(2026)))
            .unwrap();
        assert_eq!((loss.date, loss.days), (d("2027-04-30"), Days(200)));
        // The period starts on 1 May: a day in April belongs to 2025/26.
        assert_eq!(r.feriefridage_year(d("2027-04-30")), FridagYear(2026));
        assert_eq!(r.feriefridage_year(d("2027-05-01")), FridagYear(2027));
        // Not employed in a period: no feriefridage for it.
        let late = rules(Some("2027-06-01"));
        assert_eq!(late.feriefridage_allowance(FridagYear(2026)), 0);
        assert_eq!(late.feriefridage_allowance(FridagYear(2027)), 5);
    }

    #[test]
    fn forskud_over_the_limit_needs_a_form() {
        // Employed from 1 May 2026; 15 days in January 2027 draw on 2026/27,
        // which has 8,32 usable then: the 14th day (21 Jan) goes 5,68 ahead.
        let r = vacation_only(Some("2026-05-01"));
        let p = project(&r, &ferie("2027-01-04", "2027-01-22"), d("2026-10-31"));
        assert_eq!(
            p.forskud,
            Some(Forskud {
                date: d("2027-01-21"),
                days: Days(568),
            })
        );
        // Ten days stay within the limit (1,68 ahead).
        let p = project(&r, &ferie("2027-01-04", "2027-01-15"), d("2026-10-31"));
        assert_eq!(p.forskud, None);
    }

    #[test]
    fn a_draft_can_remove_a_loss() {
        let c = cfg("employment_start = 2026-05-01\nferiefridage = 0");
        let today = d("2026-10-31");
        let y = Pool::Year(HolidayYear(2025));
        let p = plan(&c, &[], today, &Draft::default());
        assert_eq!(of(&p.losses, y), Some(Days(832)));
        assert_eq!(
            status(&p, &[]).headline,
            "8,32 days are lost on 31 Dec 2026"
        );

        // Nine weekdays in November use up 2025/26.
        let draft = Draft {
            add: d("2026-11-02")
                .iter_days()
                .take_while(|x| *x <= d("2026-11-12"))
                .collect(),
            remove: BTreeSet::new(),
        };
        assert_eq!(draft.len(), 11);
        let p = plan(&c, &[], today, &draft);
        assert_eq!(of(&p.losses, y), None);
        assert_eq!(p.overview.periods[0].working_days, 9);
        assert_eq!(p.overview.periods[0].title, "Vacation");
        assert!(status(&p, &[]).headline.ends_with("lost on 31 Dec 2027"));

        // Taking the same dates out drops them.
        let back = Draft {
            add: BTreeSet::new(),
            remove: draft.add.clone(),
        };
        let days = with_draft(&c, &ferie("2026-11-02", "2026-11-12"), &back);
        assert!(days.is_empty());
    }

    #[test]
    fn a_repeat_is_replaced_by_a_calendar_event_in_that_year() {
        let c = cfg("repeats = [{ title = \"Sommerferie\", weeks = [28, 29, 30] }]");
        let today = d("2026-10-31");
        // 2027 has its own Sommerferie in the calendar.
        let calendar = event(
            LeaveKind::Ferie,
            "Sommerferie i Norge",
            "2027-07-05",
            "2027-07-23",
            &[],
        );
        let days = repeat_days(&c, &calendar, today, horizon(today));
        let monday = NaiveDate::from_isoywd_opt(2028, 28, Weekday::Mon).unwrap();
        assert_eq!(days.first().map(|d| d.date), Some(monday));
        assert_eq!(days.len(), 21);
        assert_eq!(days.iter().filter(|d| d.working_day).count(), 15);
        assert!(days.iter().all(|d| d.title == "Sommerferie"));

        let p = plan(&c, &calendar, today, &Draft::default());
        assert_eq!(p.repeat_days.len(), 21);
        assert!(!p.repeat_days.contains(&d("2027-07-05")));
        // Without the event, 2027 repeats too (2026's weeks are past).
        let p = plan(&c, &[], today, &Draft::default());
        assert_eq!(p.repeat_days.len(), 42);
    }

    #[test]
    fn the_christmas_repeat_runs_from_the_monday_before_24_december() {
        let c = cfg("repeats = [{ title = \"Juleferie\", christmas = true }]");
        let days = repeat_days(&c, &[], d("2026-10-31"), d("2029-12-31"));
        let year = |y: i32| -> (Vec<u32>, usize) {
            let of_year: Vec<&LeaveDay> = days.iter().filter(|d| d.date.year() == y).collect();
            (
                of_year.iter().map(|d| d.date.day()).collect(),
                of_year.iter().filter(|d| d.working_day).count(),
            )
        };
        // Thursday 24 Dec 2026: Mon 21 – Wed 23.
        assert_eq!(year(2026), (vec![21, 22, 23], 3));
        // Friday 24 Dec 2027: Mon 20 – Thu 23.
        assert_eq!(year(2027), (vec![20, 21, 22, 23], 4));
        // Sunday 24 Dec 2028: Mon 18 – Sat 23, five working days.
        assert_eq!(year(2028), (vec![18, 19, 20, 21, 22, 23], 5));
        // Monday 24 Dec 2029: the Monday before, 17 – Sun 23.
        assert_eq!(year(2029), (vec![17, 18, 19, 20, 21, 22, 23], 5));
    }

    fn bare_plan(today: NaiveDate) -> Plan {
        Plan {
            today,
            overview: Overview {
                years: vec![],
                periods: vec![],
                feriefridage: vec![],
                expiring: vec![],
            },
            months: vec![],
            losses: vec![],
            payouts: vec![],
            forskud: None,
            max_forskud: Days(500),
            repeat_days: BTreeSet::new(),
        }
    }

    #[test]
    fn status_takes_the_first_match_in_order() {
        let today = d("2026-10-31");
        let mut p = bare_plan(today);
        let s = status(&p, &[]);
        assert_eq!(
            (s.kind, s.headline.as_str(), s.next_step.as_str()),
            (StatusKind::Clear, "Nothing is lost", "Nothing to do now")
        );
        assert!(!s.warn());

        p.overview.periods =
            overview(&rules(None), &ferie("2026-12-14", "2026-12-18"), today).periods;
        let s = status(&p, &[]);
        assert_eq!(s.kind, StatusKind::Register);
        assert_eq!(
            s.next_step,
            "Register Ferie (14 Dec 2026 – 18 Dec 2026) in Danløn, Calendar and Teams"
        );
        let ticked = [("Ferie".to_string(), d("2026-12-14"))];
        assert_eq!(status(&p, &ticked).kind, StatusKind::Clear);

        p.payouts.push(Settlement {
            pool: Pool::Year(HolidayYear(2025)),
            date: d("2026-12-31"),
            days: Days(132),
        });
        assert_eq!(
            status(&p, &[]).next_step,
            "1,32 days beyond 4 weeks are paid out after 31 Dec 2026: take them or agree a transfer by 31 Dec 2026"
        );

        p.forskud = Some(Forskud {
            date: d("2027-01-21"),
            days: Days(568),
        });
        let s = status(&p, &[]);
        assert_eq!(
            (s.kind, s.next_step.as_str()),
            (
                StatusKind::Forskud,
                "Ask management for a forskud form before 21 Jan 2027"
            )
        );

        p.losses.push(Settlement {
            pool: Pool::Feriefridage(FridagYear(2026)),
            date: d("2027-04-30"),
            days: Days(100),
        });
        let s = status(&p, &[]);
        assert_eq!(
            (s.kind, s.headline.as_str(), s.next_step.as_str()),
            (
                StatusKind::Loss,
                "1 feriefridag is lost on 30 Apr 2027",
                "Plan 1 more feriefridag before 30 Apr 2027"
            )
        );
    }
}
