//! The Month screen: month navigation, the private-time list, the totals
//! and one row per day, as `report` computes them. A day opens the Day
//! screen.

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use chrono_tz::Tz;
use dioxus::prelude::*;
use vw_danlon::month::{Day, Month, OffReason};
use vw_danlon::service::{self, PrivateState, PrivateTime};
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
        None if d.norm_hours == 0.0 => "Weekend".to_string(),
        None => String::new(),
    }
}

fn note_class(d: &Day) -> &'static str {
    if d.check.is_some() {
        "chip warn strong"
    } else if matches!(d.off_reason, Some(OffReason::Leave { .. })) {
        "chip"
    } else if !d.appointments.is_empty() {
        "chip info"
    } else {
        "chip"
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
    let _ = version();
    let m = month();
    let now = super::now(&cfg);
    let today = now.date_naive();
    let loaded = loaded.read();
    let loaded = match &*loaded {
        None => {
            return rsx! {
                MonthHeader { month, screen }
                p { class: "placeholder", role: "status", "Loading {month_title(m)}…" }
            };
        }
        Some(Err(e)) => {
            return rsx! {
                MonthHeader { month, screen }
                div { class: "alert", role: "alert",
                    span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                    div { b { "The month can't be shown: " } "{e}" }
                }
            };
        }
        Some(Ok(l)) => l,
    };
    let view = match service::month_view(&cfg, m, &loaded.trips, &loaded.occurrences, now) {
        Ok(v) => v,
        Err(e) => {
            return rsx! {
                MonthHeader { month, screen }
                div { class: "alert", role: "alert", "{e:#}" }
            };
        }
    };

    if let Some(date) = open_day().filter(|d| m.contains(*d)) {
        return super::day::render(super::day::DayArgs {
            cfg: &cfg,
            view: &view,
            loaded,
            date,
            now: now.naive_local(),
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
    let mut kpis = vec![
        (
            "Norm hours",
            hours(report.norm_total()),
            format!("{working_days} working days · {vacation_days} vacation days"),
            "",
        ),
        (
            "Worked",
            hours(report.worked_total()),
            "after lunch and private time".to_string(),
            "",
        ),
        (
            "Difference",
            signed(diff),
            match to_check.len() {
                0 => "nothing to check".to_string(),
                1 => "1 day to check".to_string(),
                n => format!("{n} days to check"),
            },
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
        }
        if let Some(w) = &loaded.calendar_warning {
            div { class: "alert", role: "status", "{w}" }
        }
        if let (Some(e), Some(at)) = (&cal_sync.read().error, loaded.calendar_fetched_at) {
            div { class: "alert", role: "status",
                "The calendar can't be refreshed ({e}). Showing the copy {super::calendar_age(at, now)}."
            }
        }

        PrivateCard {
            private: view.private.clone(),
            today,
            now: now.naive_local(),
            tz: cfg.timezone,
            expanded,
            message: message_for(&message(), Panel::Private),
            on_action,
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
                    "{lunch_rule}"
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
                                let row = if d.check.is_some() {
                                    "flagged"
                                } else if quiet(d) {
                                    "quiet"
                                } else {
                                    ""
                                };
                                let remark = d.remark();
                                rsx! {
                                    tr { class: "{row}",
                                        td { class: "nowrap",
                                            button {
                                                class: "link-btn",
                                                onclick: move |_| go.call(Some(date)),
                                                {date.format("%-d %b").to_string()}
                                            }
                                            " "
                                            span { class: "muted", "{weekday_da(date.weekday())}" }
                                        }
                                        td { class: "nowrap", "{planned(d)}" }
                                        td { "{place(d)}" }
                                        td { class: "mono small", "{d.periods_text()}" }
                                        td { class: "r", "{hours(d.worked_hours)}" }
                                        td { class: "r", "{hours(d.norm_hours)}" }
                                        td { class: "r strong {diff_class(d.difference())}", "{signed(d.difference())}" }
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
                for d in report.days.iter() {
                    {
                        let date = d.date;
                        let remark = d.remark();
                        rsx! {
                            li {
                                button {
                                    class: if quiet(d) { "day-row quiet" } else { "day-row" },
                                    onclick: move |_| go.call(Some(date)),
                                    span { class: "day-num",
                                        b { "{date.day()}" }
                                        span { "{weekday_da(date.weekday())}" }
                                    }
                                    span { class: "day-mid",
                                        span { class: "ellipsis", "{place(d)}" }
                                        if !remark.is_empty() {
                                            span { class: "{note_class(d)} ellipsis", "{remark}" }
                                        }
                                    }
                                    span { class: "day-nums",
                                        if !quiet(d) {
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

/// The private-time list: collapsed, the current or next entry with
/// Start/Stop and the running timer's status; expanded, today's and the
/// coming entries and "Start without an event".
#[component]
fn PrivateCard(
    private: PrivateTime,
    today: NaiveDate,
    now: NaiveDateTime,
    tz: Tz,
    expanded: Signal<bool>,
    message: Option<Result<String, String>>,
    on_action: Callback<Action, bool>,
) -> Element {
    let open = expanded();
    let current = private.current.map(|i| private.items[i].clone());
    let running = private.running.clone();
    let (kicker, title) = match &current {
        Some(item) => {
            let at = match item.planned {
                Some((s, e)) if s.date() == today => format!("{}–{}", hm(s), hm(e)),
                Some((s, _)) => s.format("%a %-d %b %H:%M").to_string(),
                None => format!("since {}", hm(item.start())),
            };
            let now_on = matches!(item.state, PrivateState::Running { .. })
                || item.planned.is_some_and(|(s, e)| s <= now && now < e);
            (
                if now_on {
                    "Private time · now"
                } else {
                    "Private time · next"
                },
                format!("{} · {at}", item.label),
            )
        }
        None => ("Private time", "No private appointments coming".to_string()),
    };
    // Start links the shown appointment when it is today.
    let link = current
        .as_ref()
        .filter(|i| i.state == PrivateState::Planned)
        .filter(|i| i.planned.is_some_and(|(s, _)| s.date() == today))
        .map(|i| i.label.clone());
    let is_running = running.is_some();

    rsx! {
        section {
            class: if is_running { "pt-card running" } else { "pt-card" },
            "aria-label": "Private time",
            div { class: "pt-head",
                span { class: if is_running { "pt-dot on" } else { "pt-dot" }, "aria-hidden": "true" }
                span { class: "pt-title",
                    span { class: "pt-kicker", "{kicker}" }
                    b { class: "ellipsis", "{title}" }
                }
                if is_running {
                    button {
                        class: "pt-btn stop",
                        "aria-pressed": "true",
                        onclick: move |_| {
                            on_action.call(Action::Stop);
                        },
                        "Stop"
                    }
                } else {
                    button {
                        class: "pt-btn",
                        "aria-pressed": "false",
                        onclick: move |_| {
                            on_action.call(Action::Start(link.clone()));
                        },
                        "Start"
                    }
                }
                button {
                    class: "icon-btn",
                    "aria-expanded": if open { "true" } else { "false" },
                    "aria-controls": "pt-list",
                    "aria-label": if open { "Hide private time list" } else { "Show private time list" },
                    onclick: move |_| expanded.set(!open),
                    span { class: if open { "chevron up" } else { "chevron" },
                        Icon { d: "m6 9 6 6 6-6", size: 22 }
                    }
                }
            }
            if let Some(r) = running {
                RunningStatus {
                    since: r.start,
                    planned: r.replaces.as_ref().map(|w| (w.start, w.end)),
                    tz,
                }
            }
            {status(message)}
            if open {
                ul { id: "pt-list", class: "pt-list",
                    PrivateList {
                        items: private.items.clone(),
                        current: private.current,
                        today,
                        on_action,
                    }
                    li { class: "pt-item",
                        button {
                            class: "pt-plain",
                            disabled: is_running,
                            onclick: move |_| {
                                on_action.call(Action::Start(None));
                            },
                            Icon { d: "M12 5v14M5 12h14", size: 16 }
                            "Start without an event"
                        }
                    }
                }
            }
        }
    }
}

/// "Running 0:43 · since 12:30 · planned 13:00–14:00", kept current.
#[component]
fn RunningStatus(
    since: NaiveDateTime,
    planned: Option<(NaiveDateTime, NaiveDateTime)>,
    tz: Tz,
) -> Element {
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            tick += 1;
        }
    });
    let _ = tick();
    let now = chrono::Utc::now().with_timezone(&tz).naive_local();
    let minutes = (now - since).num_minutes().max(0);
    let forgotten = vw_danlon::state::RunningTimer {
        start: since,
        label: String::new(),
        replaces: None,
    }
    .forgotten(now);
    let since_text = if since.date() == now.date() {
        hm(since)
    } else {
        since.format("%a %-d %b %H:%M").to_string()
    };
    let planned = planned
        .map(|(s, e)| format!(" · planned {}–{} · counts travel and waiting", hm(s), hm(e)))
        .unwrap_or_default();
    rsx! {
        div { class: "pt-status warn", role: "status",
            b { "Running {minutes / 60}:{minutes % 60:02}" }
            span { " since {since_text}{planned}" }
            if forgotten {
                span { " · still running? Stop it and fix the end time" }
            }
        }
    }
}
