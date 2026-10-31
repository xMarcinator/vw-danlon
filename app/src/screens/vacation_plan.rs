//! What the Vacation screen draws from a [`Plan`]: the 24-month chart, the
//! warnings under it, when each pool expires, the periods to register and
//! the year calendar with its draft. Pure, so it is tested here; the
//! screen in `vacation.rs` turns it into markup.

use std::collections::BTreeMap;

use chrono::{Datelike, Duration, NaiveDate};
use flow_next_charts::config::{
    Annotation, Grid, LabelPosition, LegendOptions, Position, Ticks,
};
use flow_next_charts::{self as charts, ChartConfig, ChartType, ColorRef, Dataset, Scale, Scene};
use flow_next_draw::Pattern;
use vw_danlon::config::{Config, Repeat};
use vw_danlon::holidays;
use vw_danlon::month::LeaveKind;
use vw_danlon::state::{Registered, RegisteredPeriod};
use vw_danlon::vacation::{
    Days, Draft, FridagYear, LeaveDay, Plan, Pool, Rules, Settlement, Status, StatusKind,
};

/// Hours a vacation day stands for in the Danløn app.
const HOURS_PER_DAY: f64 = 7.4;

pub fn date(d: NaiveDate) -> String {
    d.format("%-d %b %Y").to_string()
}

/// "5" for whole days, else "1,32".
pub fn whole(d: Days) -> String {
    if d.0 % 100 == 0 {
        (d.0 / 100).to_string()
    } else {
        d.to_string()
    }
}

fn plural(n: String, one: &str, many: &str) -> String {
    let word = if n == "1" { one } else { many };
    format!("{n} {word}")
}

/// What a settlement is, in words: "6,00 days of 2026/27", "2 feriefridage".
fn amount(s: &Settlement) -> String {
    match s.pool {
        Pool::Year(y) => format!("{} days of {y}", s.days),
        Pool::Feriefridage(_) => plural(whole(s.days), "feriefridag", "feriefridage"),
    }
}

/// The chart's size, as in the mockup; it scales to the panel's width.
pub const W: f32 = 340.0;
pub const H: f32 = 196.0;

/// The next 24 months as a `flow-next-charts` config: days used as bars
/// down from 0 (vacation, then feriefridage, stacked), the balance at month
/// end as a line, and red lines after December and after the month the
/// feriefridage end, labelled with what is lost or paid out there.
#[derive(Debug, Clone, PartialEq)]
pub struct Chart {
    pub config: ChartConfig,
    /// The summary for screen readers.
    pub label: String,
}

impl Chart {
    /// The chart laid out at its mockup size; an error names what the
    /// library refused in the config (a bug here, not bad data).
    pub fn scene(&self) -> Result<Scene, String> {
        let mut chart = charts::Chart::new(self.config.clone()).map_err(|e| e.to_string())?;
        Ok(chart.layout(W, H))
    }
}

/// The dashes of the feriefridage deadline; 31 December is solid.
pub const FRIDAG_DASH: [f32; 2] = [4.0, 3.0];

