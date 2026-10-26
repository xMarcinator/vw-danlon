//! The Month screen: month navigation, today's work card (work, private
//! time and today's private list), the totals and one row per day, as
//! `report` computes them. A day opens the Day screen.

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime};
use chrono_tz::Tz;
use dioxus::prelude::*;
use vw_danlon::month::{Day, Month, OffReason};
use vw_danlon::service::{PrivateItem, PrivateState, WorkAction, WorkState, WorkToday};
use vw_danlon::timesheet::weekday_da;

use super::{
    Action, Message, Panel, PrivateList, apply, diff_class, hm, hours, kroner, message_for, signed,
    status,
};
use crate::setup::Setup;
use crate::{Icon, Screen};

pub(super) fn this_month(setup: &Setup) -> Month {
    let today = match &setup.config {
        Some(cfg) => super::now(cfg).date_naive(),
        None => chrono::Local::now().date_naive(),
    };
    Month {
        year: today.year(),
        month: today.month(),
    }
}

pub(super) fn step(m: Month, by: i32) -> Month {
    let index = m.year * 12 + m.month as i32 - 1 + by;
    Month {
        year: index.div_euclid(12),
        month: index.rem_euclid(12) as u32 + 1,
    }
}

pub(super) fn month_title(m: Month) -> String {
    m.first_day().format("%B %Y").to_string()
}

/// Where the day was, or why it was off.
fn place(d: &Day) -> String {
    match &d.off_reason {
        Some(r) => r.to_string(),
        None if !d.work_locations.is_empty() => d.work_locations.join(" | "),
        None if !d.home.is_empty() => "Working from home".to_string(),
        None if d.norm_hours == 0.0 => "Weekend".to_string(),
        None => String::new(),
    }
}

fn note_class(d: &Day) -> &'static str {
    if d.check.is_some() {
        "chip warn strong"
    } else if matches!(d.off_reason, Some(OffReason::Leave { .. })) {
        "chip"
    } else if !d.appointments.is_empty() || !d.home.is_empty() {
        "chip info"
    } else {
        "chip"
    }
}

fn quiet(d: &Day) -> bool {
    d.norm_hours == 0.0 && d.worked_hours == 0.0
}

/// Hours worked from home on a day.
fn home_hours(d: &Day) -> f64 {
    d.home
        .iter()
        .map(|(s, e)| (*e - *s).num_minutes() as f64 / 60.0)
        .sum()
}

/// The note of a day after today: its planned norm only.
fn planned_note(d: &Day) -> String {
    match &d.off_reason {
        Some(r) => r.to_string(),
        None if d.norm_hours > 0.0 => "Planned".to_string(),
        None => String::new(),
    }
}

fn planned(d: &Day) -> String {
    d.planned
        .map(|(s, e)| format!("{}–{}", s.format("%H:%M"), e.format("%H:%M")))
        .unwrap_or_default()
}

/// "1:10" for 70 minutes; "−1:10" below zero.
fn hmm(minutes: i64) -> String {
    let sign = if minutes < 0 { "−" } else { "" };
    let m = minutes.abs();
    format!("{sign}{}:{:02}", m / 60, m % 60)
}

/// Hours as minutes, rounded.
fn minutes(h: f64) -> i64 {
    (h * 60.0).round() as i64
}

/// Private items of today only: planned, running or recorded (a timer
/// still running from an earlier day too).
fn today_items(items: &[PrivateItem], today: NaiveDate) -> Vec<PrivateItem> {
    items
        .iter()
        .filter(|i| matches!(i.state, PrivateState::Running { .. }) || i.start().date() == today)
        .cloned()
        .collect()
}

/// The first part of the planned working time today that has passed:
/// "08:00–09:10", while it is counting.
fn counting_window(d: &Day, now: DateTime<Tz>) -> Option<String> {
    let (s, e) = d.planned?;
    (d.so_far && s < now).then(|| format!("{}–{}", s.format("%H:%M"), e.min(now).format("%H:%M")))
}

