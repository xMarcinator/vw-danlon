//! The period selector the pages share: ‹ the period's title and a sub line
//! ›, then the views to choose from (Day · Week · Month on Hours, Week ·
//! Month on Mileage and Danløn) and Today, which is enabled only when
//! another period is shown; Vacation has Month · Year. A page that steps
//! whole years without views (Mileage → Fradrag) shows a note instead. On a phone the title heads
//! the page and the arrows and views sit in a dock above the tab bar. And
//! each page's period: the view and the date it is anchored on, kept while
//! the app runs.

use chrono::{Datelike, Duration, NaiveDate};
use dioxus::prelude::*;
use vw_danlon::month::Month;

use super::month::{month_title, step};
use super::week::{
    first_week, monday_of, month_of, sunday_of, week_months, week_range, week_title,
};
use crate::Icon;

/// What a page shows: a day, an ISO week, a month or a calendar year.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Day,
    Week,
    Month,
    Year,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Day => "Day",
            View::Week => "Week",
            View::Month => "Month",
            View::Year => "Year",
        }
    }

    /// The unit the arrows step, for their labels: "Previous week".
    pub fn unit(self) -> &'static str {
        match self {
            View::Day => "day",
            View::Week => "week",
            View::Month => "month",
            View::Year => "year",
        }
    }
}

/// The views of Hours.
pub const HOURS_VIEWS: [View; 3] = [View::Day, View::Week, View::Month];
/// The views of Mileage and Danløn.
pub const WEEK_MONTH: [View; 2] = [View::Week, View::Month];
/// The views of Vacation.
pub const MONTH_YEAR: [View; 2] = [View::Month, View::Year];

/// The period a page shows: the view and a date in it (the day, the week's
/// Monday, the month's first day or 1 January).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    pub view: View,
    pub anchor: NaiveDate,
}

impl Period {
    /// The month of `today`, as Hours opens.
    pub fn new(today: NaiveDate) -> Self {
        Period {
            view: View::Month,
            anchor: month_of(today).first_day(),
        }
    }

    /// The month the period is loaded from: the day's, the Monday's or the
    /// month shown.
    pub fn month(&self) -> Month {
        month_of(self.anchor)
    }

    /// How a page's text names the period: "Thursday 19 Nov", "week 48",
    /// "November 2026".
    pub fn name(&self) -> String {
        match self.view {
            View::Day => self.anchor.format("%A %-d %b").to_string(),
            View::Week => format!("week {}", self.monday().iso_week().week()),
            View::Month => month_title(self.month()),
            View::Year => self.anchor.year().to_string(),
        }
    }

    /// The Monday of the week shown (in Week view).
    pub fn monday(&self) -> NaiveDate {
        monday_of(self.anchor)
    }

    /// The first and the last day shown.
    pub fn days(&self) -> (NaiveDate, NaiveDate) {
        match self.view {
            View::Day => (self.anchor, self.anchor),
            View::Week => (self.monday(), sunday_of(self.monday())),
            View::Month => (self.month().first_day(), self.month().last_day()),
            View::Year => (new_year(self.anchor.year()), year_end(self.anchor.year())),
        }
    }

    pub fn contains(&self, d: NaiveDate) -> bool {
        let (first, last) = self.days();
        first <= d && d <= last
    }

    /// The months the period's days come from: its month, and the week's
    /// Sunday's when that is another one.
    pub fn months(&self) -> (Month, Option<Month>) {
        match self.view {
            View::Week => week_months(self.monday()),
            View::Day | View::Month | View::Year => (self.month(), None),
        }
    }

    /// Whether the period shown holds `today`.
    pub fn is_current(&self, today: NaiveDate) -> bool {
        match self.view {
            View::Day => self.anchor == today,
            View::Week => self.monday() == monday_of(today),
            View::Month => self.month() == month_of(today),
            View::Year => self.anchor.year() == today.year(),
        }
    }