pub fn chart(plan: &Plan, rules: &Rules) -> Chart {
    let ms = &plan.months;
    let fridag_end = rules.feriefridage_last(FridagYear(2000)).month();
    let labels: Vec<String> = ms
        .iter()
        .map(|m| m.month.first_day().format("%b %y").to_string())
        .collect();
    let vacation: Vec<f64> = ms.iter().map(|m| -(m.vacation_used.0 as f64) / 100.0).collect();
    let fridage: Vec<f64> = ms.iter().map(|m| -f64::from(m.feriefridage_used)).collect();
    let balance: Vec<f64> = ms.iter().map(|m| m.balance.0 as f64 / 100.0).collect();
    // Grid lines every 5 days from a multiple of 5 below the deepest bar to
    // one above the highest balance, so 0 is always one of them.
    let top = balance.iter().copied().fold(5.0, f64::max);
    let bot = (0..ms.len())
        .map(|i| vacation[i] + fridage[i])
        .fold(0.0, f64::min);
    let (top, bot) = ((top / 5.0).ceil() * 5.0, (bot / 5.0).floor() * 5.0);

    let mut annotations = Vec::new();
    for (i, m) in ms.iter().enumerate() {
        let month = m.month.month;
        if month != 12 && month != fridag_end {
            continue;
        }
        let here = |list: &[Settlement]| -> Option<Days> {
            let sum: i64 = list
                .iter()
                .filter(|s| m.month.contains(s.date))
                .map(|s| s.days.0)
                .sum();
            (sum > 0).then_some(Days(sum))
        };
        let mut said = Vec::new();
        if let Some(d) = here(&plan.losses) {
            said.push(format!("−{}", whole(d)));
        }
        if let Some(d) = here(&plan.payouts) {
            said.push(format!("paid {}", whole(d)));
        }
        // The line sits on the month's end: between this category and the next.
        let mut line = Annotation::line(
            m.month.first_day().format("%b-%Y").to_string().to_lowercase(),
            "x",
            i as f64 + 0.5,
        )
        .border_color(ColorRef::role("accent"));
        if month != 12 {
            line = line.border_dash(FRIDAG_DASH);
        }
        if !said.is_empty() {
            line = line.with_label(said.join(" · "), LabelPosition::End);
        }
        annotations.push(line);
    }

    let config = ChartConfig::bar()
        .labels(labels)
        .dataset(
            Dataset::new("Vacation days", vacation)
                .stack("spent")
                .background(ColorRef::role("series.1"))
                .pattern(Pattern::Plain)
                .order(2),
        )
        .dataset(
            Dataset::new("Feriefridage", fridage)
                .stack("spent")
                .background(ColorRef::role("series.2"))
                .pattern(Pattern::Plain)
                .order(2),
        )
        .dataset(
            Dataset::new("Balance", balance)
                .kind(ChartType::Line)
                .border(ColorRef::role("series.3"))
                .border_width(2.0)
                .border_dash(Vec::new())
                .point_radius(0.0)
                .order(0),
        )
        .scale(
            "x",
            Scale::category()
                .stacked(true)
                .ticks(Ticks::default().max_rotation(0.0).max_ticks_limit(8))
                .grid(Grid {
                    display: false,
                    ..Grid::default()
                }),
        )
        .scale(
            "y",
            Scale::linear()
                .stacked(true)
                .min(bot)
                .max(top)
                .ticks(Ticks::default().step_size(5.0).auto_skip(false))
                .grid(Grid {
                    border_dash: vec![2.0, 3.0],
                    ..Grid::default()
                }),
        )
        .legend(LegendOptions {
            display: false,
            position: Position::Top,
        })
        .annotations(annotations);

    let mut said: Vec<String> = plan
        .losses
        .iter()
        .map(|l| format!("{} lost {}", amount(l), date(l.date)))
        .collect();
    said.extend(
        plan.payouts
            .iter()
            .map(|p| format!("{} paid out after {}", amount(p), date(p.date))),
    );
    let label = format!(
        "Vacation over the next 24 months: days used per month as bars, the balance as a line, deadlines in red. {}.",
        if said.is_empty() {
            "Nothing is lost".to_string()
        } else {
            said.join(", ")
        }
    );
    Chart { config, label }
}

/// The warnings under the chart: every loss and payout the status line does
/// not already name, and a forskud over the limit.
pub fn warnings(plan: &Plan, status: &Status) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(f) = plan.forskud
        && status.kind != StatusKind::Forskud
    {
        out.push(format!(
            "{} days on forskud on {}: more than {} needs a form signed by management.",
            f.days,
            date(f.date),
            whole(plan.max_forskud)
        ));
    }
    let named_loss = (status.kind == StatusKind::Loss) as usize;
    for l in plan.losses.iter().skip(named_loss) {
        let verb = if l.days.0 == 100 { "is" } else { "are" };
        out.push(format!("{} {verb} lost {}.", amount(l), date(l.date)));
    }
    let named_payout = (status.kind == StatusKind::Payout) as usize;
    for p in plan.payouts.iter().skip(named_payout) {
        out.push(format!(
            "{} beyond 4 weeks are paid out after {}.",
            amount(p),
            date(p.date)
        ));
    }
    out
}

