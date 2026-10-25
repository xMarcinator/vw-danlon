//! The Day screen: the day's numbers, a timeline of planned time, time at
//! work, private time, lunch and driving, the lunch break and where it
//! comes from (changeable), the private time (recorded intervals editable)
//! and the trips, where a missing trip can be added by hand.

use chrono::{NaiveDate, NaiveDateTime};
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
    pub date: NaiveDate,
    pub now: NaiveDateTime,
    pub message: Message,
    pub on_action: Callback<Action, bool>,
    pub on_back: Callback<()>,
    /// Whether the add-trip form is open.
    pub adding: Signal<bool>,
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

/// The private items' times: as measured, else as planned.
fn private_span(i: &PrivateItem, now: NaiveDateTime) -> Option<(NaiveDateTime, NaiveDateTime)> {
    match i.state {
        PrivateState::Recorded { start, end, .. } => Some((start, end)),
        PrivateState::Running { since } => Some((since, now.max(since))),
        PrivateState::Planned => i.planned,
    }
}

/// One step of the phone timeline.
struct Step {
    at: NaiveDateTime,
    title: String,
    sub: String,
    kind: &'static str,
}

pub fn render(a: DayArgs) -> Element {
    let DayArgs {
        cfg,
        view,
        loaded,
        date,
        now,
        message,
        on_action,
        on_back,
        mut adding,
    } = a;
    let Some(d) = view.report.days.iter().find(|d| d.date == date) else {
        return rsx! {};
    };
    let private = match State::load(&cfg.database) {
        Ok(s) => service::private_time(
            &service::timed_entries(cfg, &loaded.occurrences),
            &s.private,
            date,
            date,
            now,
        ),
        Err(e) => {
            return rsx! { div { class: "alert", role: "alert", "{e:#}" } };
        }
    };
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
    let title = date.format("%A %-d %B").to_string();
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
                h2 { "Worked" }
                span { class: "kpi-value", "{hours(d.worked_hours)}" }
                span { class: "kpi-sub", "{d.periods_text()}" }
            }
            section { class: "kpi",
                h2 { "Difference" }
                span { class: "kpi-value {diff_class(d.difference())}", "{signed(d.difference())}" }
                span { class: "kpi-sub", "norm {hours(d.norm_hours)} h" }
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

        {timeline(d, &private.items, &trips, now)}

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

        section { class: "panel", "aria-labelledby": "priv-h",
            div { class: "panel-head",
                h2 { id: "priv-h", "Private time" }
                span { class: "note", "Not company time. Recorded time replaces the planned window it was started for." }
            }
            {status(message_for(&message, Panel::Private))}
            if private.items.is_empty() {
                p { class: "empty", "No private appointments or recorded private time." }
            } else {
                ul { class: "pt-list",
                    PrivateList {
                        items: private.items.clone(),
                        current: None,
                        today,
                        on_action,
                    }
                }
            }
        }

        section { class: "panel", "aria-labelledby": "trips-h",
            div { class: "panel-head",
                h2 { id: "trips-h", "Trips" }
                span { class: "note grow", "The first arrival at and the last departure from a calendar address are business trips" }
                button {
                    class: "small-btn",
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
/// driving. Narrow: the same as a list of steps.
fn timeline(
    d: &Day,
    private: &[PrivateItem],
    trips: &[(&vw_danlon::trips::Trip, Option<Status>)],
    now: NaiveDateTime,
) -> Element {
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
            sub: format!("until {}", e.format("%H:%M")),
            kind: "work",
        });
    }
    for i in private {
        if let Some((s, e)) = private_span(i, now) {
            steps.push(Step {
                at: s,
                title: i.label.clone(),
                sub: format!("Private · {}–{} · not company time", hm(s), hm(e)),
                kind: "private",
            });
        }
    }
    if let Some((s, e)) = d.lunch.window {
        steps.push(Step {
            at: naive(s),
            title: "Lunch break".into(),
            sub: format!(
                "{} min · {}",
                (e - s).num_minutes(),
                lunch_source(d.lunch.source).to_lowercase()
            ),
            kind: "lunch",
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
            }
            div { class: "tl", role: "img", "aria-label": "{label}",
                for (s , e , row , class) in spans.iter() {
                    span {
                        class: "{class}",
                        style: "left: {pct(*s)}%; width: {(pct(*e) - pct(*s)).max(0.4)}%; top: {4 + row * 26}px",
                    }
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
                                span { class: "step-sub {s.kind}", "{s.sub}" }
                            }
                        }
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
            span { class: "note grow",
                "Only lunch inside your time at work is deducted. A “{title}” event in the calendar sets another time."
            }
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
            span { class: "note span-all",
                "Added by you and marked so: a later import or sync of the same trip (same start minute) does not add it twice."
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