    /// The period `by` units before (negative) or after.
    pub fn step(self, by: i32) -> Self {
        let anchor = match self.view {
            View::Day => self.anchor + Duration::days(i64::from(by)),
            View::Week => self.monday() + Duration::days(7 * i64::from(by)),
            View::Month => step(self.month(), by).first_day(),
            View::Year => new_year(self.anchor.year() + by),
        };
        Period { anchor, ..self }
    }

    /// The current period, in the same view.
    pub fn today(self, today: NaiveDate) -> Self {
        let anchor = match self.view {
            View::Day => today,
            View::Week => monday_of(today),
            View::Month => month_of(today).first_day(),
            View::Year => new_year(today.year()),
        };
        Period { anchor, ..self }
    }

    /// Another view of the period shown: the one holding today when the
    /// period does, else the first one starting in it (the first day, or the
    /// first week starting in the month), so switching back returns to it.
    pub fn pick(self, to: View, today: NaiveDate) -> Self {
        if to == self.view {
            return self;
        }
        let holds_today = self.is_current(today);
        let anchor = match to {
            View::Day if holds_today => today,
            View::Day => match self.view {
                View::Week => self.monday(),
                _ => self.month().first_day(),
            },
            View::Week if holds_today => monday_of(today),
            View::Week => match self.view {
                View::Day => monday_of(self.anchor),
                _ => first_week(self.month()),
            },
            // From a year: today's month in this year, else January.
            View::Month if holds_today && self.view == View::Year => month_of(today).first_day(),
            View::Month => self.month().first_day(),
            View::Year => new_year(self.anchor.year()),
        };
        Period { view: to, anchor }
    }

    /// One day, in Day view.
    pub fn day(date: NaiveDate) -> Self {
        Period {
            view: View::Day,
            anchor: date,
        }
    }

    /// The selector's title and sub line: "Today" / "Thursday 26 Nov 2026",
    /// "Thursday 19 Nov" / "Week 47 · 2026", "Week 48" / "23–29 Nov 2026",
    /// "November 2026" / "This month".
    pub fn title(&self, today: NaiveDate) -> (String, String) {
        let current = self.is_current(today);
        match self.view {
            View::Day if current => (
                "Today".to_string(),
                self.anchor.format("%A %-d %b %Y").to_string(),
            ),
            View::Day => {
                let week = self.anchor.iso_week();
                (
                    self.anchor.format("%A %-d %b").to_string(),
                    format!("Week {} · {}", week.week(), week.year()),
                )
            }
            View::Week => (week_title(self.monday()), week_range(self.monday())),
            View::Month => (
                month_title(self.month()),
                if current {
                    "This month".to_string()
                } else {
                    String::new()
                },
            ),
            View::Year => calendar_year_title(self.anchor.year(), today),
        }
    }
}

/// 1 January of `year`.
fn new_year(year: i32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, 1, 1).expect("1 January exists")
}

/// 31 December of `year`.
fn year_end(year: i32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, 12, 31).expect("31 December exists")
}

/// The calendar icon of the Today button, with the day marked.
const TODAY_ICON: &str = "M3 5h18v16H3zM3 10h18M8 3v4M16 3v4M12 14v3";

/// A page's period selector. `views` are the choices of the switch; with
/// none, `note` is shown in its place (a page that steps whole years).
/// `unit` is what the arrows step, for their labels.
///
/// Wide, [`Bar::top`] is the card at the top of the page. On a phone the
/// top is the page header (the title; tapping it goes back to the current
/// period) and the arrows and the switch are [`Bar::row`], in the page's
/// [`dock`] above the tab bar. Both are rendered; the width picks one.
#[derive(Clone, PartialEq)]
pub struct Bar {
    pub title: String,
    pub sub: String,
    pub unit: &'static str,
    pub views: Vec<View>,
    pub shown: Option<View>,
    pub note: Option<&'static str>,
    pub current: bool,
    pub on_step: Callback<i32>,
    pub on_pick: Callback<View>,
    pub on_today: Callback<()>,
}