/// A row of the table of when each pool expires.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolRow {
    /// The holiday year, for Danløn's balance; `None` for feriefridage.
    pub year: Option<vw_danlon::vacation::HolidayYear>,
    pub name: String,
    /// "−6,00", else "0 lost".
    pub lost: Option<String>,
    pub paid: Option<String>,
    pub sub: String,
}

/// Each holiday year and feriefridage period with a deadline in the
/// projection: what is lost and paid out then, its last day and the
/// periods that use it.
pub fn pools(plan: &Plan, rules: &Rules) -> Vec<PoolRow> {
    let end = plan
        .months
        .last()
        .map_or(plan.today, |m| m.month.last_day());
    let mut pools: Vec<(Pool, NaiveDate)> = Vec::new();
    let first = vw_danlon::vacation::open_years(plan.today)[0];
    for y in first.0..=first.0 + 3 {
        let year = vw_danlon::vacation::HolidayYear(y);
        let by = year.use_by();
        if by >= plan.today && by <= end && rules.earning_start(year).is_some() {
            pools.push((Pool::Year(year), by));
        }
    }
    let f0 = rules.feriefridage_year(plan.today);
    for y in f0.0..=f0.0 + 2 {
        let year = FridagYear(y);
        let by = rules.feriefridage_last(year);
        if by >= plan.today && by <= end && rules.feriefridage_allowance(year) > 0 {
            pools.push((Pool::Feriefridage(year), by));
        }
    }
    pools
        .into_iter()
        .map(|(pool, by)| {
            let find = |list: &[Settlement]| list.iter().find(|s| s.pool == pool).map(|s| s.days);
            let uses: Vec<String> = plan
                .overview
                .periods
                .iter()
                .filter_map(|p| {
                    let d = plan.used_of(pool, p.first, p.last);
                    (d.0 > 0).then(|| match pool {
                        Pool::Year(_) => format!("{} {d}", p.title),
                        Pool::Feriefridage(_) => format!("{} {}", p.title, whole(d)),
                    })
                })
                .collect();
            PoolRow {
                year: match pool {
                    Pool::Year(y) => Some(y),
                    Pool::Feriefridage(_) => None,
                },
                name: match pool {
                    Pool::Year(y) => y.to_string(),
                    Pool::Feriefridage(y) => format!("Feriefridage {y}"),
                },
                lost: find(&plan.losses).map(|d| format!("−{}", whole_or_days(pool, d))),
                paid: find(&plan.payouts).map(|d| format!("{d} paid out")),
                sub: format!(
                    "use by {} · {}",
                    date(by),
                    if uses.is_empty() {
                        "nothing planned yet".to_string()
                    } else {
                        uses.join(", ")
                    }
                ),
            }
        })
        .collect()
}

fn whole_or_days(pool: Pool, d: Days) -> String {
    match pool {
        Pool::Year(_) => d.to_string(),
        Pool::Feriefridage(_) => whole(d),
    }
}

/// Days typed as "8,50", "8.5" or "8" in hundredths; `None` if it is not a
/// number of days with at most two decimals.
pub fn parse_days(text: &str) -> Option<i64> {
    let t = text.trim().replace(',', ".");
    let (neg, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest.to_string()),
        None => (false, t),
    };
    let (whole, frac) = t.split_once('.').unwrap_or((&t, ""));
    if whole.is_empty() && frac.is_empty()
        || frac.len() > 2
        || !whole
            .chars()
            .chain(frac.chars())
            .all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let frac: i64 = format!("{frac:0<2}").parse().ok()?;
    let n = whole * 100 + frac;
    Some(if neg { -n } else { n })
}

