//! The Day screen: the day's numbers, a timeline of planned time, time at
//! work, private time, lunch and driving, the lunch break and where it
//! comes from, the private time (recorded intervals editable) and the
//! trips.

use chrono::{NaiveDate, NaiveDateTime};
use dioxus::prelude::*;
use vw_danlon::config::Config;
use vw_danlon::mileage::Status;
use vw_danlon::month::{Day, LunchSource};
use vw_danlon::service::{self, MonthView, PrivateItem, PrivateState};
use vw_danlon::state::State;

use super::{Action, Loaded, PrivateList, diff_class, hm, hours, kroner, signed};
use crate::Icon;

pub struct DayArgs<'a> {
    pub cfg: &'a Config,
    pub view: &'a MonthView,
    pub loaded: &'a Loaded,
    pub date: NaiveDate,
    pub now: NaiveDateTime,
    pub message: Option<Result<String, String>>,
    pub on_action: Callback<Action>,
    pub on_back: Callback<()>,
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
    } = a;
    let Some(d) = view.report.days.iter().find(|d| d.date == date) else {
        return rsx! {};
    };
    let private = match State::load(&cfg.state_file) {
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

        section { class: "panel pad lunch", "aria-labelledby": "lunch-h",
            h2 { id: "lunch-h", "Lunch break" }
            span { class: "mono", "{lunch_window}" }
            if lunch_minutes > 0 {
                span { class: "muted", "{lunch_minutes} min" }
            }
            span { class: "chip", "{lunch_source(d.lunch.source)}" }
            span { class: "note grow",
                "Only lunch inside your time at work is deducted. A “{cfg.lunch.title}” event in the calendar sets another time."
            }
        }

        section { class: "panel", "aria-labelledby": "priv-h",
            div { class: "panel-head",
                h2 { id: "priv-h", "Private time" }
                span { class: "note", "Not company time. Recorded time replaces the planned window it was started for." }
            }
            match message {
                Some(Ok(text)) => rsx! { div { class: "pt-status info", role: "status", "{text}" } },
                Some(Err(text)) => rsx! { div { class: "pt-status bad", role: "alert", "{text}" } },
                None => rsx! {},
            }
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
                span { class: "note", "The first arrival at and the last departure from a calendar address are business trips" }
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