impl Bar {
    /// The selector over `period` with the choice of `views`; `on_change`
    /// gets each new period.
    pub fn picker(
        period: Period,
        today: NaiveDate,
        views: &[View],
        on_change: Callback<Period>,
    ) -> Self {
        let (title, sub) = period.title(today);
        Bar {
            title,
            sub,
            unit: period.view.unit(),
            views: views.to_vec(),
            shown: Some(period.view),
            note: None,
            current: period.is_current(today),
            on_step: Callback::new(move |by: i32| on_change.call(period.step(by))),
            on_pick: Callback::new(move |to: View| on_change.call(period.pick(to, today))),
            on_today: Callback::new(move |_| on_change.call(period.today(today))),
        }
    }

    /// The selector of a page that steps whole years: `note` takes the
    /// place of the views.
    pub fn years(
        (title, sub): (String, String),
        unit: &'static str,
        note: &'static str,
        current: bool,
        on_step: Callback<i32>,
        on_today: Callback<()>,
    ) -> Self {
        Bar {
            title,
            sub,
            unit,
            views: Vec::new(),
            shown: None,
            note: Some(note),
            current,
            on_step,
            on_pick: Callback::new(|_| {}),
            on_today,
        }
    }

    /// The top of the page: the selector card when wide, the page header
    /// when narrow. `extra` sits beside the card (wide only).
    pub fn top(&self, extra: Element) -> Element {
        let Bar {
            title,
            sub,
            unit,
            current,
            on_step,
            on_today,
            ..
        } = self.clone();
        rsx! {
            div { class: "period-head",
                nav { class: "period-bar", "aria-label": "Period",
                    div { class: "period-row",
                        button {
                            r#type: "button",
                            class: "period-arrow",
                            "aria-label": "Previous {unit}",
                            onclick: move |_| on_step.call(-1),
                            Icon { d: "m15 6-6 6 6 6", size: 20 }
                        }
                        div { class: "period-title",
                            h1 { "{title}" }
                            span { class: "period-sub", "{sub}" }
                        }
                        button {
                            r#type: "button",
                            class: "period-arrow",
                            "aria-label": "Next {unit}",
                            onclick: move |_| on_step.call(1),
                            Icon { d: "m9 6 6 6-6 6", size: 20 }
                        }
                    }
                    div { class: "period-row2",
                        {self.switch("period-seg")}
                        button {
                            r#type: "button",
                            class: "period-today",
                            "aria-label": "Back to this {unit}",
                            disabled: current,
                            onclick: move |_| on_today.call(()),
                            Icon { d: TODAY_ICON, size: 16 }
                            "Today"
                        }
                    }
                }
                {extra}
                // The phone's header: the title goes back to the current
                // period, and does nothing when it is shown.
                div { class: "page-head",
                    h1 {
                        button {
                            r#type: "button",
                            class: "page-title",
                            disabled: current,
                            title: if !current { "Back to this {unit}" },
                            onclick: move |_| on_today.call(()),
                            "{title}"
                            if !current {
                                Icon { d: TODAY_ICON, size: 18 }
                            }
                        }
                    }
                    span { class: "page-sub", "{sub}" }
                }
            }
        }
    }

    /// The views to choose from, or the note of a page that steps years.
    fn switch(&self, class: &'static str) -> Element {
        let on_pick = self.on_pick;
        let shown = self.shown;
        rsx! {
            if self.views.is_empty() {
                span { class: "period-note", {self.note.unwrap_or_default()} }
            } else {
                div {
                    class: "segmented {class}",
                    style: "grid-template-columns: repeat({self.views.len()}, minmax(0, 1fr))",
                    role: "radiogroup",
                    "aria-label": "Show by",
                    for v in self.views.iter().copied() {
                        button {
                            r#type: "button",
                            role: "radio",
                            "aria-checked": if Some(v) == shown { "true" } else { "false" },
                            onclick: move |_| on_pick.call(v),
                            "{v.label()}"
                        }
                    }
                }
            }
        }
    }