/// How Danløn's balance compares with the app's on the date it was read:
/// "Danløn 8,50 · app 8,32 on 31 Oct 2026" and "0,18 apart" (or "same as
/// the app").
pub fn danlon_comparison(danlon: Days, app: Days, read_on: NaiveDate) -> (String, String, bool) {
    let diff = (danlon.0 - app.0).abs();
    (
        format!("Danløn {danlon} · app {app} on {}", date(read_on)),
        if diff == 0 {
            "same as the app".to_string()
        } else {
            format!("{} apart", Days(diff))
        },
        diff != 0,
    )
}

/// A calendar period starting in the next 60 days, with its ticks.
#[derive(Debug, Clone, PartialEq)]
pub struct Registration {
    pub title: String,
    pub first: NaiveDate,
    pub last: NaiveDate,
    pub working_days: u32,
    pub ticks: Registered,
}

impl Registration {
    /// "Danløn 37,0 h": the hours to enter.
    pub fn hours(&self) -> String {
        format!("{:.1}", f64::from(self.working_days) * HOURS_PER_DAY).replace('.', ",")
    }
}

pub fn registrations(plan: &Plan, registered: &[RegisteredPeriod]) -> Vec<Registration> {
    let soon = plan.today + Duration::days(vw_danlon::vacation::REGISTER_WITHIN_DAYS);
    plan.overview
        .periods
        .iter()
        .filter(|p| {
            p.working_days > 0
                && p.first > plan.today
                && p.first <= soon
                && !plan.repeat_days.contains(&p.first)
        })
        .map(|p| Registration {
            title: p.title.clone(),
            first: p.first,
            last: p.last,
            working_days: p.working_days,
            ticks: registered
                .iter()
                .find(|r| r.title == p.title && r.first == p.first)
                .map(|r| r.ticks)
                .unwrap_or_default(),
        })
        .collect()
}

/// `list` with the ticks of one period replaced; none ticked drops it.
pub fn with_ticks(
    list: &[RegisteredPeriod],
    title: &str,
    first: NaiveDate,
    ticks: Registered,
) -> Vec<RegisteredPeriod> {
    let mut out: Vec<RegisteredPeriod> = list
        .iter()
        .filter(|r| !(r.title == title && r.first == first))
        .cloned()
        .collect();
    if ticks != Registered::default() {
        out.push(RegisteredPeriod {
            title: title.into(),
            first,
            ticks,
        });
    }
    out
}

/// "Repeats every year: week 7, weeks 28–30 and the week before Christmas."
pub fn repeats_note(repeats: &[Repeat]) -> Option<String> {
    let parts: Vec<String> = repeats
        .iter()
        .flat_map(|r| {
            let mut weeks = r.weeks.clone();
            weeks.sort_unstable();
            let mut runs: Vec<(u32, u32)> = Vec::new();
            for w in weeks {
                match runs.last_mut() {
                    Some((_, b)) if *b + 1 == w => *b = w,
                    _ => runs.push((w, w)),
                }
            }
            let mut out: Vec<String> = runs
                .into_iter()
                .map(|(a, b)| {
                    if a == b {
                        format!("week {a}")
                    } else {
                        format!("weeks {a}–{b}")
                    }
                })
                .collect();
            if r.christmas {
                out.push("the week before Christmas".into());
            }
            out
        })
        .collect();
    let list = match parts.as_slice() {
        [] => return None,
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    Some(format!(
        "Repeats every year: {list}. A vacation event in the calendar with the same title replaces that year's."
    ))
}

/// What a day of the year calendar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Plain,
    Weekend,
    Holiday,
    /// A paid day off from the contract (Grundlovsdag, juleaften, nytårsaften).
    AciesDayOff,
    Vacation,
    Repeat,
    Feriefridag,
    Add,
    Remove,
}

