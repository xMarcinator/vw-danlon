//! The Mileage logbook screen: a month's business trips at the year's
//! rates and the trips not reimbursed, as `logbook` computes them, and the
//! logbook file for signature. Without a `[mileage]` section it sets one up.

use dioxus::prelude::*;
use vw_danlon::calendar::Occurrence;
use vw_danlon::config::Config;
use vw_danlon::mileage::{self, MileageMonth, Status};
use vw_danlon::month::Month;
use vw_danlon::service;
use vw_danlon::trips::Trip;

use super::kroner;
use super::month::{month_title, step, this_month};
use crate::setup::{self, MileageForm, Setup};
use crate::{Icon, Screen};

/// "3796" → "3.796".
fn grouped(int: &str) -> String {
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// "3.796,6".
fn km(v: f64) -> String {
    let text = format!("{v:.1}");
    let (int, dec) = text.split_once('.').unwrap_or((&text, "0"));
    format!("{},{dec}", grouped(int))
}

/// "20.000".
fn whole_km(v: f64) -> String {
    grouped(&format!("{v:.0}"))
}

fn rate(v: f64) -> String {
    format!("{v:.2}").replace('.', ",")
}

/// The trips and the calendar from 1 January through the month, fetched
/// off the UI thread.
async fn load(cfg: Config, month: Month) -> Result<(Vec<Trip>, Vec<Occurrence>), String> {
    tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let trips = service::load_trips(&cfg, None)?;
        let cal = service::calendar(&cfg, month)?;
        Ok((trips, cal.occurrences))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

/// The trips not reimbursed: how many commutes and private trips, and each
/// trip that needs fixing with why.
struct Skipped {
    commute: usize,
    private: usize,
    fix: Vec<(String, &'static str)>,
}

fn skipped(m: &MileageMonth) -> Skipped {
    let mut s = Skipped {
        commute: 0,
        private: 0,
        fix: Vec::new(),
    };
    for (t, status) in &m.skipped {
        match status {
            Status::Commute => s.commute += 1,
            Status::Private => s.private += 1,
            Status::Incomplete(why) => s.fix.push((
                format!("{} {} → {}", t.start.format("%d-%m %H:%M"), t.from, t.to),
                why,
            )),
            Status::Business => {}
        }
    }
    s
}

/// Writes the month's logbook beside the config, as `logbook -o` does.
fn save_logbook(setup: &Setup, cfg: &Config, m: &MileageMonth) -> anyhow::Result<String> {
    use anyhow::Context;
    let mcfg = cfg.mileage.as_ref().context("no [mileage] section")?;
    let path = setup.dir.join(format!("logbook-{}.csv", m.month));
    let mut out = Vec::new();
    mileage::write_logbook(mcfg, m, &mut out)?;
    std::fs::write(&path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(format!("Wrote {}", path.display()))
}

#[component]
pub fn MileageScreen(setup: Signal<Setup>, screen: Signal<Screen>) -> Element {
    let mut month = use_signal(|| this_month(&setup.peek()));
    let mut message = use_signal(|| None::<Result<String, String>>);
    let loaded = use_resource(move || {
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
    if cfg.mileage.is_none() {
        return rsx! { MileageSetup { setup } };
    }
    let m = month();
    let head = rsx! {
        div { class: "screen-head",
            div { class: "month-nav",
                button {
                    class: "nav-btn",
                    "aria-label": "Previous month",
                    onclick: move |_| {
                        message.set(None);
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
                        month.set(step(m, 1));
                    },
                    Icon { d: "m9 6 6 6-6 6", size: 18 }
                }
            }
        }
    };
    if setup.read().trips.as_ref().is_none_or(|t| t.count == 0) {
        return rsx! {
            {head}
            div { class: "alert", role: "status",
                span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                div {
                    "No trips yet. Import the Min Volkswagen export or sync your account in Settings. "
                    button { class: "link-btn", onclick: move |_| screen.set(Screen::Settings), "Open Settings" }
                }
            }
        };
    }
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
                        b { "The logbook can't be shown: " }
                        "{e} "
                        button { class: "link-btn", onclick: move |_| screen.set(Screen::Settings), "Open Settings" }
                    }
                }
            };
        }
        Some(Ok(l)) => l,
    };
    let mil = match service::mileage_for(&cfg, m, trips, cal) {
        Ok(Some(mil)) => mil,
        Ok(None) => return rsx! {},
        Err(e) => {
            return rsx! {
                {head}
                div { class: "alert", role: "alert", "{e:#}" }
            };
        }
    };
    let year_km = mil.km_before + mil.total_km();
    let threshold = mil.rate.threshold_km;
    let low_km = year_km.min(threshold);
    let pct = if threshold > 0.0 {
        (low_km / threshold * 100.0).clamp(0.0, 100.0)
    } else {
        100.0
    };
    let skip = skipped(&mil);
    let not_reimbursed = mil.skipped.len();
    let entries: Vec<_> = mil
        .entries
        .iter()
        .map(|e| {
            (
                e.start.format("%d-%m-%Y").to_string(),
                e.start.format("%H:%M").to_string(),
                format!("{} → {}", e.from, e.to),
                e.purpose.clone(),
                km(e.km),
                rate(e.rate),
                kroner(e.amount()).trim_end_matches(" kr").to_string(),
            )
        })
        .collect();

    rsx! {
        {head}
        div { class: "kpis mileage-kpis",
            section { class: "kpi",
                h2 { "This month" }
                span { class: "kpi-value", "{kroner(mil.total_amount())}" }
                span { class: "kpi-sub", "{km(mil.total_km())} km · {mil.entries.len()} business trips" }
            }
            section { class: "kpi span2",
                div { class: "kpi-row",
                    h2 { "Business km in {m.year}" }
                    span { class: "small",
                        b { "{km(low_km)}" }
                        " of {whole_km(threshold)} km at the low rate"
                    }
                }
                div {
                    class: "bar-track",
                    role: "progressbar",
                    "aria-label": "Business km at the low rate",
                    "aria-valuemin": "0",
                    "aria-valuemax": "{threshold}",
                    "aria-valuenow": "{low_km.round()}",
                    div { class: "bar-fill", style: "width: {pct:.1}%" }
                }
                span { class: "kpi-sub",
                    "{rate(mil.rate.low)} kr/km up to {whole_km(threshold)} km, then {rate(mil.rate.high)} kr/km ({mil.rate.year} rates from config.toml)."
                }
            }
        }

        section { class: "panel", "aria-labelledby": "biz-h",
            div { class: "panel-head",
                h2 { id: "biz-h", "Business trips" }
                span { class: "note", "Date, route, purpose and km on every trip, as Skattestyrelsen requires" }
            }
            if entries.is_empty() {
                p { class: "empty", "No business trips in {month_title(m)}." }
            } else {
                div { class: "scroll-x wide-only",
                    table { class: "days logbook",
                        thead {
                            tr {
                                th { scope: "col", "Date" }
                                th { scope: "col", "Depart" }
                                th { scope: "col", "From → To" }
                                th { scope: "col", "Purpose" }
                                th { scope: "col", class: "r", "Km" }
                                th { scope: "col", class: "r", "Rate" }
                                th { scope: "col", class: "r", "Amount" }
                            }
                        }
                        tbody {
                            for (date , depart , route , purpose , km , rate , amount) in entries.iter().cloned() {
                                tr {
                                    td { class: "nowrap", "{date}" }
                                    td { class: "mono small", "{depart}" }
                                    td { "{route}" }
                                    td { "{purpose}" }
                                    td { class: "r", "{km}" }
                                    td { class: "r", "{rate}" }
                                    td { class: "r strong", "{amount}" }
                                }
                            }
                            tr { class: "total",
                                td { colspan: "4", "Total" }
                                td { class: "r", "{km(mil.total_km())}" }
                                td {}
                                td { class: "r", "{kroner(mil.total_amount()).trim_end_matches(\" kr\")}" }
                            }
                        }
                    }
                }
                ul { class: "plain-list narrow-only",
                    for (date , depart , route , purpose , km , _rate , amount) in entries.iter().cloned() {
                        li { class: "trip-entry",
                            b { "{purpose}" }
                            b { class: "r", "{amount}" }
                            span { class: "sub", "{date} {depart} · {route}" }
                            span { class: "note r", "{km} km" }
                        }
                    }
                }
            }
        }

        section { class: "panel", "aria-labelledby": "skip-h",
            div { class: "panel-head",
                h2 { id: "skip-h", "Not reimbursed" }
                span { class: "note", "{not_reimbursed} trips" }
            }
            ul { class: "plain-list",
                li { class: "skip-row",
                    span { class: "chip", "Commute" }
                    b { "{skip.commute}" }
                    span { class: "sub", "Home ↔ workplace is never reimbursed" }
                }
                li { class: "skip-row",
                    span { class: "chip", "Private" }
                    b { "{skip.private}" }
                    span { class: "sub", "Errands, private appointments and private time" }
                }
                li { class: "skip-row",
                    span { class: "chip warn strong", "Needs fixing" }
                    b { "{skip.fix.len()}" }
                    span { class: "sub", "Business trips missing purpose, route or km are listed here" }
                }
                for (trip , why) in skip.fix.iter() {
                    li { class: "skip-row",
                        span {}
                        span {}
                        span { class: "sub", "{trip}: {why}" }
                    }
                }
            }
        }

        div { class: "actions",
            match message() {
                None => rsx! {},
                Some(Ok(t)) => rsx! { span { class: "status-ok", role: "status", "{t}" } },
                Some(Err(e)) => rsx! { span { class: "status-bad", role: "status", "{e}" } },
            }
            button {
                class: "small-btn",
                onclick: move |_| {
                    let result = save_logbook(&setup.peek(), &cfg, &mil).map_err(|e| format!("{e:#}"));
                    message.set(Some(result));
                },
                Icon { d: "M12 3v12M7 10l5 5 5-5M4 21h16", size: 15 }
                "Logbook CSV for signature"
            }
        }
    }
}

/// Sets up the logbook: name, registration number and home, with the
/// default rates.
#[component]
fn MileageSetup(setup: Signal<Setup>) -> Element {
    let initial = crate::setup::SettingsForm::from_config(setup.peek().config.as_ref());
    let mut driver = use_signal(String::new);
    let mut plate = use_signal(String::new);
    let mut home = use_signal(|| initial.home.clone());
    let mut error = use_signal(|| None::<String>);
    let save = move |_| {
        let form = MileageForm {
            driver_name: driver(),
            registration_number: plate(),
            home: home(),
        };
        let result = setup::add_mileage(&setup.peek(), &form);
        match result {
            Ok(()) => {
                let dir = setup.peek().dir.clone();
                setup.set(Setup::load(Ok(dir)));
            }
            Err(e) => error.set(Some(format!("{e:#}"))),
        }
    };
    rsx! {
        section { class: "card", "aria-labelledby": "m-setup",
            h2 { id: "m-setup", "Set up the mileage logbook" }
            span { class: "sub",
                "The logbook (kørselsregnskab) lists every business trip with date, route, purpose and km for the tax-free mileage allowance. It needs your name and the car’s registration number; home makes home ↔ workplace trips commuting."
            }
            div { class: "grid2",
                div { class: "field",
                    label { r#for: "m-driver", "Your name" }
                    input {
                        id: "m-driver",
                        r#type: "text",
                        value: "{driver}",
                        oninput: move |e| driver.set(e.value()),
                    }
                }
                div { class: "field",
                    label { r#for: "m-plate", "Registration number" }
                    input {
                        id: "m-plate",
                        r#type: "text",
                        value: "{plate}",
                        oninput: move |e| plate.set(e.value()),
                    }
                }
                div { class: "field span2",
                    label { r#for: "m-home", "Home" }
                    input {
                        id: "m-home",
                        r#type: "text",
                        value: "{home}",
                        oninput: move |e| home.set(e.value()),
                    }
                    span { class: "note", "As it appears in the trips’ addresses" }
                }
            }
            span { class: "note",
                "The 2026 rates (3,94 kr/km up to 20.000 km, then 2,28 kr/km) are written to [mileage] in config.toml with lønart 39 Kilometerpenge. Check them on skat.dk; add each new year’s rates there."
            }
            div { class: "actions",
                if let Some(e) = error() {
                    span { class: "status-bad", role: "status", "{e}" }
                }
                button { class: "primary", onclick: save, "Set up logbook" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn km_and_rates_read_like_the_mockup() {
        assert_eq!(km(384.6), "384,6");
        assert_eq!(km(3796.64), "3.796,6");
        assert_eq!(whole_km(20_000.0), "20.000");
        assert_eq!(rate(3.94), "3,94");
    }
}