#[component]
pub fn MonthScreen(setup: Signal<Setup>, screen: Signal<Screen>) -> Element {
    let month = use_signal(|| this_month(&setup.peek()));
    let mut open_day = use_signal(|| None::<NaiveDate>);
    let mut version = use_signal(|| 0u32);
    let expanded = use_signal(|| false);
    let mut message = use_signal(|| None as Message);
    let mut adding = use_signal(|| false);
    let cal_sync = use_context::<Signal<super::CalendarSync>>();
    let mut loaded = use_resource(move || {
        let m = month();
        let _ = cal_sync.read().version;
        let cfg = setup.read().config.clone();
        async move {
            match cfg {
                Some(cfg) => super::load(cfg, m).await,
                None => Err("no config".to_string()),
            }
        }
    });
    // Today counts up to now: the month is computed again every minute. A
    // minute missed while the app was paused is not caught up.
    let mut minute = use_signal(|| 0u64);
    use_future(move || async move {
        let mut every = tokio::time::interval(std::time::Duration::from_secs(60));
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        every.tick().await;
        loop {
            every.tick().await;
            minute += 1;
        }
    });
    // The month's days, mileage, private time and today's work, built off
    // the UI thread (it reads the database and may record calendar time).
    let computed = use_resource(move || {
        let _ = minute();
        let _ = version();
        let cfg = setup.read().config.clone();
        let l = match &*loaded.read() {
            Some(Ok(l)) => Some(l.clone()),
            _ => None,
        };
        async move {
            match (cfg, l) {
                (Some(cfg), Some(l)) => Some(super::compute(cfg, l).await),
                _ => None,
            }
        }
    });
    // Opening or leaving a day clears the last private-time message.
    let go = use_callback(move |d: Option<NaiveDate>| {
        message.set(None);
        adding.set(false);
        open_day.set(d);
        document::eval("document.querySelector('.content')?.scrollTo(0, 0)");
    });
    // True when the action was done; a refused one leaves its editor open.
    let on_action = use_callback(move |a: Action| -> bool {
        let Some(cfg) = setup.peek().config.clone() else {
            return false;
        };
        let result = match &*loaded.peek() {
            Some(Ok(l)) => apply(&cfg, &l.occurrences, &a).map_err(|e| format!("{e:#}")),
            _ => Err("the month is still loading".to_string()),
        };
        let done = result.is_ok();
        if done && a.panel() == Panel::Trips {
            // The trips are reread; the day stays open.
            loaded.restart();
        }
        message.set(Some((a.panel(), result)));
        version += 1;
        done
    });

    let Some(cfg) = setup.read().config.clone() else {
        return rsx! {};
    };
    let m = month();
    let loaded = loaded.read();
    let loaded = match &*loaded {
        Some(Ok(l)) if l.month == m => l,
        Some(Err(e)) => {
            return rsx! {
                MonthHeader { month, screen }
                div { class: "alert", role: "alert",
                    span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                    div { b { "The month can't be shown: " } "{e}" }
                }
            };
        }
        _ => {
            return rsx! {
                MonthHeader { month, screen }
                p { class: "placeholder", role: "status", "Loading {month_title(m)}…" }
            };
        }
    };
    let computed = computed.read();
    let computed = match &*computed {
        Some(Some(Ok(c))) if c.month == m => c,
        Some(Some(Err(e))) => {
            return rsx! {
                MonthHeader { month, screen }
                div { class: "alert", role: "alert", "{e}" }
            };
        }
        _ => {
            return rsx! {
                MonthHeader { month, screen }
                p { class: "placeholder", role: "status", "Loading {month_title(m)}…" }
            };
        }
    };
    let view = &computed.view;
    let now = computed.now;
    let today = now.date_naive();

    if let Some(date) = open_day().filter(|d| m.contains(*d)) {
        return super::day::render(super::day::DayArgs {
            cfg: &cfg,
            view,
            state: &computed.state,
            loaded,
            date,
            now,
            message: message(),
            on_action,
            on_back: Callback::new(move |_| go.call(None)),
            adding,
        });
    }

    let report = &view.report;
    let to_check: Vec<&Day> = report.days.iter().filter(|d| d.check.is_some()).collect();
    let working_days = report.days.iter().filter(|d| d.norm_hours > 0.0).count();
    let vacation_days = report
        .days
        .iter()
        .filter(|d| {
            matches!(
                d.off_reason,
                Some(OffReason::Leave {
                    working_day: true,
                    ..
                })
            )
        })
        .count();
    let diff = report.worked_total() - report.norm_total();
    let month_norm = report.month_norm_total();
    let from_home: f64 = report.to_date().map(home_hours).sum();
    let trips_in_month = loaded
        .trips
        .iter()
        .filter(|t| m.contains(t.start.date_naive()))
        .count();
    let calendar_name = cfg
        .calendar
        .as_ref()
        .and_then(|c| c.name.clone())
        .map(|n| format!("Calendar “{n}”"))
        .unwrap_or_else(|| "Calendar".to_string());
    let has_calendar = cfg.calendar.is_some();
    // Today so far, when it is in this month.
    let today_day = report.days.iter().find(|d| d.date == today && d.so_far);
    let not_at_work_yet = matches!(
        view.work.as_ref().map(|w| &w.state),
        Some(WorkState::NotWorking { late_by: Some(_) })
    ) && today_day.is_some_and(|d| d.worked_hours == 0.0);
    let norm_sub = if m.last_day() <= report.today {
        format!("{working_days} working days · {vacation_days} vacation days")
    } else if m.first_day() > report.today {
        format!("month not started · {} planned", hours(month_norm))
    } else if let Some(w) = today_day.and_then(|d| counting_window(d, now)) {
        format!("today counts {w} · {} for the month", hours(month_norm))
    } else {
        format!(
            "until today, {} · {} for the whole month",
            report.today.format("%-d %b"),
            hours(month_norm)
        )
    };
    let worked_sub = if not_at_work_yet {
        "not at work yet today".to_string()
    } else if from_home > 0.0 {
        format!(
            "incl. {} h from home · after lunch and private time",
            hours(from_home)
        )
    } else {
        "after lunch and private time".to_string()
    };
    let checks = match to_check.len() {
        0 => None,
        1 => Some("1 day to check".to_string()),
        n => Some(format!("{n} days to check")),
    };
    let diff_sub = match (today_day.filter(|d| d.expected_hours > 0.0), checks) {
        (Some(d), Some(c)) => format!("today {} so far · {c}", signed(d.difference())),
        (Some(d), None) => format!("today {} so far", signed(d.difference())),
        (None, Some(c)) => c,
        (None, None) => "nothing to check".to_string(),
    };
    let mut kpis = vec![
        ("Norm to date", hours(report.norm_total()), norm_sub, ""),
        (
            "Worked to date",
            hours(report.worked_total()),
            worked_sub,
            "",
        ),
        (
            "Difference to date",
            signed(diff),
            diff_sub,
            diff_class(diff),
        ),
    ];
    if let Some(mil) = &view.mileage {
        kpis.push((
            "Mileage allowance",
            kroner(mil.total_amount()),
            format!(
                "{} km · {} business trips",
                format!("{:.1}", mil.total_km()).replace('.', ","),
                mil.entries.len()
            ),
            "",
        ));
    }
    let first_check = to_check.first().map(|d| d.date);
    let lunch_rule = format!(
        "Working time = time at the work address, minus lunch ({} min from {}) and private time",
        cfg.lunch.minutes,
        cfg.lunch.start.format("%H:%M")
    );
    let calendar_chip = match loaded.calendar_fetched_at {
        Some(at) => format!("{calendar_name} · {}", super::calendar_age(at, now)),
        None => calendar_name.to_string(),
    };
    // The fixed workplace by its short name, as in the Danløn checklist.
    let short = |place: String| match &view.workplace {
        Some((w, _)) if vw_danlon::address::matches(&place, w) => {
            cfg.checklist.workplace_name.clone()
        }
        _ => place,
    };
    let place_today = today_day
        .and_then(|d| d.work_locations.first().cloned())
        .map(short)
        .unwrap_or_default();
    let work = view.work.clone().map(|mut w| {
        if let WorkState::AtWork { place, .. } = &mut w.state {
            *place = short(std::mem::take(place));
        }
        w
    });
    let items = today_items(&view.private.items, today);

    rsx! {
        MonthHeader { month, screen }

        div { class: "chips wide-only",
            span { class: "src-chip",
                span { class: if has_calendar { "dot" } else { "dot warn" }, "aria-hidden": "true" }
                if has_calendar {
                    "{calendar_chip}"
                } else {
                    "No calendar · norm from [work_week]"
                }
            }
            span { class: "src-chip",
                span { class: if loaded.trips.is_empty() { "dot warn" } else { "dot" }, "aria-hidden": "true" }
                "{trips_in_month} trips this month"
            }
            if let Some(date) = first_check {
                button {
                    class: "src-chip warn wide-only",
                    onclick: move |_| go.call(Some(date)),
                    if to_check.len() == 1 {
                        "! 1 day to check"
                    } else {
                        "! {to_check.len()} days to check"
                    }
                }
            }
            if loaded.calendar_fetched_at.is_some() {
                button {
                    class: "src-chip",
                    disabled: cal_sync.read().refreshing,
                    onclick: move |_| { spawn(super::refresh_calendar(setup, cal_sync)); },
                    if cal_sync.read().refreshing { "Refreshing…" } else { "Refresh calendar" }
                }
            }
            span { class: "grow" }
            if let Some(work) = &work {
                WorkPill { work: work.clone(), today, on_action }
            }
        }
        if let Some(w) = &loaded.calendar_warning {
            div { class: "alert", role: "status", "{w}" }
        }
        if let (Some(e), Some(at)) = (&cal_sync.read().error, loaded.calendar_fetched_at) {
            div { class: "alert", role: "status",
                "The calendar can't be refreshed ({e}). Showing the copy {super::calendar_age(at, now)}."
            }
        }

        if let Some(work) = &work {
            WorkCard {
                work: work.clone(),
                items,
                running_replaces: view
                    .private
                    .running
                    .as_ref()
                    .and_then(|r| r.replaces.as_ref())
                    .map(|w| (w.start, w.end)),
                place: place_today,
                today,
                now: now.naive_local(),
                tz: cfg.timezone,
                auto_start: cfg.private.auto_start,
                grace_minutes: cfg.private.grace_minutes,
                expanded,
                message: message_for(&message(), Panel::Private)
                    .or_else(|| message_for(&message(), Panel::Home)),
                on_action,
            }
        }

        div { class: "kpis",
            for (label , value , sub , class) in kpis {
                section { class: "kpi",
                    h2 { "{label}" }
                    span { class: "kpi-value {class}", "{value}" }
                    span { class: "kpi-sub", "{sub}" }
                }
            }
        }

        for d in to_check.iter() {
            {
                let date = d.date;
                rsx! {
                    button {
                        class: "flag narrow-only",
                        onclick: move |_| go.call(Some(date)),
                        span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                        span { class: "grow",
                            b { {date.format("%-d %b").to_string()} }
                            " · {d.check.as_deref().unwrap_or_default()}"
                        }
                        Icon { d: "m9 6 6 6-6 6", size: 18 }
                    }
                }
            }
        }

        section { class: "panel wide-only", "aria-labelledby": "days-h",
            div { class: "panel-head",
                h2 { id: "days-h", "Days" }
                span { class: "note",
                    "Totals count up to today (today from the planned start up to now) · days after today are planned only. {lunch_rule}"
                }
            }
            div { class: "scroll-x",
                table { class: "days",
                    thead {
                        tr {
                            th { scope: "col", "Date" }
                            th { scope: "col", "Planned" }
                            th { scope: "col", "Work address" }
                            th { scope: "col", "At work" }
                            th { scope: "col", class: "r", "Worked" }
                            th { scope: "col", class: "r", "Norm" }
                            th { scope: "col", class: "r", "Diff" }
                            th { scope: "col", "Note" }
                        }
                    }
                    tbody {
                        for d in report.days.iter() {
                            {
                                let date = d.date;
                                let future = date > report.today;
                                let is_today = date == report.today;
                                let row = if future {
                                    "quiet"
                                } else if d.check.is_some() {
                                    "flagged"
                                } else if is_today {
                                    "today"
                                } else if quiet(d) {
                                    "quiet"
                                } else {
                                    ""
                                };
                                let remark = if future { planned_note(d) } else { d.remark() };
                                let note_class = if future { "chip planned" } else { note_class(d) };
                                let place = if is_today && not_at_work_yet {
                                    "Not at work yet".to_string()
                                } else {
                                    place(d)
                                };
                                rsx! {
                                    tr { class: "{row}", "aria-current": if is_today { "date" } else { "false" },
                                        td { class: "nowrap",
                                            button {
                                                class: "link-btn",
                                                onclick: move |_| go.call(Some(date)),
                                                {date.format("%-d %b").to_string()}
                                            }
                                            " "
                                            span { class: "muted", "{weekday_da(date.weekday())}" }
                                            if is_today {
                                                " "
                                                span { class: "chip today", "Today" }
                                            }
                                        }
                                        td { class: "nowrap", "{planned(d)}" }
                                        if future {
                                            td {}
                                            td {}
                                            td {}
                                        } else {
                                            td { "{place}" }
                                            td { class: "mono small", "{d.periods_text()}" }
                                            td { class: "r", "{hours(d.worked_hours)}" }
                                        }
                                        td { class: "r",
                                            if !future || d.norm_hours > 0.0 {
                                                "{hours(d.norm_hours)}"
                                            }
                                        }
                                        if future {
                                            td {}
                                        } else {
                                            td { class: "r strong {diff_class(d.difference())}",
                                                "{signed(d.difference())}"
                                                if d.so_far {
                                                    span { class: "muted small", " so far" }
                                                }
                                            }
                                        }
                                        td {
                                            if !remark.is_empty() {
                                                span { class: "{note_class}", "{remark}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        section { class: "panel narrow-only", "aria-label": "Days",
            ul { class: "day-list",
                for d in report.days.iter() {
                    {
                        let date = d.date;
                        let future = date > report.today;
                        let is_today = date == report.today;
                        let mut remark = if future { String::new() } else { d.remark() };
                        let place = match (&d.off_reason, future) {
                            (None, true) if d.norm_hours > 0.0 => "Planned".to_string(),
                            _ if is_today && not_at_work_yet => "Not at work yet".to_string(),
                            _ => place(d),
                        };
                        if is_today
                            && not_at_work_yet
                            && remark.is_empty()
                            && let Some((s, _)) = d.planned
                        {
                            remark = format!("Today · planned since {}", s.format("%H:%M"));
                        }
                        let class = if is_today {
                            "day-row today"
                        } else if future || quiet(d) {
                            "day-row quiet"
                        } else {
                            "day-row"
                        };
                        let chip = if is_today && not_at_work_yet { "chip info" } else { note_class(d) };
                        rsx! {
                            li { "aria-current": if is_today { "date" } else { "false" },
                                button {
                                    class: "{class}",
                                    onclick: move |_| go.call(Some(date)),
                                    span { class: "day-num",
                                        b { "{date.day()}" }
                                        if is_today {
                                            span { class: "today-label", "Today" }
                                        } else {
                                            span { "{weekday_da(date.weekday())}" }
                                        }
                                    }
                                    span { class: "day-mid",
                                        span { class: "ellipsis", "{place}" }
                                        if !remark.is_empty() {
                                            span { class: "{chip} ellipsis", "{remark}" }
                                        }
                                    }
                                    span { class: "day-nums",
                                        if future {
                                            if d.norm_hours > 0.0 {
                                                span { class: "quiet", "{hours(d.norm_hours)}" }
                                            }
                                        } else if !quiet(d) || d.so_far && d.expected_hours > 0.0 {
                                            b { "{hours(d.worked_hours)}" }
                                            span { class: "{diff_class(d.difference())}", "{signed(d.difference())}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Month navigation; the Danløn checklist when wide, the Settings gear
/// when narrow.
#[component]
fn MonthHeader(month: Signal<Month>, screen: Signal<Screen>) -> Element {
    let m = month();
    rsx! {
        div { class: "screen-head",
            div { class: "month-nav",
                button {
                    class: "nav-btn",
                    "aria-label": "Previous month",
                    onclick: move |_| month.set(step(m, -1)),
                    Icon { d: "m15 6-6 6 6 6", size: 18 }
                }
                h1 { "{month_title(m)}" }
                button {
                    class: "nav-btn",
                    "aria-label": "Next month",
                    onclick: move |_| month.set(step(m, 1)),
                    Icon { d: "m9 6 6 6-6 6", size: 18 }
                }
            }
            button {
                class: "primary-sm wide-only",
                onclick: move |_| screen.set(Screen::Danlon),
                Icon { d: Screen::Danlon.icon(), size: 15 }
                "Danløn app checklist"
            }
            button {
                class: "icon-btn narrow-only",
                "aria-label": "Settings",
                onclick: move |_| screen.set(Screen::Settings),
                Icon { d: Screen::Settings.icon(), size: 20 }
            }
        }
    }
}

/// The head of the work card: kicker, title, the title's and the dot's
/// class, and the status line with its class.
struct WorkHead {
    kicker: String,
    title: String,
    tone: &'static str,
    status: Option<(String, &'static str)>,
}

fn work_head(w: &WorkToday, place: &str, now: NaiveDateTime) -> WorkHead {
    let worked = hmm(minutes(w.worked));
    let at = |t: DateTime<Tz>| hm(t.naive_local());
    match &w.state {
        WorkState::NotWorking { late_by: Some(_) } => {
            let start = w.planned.map(|(s, _)| at(s)).unwrap_or_default();
            WorkHead {
                kicker: format!("Today · planned from {start} · now {}", hm(now)),
                title: format!("Not at work · {} so far", hmm(minutes(w.difference()))),
                tone: "bad",
                status: Some((
                    format!(
                        "Counting since {start}: the planned time that has passed counts against you until you start."
                    ),
                    "bad",
                )),
            }
        }
        WorkState::NotWorking { late_by: None } => {
            let place_part = if place.is_empty() {
                String::new()
            } else {
                format!(" · {place}")
            };
            match w.planned {
                Some((s, e)) if now < s.naive_local() => WorkHead {
                    kicker: format!("Today · planned {}–{}{place_part}", at(s), at(e)),
                    title: "Not working yet".into(),
                    tone: "",
                    status: Some((
                        format!(
                            "Starts by itself when you arrive at {}, or tap Start work at home.",
                            if place.is_empty() { "work" } else { place }
                        ),
                        "info",
                    )),
                },
                Some((s, e)) => WorkHead {
                    kicker: format!("Today · planned {}–{}{place_part}", at(s), at(e)),
                    title: if w.worked > 0.0 {
                        format!("Not working · {worked} today")
                    } else {
                        "Not working".into()
                    },
                    tone: "",
                    status: None,
                },
                None => WorkHead {
                    kicker: "Today · nothing planned".into(),
                    title: if w.worked > 0.0 {
                        format!("Not working · {worked} today")
                    } else {
                        "Not working".into()
                    },
                    tone: "",
                    status: None,
                },
            }
        }
        WorkState::AtWork { place, since } => WorkHead {
            kicker: format!("Working · {place}"),
            title: format!("Since {} · {worked} today", at(*since)),
            tone: "on",
            status: None,
        },
        WorkState::Home { since } => WorkHead {
            kicker: "Working from home".into(),
            title: format!("Since {} · {worked} today", at(*since)),
            tone: "on",
            status: None,
        },
        WorkState::Private { label, since, .. } => {
            let since_local = since.naive_local();
            let elapsed = hmm((now - since_local).num_minutes().max(0));
            let since_text = if since_local.date() == now.date() {
                hm(since_local)
            } else {
                since_local.format("%a %-d %b %H:%M").to_string()
            };
            WorkHead {
                kicker: format!("Private time · {label}"),
                title: format!("{elapsed} · since {since_text}"),
                tone: "warn",
                status: None,
            }
        }
    }
}

/// Today's work: on a narrow screen the status over the full width, then
/// its buttons in a row of their own ([`CardActions`]) and Stop work as a
/// link at the bottom; the next private appointment with its own Start, and
/// today's private list under the chevron. On a wide screen the head and
/// buttons are the header's [`WorkPill`].
#[component]
fn WorkCard(
    work: WorkToday,
    items: Vec<PrivateItem>,
    running_replaces: Option<(NaiveDateTime, NaiveDateTime)>,
    place: String,
    today: NaiveDate,
    now: NaiveDateTime,
    tz: Tz,
    auto_start: bool,
    grace_minutes: u32,
    expanded: Signal<bool>,
    message: Option<Result<String, String>>,
    on_action: Callback<Action, bool>,
) -> Element {
    // The elapsed private time is kept current between the minutes.
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        let mut every = tokio::time::interval(std::time::Duration::from_secs(15));
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            every.tick().await;
            tick += 1;
        }
    });
    let _ = tick();
    let live = chrono::Utc::now().with_timezone(&tz).naive_local().max(now);
    let head = work_head(&work, &place, live);
    let open = expanded();
    let private_now = match &work.state {
        WorkState::Private { label, since, auto } => Some((label.clone(), *since, *auto)),
        _ => None,
    };
    // The appointment started automatically: its planned end, for AutoStatus.
    let auto = private_now
        .as_ref()
        .filter(|p| p.2)
        .and_then(|(label, since, _)| {
            items
                .iter()
                .find(|i| {
                    matches!(i.state, PrivateState::StartedAutomatically { .. })
                        && &i.label == label
                })
                .and_then(|i| i.planned)
                .map(|(_, end)| (label.clone(), since.naive_local(), end))
        });
    let status = match (&private_now, &head.status) {
        (Some((_, since, false)), _) => {
            let planned = running_replaces
                .map(|(s, e)| format!(" Planned {}–{}; the real time replaces it.", hm(s), hm(e)))
                .unwrap_or_default();
            let forgotten = if since.date_naive() < today {
                " Still running from an earlier day? Stop it and fix the end time."
            } else {
                ""
            };
            Some((
                format!("Work continues when you stop.{planned}{forgotten}"),
                "warn",
            ))
        }
        (_, s) => s.clone(),
    };
    let next = work.next_private.clone();
    let next_today = next
        .as_ref()
        .and_then(|i| i.planned)
        .is_some_and(|(s, _)| s.date() == today);
    let current = next
        .as_ref()
        .and_then(|n| items.iter().position(|i| i == n));
    let card_class = match head.tone {
        "on" => "work-card on",
        "warn" => "work-card running",
        "bad" => "work-card late",
        _ => "work-card",
    };
    let working = matches!(
        work.state,
        WorkState::AtWork { .. } | WorkState::Home { .. }
    );
    let stop = if !working {
        None
    } else if work.actions.contains(&WorkAction::StopWork) {
        Some(("Stop working from home", Action::HomeStop))
    } else if work.actions.contains(&WorkAction::LeaveWork) {
        Some(("Stop work (left early)", Action::LeaveWork))
    } else {
        None
    };
    rsx! {
        section { class: "{card_class}", "aria-label": "Work today",
            div { class: "pt-head narrow-only",
                span { class: "pt-dot {head.tone}", "aria-hidden": "true" }
                span { class: "pt-title",
                    span { class: "pt-kicker", "{head.kicker}" }
                    b { class: "{head.tone}", "{head.title}" }
                }
            }
            if let Some((label, since, end)) = auto {
                AutoStatus {
                    label,
                    since,
                    end,
                    now,
                    grace_minutes,
                    on_action,
                }
            } else if let Some((text, class)) = status {
                div { class: "pt-status {class}", role: "status", "{text}" }
            }
            CardActions { work: work.clone(), now: live, on_action }
            {status_line(message)}
            div { class: "next-row",
                span { class: "pt-title",
                    if let Some((label, since, _)) = &private_now {
                        span { class: "pt-kicker", "Running now" }
                        span { class: "next-line", b { "{label}" } }
                        {
                            let planned = running_replaces
                                .map(|(s, e)| format!(" · planned {}–{}", hm(s), hm(e)))
                                .unwrap_or_default();
                            rsx! { span { class: "next-sub", "Since {hm(since.naive_local())}{planned}" } }
                        }
                    } else if let Some(n) = &next {
                        span { class: "pt-kicker", if next_today { "Next private · today" } else { "Next private" } }
                        {
                            let (s, e) = n.planned.unwrap_or((n.start(), n.start()));
                            let when = if next_today {
                                format!("{}–{}", hm(s), hm(e))
                            } else {
                                super::when(s, Some(e), today)
                            };
                            rsx! {
                                span { class: "next-line", b { "{n.label}" } " · {when}" }
                                if auto_start && next_today {
                                    span { class: "next-sub", "Starts automatically at {hm(s)}" }
                                }
                            }
                        }
                    } else {
                        span { class: "pt-kicker", "Next private" }
                        span { class: "muted", "No private appointments coming" }
                    }
                }
                if let Some(n) = next.as_ref().filter(|_| next_today && private_now.is_none()) {
                    {
                        let title = n.label.clone();
                        rsx! {
                            button {
                                class: "small-btn next-start",
                                "aria-label": "Start private time for {title}",
                                onclick: move |_| {
                                    on_action.call(Action::Start(Some(title.clone())));
                                },
                                "Start"
                            }
                        }
                    }
                }
                button {
                    class: "icon-btn",
                    "aria-expanded": if open { "true" } else { "false" },
                    "aria-controls": "pt-list",
                    "aria-label": if open { "Hide today's private time" } else { "Show today's private time" },
                    onclick: move |_| expanded.set(!open),
                    span { class: if open { "chevron up" } else { "chevron" },
                        Icon { d: "m6 9 6 6 6-6", size: 20 }
                    }
                }
            }
            if open {
                ul { id: "pt-list", class: "pt-list",
                    if items.is_empty() {
                        li { class: "pt-item",
                            div { class: "pt-row muted", "No private time today" }
                        }
                    } else {
                        PrivateList {
                            items: items.clone(),
                            current,
                            today,
                            auto_start,
                            on_action,
                        }
                    }
                }
            }
            if let Some((label, action)) = stop {
                div { class: "card-stop narrow-only",
                    button {
                        class: "link-btn bad",
                        onclick: move |_| {
                            on_action.call(action.clone());
                        },
                        "{label}"
                    }
                }
            }
        }
    }
}

/// The work card's buttons on a narrow screen, in a row of their own under
/// the status so neither is cut off: Start work, Stop private time, or
/// Start private time and Started earlier… (its time field opens in place,
/// not in a menu over the list).
#[component]
fn CardActions(work: WorkToday, now: NaiveDateTime, on_action: Callback<Action, bool>) -> Element {
    let mut earlier = use_signal(|| false);
    let mut at = use_signal(String::new);
    match &work.state {
        WorkState::NotWorking { .. } => rsx! {
            if work.actions.contains(&WorkAction::StartWork) {
                div { class: "card-actions narrow-only",
                    button {
                        class: "pt-btn",
                        onclick: move |_| {
                            on_action.call(Action::HomeStart);
                        },
                        "Start work"
                    }
                }
            }
        },
        WorkState::Private { .. } => rsx! {
            div { class: "card-actions narrow-only",
                button {
                    class: "pt-btn stop",
                    onclick: move |_| {
                        on_action.call(Action::Stop);
                    },
                    "Stop private time"
                }
            }
        },
        WorkState::AtWork { .. } | WorkState::Home { .. } => {
            let open = earlier();
            rsx! {
                div { class: "card-actions narrow-only",
                    button {
                        class: "pt-btn",
                        onclick: move |_| {
                            earlier.set(false);
                            on_action.call(Action::Start(None));
                        },
                        "Start private time"
                    }
                    button {
                        class: "pt-btn secondary",
                        "aria-expanded": if open { "true" } else { "false" },
                        "aria-controls": "pt-earlier-row",
                        onclick: move |_| {
                            at.set(hm(now));
                            earlier.set(!open);
                        },
                        "Started earlier…"
                    }
                }
                if open {
                    div { id: "pt-earlier-row", class: "card-earlier narrow-only",
                        label { r#for: "pt-earlier-card", "Started at" }
                        input {
                            id: "pt-earlier-card",
                            r#type: "time",
                            value: "{at}",
                            oninput: move |e| at.set(e.value()),
                        }
                        button { class: "small-btn", onclick: move |_| earlier.set(false), "Cancel" }
                        button {
                            class: "small-btn primary-sm",
                            onclick: move |_| {
                                if on_action.call(Action::StartAt(at())) {
                                    earlier.set(false);
                                }
                            },
                            "Start"
                        }
                    }
                }
            }
        }
    }
}

/// The status line of the last action.
fn status_line(message: Option<Result<String, String>>) -> Element {
    status(message)
}

/// The work buttons: Start work, Stop private, or the split button
/// Private time ▾ whose menu starts private time now or earlier and stops
/// work. `pill` for the wide header.
#[component]
fn WorkButtons(work: WorkToday, pill: bool, on_action: Callback<Action, bool>) -> Element {
    let mut menu = use_signal(|| false);
    let mut earlier = use_signal(|| false);
    let mut at = use_signal(String::new);
    let (btn, stop) = if pill {
        ("pill-btn", "pill-btn stop")
    } else {
        ("pt-btn", "pt-btn stop")
    };
    let has = |a: WorkAction| work.actions.contains(&a);
    match &work.state {
        WorkState::NotWorking { .. } => rsx! {
            if has(WorkAction::StartWork) {
                button {
                    class: "{btn}",
                    onclick: move |_| {
                        on_action.call(Action::HomeStart);
                    },
                    "Start work"
                }
            }
        },
        WorkState::Private { .. } => rsx! {
            button {
                class: "{stop}",
                onclick: move |_| {
                    on_action.call(Action::Stop);
                },
                "Stop private"
            }
        },
        WorkState::AtWork { .. } | WorkState::Home { .. } => {
            let stop_item = if has(WorkAction::StopWork) {
                Some(("Stop working from home", Action::HomeStop))
            } else if has(WorkAction::LeaveWork) {
                Some(("Stop work (left early)", Action::LeaveWork))
            } else {
                None
            };
            let open = menu();
            let mut close = move || {
                menu.set(false);
                earlier.set(false);
            };
            rsx! {
                span { class: if pill { "split pill" } else { "split" }, role: "group", "aria-label": "Work actions",
                    button {
                        class: "{btn} split-main",
                        onclick: move |_| {
                            close();
                            on_action.call(Action::Start(None));
                        },
                        "Private time"
                    }
                    button {
                        class: "{btn} split-caret",
                        "aria-haspopup": "menu",
                        "aria-expanded": if open { "true" } else { "false" },
                        "aria-label": "More work actions",
                        onclick: move |_| {
                            earlier.set(false);
                            menu.set(!open);
                        },
                        Icon { d: "m6 9 6 6 6-6", size: 16 }
                    }
                    if open {
                        div { class: "menu-backdrop", onclick: move |_| close() }
                        div { class: "menu", role: "menu",
                            if earlier() {
                                div { class: "menu-edit",
                                    label { r#for: "pt-earlier", "Started at" }
                                    input {
                                        id: "pt-earlier",
                                        r#type: "time",
                                        value: "{at}",
                                        oninput: move |e| at.set(e.value()),
                                    }
                                    button { class: "small-btn", onclick: move |_| earlier.set(false), "Cancel" }
                                    button {
                                        class: "small-btn primary-sm",
                                        onclick: move |_| {
                                            if on_action.call(Action::StartAt(at())) {
                                                close();
                                            }
                                        },
                                        "Start"
                                    }
                                }
                            } else {
                                button {
                                    class: "menu-item",
                                    role: "menuitem",
                                    onclick: move |_| {
                                        close();
                                        on_action.call(Action::Start(None));
                                    },
                                    "Start private time now"
                                }
                                button {
                                    class: "menu-item",
                                    role: "menuitem",
                                    onclick: move |_| {
                                        at.set(hm(chrono::Local::now().naive_local()));
                                        earlier.set(true);
                                    },
                                    "Private time started earlier…"
                                }
                                if let Some((label, action)) = stop_item {
                                    div { class: "menu-sep", role: "separator" }
                                    button {
                                        class: "menu-item",
                                        role: "menuitem",
                                        onclick: move |_| {
                                            close();
                                            on_action.call(action.clone());
                                        },
                                        "{label}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Today's work on a wide screen, beside the sources: where it stands, the
/// work buttons and the next private appointment.
#[component]
fn WorkPill(work: WorkToday, today: NaiveDate, on_action: Callback<Action, bool>) -> Element {
    let at = |t: DateTime<Tz>| hm(t.naive_local());
    let (class, label, rest) = match &work.state {
        WorkState::NotWorking { late_by: Some(_) } => (
            "late",
            "Not at work".to_string(),
            format!(" · {} so far", hmm(minutes(work.difference()))),
        ),
        WorkState::NotWorking { late_by: None } => ("", "Not working".to_string(), String::new()),
        WorkState::AtWork { place, since } => (
            "on",
            format!("Working · {place}"),
            format!(" since {}", at(*since)),
        ),
        WorkState::Home { since } => (
            "on",
            "Working from home".to_string(),
            format!(" since {}", at(*since)),
        ),
        WorkState::Private { label, since, .. } => (
            "running",
            format!("Private time · {label}"),
            format!(" since {}", at(*since)),
        ),
    };
    let next = work
        .next_private
        .as_ref()
        .filter(|_| !matches!(work.state, WorkState::Private { .. }))
        .and_then(|i| i.planned.map(|(s, _)| (i.label.clone(), s)))
        .map(|(label, s)| {
            let day = if s.date() == today {
                "today".to_string()
            } else {
                s.format("%a %-d %b").to_string()
            };
            (label, format!(" {day} {}", hm(s)))
        });
    rsx! {
        span { class: "src-chip work-pill {class}",
            span { class: "dot", "aria-hidden": "true" }
            b { "{label}" }
            "{rest}"
            WorkButtons { work: work.clone(), pill: true, on_action }
        }
        if let Some((label, when)) = next {
            span { class: "next-note",
                "Next private: "
                b { "{label}" }
                "{when}"
            }
        }
    }
}

/// An appointment started automatically: when, what happens if it is left
/// alone, and "Started at …" to set the real start.
#[component]
fn AutoStatus(
    label: String,
    since: NaiveDateTime,
    end: NaiveDateTime,
    now: NaiveDateTime,
    grace_minutes: u32,
    on_action: Callback<Action, bool>,
) -> Element {
    let mut editing = use_signal(|| false);
    let mut at = use_signal(|| hm(since));
    let settles = end + chrono::Duration::minutes(i64::from(grace_minutes));
    let grace = if grace_minutes % 60 == 0 {
        format!("{} h", grace_minutes / 60)
    } else {
        format!("{grace_minutes} min")
    };
    let on_now = if now < end {
        format!("{label} is on your calendar now")
    } else {
        format!("{label} planned until {}", hm(end))
    };
    rsx! {
        div { class: "pt-status warn auto", role: "status",
            span {
                b { "Started automatically at {hm(since)}" }
                " · {on_now}"
            }
            span { class: "pt-auto-sub",
                "Left earlier? Set the real start. Not touched by {hm(settles)} ({grace} after the planned end), the calendar time {hm(since)}–{hm(end)} is used."
            }
            if editing() {
                div { class: "pt-auto-edit",
                    label { r#for: "pt-auto-at", "Started at" }
                    input {
                        id: "pt-auto-at",
                        r#type: "time",
                        value: "{at}",
                        oninput: move |e| at.set(e.value()),
                    }
                    button { class: "small-btn", onclick: move |_| editing.set(false), "Cancel" }
                    button {
                        class: "small-btn primary-sm",
                        onclick: move |_| {
                            if on_action.call(Action::StartedAt(at())) {
                                editing.set(false);
                            }
                        },
                        "Save"
                    }
                }
            } else {
                button {
                    class: "small-btn pt-auto-btn",
                    onclick: move |_| editing.set(true),
                    "Started at …"
                }
            }
        }
    }
}