impl CellKind {
    pub fn class(self) -> &'static str {
        match self {
            CellKind::Plain => "cal-day",
            CellKind::Weekend => "cal-day we",
            CellKind::Holiday => "cal-day hol",
            CellKind::AciesDayOff => "cal-day acies",
            CellKind::Vacation => "cal-day ferie",
            CellKind::Repeat => "cal-day repeat",
            CellKind::Feriefridag => "cal-day fri",
            CellKind::Add => "cal-day add",
            CellKind::Remove => "cal-day del",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub date: NaiveDate,
    pub kind: CellKind,
    pub label: String,
    pub tappable: bool,
    pub past: bool,
    pub today: bool,
    /// What tapping does: true when the day is planned (the tap removes it).
    pub planned: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalMonth {
    pub name: String,
    /// ISO week number and the seven days, Monday first; `None` outside
    /// the month.
    pub weeks: Vec<(u32, [Option<Cell>; 7])>,
}

/// A planned day in the calendar: its kind, title and whether it is a repeat.
pub type Planned = BTreeMap<NaiveDate, (LeaveKind, String, bool)>;

/// The calendar's leave days and the repeats by date.
pub fn planned(calendar: &[LeaveDay], repeats: &[LeaveDay]) -> Planned {
    let mut out: Planned = repeats
        .iter()
        .map(|d| (d.date, (d.kind, d.title.clone(), true)))
        .collect();
    for d in calendar {
        out.insert(d.date, (d.kind, d.title.clone(), false));
    }
    out
}

const ACIES: [&str; 3] = ["Grundlovsdag", "Juleaftensdag", "Nytårsaftensdag"];

/// January – December of `year`, Monday first, with ISO week numbers.
pub fn year_calendar(
    cfg: &Config,
    year: i32,
    today: NaiveDate,
    planned: &Planned,
    draft: &Draft,
) -> Vec<CalMonth> {
    let off: BTreeMap<NaiveDate, &str> = holidays::days_off(year, &cfg.holidays)
        .into_iter()
        .collect();
    (1..=12)
        .map(|m| {
            let first = NaiveDate::from_ymd_opt(year, m, 1).unwrap();
            let mut day = first - Duration::days(first.weekday().num_days_from_monday().into());
            let mut weeks = Vec::new();
            loop {
                let week = day.iso_week().week();
                let cells: [Option<Cell>; 7] = std::array::from_fn(|i| {
                    let date = day + Duration::days(i as i64);
                    (date.month() == m).then(|| {
                        let mut label = date.format("%-d %b %Y").to_string();
                        let weekend = cfg.work_week.hours(date.weekday()) <= 0.0;
                        let name = off.get(&date).copied();
                        let plan = planned.get(&date);
                        let kind = if weekend {
                            CellKind::Weekend
                        } else if let Some(n) = name {
                            if ACIES.contains(&n) {
                                label.push_str(&format!(", {n} (Acies)"));
                                CellKind::AciesDayOff
                            } else {
                                label.push_str(&format!(", {n}"));
                                CellKind::Holiday
                            }
                        } else if draft.add.contains(&date) {
                            label.push_str(", draft: add");
                            CellKind::Add
                        } else if draft.remove.contains(&date) {
                            label.push_str(", draft: remove");
                            CellKind::Remove
                        } else if let Some((kind, title, repeat)) = plan {
                            label.push_str(&format!(", {title}"));
                            match (kind, repeat) {
                                (LeaveKind::Feriefridag, _) => CellKind::Feriefridag,
                                (_, true) => CellKind::Repeat,
                                _ => CellKind::Vacation,
                            }
                        } else {
                            CellKind::Plain
                        };
                        let past = date <= today;
                        Cell {
                            date,
                            kind,
                            label,
                            tappable: !past && !weekend && name.is_none(),
                            past,
                            today: date == today,
                            planned: plan.is_some(),
                        }
                    })
                });
                weeks.push((week, cells));
                day += Duration::days(7);
                if day.month() != m || day.year() != year {
                    break;
                }
            }
            CalMonth {
                name: first.format("%B").to_string(),
                weeks,
            }
        })
        .collect()
}

/// Tapping a day: a change in the draft is undone, a planned day is taken
/// out, a free day is added.
pub fn toggle(draft: &mut Draft, cell: &Cell) {
    let d = cell.date;
    if !draft.add.remove(&d) && !draft.remove.remove(&d) {
        if cell.planned {
            draft.remove.insert(d);
        } else {
            draft.add.insert(d);
        }
    }
}

/// The draft as text to copy into the calendar.
pub fn draft_text(draft: &Draft) -> String {
    let list = |dates: &std::collections::BTreeSet<NaiveDate>| {
        dates
            .iter()
            .map(|d| date(*d))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut out = vec!["Vacation changes".to_string()];
    if !draft.add.is_empty() {
        out.push(format!("Add all-day “Vacation”: {}", list(&draft.add)));
    }
    if !draft.remove.is_empty() {
        out.push(format!("Remove: {}", list(&draft.remove)));
    }
    out.join("\n")
}

/// The calendar year the Vacation calendar shows for a holiday year: this
/// year while the holiday year holds today, else the year it starts in.
pub fn calendar_year(shown: vw_danlon::vacation::HolidayYear, today: NaiveDate) -> i32 {
    if vw_danlon::vacation::HolidayYear::containing(today) == shown {
        today.year()
    } else {
        shown.0
    }
}

/// Monday-first weekday initials for the calendar head.
pub const WEEKDAYS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];

#[cfg(test)]
mod tests {
    use super::*;
    use flow_next_charts::DataPoint;
    use vw_danlon::vacation::{self, HolidayYear};

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn cfg(vacation: &str) -> Config {
        Config::parse(&format!(
            "employee_number = \"1\"\n[danlon]\n[vacation]\n{vacation}\n"
        ))
        .unwrap()
    }

    fn leave(title: &str, first: &str, last: &str) -> Vec<LeaveDay> {
        d(first)
            .iter_days()
            .take_while(|x| *x <= d(last))
            .map(|date| LeaveDay {
                date,
                kind: LeaveKind::Ferie,
                title: title.into(),
                working_day: date.weekday().num_days_from_monday() < 5,
            })
            .collect()
    }

    #[test]
    fn the_chart_stacks_spent_days_under_zero_with_a_line_and_deadlines() {
        let c = cfg("employment_start = 2026-05-01\nferiefridage = 0");
        let today = d("2026-10-31");
        let p = vacation::plan(
            &c,
            &leave("Juleferie", "2026-12-14", "2026-12-18"),
            today,
            &Draft::default(),
        );
        let ch = chart(&p, &vacation::Rules::from_config(&c));
        let ds = &ch.config.data.datasets;
        assert_eq!(ds.len(), 3);
        assert_eq!(ch.config.data.labels[0], "Oct 26");
        // December's bar goes down 5 days; the two bar datasets share a stack.
        assert_eq!(ds[0].data[2], DataPoint::from(-5.0));
        assert_eq!(ds[0].stack.as_deref(), Some("spent"));
        assert_eq!(ds[1].stack.as_deref(), Some("spent"));
        assert_eq!(ds[2].kind, Some(ChartType::Line));
        assert_eq!(ds[2].data.len(), 24);
        // Deadlines after every December (solid) and April (dashed), on the
        // month's end: half a category past it.
        let lines = &ch.config.options.plugins.as_ref().unwrap().annotation.as_ref().unwrap().annotations;
        let dashed: Vec<bool> = lines.iter().map(|l| !l.border_dash.is_empty()).collect();
        assert_eq!(dashed, [false, true, false, true]);
        assert_eq!(lines[0].id, "dec-2026");
        assert_eq!(lines[0].value, Some(2.5));
        // 3,32 of 2025/26's 8,32 are left on 31 Dec 2026, all lost.
        assert_eq!(lines[0].label.as_ref().map(|l| l.content.as_str()), Some("−3,32"));
        assert!(lines[1].label.is_none());
        assert!(
            ch.label.contains("3,32 days of 2025/26 lost 31 Dec 2026"),
            "{}",
            ch.label
        );
        // The library takes the config and draws the deadline lines.
        let scene = ch.scene().unwrap();
        let svg = scene.to_svg(&charts::ChartTheme::fallback());
        assert!(svg.contains("annotation-dec-2026"), "{svg}");
        assert!(svg.contains("dataset-2"), "{svg}");
    }

    /// `cargo test -p vw-danlon-app dump_chart -- --ignored` writes the chart
    /// in the library's fallback colours to $VW_CHART_SVG for a look.
    #[test]
    #[ignore]
    fn dump_chart_svg() {
        let c = cfg("employment_start = 2026-05-01\nferiefridage = 5");
        let mut cal = leave("Juleferie", "2026-12-14", "2026-12-18");
        cal.extend(leave("Sommer", "2027-07-05", "2027-07-23"));
        let p = vacation::plan(&c, &cal, d("2026-10-31"), &Draft::default());
        let ch = chart(&p, &vacation::Rules::from_config(&c));
        let svg = ch.scene().unwrap().to_svg(&charts::ChartTheme::fallback());
        std::fs::write(std::env::var("VW_CHART_SVG").unwrap(), svg).unwrap();
    }

    #[test]
    fn warnings_skip_what_the_status_says() {
        let c = cfg("employment_start = 2026-05-01\nferiefridage = 0");
        let p = vacation::plan(&c, &[], d("2026-10-31"), &Draft::default());
        let s = vacation::status(&p, &[]);
        assert_eq!(s.kind, StatusKind::Loss);
        let w = warnings(&p, &s);
        assert_eq!(
            w,
            [
                "20,00 days of 2026/27 are lost 31 Dec 2027.",
                "4,96 days of 2026/27 beyond 4 weeks are paid out after 31 Dec 2027."
            ]
        );
        let rows = pools(&p, &vacation::Rules::from_config(&c));
        let r: Vec<(&str, Option<&str>, Option<&str>)> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.lost.as_deref(), r.paid.as_deref()))
            .collect();
        assert_eq!(
            r,
            [
                ("2025/26", Some("−8,32"), None),
                ("2026/27", Some("−20,00"), Some("4,96 paid out")),
            ]
        );
        assert_eq!(rows[0].sub, "use by 31 Dec 2026 · nothing planned yet");
    }

