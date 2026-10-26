//! The Hours screen: the period selector (Day · Week · Month), today's work
//! card (work, private time and today's private list), the totals and one
//! row per day, as `report` computes them. A day opens Day on that date.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime};
use chrono_tz::Tz;
use dioxus::prelude::*;
use vw_danlon::mileage::MileageMonth;
use vw_danlon::month::{Day, Month, OffReason};
use vw_danlon::service::{PrivateItem, PrivateState, WorkAction, WorkState, WorkToday};

use super::period::{Bar, HOURS_VIEWS, Period, View, dock};
use super::week::{self, in_week, week_mileage, week_months};
use super::{
    Action, Computed, Loaded, Message, Panel, PrivateList, apply, diff_class, hm, hours, kroner,
    message_for, signed, status, weekday,
};
use crate::setup::Setup;
use crate::{Icon, Screen};

pub(crate) fn today_of(setup: &Setup) -> NaiveDate {
    match &setup.config {
        Some(cfg) => super::now(cfg).date_naive(),
        None => chrono::Local::now().date_naive(),
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
    } else if !d.clients.is_empty() {
        "chip client"
    } else {
        "chip"
    }
}

/// The day's note; on a client visit without one, the km driven for it.
fn remark(d: &Day, business_km: &BTreeMap<NaiveDate, f64>) -> String {
    let remark = d.remark();
    if !remark.is_empty() || d.clients.is_empty() {
        return remark;
    }
    match business_km.get(&d.date) {
        Some(km) if *km > 0.0 => format!("{km:.1} km business").replace('.', ","),
        _ => "Client visit".to_string(),
    }
}

