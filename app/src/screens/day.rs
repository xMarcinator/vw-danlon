//! The Day screen: the day's numbers (today: worked and expected so far,
//! the difference and how late), a timeline of planned time, time at work,
//! private time, lunch and driving (today with the planned start, now and
//! the planned end), the lunch break and where it comes from (changeable),
//! the private time (recorded intervals editable) and the trips, where a
//! missing trip can be added by hand.

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use chrono_tz::Tz;
use dioxus::prelude::*;
use vw_danlon::config::Config;
use vw_danlon::mileage::Status;
use vw_danlon::month::{Day, LunchSource};
use vw_danlon::service::{self, LunchChoice, ManualTripForm, MonthView, PrivateItem, PrivateState};
use vw_danlon::state::State;

use super::{
    Action, Loaded, Message, Panel, PrivateList, diff_class, hm, hours, kroner, message_for,
    signed, status,
};
use crate::Icon;

pub struct DayArgs<'a> {
    pub cfg: &'a Config,
    pub view: &'a MonthView,
    pub loaded: &'a Loaded,
    /// The state the view was built from.
    pub state: &'a State,
    pub date: NaiveDate,
    pub now: DateTime<Tz>,
    pub message: Message,
    pub on_action: Callback<Action, bool>,
    pub on_back: Callback<()>,
    /// Whether the add-trip form is open.
    pub adding: Signal<bool>,
    /// The add or edit time sheet, when open.
    pub sheet: Signal<Option<Sheet>>,
}

/// The time sheet: adding private time or home working to a day, or
/// editing a recorded interval (`edit`: its id).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Sheet {
    pub edit: Option<u32>,
    pub home: bool,
    pub label: String,
    pub start: String,
    pub end: String,
}

impl Sheet {
    fn edit(home: bool, id: u32, label: &str, start: NaiveDateTime, end: NaiveDateTime) -> Self {
        Sheet {
            edit: Some(id),
            home,
            label: label.to_string(),
            start: hm(start),
            end: hm(end),
        }
    }
}

/// A recorded home-working interval of the day, or the timer running.
struct HomeEntry {
    id: Option<u32>,
    label: String,
    start: NaiveDateTime,
    end: Option<NaiveDateTime>,
}

fn home_entries(state: &State, date: NaiveDate) -> Vec<HomeEntry> {
    let mut out: Vec<HomeEntry> = state
        .home
        .intervals
        .iter()
        .filter(|i| i.start.date() == date)
        .map(|i| HomeEntry {
            id: Some(i.id),
            label: i.label.clone(),
            start: i.start,
            end: Some(i.end),
        })
        .collect();
    if let Some(r) = state
        .home
        .running
        .as_ref()
        .filter(|r| r.start.date() == date)
    {
        out.push(HomeEntry {
            id: None,
            label: r.label.clone(),
            start: r.start,
            end: None,
        });
    }
    out.sort_by_key(|e| e.start);
    out
}

/// How a home-working entry is titled: its label, or "Worked from home"
/// for the default one.
fn home_title(label: &str) -> String {
    if label == vw_danlon::state::HOME_LABEL {
        "Worked from home".to_string()
    } else {
        label.to_string()
    }
}

fn lunch_source(s: LunchSource) -> &'static str {
    match s {
        LunchSource::Default => "Default",
        LunchSource::Calendar => "From the calendar",
        LunchSource::Override => "Set by you",
    }
}

fn trip_kind(s: &Option<Status>) -> (&'static str, &'static str) {
    match s {
        Some(Status::Business) => ("Business", "chip acc strong"),
        Some(Status::Commute) => ("Commute", "chip"),
        Some(Status::Private) => ("Private", "chip"),
        Some(Status::Incomplete(_)) => ("Incomplete", "chip warn"),
        None => ("", "chip"),
    }
}