    #[test]
    fn registrations_list_calendar_periods_in_the_next_60_days() {
        let c = cfg("repeats = [{ title = \"Juleferie\", christmas = true }]");
        let today = d("2026-10-31");
        let mut cal = leave("Ferie", "2026-11-16", "2026-11-20");
        cal.extend(leave("Vinterferie", "2027-02-15", "2027-02-19"));
        let p = vacation::plan(&c, &cal, today, &Draft::default());
        let ticked = vec![RegisteredPeriod {
            title: "Ferie".into(),
            first: d("2026-11-16"),
            ticks: Registered {
                danlon: true,
                calendar: false,
                teams: false,
            },
        }];
        let regs = registrations(&p, &ticked);
        // The Christmas repeat is not in the calendar; February is too far.
        assert_eq!(regs.len(), 1);
        assert_eq!(regs[0].hours(), "37,0");
        assert!(regs[0].ticks.danlon && !regs[0].ticks.teams);
        let all = Registered {
            danlon: true,
            calendar: true,
            teams: true,
        };
        let list = with_ticks(&ticked, "Ferie", d("2026-11-16"), all);
        assert_eq!(list.len(), 1);
        assert!(list[0].ticks.all());
        assert!(with_ticks(&list, "Ferie", d("2026-11-16"), Registered::default()).is_empty());
    }