fn quiet(d: &Day) -> bool {
    d.norm_hours == 0.0 && d.worked_hours == 0.0
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

#[component]
pub fn MonthScreen(setup: Signal<Setup>, screen: Signal<Screen>) -> Element {
    // The day, week or month shown, kept while the app runs.
    let mut period = use_context::<Signal<Period>>();
    // The month it is loaded from: the day's, the week's Monday's or the
    // month shown.
    let month = use_memo(move || period().month());
    // The week's second month, when its Sunday is in the next one.
    let month2 = use_memo(move || {
        let p = period();
        match p.view {
            View::Week => week_months(p.monday()).1,
            View::Day | View::Month => None,
        }
    });
    let mut version = use_signal(|| 0u32);
    let expanded = use_signal(|| false);
    let mut message = use_signal(|| None as Message);
    let mut adding = use_signal(|| false);
    let mut sheet = use_signal(|| None::<super::day::Sheet>);
    let cal_sync = use_context::<Signal<super::CalendarSync>>();
    // Coming back to the app computes today again at once.
    let resumed = use_context::<Signal<super::Resumed>>();
    let content = use_context::<super::Content>();
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
    let mut loaded2 = use_resource(move || {
        let m = month2();
        let _ = cal_sync.read().version;
        let cfg = setup.read().config.clone();
        async move {
            match (cfg, m) {
                (Some(cfg), Some(m)) => Some(super::load(cfg, m).await),
                _ => None,
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
        let _ = resumed.read();
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
    // The same for the week's second month.
    let computed2 = use_resource(move || {
        let _ = minute();
        let _ = resumed.read();
        let _ = version();
        let cfg = setup.read().config.clone();
        let l = match &*loaded2.read() {
            Some(Some(Ok(l))) => Some(l.clone()),
            _ => None,
        };
        async move {
            match (cfg, l) {
                (Some(cfg), Some(l)) => Some(super::compute(cfg, l).await),
                _ => None,
            }
        }
    });
    // Another period clears the last private-time message and the open
    // forms.
    let change = use_callback(move |p: Period| {
        message.set(None);
        adding.set(false);
        sheet.set(None);
        period.set(p);
        content.scroll_to_top();
    });
    // A day in the list opens Day on that date.
    let go = use_callback(move |d: NaiveDate| change.call(Period::day(d)));
    // True when the action was done; a refused one leaves its editor open.
    // It runs with the calendar of the month of the day shown, or of
    // today: the week's second month when that is where it is.
    let on_action = use_callback(move |a: Action| -> bool {
        let Some(cfg) = setup.peek().config.clone() else {
            return false;
        };
        let p = *period.peek();
        let target = match p.view {
            View::Day => p.anchor,
            View::Week | View::Month => super::now(&cfg).date_naive(),
        };
        let second = month2.peek().is_some_and(|m| m.contains(target));
        let result = if second {
            match &*loaded2.peek() {
                Some(Some(Ok(l))) => apply(&cfg, &l.occurrences, &a).map_err(|e| format!("{e:#}")),
                _ => Err("the month is still loading".to_string()),
            }
        } else {
            match &*loaded.peek() {
                Some(Ok(l)) => apply(&cfg, &l.occurrences, &a).map_err(|e| format!("{e:#}")),
                _ => Err("the month is still loading".to_string()),
            }
        };
        let done = result.is_ok();
        if done && a.panel() == Panel::Trips {
            // The trips are reread; the day stays open.
            if second {
                loaded2.restart();
            } else {
                loaded.restart();
            }
        }
        message.set(Some((a.panel(), result)));
        version += 1;
        done
    });

    let Some(cfg) = setup.read().config.clone() else {
        return rsx! {};
    };
    let m = month();
    let p = period();
    let monday = p.monday();
    let m2 = month2();
    let today_now = super::now(&cfg).date_naive();
    let bar = Bar::picker(p, today_now, &HOURS_VIEWS, change);
    let head = || {
        bar.top(rsx! {
            button {
                class: "primary-sm wide-only",
                onclick: move |_| { let mut sc = screen; sc.set(Screen::Danlon) },
                Icon { d: Screen::Danlon.icon(), size: 15 }
                "Danløn app checklist"
            }
        })
    };
    // The phone's dock: the work buttons over the period row while today's
    // work is shown.
    let docked = |work: Option<(WorkToday, NaiveDateTime)>,
                  message: Option<Result<String, String>>| {
        dock(
            &bar,
            rsx! {
                if let Some((work, now)) = work {
                    {status(message)}
                    CardActions { work, now, on_action }
                }
            },
        )
    };
    let title = p.title(today_now).0;
    let failed = |e: &str| {
        rsx! {
            {head()}
            div { class: "alert", role: "alert",
                span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                div { b { "The month can't be shown: " } "{e}" }
            }
            {docked(None, None)}
        }
    };
    let loading = || {
        rsx! {
            {head()}
            p { class: "placeholder", role: "status", "Loading {title}…" }
            {docked(None, None)}
        }
    };
    let loaded = loaded.read();
    let loaded = match &*loaded {
        Some(Ok(l)) if l.month == m => l,
        Some(Err(e)) => return failed(e),
        _ => return loading(),
    };
    let computed = computed.read();
    let computed = match &*computed {
        Some(Some(Ok(c))) if c.month == m => c,
        Some(Some(Err(e))) => return failed(e),
        _ => return loading(),
    };
    let loaded2 = loaded2.read();
    let computed2 = computed2.read();
    let second: Option<(&Loaded, &Computed)> = match m2 {
        None => None,
        Some(m2) => {
            let l2 = match &*loaded2 {
                Some(Some(Ok(l))) if l.month == m2 => l,
                Some(Some(Err(e))) => return failed(e),
                _ => return loading(),
            };
            let c2 = match &*computed2 {
                Some(Some(Ok(c))) if c.month == m2 => c,
                Some(Some(Err(e))) => return failed(e),
                _ => return loading(),
            };
            Some((l2, c2))
        }
    };
    let pairs: Vec<(&Loaded, &Computed)> =
        std::iter::once((loaded, computed)).chain(second).collect();
    let view = &computed.view;
    let now = computed.now;
    let today = now.date_naive();

    // Today's work, when the period shown holds today.
    let today_work = pairs
        .iter()
        .find_map(|(_, c)| c.view.work.clone())
        .filter(|_| p.is_current(today));

    if p.view == View::Day
        && let Some((l, c)) = pairs.iter().find(|(l, _)| l.month.contains(p.anchor))
    {
        return rsx! {
            {head()}
            {super::day::render(super::day::DayArgs {
                cfg: &cfg,
                view: &c.view,
                state: &c.state,
                loaded: l,
                date: p.anchor,
                now,
                message: message(),
                on_action,
                adding,
                sheet,
            })}
            {docked(
                today_work.map(|w| (w, now.naive_local())),
                message_for(&message(), Panel::Private),
            )}
        };
    }

    let is_week = p.view == View::Week;
    // The current month or week; another one shows no work card and no
    // days to check.
    let current = p.is_current(today);
    let in_view = |d: &&Day| !is_week || in_week(monday, d.date);
    let days: Vec<&Day> = pairs
        .iter()
        .flat_map(|(_, c)| c.view.report.days.iter().filter(in_view))
        .collect();
    let to_check: Vec<&Day> = days
        .iter()
        .copied()
        .filter(|d| current && d.check.is_some())
        .collect();
    let totals = week::totals(&days, today);
    let diff = totals.difference();
    let trips_shown: usize = pairs
        .iter()
        .map(|(l, _)| {
            l.trips
                .iter()
                .map(|t| t.start.date_naive())
                .filter(|&d| l.month.contains(d) && (!is_week || in_week(monday, d)))
                .count()
        })
        .sum();
    let calendar_name = cfg
        .calendar
        .as_ref()
        .and_then(|c| c.name.clone())
        .map(|n| format!("Calendar “{n}”"))
        .unwrap_or_else(|| "Calendar".to_string());
    let has_calendar = cfg.calendar.is_some();
    // Today's work, from the month holding today, in the current period.
    let work_view = pairs
        .iter()
        .map(|(_, c)| &c.view)
        .find(|v| v.work.is_some())
        .filter(|_| current);
    // Today so far, when it is shown.
    let today_day = days.iter().copied().find(|d| d.date == today && d.so_far);
    let not_at_work_yet = matches!(
        work_view.and_then(|v| v.work.as_ref()).map(|w| &w.state),
        Some(WorkState::NotWorking { late_by: Some(_) })
    ) && today_day.is_some_and(|d| d.worked_hours == 0.0);
    let to_date = if current { " to date" } else { "" };
    // (label, label on a phone, value, class)
    let mut kpis = vec![
        (format!("Norm{to_date}"), None, hours(totals.norm), ""),
        (format!("Worked{to_date}"), None, hours(totals.worked), ""),
        (
            format!("Difference{to_date}"),
            None,
            signed(diff),
            diff_class(diff),
        ),
    ];
    if is_week {
        let logs: Vec<&MileageMonth> = pairs
            .iter()
            .filter_map(|(_, c)| c.view.mileage.as_ref())
            .collect();
        if !logs.is_empty() {
            let entries = logs.iter().flat_map(|mil| {
                mil.entries
                    .iter()
                    .filter(|e| mil.month.contains(e.start.date_naive()))
            });
            let (amount, _, _) = week_mileage(entries, monday);
            kpis.push((
                "Mileage allowance".to_string(),
                Some("Mileage"),
                kroner(amount),
                "",
            ));
        }
    } else if let Some(mil) = &view.mileage {
        kpis.push((
            "Mileage allowance".to_string(),
            Some("Mileage"),
            kroner(mil.total_amount()),
            "",
        ));
    }
    // The km reimbursed per day, for the client visits' note.
    let mut business_km: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for mil in pairs.iter().filter_map(|(_, c)| c.view.mileage.as_ref()) {
        for e in &mil.entries {
            *business_km.entry(e.start.date_naive()).or_default() += e.km;
        }
    }
    let first_check = to_check.first().map(|d| d.date);
    // Newest first, today on top; the days after today are not listed.
    let listed = week::listed(&days, today);
    let days_note = if current { "Totals to today" } else { "" };
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
    let work = work_view.and_then(|v| v.work.clone()).map(|mut w| {
        if let WorkState::AtWork { place, .. } = &mut w.state {
            *place = short(std::mem::take(place));
        }
        w
    });
    let items = work_view
        .map(|v| today_items(&v.private.items, today))
        .unwrap_or_default();
    let running_replaces = work_view
        .and_then(|v| v.private.running.as_ref())
        .and_then(|r| r.replaces.as_ref())
        .map(|w| (w.start, w.end));

    rsx! {
        {head()}

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
                "{trips_shown} trips"
            }
            if let Some(date) = first_check {
                button {
                    class: "src-chip warn wide-only",
                    onclick: move |_| go.call(date),
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
                running_replaces,
                today,
                now: now.naive_local(),
                tz: cfg.timezone,
                auto_start: cfg.private.auto_start,
                expanded,
                message: message_for(&message(), Panel::Private)
                    .or_else(|| message_for(&message(), Panel::Home)),
                on_action,
            }
        }

        div { class: "kpis",
            for (label , short , value , class) in kpis {
                section { class: "kpi",
                    if let Some(short) = short {
                        h2 { span { class: "wide-only", "{label}" } span { class: "narrow-only", "{short}" } }
                    } else {
                        h2 { "{label}" }
                    }
                    span { class: "kpi-value {class}", "{value}" }
                }
            }
        }

        for d in to_check.iter() {
            {
                let date = d.date;
                rsx! {
                    button {
                        class: "flag narrow-only",
                        onclick: move |_| go.call(date),
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

        {day_rows(&listed, &business_km, today, not_at_work_yet, days_note, go)}
        {docked(today_work.map(|w| (w, now.naive_local())), None)}
    }
}

/// The days, one row each: a table when wide, a list when narrow. Newest
/// first, today on top and marked; no days after today.
fn day_rows(
    days: &[&Day],
    business_km: &BTreeMap<NaiveDate, f64>,
    today: NaiveDate,
    not_at_work_yet: bool,
    note: &str,
    go: Callback<NaiveDate>,
) -> Element {
    rsx! {
        section { class: "panel wide-only", "aria-labelledby": "days-h",
            div { class: "panel-head",
                h2 { id: "days-h", "Days" }
                if !note.is_empty() {
                    span { class: "note", "{note}" }
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
                        for d in days.iter().copied() {
                            {
                                let date = d.date;
                                let is_today = date == today;
                                let row = if d.check.is_some() {
                                    "flagged"
                                } else if is_today {
                                    "today"
                                } else if quiet(d) {
                                    "quiet"
                                } else {
                                    ""
                                };
                                let remark = remark(d, business_km);
                                let place = if is_today && not_at_work_yet {
                                    "Not at work yet".to_string()
                                } else {
                                    place(d)
                                };
                                rsx! {
                                    tr { key: "{date}", class: "{row}", "aria-current": if is_today { "date" } else { "false" },
                                        td { class: "nowrap",
                                            button {
                                                class: "link-btn",
                                                onclick: move |_| go.call(date),
                                                {date.format("%-d %b").to_string()}
                                            }
                                            " "
                                            span { class: "muted", "{weekday(date)}" }
                                            if is_today {
                                                " "
                                                span { class: "chip today", "Today" }
                                            }
                                        }
                                        td { class: "nowrap", "{planned(d)}" }
                                        td { "{place}" }
                                        td { class: "mono small", "{d.periods_text()}" }
                                        td { class: "r", "{hours(d.worked_hours)}" }
                                        td { class: "r", "{hours(d.norm_hours)}" }
                                        td { class: "r strong {diff_class(d.difference())}",
                                            "{signed(d.difference())}"
                                            if d.so_far {
                                                span { class: "muted small", " so far" }
                                            }
                                        }
                                        td {
                                            if !remark.is_empty() {
                                                span { class: "{note_class(d)}", "{remark}" }
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
                for d in days.iter().copied() {
                    {
                        let date = d.date;
                        let is_today = date == today;
                        let mut remark = remark(d, business_km);
                        let place = if is_today && not_at_work_yet {
                            "Not at work yet".to_string()
                        } else {
                            place(d)
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
                        } else if quiet(d) {
                            "day-row quiet"
                        } else {
                            "day-row"
                        };
                        let chip = if is_today && not_at_work_yet { "chip info" } else { note_class(d) };
                        rsx! {
                            li { key: "{date}", "aria-current": if is_today { "date" } else { "false" },
                                button {
                                    class: "{class}",
                                    onclick: move |_| go.call(date),
                                    span { class: "day-num",
                                        b { "{date.day()}" }
                                        if is_today {
                                            span { class: "today-label", "Today" }
                                        } else {
                                            span { "{weekday(date)}" }
                                        }
                                    }
                                    span { class: "day-mid",
                                        span { class: "ellipsis", "{place}" }
                                        if !remark.is_empty() {
                                            span { class: "{chip} ellipsis", "{remark}" }
                                        }
                                    }
                                    span { class: "day-nums",
                                        if !quiet(d) || d.so_far && d.expected_hours > 0.0 {
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

/// The head of the work card: kicker, title and tone (the dot's, the
/// title's and the card's class).
struct WorkHead {
    kicker: String,
    title: String,
    tone: &'static str,
}

/// "07:58", or "Mon 23 Nov 16:00" for a time on another day.
fn since_text(t: NaiveDateTime, now: NaiveDateTime) -> String {
    if t.date() == now.date() {
        hm(t)
    } else {
        t.format("%a %-d %b %H:%M").to_string()
    }
}

/// Minutes worked shown live: those computed at `computed_at`, plus the
/// time since then up to `live` (the card ticks between the minute
/// recomputes).
fn live_minutes(computed: i64, computed_at: NaiveDateTime, live: NaiveDateTime) -> i64 {
    computed + (live - computed_at).num_minutes().max(0)
}

/// The work card's head at `now` (live); `computed_at` is when `w` was
/// computed.
fn work_head(w: &WorkToday, computed_at: NaiveDateTime, now: NaiveDateTime) -> WorkHead {
    let counting = matches!(w.state, WorkState::AtWork { .. } | WorkState::Home { .. });
    let worked = if counting {
        hmm(live_minutes(minutes(w.worked), computed_at, now))
    } else {
        hmm(minutes(w.worked))
    };
    let at = |t: DateTime<Tz>| hm(t.naive_local());
    let not_working = || {
        if w.worked > 0.0 {
            format!("Not working · {worked}")
        } else {
            "Not working".into()
        }
    };
    match &w.state {
        WorkState::NotWorking { late_by: Some(_) } => WorkHead {
            kicker: match w.planned {
                Some((s, _)) => format!("Planned from {}", at(s)),
                None => String::new(),
            },
            title: format!("Not at work · {}", hmm(minutes(w.difference()))),
            tone: "bad",
        },
        WorkState::NotWorking { late_by: None } => match w.planned {
            Some((s, e)) => WorkHead {
                kicker: format!("Planned {}–{}", at(s), at(e)),
                title: if now < s.naive_local() && w.worked == 0.0 {
                    "Not working yet".into()
                } else {
                    not_working()
                },
                tone: "",
            },
            None => WorkHead {
                kicker: "Nothing planned".into(),
                title: not_working(),
                tone: "",
            },
        },
        WorkState::AtWork { place, since } => WorkHead {
            kicker: if place.is_empty() {
                "At work".into()
            } else {
                format!("At work · {place}")
            },
            title: format!("Since {} · {worked}", at(*since)),
            tone: "on",
        },
        WorkState::Home { since } => WorkHead {
            kicker: "Working from home".into(),
            title: format!("Since {} · {worked}", at(*since)),
            tone: "on",
        },
        WorkState::Private { label, since, .. } => {
            let since = since.naive_local();
            let elapsed = hmm((now - since).num_minutes().max(0));
            WorkHead {
                kicker: format!("Private time · {label}"),
                title: format!("Since {} · {elapsed}", since_text(since, now)),
                tone: "private",
            }
        }
    }
}

/// Today's work: on a narrow screen the status, then one row of buttons
/// ([`CardActions`]); the next private appointment with its own Start, and
/// today's private list under the chevron. On a wide screen the head and
/// buttons are the header's [`WorkPill`].
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_work_card_counts_on_between_the_minutes() {
        let at = |s: &str| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap();
        let computed = at("2026-11-24 12:10:05");
        assert_eq!(live_minutes(312, computed, computed), 312);
        assert_eq!(live_minutes(312, computed, at("2026-11-24 12:10:50")), 312);
        assert_eq!(live_minutes(312, computed, at("2026-11-24 12:11:05")), 313);
        assert_eq!(live_minutes(312, computed, at("2026-11-24 14:40:05")), 462);
        // A clock behind the computed time adds nothing.
        assert_eq!(live_minutes(312, computed, at("2026-11-24 12:00:00")), 312);
    }
}

#[component]
fn WorkCard(
    work: WorkToday,
    items: Vec<PrivateItem>,
    running_replaces: Option<(NaiveDateTime, NaiveDateTime)>,
    today: NaiveDate,
    now: NaiveDateTime,
    tz: Tz,
    auto_start: bool,
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
    let head = work_head(&work, now, live);
    let open = expanded();
    let private_now = match &work.state {
        WorkState::Private { label, since, auto } => Some((label.clone(), *since, *auto)),
        _ => None,
    };
    // An appointment started automatically can get its real start.
    let auto_since = private_now
        .as_ref()
        .filter(|p| p.2)
        .map(|(_, since, _)| since.naive_local());
    let next = work.next_private.clone();
    let next_today = next
        .as_ref()
        .and_then(|i| i.planned)
        .is_some_and(|(s, _)| s.date() == today);
    let current = next
        .as_ref()
        .and_then(|n| items.iter().position(|i| i == n));
    let card_class = match head.tone {
        "" => "work-card".to_string(),
        tone => format!("work-card {tone}"),
    };
    rsx! {
        section { class: "{card_class}", "aria-label": "Work today",
            div { class: "pt-head narrow-only",
                span { class: "pt-dot {head.tone}", "aria-hidden": "true" }
                span { class: "pt-title",
                    if !head.kicker.is_empty() {
                        span { class: "pt-kicker", "{head.kicker}" }
                    }
                    b { class: "{head.tone}", "{head.title}" }
                }
            }
            if let Some(since) = auto_since {
                AutoStart { since, on_action }
            }
            {status(message)}
            div { class: "next-row",
                span { class: "pt-title",
                    if let Some((label, since, _)) = &private_now {
                        span { class: "pt-kicker", "Running now" }
                        {
                            let when = running_replaces
                                .map(|(s, e)| format!("{}–{}", hm(s), hm(e)))
                                .unwrap_or_else(|| format!("since {}", hm(since.naive_local())));
                            rsx! { span { class: "next-line", b { "{label}" } " · {when}" } }
                        }
                    } else if let Some(n) = &next {
                        span { class: "pt-kicker", "Next private" }
                        {
                            let (s, e) = n.planned.unwrap_or((n.start(), n.start()));
                            let when = if next_today {
                                format!("{}–{}", hm(s), hm(e))
                            } else {
                                super::when(s, Some(e), today)
                            };
                            rsx! { span { class: "next-line", b { "{n.label}" } " · {when}" } }
                        }
                    } else {
                        span { class: "pt-kicker", "Next private" }
                        span { class: "muted", "None planned" }
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
                    "aria-label": if open { "Hide private time list" } else { "Show private time list" },
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
        }
    }
}

/// The work buttons in the phone's dock: Start work ▾ when not working;
/// Private time ▾ and Stop work at work or at home; Stop private time while
/// it runs. ▾ opens "Started at" above them, for a start earlier today.
#[component]
fn CardActions(work: WorkToday, now: NaiveDateTime, on_action: Callback<Action, bool>) -> Element {
    let mut earlier = use_signal(|| false);
    let mut at = use_signal(String::new);
    let has = |a: WorkAction| work.actions.contains(&a);
    let (main, main_action, caret_label, stop) = match &work.state {
        WorkState::NotWorking { .. } if has(WorkAction::StartWork) => (
            "Start work",
            Action::HomeStart,
            Some("Started work earlier"),
            None,
        ),
        WorkState::NotWorking { .. } => return rsx! {},
        WorkState::Private { .. } => ("Stop private time", Action::Stop, None, None),
        WorkState::AtWork { .. } | WorkState::Home { .. } => (
            "Private time",
            Action::Start(None),
            Some("Private time started earlier"),
            if has(WorkAction::StopWork) {
                Some(Action::HomeStop)
            } else if has(WorkAction::LeaveWork) {
                Some(Action::LeaveWork)
            } else {
                None
            },
        ),
    };
    let working = matches!(
        work.state,
        WorkState::AtWork { .. } | WorkState::Home { .. }
    );
    let private = matches!(work.state, WorkState::Private { .. });
    let open = earlier() && caret_label.is_some();
    rsx! {
        if open {
            div { id: "pt-earlier-row", class: "card-earlier dock-earlier",
                label { r#for: "pt-earlier-card", "Started at" }
                input {
                    id: "pt-earlier-card",
                    r#type: "time",
                    value: "{at}",
                    oninput: move |e| at.set(e.value()),
                }
                button {
                    class: "small-btn primary-sm",
                    onclick: move |_| {
                        let action = if working {
                            Action::StartAt(at())
                        } else {
                            Action::HomeStartAt(at())
                        };
                        if on_action.call(action) {
                            earlier.set(false);
                        }
                    },
                    "Start"
                }
            }
        }
        div { class: "dock-work",
            span { class: "split dock-split", role: "group", "aria-label": "{main}",
                button {
                    class: if private { "pt-btn private" } else if working { "pt-btn split-main working" } else { "pt-btn split-main" },
                    onclick: move |_| {
                        earlier.set(false);
                        on_action.call(main_action.clone());
                    },
                    "{main}"
                }
                if let Some(label) = caret_label {
                    button {
                        class: if working { "pt-btn split-caret working" } else { "pt-btn split-caret" },
                        "aria-expanded": if open { "true" } else { "false" },
                        "aria-controls": "pt-earlier-row",
                        "aria-label": "{label}",
                        onclick: move |_| {
                            at.set(hm(now));
                            earlier.set(!open);
                        },
                        Icon { d: "m6 9 6 6 6-6", size: 16 }
                    }
                }
            }
            if let Some(action) = stop {
                button {
                    class: "pt-btn stop-work",
                    onclick: move |_| {
                        earlier.set(false);
                        on_action.call(action.clone());
                    },
                    "Stop work"
                }
            }
        }
    }
}

/// The work buttons for the wide header: Start work, Stop private, or the
/// split button Private time ▾ whose menu starts private time now or
/// earlier and stops work.
#[component]
fn WorkButtons(work: WorkToday, on_action: Callback<Action, bool>) -> Element {
    let mut menu = use_signal(|| false);
    let mut earlier = use_signal(|| false);
    let mut at = use_signal(String::new);
    let has = |a: WorkAction| work.actions.contains(&a);
    match &work.state {
        WorkState::NotWorking { .. } => rsx! {
            if has(WorkAction::StartWork) {
                button {
                    class: "pill-btn",
                    onclick: move |_| {
                        on_action.call(Action::HomeStart);
                    },
                    "Start work"
                }
            }
        },
        WorkState::Private { .. } => rsx! {
            button {
                class: "pill-btn private",
                onclick: move |_| {
                    on_action.call(Action::Stop);
                },
                "Stop private time"
            }
        },
        WorkState::AtWork { .. } | WorkState::Home { .. } => {
            let stop_item = if has(WorkAction::StopWork) {
                Some(Action::HomeStop)
            } else if has(WorkAction::LeaveWork) {
                Some(Action::LeaveWork)
            } else {
                None
            };
            let open = menu();
            let mut close = move || {
                menu.set(false);
                earlier.set(false);
            };
            rsx! {
                span { class: "split pill", role: "group", "aria-label": "Work actions",
                    button {
                        class: "pill-btn split-main",
                        onclick: move |_| {
                            close();
                            on_action.call(Action::Start(None));
                        },
                        "Private time"
                    }
                    button {
                        class: "pill-btn split-caret",
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
                                    "Private time now"
                                }
                                button {
                                    class: "menu-item",
                                    role: "menuitem",
                                    onclick: move |_| {
                                        at.set(hm(chrono::Local::now().naive_local()));
                                        earlier.set(true);
                                    },
                                    "Private time started at…"
                                }
                                if let Some(action) = stop_item {
                                    div { class: "menu-sep", role: "separator" }
                                    button {
                                        class: "menu-item",
                                        role: "menuitem",
                                        onclick: move |_| {
                                            close();
                                            on_action.call(action.clone());
                                        },
                                        "Stop work"
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
            format!(" · {}", hmm(minutes(work.difference()))),
        ),
        WorkState::NotWorking { late_by: None } => ("", "Not working".to_string(), String::new()),
        WorkState::AtWork { place, since } => (
            "on",
            if place.is_empty() {
                "At work".to_string()
            } else {
                format!("At work · {place}")
            },
            format!(" since {}", at(*since)),
        ),
        WorkState::Home { since } => (
            "on",
            "Working from home".to_string(),
            format!(" since {}", at(*since)),
        ),
        WorkState::Private { label, since, .. } => (
            "private",
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
            let when = if s.date() == today {
                hm(s)
            } else {
                s.format("%a %-d %b %H:%M").to_string()
            };
            (label, format!(" {when}"))
        });
    rsx! {
        span { class: "src-chip work-pill {class}",
            span { class: "dot", "aria-hidden": "true" }
            b { "{label}" }
            "{rest}"
            WorkButtons { work: work.clone(), on_action }
        }
        if let Some((label, when)) = next {
            span { class: "next-note",
                "Next: "
                b { "{label}" }
                "{when}"
            }
        }
    }
}

/// An appointment started automatically: "Started at …" sets its real
/// start.
#[component]
fn AutoStart(since: NaiveDateTime, on_action: Callback<Action, bool>) -> Element {
    let mut editing = use_signal(|| false);
    let mut at = use_signal(|| hm(since));
    rsx! {
        div { class: "card-earlier narrow-only",
            if editing() {
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
            } else {
                span { class: "muted small grow", "Started by itself at {hm(since)}" }
                button {
                    class: "small-btn",
                    onclick: move |_| editing.set(true),
                    "Started at…"
                }
            }
        }
    }
}
