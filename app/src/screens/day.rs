//! The Day screen: the day's numbers (worked, norm and the difference;
//! today: worked and expected so far, the difference and how late), a
//! timeline of planned time, time at work, private time, lunch and driving
//! (today with the planned start, now and the planned end), the day as a
//! list where private time, home working and the lunch break have Edit, and
//! the trips with the day's km and kroner, where a missing trip can be
//! added by hand.

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use chrono_tz::Tz;
use dioxus::prelude::*;
use vw_danlon::config::Config;
use vw_danlon::mileage::{Reason, Status};
use vw_danlon::month::{Day, EntryKind, LunchSource, TimedEntry};
use vw_danlon::service::{
    self, LocalSpan, LunchChoice, ManualTripForm, MonthView, PrivateItem, PrivateState,
    TimelineKind,
};
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
/// editing a recorded interval (`edit`: its id); with `lunch` the lunch
/// break's sheet (`start` and `minutes`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Sheet {
    pub edit: Option<u32>,
    pub home: bool,
    pub label: String,
    pub start: String,
    pub end: String,
    pub lunch: bool,
    pub minutes: String,
}

impl Sheet {
    fn edit(home: bool, id: u32, label: &str, start: NaiveDateTime, end: NaiveDateTime) -> Self {
        Sheet {
            edit: Some(id),
            home,
            label: label.to_string(),
            start: hm(start),
            end: hm(end),
            ..Sheet::default()
        }
    }