    #[test]
    fn the_year_calendar_marks_days_and_taps_into_the_draft() {
        let c = cfg("repeats = [{ title = \"Juleferie\", christmas = true }]");
        let today = d("2026-10-31");
        let cal = leave("Ferie", "2026-11-16", "2026-11-20");
        let reps = vacation::repeat_days(&c, &cal, today, vacation::horizon(today));
        let plan = planned(&cal, &reps);
        let mut draft = Draft::default();
        let months = year_calendar(&c, 2026, today, &plan, &draft);
        assert_eq!(months.len(), 12);
        let cell = |months: &[CalMonth], s: &str| -> Cell {
            let x = d(s);
            months[x.month0() as usize]
                .weeks
                .iter()
                .flat_map(|(_, w)| w.iter().flatten())
                .find(|c| c.date == x)
                .cloned()
                .unwrap()
        };
        // 1 January 2026 is a Thursday in ISO week 1.
        assert_eq!(months[0].weeks[0].0, 1);
        assert!(months[0].weeks[0].1[0].is_none());
        assert_eq!(cell(&months, "2026-01-01").kind, CellKind::Holiday);
        assert_eq!(cell(&months, "2026-06-05").kind, CellKind::AciesDayOff);
        assert_eq!(cell(&months, "2026-11-21").kind, CellKind::Weekend);
        assert_eq!(cell(&months, "2026-11-16").kind, CellKind::Vacation);
        assert_eq!(cell(&months, "2026-12-21").kind, CellKind::Repeat);
        let past = cell(&months, "2026-10-30");
        assert!(past.past && !past.tappable);
        assert!(cell(&months, "2026-10-31").today);

        // A free day is added, a planned one taken out, a second tap undoes.
        toggle(&mut draft, &cell(&months, "2026-11-23"));
        toggle(&mut draft, &cell(&months, "2026-11-16"));
        let months = year_calendar(&c, 2026, today, &plan, &draft);
        assert_eq!(cell(&months, "2026-11-23").kind, CellKind::Add);
        assert_eq!(cell(&months, "2026-11-16").kind, CellKind::Remove);
        assert_eq!(
            draft_text(&draft),
            "Vacation changes\nAdd all-day “Vacation”: 23 Nov 2026\nRemove: 16 Nov 2026"
        );
        toggle(&mut draft, &cell(&months, "2026-11-23"));
        assert!(draft.add.is_empty() && draft.len() == 1);
    }

