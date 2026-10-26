//! The Danløn app checklist screen: per day, what to type into the Danløn
//! app, as `checklist <month>` prints it, with the copy actions in a ⋯ menu
//! and a tick per day, and a button that opens the Danløn app (danlon.dk on
//! the desktop). Nothing is sent to Danløn; ticks go to the state file.

use std::collections::BTreeSet;

use chrono::{Datelike, NaiveDate};
use dioxus::prelude::*;
use vw_danlon::calendar::Occurrence;
use vw_danlon::checklist::{self, ChecklistDay, Korsel, Status};
use vw_danlon::config::Config;
use vw_danlon::month::Month;
use vw_danlon::service;

use vw_danlon::trips::Trip;

use super::period::{Bar, PagePeriods, Period, WEEK_MONTH, dock};
use crate::setup::Setup;
use crate::{Icon, Screen};

/// A month's trips and calendar, from 1 January through the month.
struct Loaded {
    month: Month,
    trips: Vec<Trip>,
    cal: Vec<Occurrence>,
}

/// Fetched off the UI thread.
async fn load(cfg: Config, month: Month) -> Result<Loaded, String> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let trips = service::load_trips(&cfg, None, month)?;
        let cal = service::calendar(&cfg, month, service::CalendarMode::Cached)?;
        Ok(Loaded {
            month,
            trips,
            cal: cal.occurrences,
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

/// Which days the list shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Show {
    /// Days to enter that are not ticked yet, and days to fix first.
    ToEnter,
    Entered,
    All,
}

impl Show {
    fn includes(self, day: &ChecklistDay, entered: &BTreeSet<NaiveDate>) -> bool {
        let ticked = entered.contains(&day.date);
        match self {
            Show::ToEnter => match day.status {
                Status::ToEnter => !ticked,
                Status::FixFirst(_) => true,
                Status::NothingToEnter(_) => false,
            },
            Show::Entered => ticked && day.status == Status::ToEnter,
            Show::All => true,
        }
    }
}

/// The ⋯ menu of a Kørsel registration: each item's label and the text it
/// copies. Times are not copied: the Danløn app takes them on a keypad.
fn copy_actions(k: &Korsel) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if !k.purpose.is_empty() {
        out.push((format!("Copy purpose · {}", k.purpose), k.purpose.clone()));
    }
    for (name, address) in &k.addresses {
        out.push((
            format!("Copy address of {name} · {address}"),
            address.clone(),
        ));
    }
    out.push((format!("Copy km · {}", k.km_text()), k.km_text()));
    out
}

/// An arrow out of a box.
const OPEN_ICON: &str = "M14 4h6v6M20 4l-9 9M18 14v6H4V6h6";