    /// The phone's period row: ‹ the views (or the note) ›.
    pub fn row(&self) -> Element {
        let unit = self.unit;
        let on_step = self.on_step;
        rsx! {
            nav { class: "dock-period", "aria-label": "Period",
                button {
                    r#type: "button",
                    class: "period-arrow",
                    "aria-label": "Previous {unit}",
                    onclick: move |_| on_step.call(-1),
                    Icon { d: "m15 6-6 6 6 6", size: 22 }
                }
                {self.switch("dock-seg")}
                button {
                    r#type: "button",
                    class: "period-arrow",
                    "aria-label": "Next {unit}",
                    onclick: move |_| on_step.call(1),
                    Icon { d: "m9 6 6 6-6 6", size: 22 }
                }
            }
        }
    }
}

/// A page's dock on a phone, fixed above the tab bar: the page's main
/// `actions`, then the period row. It is the last thing on the page, so
/// the page's last row scrolls up above it. Hidden when wide.
pub fn dock(bar: &Bar, actions: Element) -> Element {
    rsx! {
        div { class: "thumb-dock",
            {actions}
            {bar.row()}
        }
    }
}

/// Mileage → Fradrag's title and sub line: "2026" / "Calendar year · so
/// far", "2025" / "Calendar year".
pub fn calendar_year_title(year: i32, today: NaiveDate) -> (String, String) {
    let sub = if year == today.year() {
        "Calendar year · so far"
    } else {
        "Calendar year"
    };
    (year.to_string(), sub.to_string())
}

/// The periods of the other pages, kept while the app runs, like Hours'.
#[derive(Clone, Copy)]
pub struct PagePeriods {
    pub mileage: Signal<Period>,
    pub danlon: Signal<Period>,
    /// Mileage → Fradrag's calendar year.
    pub fradrag: Signal<i32>,
    /// Vacation's month or calendar year; it opens on the year.
    pub vacation: Signal<Period>,
}