/// "25 min", "1 h 10 min".
fn minutes_text(m: i64) -> String {
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// The private items' times: as measured, else as planned.
fn private_span(i: &PrivateItem, now: NaiveDateTime) -> Option<(NaiveDateTime, NaiveDateTime)> {
    match i.state {
        PrivateState::Recorded { start, end, .. } => Some((start, end)),
        PrivateState::Running { since } => Some((since, now.max(since))),
        PrivateState::StartedAutomatically { .. } | PrivateState::Planned => i.planned,
    }
}

/// One step of the phone timeline.
struct Step {
    at: NaiveDateTime,
    title: String,
    sub: String,
    kind: &'static str,
    /// Private time or home working recorded: Edit opens it.
    edit: Option<Sheet>,
}

pub fn render(a: DayArgs) -> Element {
    let DayArgs {
        cfg,
        view,
        state,
        loaded,
        date,
        now: now_tz,
        message,
        on_action,
        on_back,
        mut adding,
        mut sheet,
    } = a;
    let Some(d) = view.report.days.iter().find(|d| d.date == date) else {
        return rsx! {};
    };
    let now = now_tz.naive_local();
    let private = service::private_time(
        &service::timed_entries(cfg, &loaded.occurrences),
        state,
        &cfg.private,
        date,
        date,
        now,
    );
    let trips = service::day_trips(&loaded.trips, view.mileage.as_ref(), date);
    let (km, amount) = view
        .mileage
        .as_ref()
        .map(|m| {
            m.entries
                .iter()
                .filter(|e| e.start.date_naive() == date)
                .fold((0.0, 0.0), |(k, a), e| (k + e.km, a + e.amount()))
        })
        .unwrap_or_default();
    let month_title = date.format("%B %Y").to_string();
    let title = if date == now_tz.date_naive() {
        date.format("Today · %a %-d %b").to_string()
    } else {
        date.format("%A %-d %B").to_string()
    };
    let planned = d
        .planned
        .map(|(s, e)| format!("{}–{}", s.format("%H:%M"), e.format("%H:%M")));
    let place = d.work_locations.join(" | ");
    let planned_text = planned.unwrap_or_else(|| "Nothing planned".into());
    let km_text = format!("{km:.1}").replace('.', ",");
    let (lunch_window, lunch_minutes) = match d.lunch.window {
        Some((s, e)) => (
            format!("{}–{}", s.format("%H:%M"), e.format("%H:%M")),
            (e - s).num_minutes(),
        ),
        None => ("No lunch".to_string(), 0),
    };
    let today = now.date();
    // Today so far: the planned start counts, not the arrival.
    let so_far = d.so_far;
    let late = match (d.planned, d.first_start) {
        (Some((s, _)), Some(first)) if first > s => Some((first - s).num_minutes()),
        (Some((s, _)), None) if so_far && s < now_tz => Some((now_tz - s).num_minutes()),
        _ => None,
    };
    let late_text = match (late, d.first_start) {
        (Some(m), Some(_)) => format!("{} late", minutes_text(m)),
        (Some(m), None) => format!("not at work yet · {} late", minutes_text(m)),
        (None, Some(_)) if d.planned.is_some() => "on time".to_string(),
        _ => String::new(),
    };
    let worked_sub = match (d.first_start, d.last_end) {
        (Some(first), Some(last)) if so_far && last == now_tz => {
            format!("{} – now", first.format("%H:%M"))
        }
        (None, _) if so_far => "not at work yet".to_string(),
        _ => d.periods_text(),
    };
    let expected_sub = match d.planned.filter(|(s, _)| *s < now_tz) {
        Some((s, e)) => format!(
            "{}–{} of {}",
            s.format("%H:%M"),
            e.min(now_tz).format("%H:%M"),
            hours(d.norm_hours)
        ),
        None => format!("of {}", hours(d.norm_hours)),
    };
    let left_work = state.left_work.get(&date).copied();
    let home = home_entries(state, date);
    let planned_hm = d
        .planned
        .map(|(s, e)| (s.format("%H:%M").to_string(), e.format("%H:%M").to_string()));
    let open_add = move |_| {
        sheet.set(if sheet.peek().as_ref().is_some_and(|s| s.edit.is_none()) {
            None
        } else {
            Some(Sheet::default())
        })
    };

    rsx! {
        nav { class: "crumbs wide-only", "aria-label": "Breadcrumb",
            button { class: "link-btn", onclick: move |_| on_back.call(()), "{month_title}" }
            span { class: "muted", {format!(" / {}", date.format("%-d %b"))} }
        }
        div { class: "screen-head",
            button {
                class: "icon-btn narrow-only back",
                "aria-label": "Back to {month_title}",
                onclick: move |_| on_back.call(()),
                Icon { d: "m15 6-6 6 6 6", size: 22 }
            }
            h1 { class: "grow", "{title}" }
            button {
                class: "small-btn wide-only",
                "aria-expanded": if adding() { "true" } else { "false" },
                onclick: move |_| adding.set(!adding()),
                Icon { d: "M12 5v14M5 12h14", size: 15 }
                "Add missing trip"
            }
            button {
                class: "small-btn acc wide-only",
                "aria-expanded": if sheet().is_some_and(|s| s.edit.is_none()) { "true" } else { "false" },
                onclick: open_add,
                Icon { d: "M12 5v14M5 12h14", size: 15 }
                "Add time"
            }
        }

        div { class: "kpis",
            section { class: "kpi wide-only",
                h2 { "Calendar" }
                span { class: "kpi-text",
                    if let Some(r) = &d.off_reason {
                        "{r}"
                    } else if place.is_empty() {
                        "No work address"
                    } else {
                        "{place}"
                    }
                }
                span { class: "kpi-sub", "{planned_text}" }
            }
            section { class: "kpi",
                h2 { if so_far { "Worked so far" } else { "Worked" } }
                span { class: "kpi-value", "{hours(d.worked_hours)}" }
                span { class: "kpi-sub", "{worked_sub}" }
            }
            if so_far {
                section { class: "kpi",
                    h2 { "Expected so far" }
                    span { class: "kpi-value", "{hours(d.expected_hours)}" }
                    span { class: "kpi-sub", "{expected_sub}" }
                }
            }
            section { class: "kpi",
                h2 { if so_far { "Diff" } else { "Difference" } }
                span { class: "kpi-value {diff_class(d.difference())}", "{signed(d.difference())}" }
                span { class: "kpi-sub",
                    if so_far {
                        "{late_text}"
                    } else {
                        "norm {hours(d.norm_hours)} h"
                    }
                }
            }
            if view.mileage.is_some() {
                section { class: "kpi",
                    h2 { "Mileage" }
                    span { class: "kpi-value", "{kroner(amount)}" }
                    span { class: "kpi-sub", "{km_text} km business" }
                }
            }
        }

        if let Some(note) = &d.check {
            div { class: "alert", role: "status",
                span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                span { "{note}" }
            }
        }

        if let Some(at) = left_work {
            div { class: "alert info", role: "status",
                span { class: "grow",
                    "Left work at {hm(at)} (set by you)"
                }
                button {
                    class: "small-btn",
                    onclick: move |_| {
                        on_action.call(Action::UndoLeaveWork(date));
                    },
                    "Undo"
                }
            }
        }
        {status(message_for(&message, Panel::Home))}

        if sheet().is_some() {
            TimeSheet { date, sheet, planned: planned_hm, on_action }
        }

        {timeline(d, &private.items, &home, &trips, now_tz, sheet)}

        LunchPanel {
            date,
            window: lunch_window,
            minutes: lunch_minutes,
            source: lunch_source(d.lunch.source),
            title: cfg.lunch.title.clone(),
            start: d
                            .lunch
                .window
                .map(|(s, _)| s.format("%H:%M").to_string())
                .unwrap_or_else(|| cfg.lunch.start.format("%H:%M").to_string()),
            default_minutes: cfg.lunch.minutes,
            message: message_for(&message, Panel::Lunch),
            on_action,
        }

        section { class: "panel wide-only", "aria-labelledby": "priv-h",
            div { class: "panel-head",
                h2 { id: "priv-h", "Private time and home working" }
            }
            {status(message_for(&message, Panel::Private))}
            if private.items.is_empty() && home.is_empty() {
                p { class: "empty", "Nothing recorded." }
            } else {
                ul { class: "pt-list",
                    PrivateList {
                        items: private
                            .items
                            .iter()
                            .filter(|i| !matches!(i.state, PrivateState::Recorded { .. }))
                            .cloned()
                            .collect::<Vec<_>>(),
                        current: None,
                        today,
                        auto_start: cfg.private.auto_start,
                        on_action,
                    }
                    for i in private.items.iter() {
                        if let PrivateState::Recorded { id, start, end, .. } = i.state {
                            {
                                let edit = Sheet::edit(false, id, &i.label, start, end);
                                rsx! {
                                    li { class: "pt-item",
                                        div { class: "pt-row",
                                            span { class: "pt-text",
                                                span {
                                                    b { "{i.label}" }
                                                    " · "
                                                    span { class: "num", "{hm(start)}–{hm(end)}" }
                                                }
                                            }
                                            span { class: "chip info", "Private" }
                                            button {
                                                class: "small-btn",
                                                "aria-label": "Edit {i.label}",
                                                onclick: move |_| sheet.set(Some(edit.clone())),
                                                "Edit"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    for e in home.iter() {
                        {
                            let title = home_title(&e.label);
                            let when = match e.end {
                                Some(end) => format!("{}–{}", hm(e.start), hm(end)),
                                None => format!("since {}", hm(e.start)),
                            };
                            let edit = e
                                .id
                                .zip(e.end)
                                .map(|(id, end)| Sheet::edit(true, id, &e.label, e.start, end));
                            rsx! {
                                li { class: "pt-item",
                                    div { class: "pt-row",
                                        span { class: "pt-text",
                                            span {
                                                b { "{title}" }
                                                " · "
                                                span { class: "num", "{when}" }
                                            }
                                        }
                                        span { class: "chip acc", "Home" }
                                        if let Some(edit) = edit {
                                            button {
                                                class: "small-btn",
                                                "aria-label": "Edit {title}",
                                                onclick: move |_| sheet.set(Some(edit.clone())),
                                                "Edit"
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

        section { class: "panel", "aria-labelledby": "trips-h",
            div { class: "panel-head",
                h2 { id: "trips-h", "Trips" }
                span { class: "grow" }
                button {
                    class: "small-btn narrow-only",
                    "aria-expanded": if adding() { "true" } else { "false" },
                    onclick: move |_| adding.set(!adding()),
                    Icon { d: "M12 5v14M5 12h14", size: 15 }
                    "Add missing trip"
                }
            }
            {status(message_for(&message, Panel::Trips))}
            if adding() {
                TripForm {
                    date,
                    on_action: move |a| {
                        let done = on_action.call(a);
                        if done {
                            adding.set(false);
                        }
                        done
                    },
                    on_cancel: move |_| adding.set(false),
                }
            }
            if trips.is_empty() {
                p { class: "empty", "No trips this day." }
            } else {
                ul { class: "trip-list",
                    for (t , status) in trips.iter() {
                        {
                            let (kind, class) = trip_kind(status);
                            let km = t
                                .distance_km
                                .map(|k| format!("{k:.1} km").replace('.', ","))
                                .unwrap_or_default();
                            let purpose = match status {
                                Some(Status::Incomplete(why)) => why.to_string(),
                                _ => t.purpose.clone(),
                            };
                            let start = t.start;
                            let at = start.format("%H:%M").to_string();
                            let manual = loaded.manual.contains(&start);
                            rsx! {
                                li { class: "trip",
                                    span { class: "mono small nowrap",
                                        {format!("{}–{}", t.start.format("%H:%M"), t.end.format("%H:%M"))}
                                    }
                                    span { class: "trip-route",
                                        "{t.from} "
                                        span { class: "muted", "aria-hidden": "true", "→" }
                                        " {t.to}"
                                    }
                                    span { class: "r num nowrap", "{km}" }
                                    span {
                                        if !kind.is_empty() {
                                            span { class: "{class}", "{kind}" }
                                        }
                                    }
                                    span { class: "muted", "{purpose}" }
                                    if manual {
                                        span { class: "trip-manual",
                                            span { class: "chip info", "Added by you" }
                                            button {
                                                class: "small-btn danger",
                                                "aria-label": "Delete the trip at {at}",
                                                onclick: move |_| {
                                                    on_action.call(Action::DeleteTrip(start));
                                                },
                                                "Delete"
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
}

/// Wide: bars for planned time, time at work, private time, lunch and
/// driving. Narrow: the same as a list of steps, private time and home
/// working with Edit, and Add time under it.
fn timeline(
    d: &Day,
    private: &[PrivateItem],
    home: &[HomeEntry],
    trips: &[(&vw_danlon::trips::Trip, Option<Status>)],
    now_tz: DateTime<Tz>,
    mut sheet: Signal<Option<Sheet>>,
) -> Element {
    let now = now_tz.naive_local();
    let naive = |t: chrono::DateTime<chrono_tz::Tz>| t.naive_local();
    let mut spans: Vec<(NaiveDateTime, NaiveDateTime, u32, &'static str)> = Vec::new();
    if let Some((s, e)) = d.planned {
        spans.push((naive(s), naive(e), 0, "bar planned"));
    }
    for (s, e) in &d.segments {
        spans.push((naive(*s), naive(*e), 1, "bar work"));
    }
    for i in private {
        if let Some((s, e)) = private_span(i, now) {
            spans.push((s, e, 1, "bar private"));
        }
    }
    if let Some((s, e)) = d.lunch.window {
        spans.push((naive(s), naive(e), 1, "bar lunch"));
    }
    for (t, _) in trips {
        spans.push((naive(t.start), naive(t.end), 2, "bar drive"));
    }
    let day_start = d.date.and_hms_opt(0, 0, 0).unwrap();
    let hour = |t: NaiveDateTime| (t - day_start).num_minutes() as f64 / 60.0;
    let from = spans
        .iter()
        .map(|s| hour(s.0).floor())
        .fold(6.0_f64, f64::min)
        .max(0.0);
    let to = spans
        .iter()
        .map(|s| hour(s.1).ceil())
        .fold(18.0_f64, f64::max)
        .min(24.0);
    let pct = |t: NaiveDateTime| ((hour(t) - from) / (to - from) * 100.0).clamp(0.0, 100.0);
    let ticks: Vec<u32> = (from as u32..=to as u32).step_by(2).collect();

    let mut steps: Vec<Step> = Vec::new();
    let place = d.work_locations.first().cloned().unwrap_or_default();
    for (s, e) in &d.segments {
        steps.push(Step {
            at: naive(*s),
            title: if place.is_empty() {
                "At work".into()
            } else {
                format!("At work · {place}")
            },
            sub: if d.so_far && *e == now_tz {
                format!("since {}", s.format("%H:%M"))
            } else {
                format!("until {}", e.format("%H:%M"))
            },
            kind: "work",
            edit: None,
        });
    }
    // Today: counting starts at the planned start; now; the planned end.
    if d.so_far
        && let Some((ps, pe)) = d.planned
    {
        let late = match d.first_start {
            Some(first) if first > ps => {
                format!("{} late", minutes_text((first - ps).num_minutes()))
            }
            None if ps < now_tz => "Not at work yet".to_string(),
            _ => String::new(),
        };
        steps.push(Step {
            at: naive(ps),
            title: "Planned start".into(),
            kind: if late.is_empty() { "planned" } else { "late" },
            sub: late,
            edit: None,
        });
        if ps < now_tz && now_tz < pe {
            steps.push(Step {
                at: now,
                title: "Now".into(),
                sub: if d.open.is_some() {
                    "At work".into()
                } else if d.home.iter().any(|(_, e)| *e == now_tz) {
                    "Working from home".into()
                } else if d.first_start.is_none() {
                    "Not at work yet".into()
                } else {
                    "Not at work".into()
                },
                kind: "now",
                edit: None,
            });
            steps.push(Step {
                at: naive(pe),
                title: "Planned end".into(),
                sub: format!(
                    "{} h to go",
                    hours((d.norm_hours - d.expected_hours).max(0.0))
                ),
                kind: "planned",
                edit: None,
            });
        }
    }
    for i in private {
        if let Some((s, e)) = private_span(i, now) {
            let edit = match i.state {
                PrivateState::Recorded { id, start, end, .. } => {
                    Some(Sheet::edit(false, id, &i.label, start, end))
                }
                _ => None,
            };
            steps.push(Step {
                at: s,
                title: i.label.clone(),
                sub: format!("Private · {}–{}", hm(s), hm(e)),
                kind: "private",
                edit,
            });
        }
    }
    for e in home {
        steps.push(Step {
            at: e.start,
            title: home_title(&e.label),
            sub: match e.end {
                Some(end) => format!("{}–{}", hm(e.start), hm(end)),
                None => format!("since {}", hm(e.start)),
            },
            kind: "work",
            edit: e
                .id
                .zip(e.end)
                .map(|(id, end)| Sheet::edit(true, id, &e.label, e.start, end)),
        });
    }
    if let Some((s, e)) = d.lunch.window {
        steps.push(Step {
            at: naive(s),
            title: "Lunch break".into(),
            sub: format!("{} min", (e - s).num_minutes()),
            kind: "lunch",
            edit: None,
        });
    }
    for (t, status) in trips {
        let km = t
            .distance_km
            .map(|k| format!(" · {k:.1} km").replace('.', ","))
            .unwrap_or_default();
        let (kind, _) = trip_kind(status);
        steps.push(Step {
            at: naive(t.start),
            title: format!("Drive {} → {}", t.from, t.to),
            sub: format!("{}{km}", if kind.is_empty() { "Trip" } else { kind }),
            kind: "drive",
            edit: None,
        });
    }
    steps.sort_by_key(|s| s.at);
    let label = spans
        .iter()
        .map(|(s, e, _, class)| {
            format!("{} {}–{}", class.trim_start_matches("bar "), hm(*s), hm(*e))
        })
        .collect::<Vec<_>>()
        .join("; ");

    rsx! {
        section { class: "panel pad wide-only", "aria-labelledby": "tl-h",
            div { class: "tl-head",
                h2 { id: "tl-h", class: "grow", "Timeline" }
                span { class: "legend", span { class: "swatch planned" } "Planned" }
                span { class: "legend", span { class: "swatch work" } "At work" }
                span { class: "legend", span { class: "swatch private" } "Private" }
                span { class: "legend", span { class: "swatch lunch" } "Lunch" }
                span { class: "legend", span { class: "swatch drive" } "Driving" }
                if d.so_far {
                    span { class: "legend", span { class: "swatch now" } "Now" }
                }
            }
            div { class: "tl", role: "img", "aria-label": "{label}",
                for (s , e , row , class) in spans.iter() {
                    span {
                        class: "{class}",
                        style: "left: {pct(*s)}%; width: {(pct(*e) - pct(*s)).max(0.4)}%; top: {4 + row * 26}px",
                    }
                }
                if d.so_far {
                    span { class: "now-mark", style: "left: {pct(now)}%", title: "Now {hm(now)}" }
                }
            }
            div { class: "tl-ticks", "aria-hidden": "true",
                for h in ticks {
                    span { style: "left: {(h as f64 - from) / (to - from) * 100.0}%", "{h:02}" }
                }
            }
        }
        section { class: "panel narrow-only", "aria-labelledby": "steps-h",
            h2 { id: "steps-h", class: "steps-h", "Day" }
            if steps.is_empty() {
                p { class: "empty", "Nothing recorded this day." }
            } else {
                ol { class: "steps",
                    for s in steps {
                        li {
                            span { class: "mono small muted", {s.at.format("%H:%M").to_string()} }
                            span { class: "step-dot {s.kind}", "aria-hidden": "true" }
                            span { class: "step-text",
                                span { "{s.title}" }
                                if !s.sub.is_empty() {
                                    span { class: "step-sub {s.kind}", "{s.sub}" }
                                }
                            }
                            if let Some(edit) = s.edit {
                                button {
                                    class: "step-edit",
                                    "aria-label": "Edit {s.title}",
                                    onclick: move |_| sheet.set(Some(edit.clone())),
                                    "Edit"
                                }
                            }
                        }
                    }
                }
            }
            div { class: "steps-add",
                button {
                    class: "add-time",
                    onclick: move |_| sheet.set(Some(Sheet::default())),
                    Icon { d: "M12 5v14M5 12h14", size: 16 }
                    "Add time"
                }
            }
        }
    }
}

/// Adds private time or home working to the day, or edits a recorded
/// interval: a bottom sheet on a phone, a panel on a wide screen.
#[component]
fn TimeSheet(
    date: NaiveDate,
    sheet: Signal<Option<Sheet>>,
    planned: Option<(String, String)>,
    on_action: Callback<Action, bool>,
) -> Element {
    let Some(form) = sheet() else {
        return rsx! {};
    };
    let mut set = move |f: fn(&mut Sheet, String), v: String| {
        if let Some(s) = sheet.write().as_mut() {
            f(s, v);
        }
    };
    let adding = form.edit.is_none();
    let home = form.home;
    let title = match (form.edit, home) {
        (None, _) => format!("Add time · {}", date.format("%a %-d %b")),
        (Some(_), true) => format!("Edit home working · {}", date.format("%-d %b")),
        (Some(_), false) => format!("Edit private time · {}", date.format("%-d %b")),
    };
    let mut close = move || sheet.set(None);
    let mut run = move |a: Action| {
        if on_action.call(a) {
            sheet.set(None);
        }
    };
    let submit = {
        let form = form.clone();
        move |_| {
            let f = form.clone();
            run(match f.edit {
                None => Action::AddTime {
                    date,
                    home: f.home,
                    label: f.label,
                    start: f.start,
                    end: f.end,
                },
                Some(id) => Action::Change {
                    home: f.home,
                    id,
                    label: f.label,
                    start: f.start,
                    end: f.end,
                },
            })
        }
    };
    let seg = |on: bool| if on { "true" } else { "false" };
    rsx! {
        div { class: "sheet-backdrop narrow-only", onclick: move |_| close() }
        section {
            class: "time-sheet",
            role: "dialog",
            "aria-labelledby": "ts-h",
            span { class: "sheet-grip narrow-only", "aria-hidden": "true" }
            div { class: "ts-head",
                h2 { id: "ts-h", class: "grow", "{title}" }
                if adding {
                    div { class: "segmented ts-kind", role: "radiogroup", "aria-label": "Kind of time",
                        button {
                            r#type: "button",
                            role: "radio",
                            "aria-checked": seg(!home),
                            onclick: move |_| {
                                if let Some(s) = sheet.write().as_mut() {
                                    s.home = false;
                                }
                            },
                            "Private time"
                        }
                        button {
                            r#type: "button",
                            role: "radio",
                            "aria-checked": seg(home),
                            onclick: move |_| {
                                if let Some(s) = sheet.write().as_mut() {
                                    s.home = true;
                                }
                            },
                            "Worked from home"
                        }
                    }
                }
            }
            div { class: "ts-fields",
                div { class: "field",
                    label { r#for: "ts-what", "What" }
                    input {
                        id: "ts-what",
                        r#type: "text",
                        placeholder: if home { "Home" } else { "Private" },
                        value: "{form.label}",
                        oninput: move |e| set(|s, v| s.label = v, e.value()),
                    }
                }
                div { class: "field",
                    label { r#for: "ts-from", "From" }
                    input {
                        id: "ts-from",
                        r#type: "time",
                        value: "{form.start}",
                        oninput: move |e| set(|s, v| s.start = v, e.value()),
                    }
                }
                div { class: "field",
                    label { r#for: "ts-to", "To" }
                    input {
                        id: "ts-to",
                        r#type: "time",
                        value: "{form.end}",
                        oninput: move |e| set(|s, v| s.end = v, e.value()),
                    }
                }
            }
            if adding && home {
                if let Some((s, e)) = planned {
                    button {
                        r#type: "button",
                        class: "small-btn ts-planned",
                        onclick: move |_| {
                            if let Some(f) = sheet.write().as_mut() {
                                f.start = s.clone();
                                f.end = e.clone();
                            }
                        },
                        "Planned day {s}–{e}"
                    }
                }
            }
            if adding {
                div { class: "ts-note", role: "note",
                    if home {
                        "Counts as worked time."
                    } else {
                        "Not company time. Trips in this window are private."
                    }
                }
            }
            div { class: "ts-actions",
                if let Some(id) = form.edit {
                    button {
                        class: "ts-btn danger",
                        onclick: move |_| run(Action::Delete { home, id }),
                        "Delete"
                    }
                }
                button { class: "ts-btn", onclick: move |_| close(), "Cancel" }
                button { class: "ts-btn primary", onclick: submit,
                    if adding {
                        "Add"
                    } else {
                        "Save"
                    }
                }
            }
        }
    }
}

/// The lunch break, where it comes from, and its editor: a time and length,
/// no lunch, or back to the default.
#[component]
fn LunchPanel(
    date: NaiveDate,
    window: String,
    minutes: i64,
    source: &'static str,
    title: String,
    start: String,
    default_minutes: u32,
    message: Option<Result<String, String>>,
    on_action: Callback<Action, bool>,
) -> Element {
    let mut editing = use_signal(|| false);
    let mut start_in = use_signal(|| start.clone());
    let mut minutes_in = use_signal(|| {
        if minutes > 0 {
            minutes
        } else {
            default_minutes.into()
        }
        .to_string()
    });
    // Closes the editor when the change was made.
    let mut run = move |a: Action| {
        if on_action.call(a) {
            editing.set(false);
        }
    };
    rsx! {
        section { class: "panel pad lunch", "aria-labelledby": "lunch-h",
            h2 { id: "lunch-h", "Lunch break" }
            span { class: "mono", "{window}" }
            if minutes > 0 {
                span { class: "muted", "{minutes} min" }
            }
            span { class: "chip", "{source}" }
            span { class: "grow", title: "A “{title}” event in the calendar sets another time" }
            button {
                class: "small-btn",
                "aria-expanded": if editing() { "true" } else { "false" },
                onclick: move |_| {
                    if !editing() {
                        start_in.set(start.clone());
                    }
                    editing.set(!editing());
                },
                "Change"
            }
            if editing() {
                div { class: "pt-edit lunch-edit",
                    div { class: "field",
                        label { r#for: "lunch-start", "Start" }
                        input {
                            id: "lunch-start",
                            r#type: "time",
                            value: "{start_in}",
                            oninput: move |e| start_in.set(e.value()),
                        }
                    }
                    div { class: "field",
                        label { r#for: "lunch-min", "Minutes" }
                        input {
                            id: "lunch-min",
                            r#type: "number",
                            min: "0",
                            step: "5",
                            inputmode: "numeric",
                            value: "{minutes_in}",
                            oninput: move |e| minutes_in.set(e.value()),
                        }
                    }
                    div { class: "pt-edit-actions",
                        button {
                            class: "small-btn",
                            onclick: move |_| run(Action::Lunch(date, LunchChoice::None)),
                            "No lunch"
                        }
                        button {
                            class: "small-btn",
                            onclick: move |_| run(Action::Lunch(date, LunchChoice::Default)),
                            "Use default"
                        }
                        span { class: "grow" }
                        button {
                            class: "small-btn",
                            onclick: move |_| editing.set(false),
                            "Cancel"
                        }
                        button {
                            class: "small-btn primary-sm",
                            onclick: move |_| {
                                run(Action::LunchAt {
                                    date,
                                    start: start_in(),
                                    minutes: minutes_in(),
                                })
                            },
                            "Save"
                        }
                    }
                }
            }
            {status(message)}
        }
    }
}

/// A trip typed in by hand: when, from and to, km and purpose.
#[component]
fn TripForm(
    date: NaiveDate,
    on_action: Callback<Action, bool>,
    on_cancel: EventHandler<()>,
) -> Element {
    let mut form = use_signal(ManualTripForm::default);
    let field = move |id: &'static str,
                      label: &'static str,
                      kind: &'static str,
                      get: fn(&ManualTripForm) -> &String,
                      set: fn(&mut ManualTripForm, String)| {
        rsx! {
            div { class: "field",
                label { r#for: "{id}", "{label}" }
                input {
                    id: "{id}",
                    r#type: "{kind}",
                    value: "{get(&form.read())}",
                    oninput: move |e| set(&mut form.write(), e.value()),
                }
            }
        }
    };
    rsx! {
        div {
            class: "pt-edit trip-form",
            role: "group",
            "aria-label": "Add a missing trip",
            {field("trip-start", "Start", "time", |f| &f.start, |f, v| f.start = v)}
            {field("trip-end", "End", "time", |f| &f.end, |f, v| f.end = v)}
            {field("trip-from", "From", "text", |f| &f.from, |f, v| f.from = v)}
            {field("trip-to", "To", "text", |f| &f.to, |f, v| f.to = v)}
            {field("trip-km", "Km", "text", |f| &f.km, |f, v| f.km = v)}
            {
                field(
                    "trip-purpose",
                    "Purpose (optional)",
                    "text",
                    |f| &f.purpose,
                    |f, v| f.purpose = v,
                )
            }
            div { class: "pt-edit-actions",
                span { class: "grow" }
                button { class: "small-btn", onclick: move |_| on_cancel.call(()), "Cancel" }
                button {
                    class: "small-btn primary-sm",
                    onclick: move |_| {
                        on_action.call(Action::AddTrip(date, form()));
                    },
                    "Add trip"
                }
            }
        }
    }
}