#[component]
pub fn DanlonScreen(setup: Signal<Setup>, screen: Signal<Screen>) -> Element {
    // The week or month shown, kept while the app runs.
    let mut period = use_context::<PagePeriods>().danlon;
    let months = use_memo(move || period().months());
    let mut show = use_signal(|| Show::ToEnter);
    // Bumped after a tick so the list rereads the state file.
    let mut ticks = use_signal(|| 0u32);
    let mut menu = use_signal(|| None::<NaiveDate>);
    let mut message = use_signal(|| None::<Result<String, String>>);
    let mut toast = use_signal(|| None::<(u32, String)>);
    let cal_sync = use_context::<Signal<super::CalendarSync>>();
    let loaded = use_resource(move || {
        let _ = cal_sync.read().version;
        let m = months().0;
        let cfg = setup.read().config.clone();
        async move {
            match cfg {
                Some(cfg) => load(cfg, m).await,
                None => Err("no config".to_string()),
            }
        }
    });
    // The week's second month, when its Sunday is in the next one.
    let loaded2 = use_resource(move || {
        let _ = cal_sync.read().version;
        let m = months().1;
        let cfg = setup.read().config.clone();
        async move {
            match (cfg, m) {
                (Some(cfg), Some(m)) => Some(load(cfg, m).await),
                _ => None,
            }
        }
    });
    let Some(cfg) = setup.read().config.clone() else {
        return rsx! {};
    };
    let selected = period();
    let (m, m2) = months();
    let today = chrono::Utc::now().with_timezone(&cfg.timezone).date_naive();
    let bar = Bar::picker(
        selected,
        today,
        &WEEK_MONTH,
        Callback::new(move |next: Period| {
            message.set(None);
            menu.set(None);
            period.set(next);
        }),
    );
    let open_danlon = move |_| {
        let result = crate::platform::open_danlon().map_err(|e| format!("{e:#}"));
        message.set(Some(result));
    };
    let head = rsx! {
        {bar.top(rsx! {})}
        div { class: "screen-head",
            span { class: "note grow", "Nothing is sent from here" }
            button {
                class: "small-btn primary-sm wide-only",
                onclick: open_danlon,
                Icon { d: OPEN_ICON, size: 15 }
                "Open Danløn app"
            }
        }
    };
    // The phone's dock: Open Danløn app and, when there is a day to enter,
    // Mark it entered, over the period row.
    let docked = |mark: Option<Element>| {
        dock(
            &bar,
            rsx! {
                div { class: "dock-pair",
                    button { class: "dock-btn", onclick: open_danlon, "Open Danløn app" }
                    {mark}
                }
            },
        )
    };
    let name = selected.name();
    let failed = |e: &str| {
        rsx! {
            {head.clone()}
            div { class: "alert", role: "alert",
                span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                div {
                    b { "The checklist can't be shown: " }
                    "{e} "
                    button { class: "link-btn", onclick: move |_| screen.set(Screen::Settings), "Open Settings" }
                }
            }
            {docked(None)}
        }
    };
    let loading = || {
        rsx! {
            {head.clone()}
            p { class: "placeholder", role: "status", "Loading {name}…" }
            {docked(None)}
        }
    };
    let loaded = loaded.read();
    let l = match &*loaded {
        Some(Ok(l)) if l.month == m => l,
        Some(Err(e)) => return failed(e),
        _ => return loading(),
    };
    let loaded2 = loaded2.read();
    let l2 = match (&*loaded2, m2) {
        (_, None) => None,
        (Some(Some(Ok(l))), Some(m2)) if l.month == m2 => Some(l),
        (Some(Some(Err(e))), Some(_)) => return failed(e),
        _ => return loading(),
    };
    let _ = ticks();
    // Each month's checklist as the month computes it, then the period's
    // days of them.
    let mut all_days: Vec<ChecklistDay> = Vec::new();
    let mut entered = BTreeSet::new();
    for l in std::iter::once(l).chain(l2) {
        match service::checklist_view(&cfg, l.month, &l.trips, &l.cal, today) {
            Ok(v) => {
                all_days.extend(v.days.into_iter().filter(|d| selected.contains(d.date)));
                entered = v.entered;
            }
            Err(e) => {
                return rsx! {
                    {head}
                    div { class: "alert", role: "alert", "{e:#}" }
                    {docked(None)}
                };
            }
        }
    }
    let p = checklist::progress(&all_days, &entered);
    let pct = if p.days_total > 0 {
        p.days_done as f64 / p.days_total as f64 * 100.0
    } else {
        0.0
    };
    let count = |s: Show| all_days.iter().filter(|d| s.includes(d, &entered)).count();
    let tabs = [
        (
            Show::ToEnter,
            format!("To enter ({})", count(Show::ToEnter)),
        ),
        (Show::Entered, format!("Entered ({})", count(Show::Entered))),
        (Show::All, "All".to_string()),
    ];
    let current = show();
    let next = all_days
        .iter()
        .find(|d| d.status == Status::ToEnter && !entered.contains(&d.date))
        .map(|d| d.date);
    let shown: Vec<(ChecklistDay, bool)> = all_days
        .iter()
        .filter(|d| current.includes(d, &entered))
        .map(|d| (d.clone(), entered.contains(&d.date)))
        .collect();
    let days = all_days.clone();
    // The next day to enter, ticked off from the dock.
    let mark = next.map(|date| {
        let days = all_days.clone();
        let label = format!("Mark {} entered", date.format("%-d %b"));
        rsx! {
            button {
                class: "dock-btn primary",
                onclick: move |_| {
                    let Some(cfg) = setup.peek().config.clone() else { return };
                    let result = service::set_entered(&cfg, &days, date, true)
                        .map(|_| format!("{date}: entered"))
                        .map_err(|e| format!("{e:#}"));
                    message.set(Some(result));
                    ticks += 1;
                },
                "{label}"
            }
        }
    });

    rsx! {
        {head}
        section { class: "panel pad dl-progress", "aria-labelledby": "dl-progress-h",
            div { class: "kpi-row",
                h2 { id: "dl-progress-h", "{p.days_done} of {p.days_total} working days entered" }
                span { class: "sub small",
                    "Komme/gå {p.days_done} of {p.days_total} · Kørsel {p.trips_done} of {p.trips_total} trips"
                    if p.fix_first > 0 {
                        " · {p.fix_first} to fix first"
                    }
                }
            }
            div {
                class: "bar-track",
                role: "progressbar",
                "aria-label": "Days entered in the Danløn app",
                "aria-valuemin": "0",
                "aria-valuemax": "{p.days_total}",
                "aria-valuenow": "{p.days_done}",
                div { class: "bar-fill", style: "width: {pct:.1}%" }
            }
            span { class: "note",
                "Ticks stay on this device."
            }
        }
        div { class: "dl-tabs", role: "tablist", "aria-label": "Show",
            for (s , label) in tabs {
                button {
                    role: "tab",
                    "aria-selected": if s == current { "true" } else { "false" },
                    onclick: move |_| {
                        menu.set(None);
                        show.set(s);
                    },
                    "{label}"
                }
            }
        }
        if let Some(r) = message() {
            match r {
                Ok(t) => rsx! { span { class: "status-ok", role: "status", "{t}" } },
                Err(e) => rsx! { span { class: "status-bad", role: "status", "{e}" } },
            }
        }
        section { class: "panel dl-list", "aria-label": "Days",
            if shown.is_empty() {
                p { class: "empty",
                    match current {
                        Show::ToEnter => format!("Nothing left to enter for {name}."),
                        Show::Entered => "No days ticked off yet.".to_string(),
                        Show::All => "No days.".to_string(),
                    }
                }
            }
            ul { class: "dl-days",
                for (day , ticked) in shown {
                    {
                        let date = day.date;
                        let days = days.clone();
                        let state = match (&day.status, ticked) {
                            (Status::ToEnter, true) => "done",
                            (Status::ToEnter, false) => "todo",
                            (Status::FixFirst(_), _) => "warn",
                            (Status::NothingToEnter(_), _) => "off",
                        };
                        let check_label = match state {
                            "done" => "Entered",
                            "todo" => "Mark entered",
                            "warn" => "Fix first",
                            _ => "Nothing to enter",
                        };
                        let note = match &day.status {
                            Status::FixFirst(r) => Some(format!("Fix first: {r}")),
                            Status::NothingToEnter(r) => Some(format!("Nothing to enter: {r}")),
                            Status::ToEnter => None,
                        };
                        let to_enter = day.status == Status::ToEnter;
                        let korsel = day.korsel.clone().filter(|_| to_enter);
                        let gaps = korsel.as_ref().map(|k| k.gaps.clone()).unwrap_or_default();
                        rsx! {
                            li {
                                key: "{date}",
                                class: "dl-day {state}",
                                class: if Some(date) == next { "next" },
                                span { class: "dl-date",
                                    b { "{date.day()} {date.format(\"%b\")}" }
                                    span { class: "small muted", "{super::weekday(date)}" }
                                    if Some(date) == next {
                                        span { class: "chip acc", "next to enter" }
                                    }
                                }
                                div { class: "dl-items",
                                    if to_enter {
                                        for k in day.komme_gaa.iter() {
                                            div { class: "dl-item",
                                                span { class: "dl-kind kg", "Komme/gå" }
                                                span { class: "dl-time", "{k.time_text()}" }
                                                span { class: "dl-detail", "Break {k.break_text()} · {k.total_text()}" }
                                            }
                                        }
                                    }
                                    if let Some(k) = korsel {
                                        div { class: "dl-item",
                                            span { class: "dl-kind kr", "Kørsel" }
                                            span { class: "dl-route", "{k.route_text()}" }
                                            span { class: "dl-detail",
                                                "Add return trip: "
                                                b { if k.return_trip { "on" } else { "off" } }
                                                " · Purpose: {k.purpose} · {k.km_text()} km"
                                            }
                                            button {
                                                class: "small-btn dl-more",
                                                "aria-label": "More actions for {k.route_text()}",
                                                "aria-haspopup": "menu",
                                                "aria-expanded": if menu() == Some(date) { "true" } else { "false" },
                                                onclick: move |_| {
                                                    let open = menu() == Some(date);
                                                    menu.set(if open { None } else { Some(date) });
                                                },
                                                svg {
                                                    "aria-hidden": "true",
                                                    width: "16",
                                                    height: "16",
                                                    view_box: "0 0 24 24",
                                                    fill: "currentColor",
                                                    circle { cx: "5", cy: "12", r: "1.8" }
                                                    circle { cx: "12", cy: "12", r: "1.8" }
                                                    circle { cx: "19", cy: "12", r: "1.8" }
                                                }
                                            }
                                            if menu() == Some(date) {
                                                div {
                                                    class: "dl-menu",
                                                    role: "menu",
                                                    "aria-label": "Actions",
                                                    // On a phone it can open under the tab bar.
                                                    onmounted: move |e| async move {
                                                        let _ = e
                                                            .data()
                                                            .scroll_to_with_options(ScrollToOptions {
                                                                behavior: ScrollBehavior::Smooth,
                                                                vertical: ScrollLogicalPosition::Nearest,
                                                                horizontal: ScrollLogicalPosition::Nearest,
                                                            })
                                                            .await;
                                                    },
                                                    for (label , text) in copy_actions(&k) {
                                                        button {
                                                            role: "menuitem",
                                                            onclick: move |_| {
                                                                menu.set(None);
                                                                let text = text.clone();
                                                                spawn(async move {
                                                                    let said = match crate::platform::copy_text(&text) {
                                                                        Ok(()) => format!("Copied {text}"),
                                                                        Err(e) => format!("Copying failed: {e:#}"),
                                                                    };
                                                                    let id = toast.peek().as_ref().map_or(0, |t| t.0) + 1;
                                                                    toast.set(Some((id, said)));
                                                                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                                                    if toast.peek().as_ref().is_some_and(|t| t.0 == id) {
                                                                        toast.set(None);
                                                                    }
                                                                });
                                                            },
                                                            "{label}"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    for (from , to) in gaps {
                                        span { class: "dl-note warn", "Check: no trip from {from} to {to}" }
                                    }
                                    if let Some(n) = note {
                                        span { class: "dl-note", "{n}" }
                                    }
                                }
                                label { class: "dl-check",
                                    input {
                                        r#type: "checkbox",
                                        checked: ticked,
                                        disabled: !to_enter,
                                        onchange: move |e| {
                                            let cfg = setup.peek().config.clone();
                                            let Some(cfg) = cfg else { return };
                                            let result = service::set_entered(&cfg, &days, date, e.checked())
                                                .map(|_| {
                                                    if e.checked() {
                                                        format!("{date}: entered")
                                                    } else {
                                                        format!("{date}: tick removed")
                                                    }
                                                })
                                                .map_err(|e| format!("{e:#}"));
                                            message.set(Some(result));
                                            ticks += 1;
                                        },
                                    }
                                    "{check_label}"
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some((_, t)) = toast() {
            div { class: "toast", role: "status", "{t}" }
        }
        {docked(mark)}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_copies_purpose_addresses_and_km() {
        let k = Korsel {
            purpose: "Kunde A".into(),
            route: vec!["Hjem".into(), "Kunde A".into()],
            return_trip: true,
            km: 148.0,
            gaps: Vec::new(),
            addresses: vec![
                ("Hjem".into(), "Hjemvej 3, 8200 Aarhus N".into()),
                ("Kunde A".into(), "Vejlevej 12, 7100 Vejle".into()),
            ],
        };
        let texts: Vec<_> = copy_actions(&k).into_iter().map(|(_, t)| t).collect();
        assert_eq!(
            texts,
            [
                "Kunde A",
                "Hjemvej 3, 8200 Aarhus N",
                "Vejlevej 12, 7100 Vejle",
                "148,0"
            ]
        );
        assert_eq!(
            copy_actions(&k)[2].0,
            "Copy address of Kunde A · Vejlevej 12, 7100 Vejle"
        );
    }
}