impl PagePeriods {
    /// This month, this year and this calendar year.
    pub fn new(today: NaiveDate) -> Self {
        PagePeriods {
            mileage: Signal::new(Period::new(today)),
            danlon: Signal::new(Period::new(today)),
            fradrag: Signal::new(today.year()),
            vacation: Signal::new(Period::new(today).pick(View::Year, today)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    const TODAY: &str = "2026-11-26";

    #[test]
    fn hours_opens_on_this_month() {
        let p = Period::new(d(TODAY));
        assert_eq!(p.view, View::Month);
        assert_eq!(p.anchor, d("2026-11-01"));
        assert!(p.is_current(d(TODAY)));
        assert_eq!(
            p.title(d(TODAY)),
            ("November 2026".into(), "This month".into())
        );
    }

    #[test]
    fn day_opens_on_today_and_the_arrows_step_days() {
        let today = d(TODAY);
        let p = Period::new(today).pick(View::Day, today);
        assert_eq!(p, Period::day(today));
        assert!(p.is_current(today));
        assert_eq!(
            p.title(today),
            ("Today".into(), "Thursday 26 Nov 2026".into())
        );
        let back = p.step(-7);
        assert_eq!(back.anchor, d("2026-11-19"));
        assert!(!back.is_current(today));
        assert_eq!(
            back.title(today),
            ("Thursday 19 Nov".into(), "Week 47 · 2026".into())
        );
        assert_eq!(back.today(today), p);
        // A day across the month and the year.
        assert_eq!(Period::day(d("2026-12-31")).step(1).anchor, d("2027-01-01"));
    }

    #[test]
    fn the_arrows_step_weeks_and_months() {
        let today = d(TODAY);
        let week = Period::new(today).pick(View::Week, today);
        assert_eq!(week.anchor, d("2026-11-23"));
        assert_eq!(
            week.title(today),
            ("Week 48".into(), "23–29 Nov 2026".into())
        );
        let prev = week.step(-1);
        assert_eq!(prev.monday(), d("2026-11-16"));
        assert!(!prev.is_current(today));
        assert_eq!(prev.today(today), week);

        let month = Period::new(today).step(-1);
        assert_eq!(month.month(), month_of(d("2026-10-01")));
        assert_eq!(month.title(today), ("October 2026".into(), String::new()));
        assert_eq!(Period::new(today).step(2).anchor, d("2027-01-01"));
        assert_eq!(month.today(today), Period::new(today));
    }

    #[test]
    fn switching_view_keeps_the_period() {
        let today = d(TODAY);
        // Another month: its first day, its first week.
        let oct = Period::new(today).step(-1);
        assert_eq!(oct.pick(View::Day, today).anchor, d("2026-10-01"));
        assert_eq!(oct.pick(View::Week, today).anchor, d("2026-10-05"));
        assert_eq!(oct.pick(View::Week, today).pick(View::Month, today), oct);
        // Another week: its Monday; a day: its week and month.
        let w47 = Period::day(d("2026-11-19"));
        assert_eq!(w47.pick(View::Week, today).anchor, d("2026-11-16"));
        assert_eq!(
            w47.pick(View::Week, today).pick(View::Day, today).anchor,
            d("2026-11-16")
        );
        assert_eq!(w47.pick(View::Month, today), Period::new(today));
        // This week to Day: today.
        let this_week = Period::new(today).pick(View::Week, today);
        assert_eq!(this_week.pick(View::Day, today), Period::day(today));
        // The same view changes nothing.
        assert_eq!(w47.pick(View::Day, today), w47);
    }

    #[test]
    fn a_week_holds_its_days_and_months() {
        let today = d(TODAY);
        let month = Period::new(today);
        assert_eq!(month.days(), (d("2026-11-01"), d("2026-11-30")));
        assert_eq!(month.months(), (month_of(d("2026-11-01")), None));
        // Week 49 runs into December: both months' data are needed.
        let w49 = month.pick(View::Week, today).step(1);
        assert_eq!(w49.days(), (d("2026-11-30"), d("2026-12-06")));
        assert_eq!(
            w49.months(),
            (month_of(d("2026-11-01")), Some(month_of(d("2026-12-01"))))
        );
        assert!(w49.contains(d("2026-12-06")));
        assert!(!w49.contains(d("2026-11-29")));
        assert!(!w49.contains(d("2026-12-07")));
        // Week · Month only: Week comes back to the same month.
        assert_eq!(w49.pick(View::Month, today), month);
    }

    #[test]
    fn year_pages_title_their_years() {
        let today = d(TODAY);
        assert_eq!(
            calendar_year_title(2026, today),
            ("2026".into(), "Calendar year · so far".into())
        );
        assert_eq!(
            calendar_year_title(2025, today),
            ("2025".into(), "Calendar year".into())
        );
        // Vacation's Year view: stepped by whole years, back to this one,
        // and to Month: today's month in this year, else January.
        let year = Period::new(today).pick(View::Year, today);
        assert_eq!(year.days(), (d("2026-01-01"), d("2026-12-31")));
        assert_eq!(year.title(today), calendar_year_title(2026, today));
        assert!(year.is_current(today));
        let y25 = year.step(-1);
        assert_eq!(y25.name(), "2025");
        assert!(!y25.is_current(today));
        assert_eq!(y25.today(today), year);
        assert_eq!(year.pick(View::Month, today), Period::new(today));
        assert_eq!(y25.pick(View::Month, today).anchor, d("2025-01-01"));
        assert_eq!(Period::new(today).step(-11).pick(View::Year, today), y25);
    }
}