    fn lunch(start: String, minutes: i64) -> Self {
        Sheet {
            lunch: true,
            start,
            minutes: minutes.to_string(),
            ..Sheet::default()
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

/// A trip's type chip: text and class. Business to or from a client is a
/// client visit.
fn trip_kind(s: &Option<Status>, client: bool) -> (&'static str, &'static str) {
    match s {
        Some(Status::Business) if client => ("Client visit", "chip client"),
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

/// A trip of the day, how it counts and whether it is a client visit.
type DayTrip<'a> = (&'a vw_danlon::trips::Trip, Option<Status>, bool);

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
    let entries = service::timed_entries(cfg, &loaded.occurrences);
    let private = service::private_time(&entries, state, &cfg.private, date, date, now);
    let trips: Vec<DayTrip> = service::day_trips(&loaded.trips, view.mileage.as_ref(), date)
        .into_iter()
        .map(|(t, status)| {
            let client = view.mileage.as_ref().is_some_and(|m| {
                m.driving.iter().any(|d| {
                    d.trip == *t
                        && matches!(
                            d.reason,
                            Reason::ClientVisit | Reason::DirectKm | Reason::Detour
                        )
                })
            });
            (t, status, client)
        })
        .collect();
    // The day's mileage: business km and kroner from the logbook, and all
    // km driven.
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
    let driven = trips
        .iter()
        .filter_map(|(t, ..)| t.distance_km)
        .fold(0.0, |a, b| a + b);
    let trips_sum = match view.mileage {
        Some(_) => format!("{} km · {}", km1(driven), kroner(amount)),
        None => format!("{} km", km1(driven)),
    };
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
    let km_text = km1(km);
    let (lunch_window, lunch_minutes) = match d.lunch.window {
        Some((s, e)) => (
            format!("{}–{}", s.format("%H:%M"), e.format("%H:%M")),
            (e - s).num_minutes(),
        ),
        None => ("No lunch".to_string(), 0),
    };
    let lunch_sheet = Sheet::lunch(
        d.lunch
            .window
            .map(|(s, _)| s.format("%H:%M").to_string())
            .unwrap_or_else(|| cfg.lunch.start.format("%H:%M").to_string()),
        if lunch_minutes > 0 {
            lunch_minutes
        } else {
            cfg.lunch.minutes.into()
        },
    );
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
    let open_lunch = {
        let lunch = lunch_sheet.clone();
        move |_| {
            sheet.set(if sheet.peek().as_ref().is_some_and(|s| s.lunch) {
                None
            } else {
                Some(lunch.clone())
            })
        }
    };
    let open_add = move |_| {
        sheet.set(
            if sheet
                .peek()
                .as_ref()
                .is_some_and(|s| s.edit.is_none() && !s.lunch)
            {
                None
            } else {
                Some(Sheet::default())
            },
        )
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
                "aria-expanded": if sheet().is_some_and(|s| s.edit.is_none() && !s.lunch) { "true" } else { "false" },
                onclick: open_add,
                Icon { d: "M12 5v14M5 12h14", size: 15 }
                "Add time"
            }
        }

        div { class: "kpis day-kpis",
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
                span { class: if so_far { "kpi-sub" } else { "kpi-sub wide-only" }, "{worked_sub}" }
            }
            if !so_far {
                section { class: "kpi narrow-only",
                    h2 { "Norm" }
                    span { class: "kpi-value", "{hours(d.norm_hours)}" }
                }
            }
            if so_far {
                section { class: "kpi",
                    h2 { "Expected so far" }
                    span { class: "kpi-value", "{hours(d.expected_hours)}" }
                    span { class: "kpi-sub", "{expected_sub}" }
                }
            }
            section { class: "kpi",
                h2 {
                    if so_far {
                        "Diff"
                    } else {
                        span { class: "narrow-only", "Diff" }
                        span { class: "wide-only", "Difference" }
                    }
                }
                span { class: "kpi-value {diff_class(d.difference())}", "{signed(d.difference())}" }
                if so_far {
                    span { class: "kpi-sub", "{late_text}" }
                } else {
                    span { class: "kpi-sub wide-only", "norm {hours(d.norm_hours)} h" }
                }
            }
            if view.mileage.is_some() {
                section { class: "kpi wide-only",
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
        {status(message_for(&message, Panel::Lunch))}

        if sheet().is_some_and(|s| s.lunch) {
            LunchSheet { date, sheet, on_action }
        } else if sheet().is_some() {
            TimeSheet { date, sheet, planned: planned_hm, on_action }
        }

        {
            timeline(
                d,
                &private.items,
                &home,
                &trips,
                now_tz,
                sheet,
                day_steps(cfg, &entries, d, now, lunch_sheet.clone()),
            )
        }

        section { class: "panel pad lunch wide-only", "aria-labelledby": "lunch-h",
            h2 { id: "lunch-h", "Lunch break" }
            span { class: "mono", "{lunch_window}" }
            if lunch_minutes > 0 {
                span { class: "muted", "{lunch_minutes} min" }
            }
            span { class: "chip", "{lunch_source(d.lunch.source)}" }
            span {
                class: "grow",
                title: "A “{cfg.lunch.title}” event in the calendar sets another time",
            }
            button {
                class: "small-btn",
                "aria-expanded": if sheet().is_some_and(|s| s.lunch) { "true" } else { "false" },
                onclick: open_lunch,
                "Change"
            }
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
            div { class: "panel-head trips-head",
                h2 { id: "trips-h", "Trips" }
                span { class: "trips-sum num", "{trips_sum}" }
            }
            {status(message_for(&message, Panel::Trips))}
            if trips.is_empty() {
                p { class: "empty", "No trips this day." }
            } else {
                ul { class: "trip-list",
                    for (t , status , client) in trips.iter() {
                        {
                            let (kind, class) = trip_kind(status, *client);
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
                                        {t.start.format("%H:%M").to_string()}
                                        span { class: "wide-only",
                                            {format!("–{}", t.end.format("%H:%M"))}
                                        }
                                    }
                                    span { class: "trip-route",
                                        "{t.from} "
                                        span { class: "muted", "aria-hidden": "true", "→" }
                                        " {t.to}"
                                    }
                                    span { class: "r num nowrap", "{km}" }
                                    span { class: "trip-kind",
                                        if !kind.is_empty() {
                                            span { class: "{class}", "{kind}" }
                                        }
                                    }
                                    span { class: "muted trip-purpose", "{purpose}" }
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
            div { class: "narrow-only",
                div { class: "steps-add",
                    button {
                        class: "add-trip",
                        "aria-expanded": if adding() { "true" } else { "false" },
                        onclick: move |_| adding.set(!adding()),
                        Icon { d: "M12 5v14M5 12h14", size: 16 }
                        "Add missing trip"
                    }
                }
            }
        }
    }
}

/// The steps of the day list from the time at work (the time at a client
/// split out as a client visit) and the lunch break, also when there is
/// none, so it can be set again.
fn day_steps(
    cfg: &Config,
    entries: &[TimedEntry],
    d: &Day,
    now: NaiveDateTime,
    lunch: Sheet,
) -> Vec<Step> {
    let naive = |t: DateTime<Tz>| t.naive_local();
    let local = |spans: &[(DateTime<Tz>, DateTime<Tz>)]| -> Vec<LocalSpan> {
        spans.iter().map(|(s, e)| (naive(*s), naive(*e))).collect()
    };
    let mut steps = Vec::new();
    let place = d.work_locations.first().cloned().unwrap_or_default();
    // At work, with the time at a client split out as a client visit.
    for (s, e, kind) in service::day_work_steps(&local(&d.segments), &local(&d.clients)) {
        let client = kind == TimelineKind::Client;
        let ongoing = d.so_far && e == now;
        steps.push(Step {
            at: s,
            title: match (client, client_title(cfg, entries, s, e)) {
                (true, Some(t)) => format!("Client visit · {t}"),
                (true, None) => "Client visit".into(),
                (false, _) if place.is_empty() => "At work".into(),
                (false, _) => format!("At work · {place}"),
            },
            sub: match (ongoing, client) {
                (true, _) => format!("since {}", hm(s)),
                (false, true) => format!("{}–{}", hm(s), hm(e)),
                (false, false) => format!("until {}", hm(e)),
            },
            kind: timeline_class(kind),
            edit: None,
        });
    }
    steps.push(match d.lunch.window {
        Some((s, e)) => Step {
            at: naive(s),
            title: "Lunch break".into(),
            sub: format!("{} min", (e - s).num_minutes()),
            kind: "lunch",
            edit: Some(lunch),
        },
        None => Step {
            at: d.date.and_time(cfg.lunch.start),
            title: "Lunch break".into(),
            sub: "No lunch".into(),
            kind: "lunch",
            edit: Some(lunch),
        },
    });
    steps
}

/// The timeline's fixed hours.
const TL_FROM: f64 = 6.0;
const TL_TO: f64 = 18.0;

/// The calendar title of the work entry at a client during `s`–`e`, else
/// its address.
fn client_title(
    cfg: &Config,
    entries: &[TimedEntry],
    s: NaiveDateTime,
    e: NaiveDateTime,
) -> Option<String> {
    entries
        .iter()
        .filter(|x| {
            x.kind == EntryKind::Work
                && x.start.naive_local() < e
                && s < x.end.naive_local()
                && cfg
                    .workplace
                    .as_ref()
                    .is_none_or(|w| !vw_danlon::address::matches(x.location, w))
        })
        .map(|x| {
            if x.summary.trim().is_empty() {
                x.location.trim().to_string()
            } else {
                x.summary.trim().to_string()
            }
        })
        .find(|t| !t.is_empty())
}

/// "37,0".
fn km1(km: f64) -> String {
    format!("{km:.1}").replace('.', ",")
}

/// The timeline's legend: the class of each kind and its name.
const LEGEND: [(&str, &str); 6] = [
    ("planned", "Planned"),
    ("work", "Work"),
    ("client", "Client"),
    ("private", "Private"),
    ("lunch", "Lunch"),
    ("drive", "Driving"),
];

fn timeline_class(kind: TimelineKind) -> &'static str {
    match kind {
        TimelineKind::Planned => "planned",
        TimelineKind::Work => "work",
        TimelineKind::Client => "client",
        TimelineKind::Private => "private",
        TimelineKind::Lunch => "lunch",
        TimelineKind::Drive => "drive",
    }
}

/// The timeline on one line: the planned day as a dashed outline, time at
/// work, at a client, private time, the lunch break and driving inside it,
/// and today a line at now. Narrow, under it: the day as a list of steps,
/// private time and home working with Edit, and Add time under it.
fn timeline(
    d: &Day,
    private: &[PrivateItem],
    home: &[HomeEntry],
    trips: &[DayTrip],
    now_tz: DateTime<Tz>,
    mut sheet: Signal<Option<Sheet>>,
    steps: Vec<Step>,
) -> Element {
    let now = now_tz.naive_local();
    let naive = |t: chrono::DateTime<chrono_tz::Tz>| t.naive_local();
    let local = |spans: &[(DateTime<Tz>, DateTime<Tz>)]| -> Vec<LocalSpan> {
        spans.iter().map(|(s, e)| (naive(*s), naive(*e))).collect()
    };
    let private_spans: Vec<LocalSpan> = private
        .iter()
        .filter_map(|i| private_span(i, now))
        .collect();
    let drives: Vec<LocalSpan> = trips
        .iter()
        .map(|(t, ..)| (naive(t.start), naive(t.end)))
        .collect();
    let spans: Vec<(NaiveDateTime, NaiveDateTime, &'static str)> = service::day_timeline(
        d.planned.map(|(s, e)| (naive(s), naive(e))),
        &local(&d.segments),
        &local(&d.clients),
        d.lunch.window.map(|(s, e)| (naive(s), naive(e))),
        &private_spans,
        &drives,
    )
    .into_iter()
    .map(|(s, e, kind)| (s, e, timeline_class(kind)))
    .collect();
    // Always 06–18: what lies outside is cut at the edge.
    let day_start = d.date.and_hms_opt(0, 0, 0).unwrap();
    let hour = |t: NaiveDateTime| (t - day_start).num_minutes() as f64 / 60.0;
    let (from, to) = (TL_FROM, TL_TO);
    let pct = |t: NaiveDateTime| ((hour(t) - from) / (to - from) * 100.0).clamp(0.0, 100.0);
    let shown: Vec<_> = spans
        .iter()
        .filter(|(s, e, _)| hour(*e) > from && hour(*s) < to)
        .collect();
    let ticks: Vec<u32> = (from as u32..=to as u32).step_by(2).collect();

    let mut steps = steps;
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
    for (t, status, client) in trips {
        let km = t
            .distance_km
            .map(|k| format!(" · {k:.1} km").replace('.', ","))
            .unwrap_or_default();
        let (kind, _) = trip_kind(status, *client);
        steps.push(Step {
            at: naive(t.start),
            title: format!("Drive {} → {}", t.from, t.to),
            sub: format!("{}{km}", if kind.is_empty() { "Trip" } else { kind }),
            kind: "drive",
            edit: None,
        });
    }
    steps.sort_by_key(|s| s.at);
    let mut label = spans
        .iter()
        .map(|(s, e, class)| format!("{class} {}–{}", hm(*s), hm(*e)))
        .collect::<Vec<_>>()
        .join("; ");
    if d.so_far {
        label.push_str(&format!("; now {}", hm(now)));
    }
    let legend = rsx! {
        for (class , text) in LEGEND {
            span { class: "legend",
                span { class: "swatch {class}" }
                "{text}"
            }
        }
    };

    rsx! {
        section { class: "panel pad tl-panel", "aria-label": "Timeline",
            div { class: "tl-head wide-only",
                h2 { class: "grow", "Timeline" }
                {legend.clone()}
            }
            div { class: "tl", role: "img", "aria-label": "{label}",
                for (s , e , class) in shown {
                    span {
                        class: "bar {class}",
                        style: "left: {pct(*s)}%; width: {(pct(*e) - pct(*s)).max(0.4)}%",
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
            div { class: "narrow-only",
                div { class: "tl-legend", {legend} }
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

/// The lunch break's sheet: start and minutes (0: no lunch), or back to
/// the default; a bottom sheet on a phone, a panel on a wide screen.
#[component]
fn LunchSheet(
    date: NaiveDate,
    sheet: Signal<Option<Sheet>>,
    on_action: Callback<Action, bool>,
) -> Element {
    let Some(form) = sheet() else {
        return rsx! {};
    };
    let mut close = move || sheet.set(None);
    let mut run = move |a: Action| {
        if on_action.call(a) {
            sheet.set(None);
        }
    };
    let save = {
        let form = form.clone();
        move |_| {
            run(Action::LunchAt {
                date,
                start: form.start.clone(),
                minutes: form.minutes.clone(),
            })
        }
    };
    rsx! {
        div { class: "sheet-backdrop narrow-only", onclick: move |_| close() }
        section {
            class: "time-sheet",
            role: "dialog",
            "aria-labelledby": "ls-h",
            span { class: "sheet-grip narrow-only", "aria-hidden": "true" }
            div { class: "ts-head",
                h2 { id: "ls-h", class: "grow", {format!("Lunch break · {}", date.format("%-d %b"))} }
            }
            div { class: "ts-fields lunch-fields",
                div { class: "field",
                    label { r#for: "lunch-start", "Start" }
                    input {
                        id: "lunch-start",
                        r#type: "time",
                        value: "{form.start}",
                        oninput: move |e| {
                            if let Some(s) = sheet.write().as_mut() {
                                s.start = e.value();
                            }
                        },
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
                        value: "{form.minutes}",
                        oninput: move |e| {
                            if let Some(s) = sheet.write().as_mut() {
                                s.minutes = e.value();
                            }
                        },
                    }
                }
            }
            div { class: "ts-actions",
                button { class: "ts-btn wide-only", onclick: move |_| close(), "Cancel" }
                button {
                    class: "ts-btn",
                    onclick: move |_| run(Action::Lunch(date, LunchChoice::Default)),
                    "Use default"
                }
                button { class: "ts-btn primary", onclick: save, "Save" }
            }
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