    #[test]
    fn the_calendar_year_follows_today_in_the_current_holiday_year() {
        assert_eq!(calendar_year(HolidayYear(2026), d("2026-10-31")), 2026);
        assert_eq!(calendar_year(HolidayYear(2026), d("2027-03-01")), 2027);
        assert_eq!(calendar_year(HolidayYear(2027), d("2026-10-31")), 2027);
    }

    #[test]
    fn danlon_balances_parse_and_compare() {
        assert_eq!(parse_days("8,50"), Some(850));
        assert_eq!(parse_days(" 8.5 "), Some(850));
        assert_eq!(parse_days("8"), Some(800));
        assert_eq!(parse_days(",25"), Some(25));
        assert_eq!(parse_days("-1,5"), Some(-150));
        for bad in ["", "8,505", "8 dage", "x", ",", "1.2.3"] {
            assert_eq!(parse_days(bad), None, "{bad}");
        }
        assert_eq!(
            danlon_comparison(Days(850), Days(832), d("2026-10-31")),
            (
                "Danløn 8,50 · app 8,32 on 31 Oct 2026".to_string(),
                "0,18 apart".to_string(),
                true
            )
        );
        assert_eq!(
            danlon_comparison(Days(832), Days(832), d("2026-10-31")).1,
            "same as the app"
        );
    }

    #[test]
    fn the_repeats_note_lists_weeks_as_runs() {
        let c = cfg(
            "repeats = [{ title = \"V\", weeks = [7] }, { title = \"S\", weeks = [30, 28, 29] }, \
             { title = \"J\", christmas = true }]",
        );
        assert_eq!(
            repeats_note(&c.vacation.repeats).unwrap(),
            "Repeats every year: week 7, weeks 28–30 and the week before Christmas. \
             A vacation event in the calendar with the same title replaces that year's."
        );
        assert_eq!(repeats_note(&[]), None);
    }
}

