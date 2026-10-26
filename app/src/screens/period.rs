//! The period selector the pages share: ‹ the period's title and a sub line
//! ›, then the views to choose from (Day · Week · Month on Hours) and Today,
//! which is enabled only when another period is shown. A page that steps
//! whole years shows a note instead of the views. And Hours' period: the
//! view and the date it is anchored on, kept while the app runs.

use chrono::{Datelike, Duration, NaiveDate};
use dioxus::prelude::*;
use vw_danlon::month::Month;

use super::month::{month_title, step};
use super::week::{first_week, monday_of, month_of, week_range, week_title};
use crate::Icon;

/// What a page shows: a day, an ISO week or a month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Day,
    Week,
    Month,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Day => "Day",
            View::Week => "Week",
            View::Month => "Month",
        }
    }

    /// The unit the arrows step, for their labels: "Previous week".
    pub fn unit(self) -> &'static str {
        match self {
            View::Day => "day",
            View::Week => "week",
            View::Month => "month",
        }
    }
}

/// The period Hours shows: the view and a date in it (the day, the week's
/// Monday or the month's first day).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoursPeriod {
    pub view: View,
    pub anchor: NaiveDate,
}

impl HoursPeriod {
    /// The month of `today`, as Hours opens.
    pub fn new(today: NaiveDate) -> Self {
        HoursPeriod {
            view: View::Month,
            anchor: month_of(today).first_day(),
        }
    }

    /// The month the period is loaded from: the day's, the Monday's or the
    /// month shown.
    pub fn month(&self) -> Month {
        month_of(self.anchor)
    }

    /// The Monday of the week shown (in Week view).
    pub fn monday(&self) -> NaiveDate {
        monday_of(self.anchor)
    }

    /// Whether the period shown holds `today`.
    pub fn is_current(&self, today: NaiveDate) -> bool {
        match self.view {
            View::Day => self.anchor == today,
            View::Week => self.monday() == monday_of(today),
            View::Month => self.month() == month_of(today),
        }
    }

    /// The period `by` units before (negative) or after.
    pub fn step(self, by: i32) -> Self {
        let anchor = match self.view {
            View::Day => self.anchor + Duration::days(i64::from(by)),
            View::Week => self.monday() + Duration::days(7 * i64::from(by)),
            View::Month => step(self.month(), by).first_day(),
        };
        HoursPeriod { anchor, ..self }
    }

    /// The current period, in the same view.
    pub fn today(self, today: NaiveDate) -> Self {
        let anchor = match self.view {
            View::Day => today,
            View::Week => monday_of(today),
            View::Month => month_of(today).first_day(),
        };
        HoursPeriod { anchor, ..self }
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
            View::Month => self.month().first_day(),
        };
        HoursPeriod { view: to, anchor }
    }

    /// One day, in Day view.
    pub fn day(date: NaiveDate) -> Self {
        HoursPeriod {
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
        }
    }
}

/// The calendar icon of the Today button, with the day marked.
const TODAY_ICON: &str = "M3 5h18v16H3zM3 10h18M8 3v4M16 3v4M12 14v3";

/// The period selector. `views` are the choices of the switch; with none,
/// `note` is shown in its place (a page that steps whole years). `unit` is
/// what the arrows step, for their labels.
#[component]
pub fn PeriodBar(
    title: String,
    sub: String,
    unit: &'static str,
    views: Vec<View>,
    shown: Option<View>,
    note: Option<String>,
    current: bool,
    on_step: Callback<i32>,
    on_pick: Callback<View>,
    on_today: Callback<()>,
) -> Element {
    rsx! {
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
                if views.is_empty() {
                    span { class: "period-note", {note.unwrap_or_default()} }
                } else {
                    div {
                        class: "segmented period-seg",
                        style: "grid-template-columns: repeat({views.len()}, minmax(0, 1fr))",
                        role: "radiogroup",
                        "aria-label": "Show by",
                        for v in views.iter().copied() {
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
    }
}

/// The selector for Hours: Day · Week · Month over `period`; `on_change`
/// gets each new period.
#[component]
pub fn HoursBar(
    period: HoursPeriod,
    today: NaiveDate,
    on_change: Callback<HoursPeriod>,
) -> Element {
    let (title, sub) = period.title(today);
    rsx! {
        PeriodBar {
            title,
            sub,
            unit: period.view.unit(),
            views: vec![View::Day, View::Week, View::Month],
            shown: Some(period.view),
            note: None,
            current: period.is_current(today),
            on_step: move |by: i32| on_change.call(period.step(by)),
            on_pick: move |to: View| on_change.call(period.pick(to, today)),
            on_today: move |_| on_change.call(period.today(today)),
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
        let p = HoursPeriod::new(d(TODAY));
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
        let p = HoursPeriod::new(today).pick(View::Day, today);
        assert_eq!(p, HoursPeriod::day(today));
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
        assert_eq!(
            HoursPeriod::day(d("2026-12-31")).step(1).anchor,
            d("2027-01-01")
        );
    }

    #[test]
    fn the_arrows_step_weeks_and_months() {
        let today = d(TODAY);
        let week = HoursPeriod::new(today).pick(View::Week, today);
        assert_eq!(week.anchor, d("2026-11-23"));
        assert_eq!(
            week.title(today),
            ("Week 48".into(), "23–29 Nov 2026".into())
        );
        let prev = week.step(-1);
        assert_eq!(prev.monday(), d("2026-11-16"));
        assert!(!prev.is_current(today));
        assert_eq!(prev.today(today), week);

        let month = HoursPeriod::new(today).step(-1);
        assert_eq!(month.month(), month_of(d("2026-10-01")));
        assert_eq!(month.title(today), ("October 2026".into(), String::new()));
        assert_eq!(HoursPeriod::new(today).step(2).anchor, d("2027-01-01"));
        assert_eq!(month.today(today), HoursPeriod::new(today));
    }

    #[test]
    fn switching_view_keeps_the_period() {
        let today = d(TODAY);
        // Another month: its first day, its first week.
        let oct = HoursPeriod::new(today).step(-1);
        assert_eq!(oct.pick(View::Day, today).anchor, d("2026-10-01"));
        assert_eq!(oct.pick(View::Week, today).anchor, d("2026-10-05"));
        assert_eq!(oct.pick(View::Week, today).pick(View::Month, today), oct);
        // Another week: its Monday; a day: its week and month.
        let w47 = HoursPeriod::day(d("2026-11-19"));
        assert_eq!(w47.pick(View::Week, today).anchor, d("2026-11-16"));
        assert_eq!(
            w47.pick(View::Week, today).pick(View::Day, today).anchor,
            d("2026-11-16")
        );
        assert_eq!(w47.pick(View::Month, today), HoursPeriod::new(today));
        // This week to Day: today.
        let this_week = HoursPeriod::new(today).pick(View::Week, today);
        assert_eq!(this_week.pick(View::Day, today), HoursPeriod::day(today));
        // The same view changes nothing.
        assert_eq!(w47.pick(View::Day, today), w47);
    }
}
