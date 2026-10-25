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
use vw_danlon::timesheet::weekday_da;
use vw_danlon::trips::Trip;

use super::month::{month_title, step, this_month};
use crate::setup::Setup;
use crate::{Icon, Screen};

/// The trips and the calendar from 1 January through the month, fetched
/// off the UI thread.
async fn load(cfg: Config, month: Month) -> Result<(Vec<Trip>, Vec<Occurrence>), String> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let trips = service::load_trips(&cfg, None, month)?;
        let cal = service::calendar(&cfg, month, service::CalendarMode::Cached)?;
        Ok((trips, cal.occurrences))
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

/// Writes the text the Rust side sends to the clipboard and answers whether
/// it worked; the textarea fallback is for webviews without the async
/// clipboard (Android WebView).
const COPY_JS: &str = r#"
const text = await dioxus.recv();
let ok = false;
try {
  await navigator.clipboard.writeText(text);
  ok = true;
} catch (_) {
  const t = document.createElement('textarea');
  t.value = text;
  t.setAttribute('readonly', '');
  t.style.position = 'fixed';
  t.style.opacity = '0';
  document.body.appendChild(t);
  t.select();
  try { ok = document.execCommand('copy'); } catch (_) {}
  t.remove();
}
dioxus.send(ok);
"#;

/// An arrow out of a box.
const OPEN_ICON: &str = "M14 4h6v6M20 4l-9 9M18 14v6H4V6h6";

/// Scrolls the opened ⋯ menu into view once it is drawn (on a phone it
/// can open under the tab bar).
const SHOW_MENU_JS: &str = "setTimeout(() => document.querySelector('.dl-menu')?.scrollIntoView({ block: 'nearest' }), 50);";

async fn copy(text: String) -> bool {
    let mut eval = document::eval(COPY_JS);
    if eval.send(text).is_err() {
        return false;
    }
    eval.recv::<bool>().await.unwrap_or(false)
}

#[component]
pub fn DanlonScreen(setup: Signal<Setup>, screen: Signal<Screen>) -> Element {
    let mut month = use_signal(|| this_month(&setup.peek()));
    let mut show = use_signal(|| Show::ToEnter);
    // Bumped after a tick so the list rereads the state file.
    let mut ticks = use_signal(|| 0u32);
    let mut menu = use_signal(|| None::<NaiveDate>);
    let mut message = use_signal(|| None::<Result<String, String>>);
    let mut toast = use_signal(|| None::<(u32, String)>);
    let cal_sync = use_context::<Signal<super::CalendarSync>>();
    let loaded = use_resource(move || {
        let _ = cal_sync.read().version;
        let m = month();
        let cfg = setup.read().config.clone();
        async move {
            match cfg {
                Some(cfg) => load(cfg, m).await,
                None => Err("no config".to_string()),
            }
        }
    });
    let Some(cfg) = setup.read().config.clone() else {
        return rsx! {};
    };
    let m = month();
    let head = rsx! {
        div { class: "screen-head",
            div { class: "month-nav",
                button {
                    class: "nav-btn",
                    "aria-label": "Previous month",
                    onclick: move |_| {
                        message.set(None);
                        menu.set(None);
                        month.set(step(m, -1));
                    },
                    Icon { d: "m15 6-6 6 6 6", size: 18 }
                }
                h2 { class: "month-title", "{month_title(m)}" }
                button {
                    class: "nav-btn",
                    "aria-label": "Next month",
                    onclick: move |_| {
                        message.set(None);
                        menu.set(None);
                        month.set(step(m, 1));
                    },
                    Icon { d: "m9 6 6 6-6 6", size: 18 }
                }
            }
            span { class: "note grow", "Nothing is sent from here" }
            button {
                class: "small-btn primary-sm",
                onclick: move |_| {
                    let result = crate::platform::open_danlon().map_err(|e| format!("{e:#}"));
                    message.set(Some(result));
                },
                Icon { d: OPEN_ICON, size: 15 }
                "Open Danløn app"
            }
        }
    };
    let loaded = loaded.read();
    let (trips, cal) = match &*loaded {
        None => {
            return rsx! {
                {head}
                p { class: "placeholder", role: "status", "Loading {month_title(m)}…" }
            };
        }
        Some(Err(e)) => {
            return rsx! {
                {head}
                div { class: "alert", role: "alert",
                    span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                    div {
                        b { "The checklist can't be shown: " }
                        "{e} "
                        button { class: "link-btn", onclick: move |_| screen.set(Screen::Settings), "Open Settings" }
                    }
                }
            };
        }
        Some(Ok(l)) => l,
    };
    let _ = ticks();
    let view = match service::checklist_view(&cfg, m, trips, cal) {
        Ok(v) => v,
        Err(e) => {
            return rsx! {
                {head}
                div { class: "alert", role: "alert", "{e:#}" }
            };
        }
    };
    let p = checklist::progress(&view.days, &view.entered);
    let pct = if p.days_total > 0 {
        p.days_done as f64 / p.days_total as f64 * 100.0
    } else {
        0.0
    };
    let count = |s: Show| {
        view.days
            .iter()
            .filter(|d| s.includes(d, &view.entered))
            .count()
    };
    let tabs = [
        (
            Show::ToEnter,
            format!("To enter ({})", count(Show::ToEnter)),
        ),
        (Show::Entered, format!("Entered ({})", count(Show::Entered))),
        (Show::All, "All".to_string()),
    ];
    let current = show();
    let next = view
        .days
        .iter()
        .find(|d| d.status == Status::ToEnter && !view.entered.contains(&d.date))
        .map(|d| d.date);
    let shown: Vec<(ChecklistDay, bool)> = view
        .days
        .iter()
        .filter(|d| current.includes(d, &view.entered))
        .map(|d| (d.clone(), view.entered.contains(&d.date)))
        .collect();
    let days = view.days.clone();

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
                "Type each Komme/gå entry (time and break) into the Danløn app; private time splits the day into entries, and the break is only lunch. Tick the day off when done. Ticks are only stored on this device."
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
                        Show::ToEnter => format!("Nothing left to enter for {}.", month_title(m)),
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
                                    span { class: "small muted", "{weekday_da(date.weekday())}" }
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
                                                    if !open {
                                                        document::eval(SHOW_MENU_JS);
                                                    }
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
                                                div { class: "dl-menu", role: "menu", "aria-label": "Actions",
                                                    for (label , text) in copy_actions(&k) {
                                                        button {
                                                            role: "menuitem",
                                                            onclick: move |_| {
                                                                menu.set(None);
                                                                let text = text.clone();
                                                                spawn(async move {
                                                                    let said = if copy(text.clone()).await {
                                                                        format!("Copied {text}")
                                                                    } else {
                                                                        "Copying failed".to_string()
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
