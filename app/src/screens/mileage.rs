//! The Mileage logbook screen: a month's business trips at the year's
//! rates and the trips not reimbursed, as `logbook` computes them, and the
//! logbook for signature: a print view with signature lines (printed on the
//! desktop, shared on Android) and the CSV file (written beside the config
//! on the desktop, shared on Android). Without a `[mileage]` section it sets
//! one up.

use chrono::Datelike;
use dioxus::prelude::*;
use vw_danlon::calendar::Occurrence;
use vw_danlon::config::Config;
use vw_danlon::mileage::{self, Driven, Entry, MileageMonth, Reason, Status};
use vw_danlon::month::Month;
use vw_danlon::service;
use vw_danlon::trips::Trip;

use super::kroner;
use super::month::month_title;
use super::period::{
    PagePeriods, Period, PeriodPicker, View, WEEK_MONTH, YearBar, calendar_year_title,
};
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

/// What a month's logbook is built from, loaded off the UI thread: the
/// trips and the calendar from 1 January through the month.
struct Loaded {
    month: Month,
    trips: Vec<Trip>,
    cal: Vec<Occurrence>,
}

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

/// The year's befordringsfradrag estimate, when it can be made: without
/// home and workplace there is none (it is information only).
async fn load_fradrag(cfg: Config, year: i32) -> Option<mileage::Fradrag> {
    tokio::task::spawn_blocking(move || {
        let today = super::now(&cfg).date_naive();
        service::fradrag(&cfg, year, today, service::CalendarMode::Cached).ok()
    })
    .await
    .ok()
    .flatten()
}

/// The trips of the period shown: the month's, or the week's from its one
/// or two months, each month computed as the monthly logbook computes it.
#[derive(Default)]
struct Shown {
    entries: Vec<Entry>,
    driving: Vec<Driven>,
    skipped: Vec<(Trip, Status)>,
}

impl Shown {
    fn of(months: &[&MileageMonth], period: &Period) -> Self {
        let mut shown = Shown::default();
        for m in months {
            let within =
                |start: &chrono::DateTime<chrono_tz::Tz>| period.contains(start.date_naive());
            shown
                .entries
                .extend(m.entries.iter().filter(|e| within(&e.start)).cloned());
            shown
                .driving
                .extend(m.driving.iter().filter(|d| within(&d.trip.start)).cloned());
            shown
                .skipped
                .extend(m.skipped.iter().filter(|(t, _)| within(&t.start)).cloned());
        }
        shown
    }

    fn total_km(&self) -> f64 {
        self.entries.iter().map(|e| e.km).fold(0.0, |a, b| a + b)
    }

    fn total_amount(&self) -> f64 {
        self.entries
            .iter()
            .map(Entry::amount)
            .fold(0.0, |a, b| a + b)
    }
}

/// Business km in the year of `last` (the period's last month) up to the
/// end of the period: the low-rate bar.
fn year_km(last: &MileageMonth, period: &Period) -> f64 {
    let end = period.days().1;
    last.km_before
        + last
            .entries
            .iter()
            .filter(|e| e.start.date_naive() <= end)
            .map(|e| e.km)
            .fold(0.0, |a, b| a + b)
}

/// Which list the Mileage screen shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Reimbursable,
    All,
    Fradrag,
}

/// The year's commuting days by month, newest first: (month, days, notes
/// on the days not counted).
fn fradrag_months(f: &mileage::Fradrag) -> Vec<(Month, usize, String)> {
    let month_of = |d: &chrono::NaiveDate| Month {
        year: d.year(),
        month: d.month(),
    };
    let mut months: Vec<Month> = f
        .days
        .iter()
        .chain(f.excluded.iter().map(|(d, _)| d))
        .map(month_of)
        .collect();
    months.sort_by_key(|m| std::cmp::Reverse((m.year, m.month)));
    months.dedup();
    months
        .into_iter()
        .map(|m| {
            let days = f.days.iter().filter(|d| month_of(d) == m).count();
            let mut why: Vec<(&str, usize)> = Vec::new();
            for (_, w) in f.excluded.iter().filter(|(d, _)| month_of(d) == m) {
                match why.iter_mut().find(|(x, _)| x == w) {
                    Some((_, n)) => *n += 1,
                    None => why.push((w, 1)),
                }
            }
            let note = if why.is_empty() {
                String::new()
            } else {
                let parts: Vec<String> = why.iter().map(|(w, n)| format!("{n} {w}")).collect();
                format!("not counted: {}", parts.join(", "))
            };
            (m, days, note)
        })
        .collect()
}

/// "1 trip", "7 trips".
fn trips_text(n: usize) -> String {
    if n == 1 {
        "1 trip".to_string()
    } else {
        format!("{n} trips")
    }
}

/// The "why it counts" chip of a trip: text and class.
fn rule_chip(d: &Driven) -> (String, &'static str) {
    match &d.reason {
        Reason::ClientVisit => ("Client visit".into(), "chip client"),
        Reason::DirectKm => ("Client visit · direct km".into(), "chip client"),
        Reason::SetByYou => ("Set by you".into(), "chip warn"),
        Reason::Commute => ("Commute".into(), "chip"),
        Reason::SixtyDays { day } => (format!("60-day rule · day {day}"), "chip"),
        Reason::Detour => ("Detour".into(), "chip"),
        Reason::Errand => ("Errand".into(), "chip"),
        Reason::Private => ("Private".into(), "chip"),
        Reason::Appointment => ("Private appointment".into(), "chip"),
        Reason::MarkedPrivate => ("Marked private".into(), "chip"),
        Reason::Incomplete(why) => (format!("Needs fixing · {why}"), "chip warn strong"),
    }
}

/// What a not-reimbursable rule means, for the list of km not paid.
fn reason_text(r: &Reason) -> (&'static str, &'static str) {
    match r {
        Reason::Commute => ("Commute", "Home ↔ workplace"),
        Reason::SixtyDays { .. } => ("60-day rule", "Same client from day 61"),
        Reason::Detour => ("Detour", "Stops after a client visit"),
        Reason::Errand => ("Errand", "Errands without a business purpose"),
        Reason::Private => ("Private", "Evenings, weekends, other driving"),
        Reason::Appointment => ("Private appointment", "To, from or during private time"),
        Reason::MarkedPrivate => ("Marked private", "In the trips file"),
        Reason::SetByYou => ("Set by you", "Set to private"),
        Reason::ClientVisit | Reason::DirectKm => ("Client visit", "Reimbursed"),
        Reason::Incomplete(_) => ("Needs fixing", "Missing purpose, route or km"),
    }
}

/// The km not reimbursed by rule, in the order first met: (rule, trips,
/// km). The trips that need fixing are listed on their own.
fn not_reimbursed(m: &Shown) -> Vec<(Reason, usize, f64)> {
    let mut out: Vec<(Reason, usize, f64)> = Vec::new();
    for d in m.driving.iter().filter(|d| d.private_km() > 0.0) {
        let reason = d.private_reason();
        if matches!(reason, Reason::Incomplete(_)) {
            continue;
        }
        let key = std::mem::discriminant(&reason);
        match out
            .iter_mut()
            .find(|(r, _, _)| std::mem::discriminant(r) == key)
        {
            Some((_, n, km)) => {
                *n += 1;
                *km += d.private_km();
            }
            None => out.push((reason, 1, d.private_km())),
        }
    }
    out
}

/// The trips that need fixing, with why.
fn to_fix(m: &Shown) -> Vec<(String, &'static str)> {
    m.skipped
        .iter()
        .filter_map(|(t, status)| match status {
            Status::Incomplete(why) => Some((
                format!("{} {} → {}", t.start.format("%d-%m %H:%M"), t.from, t.to),
                *why,
            )),
            _ => None,
        })
        .collect()
}

/// One row of the logbook as shown.
#[derive(Clone, PartialEq)]
struct Row {
    start: chrono::DateTime<chrono_tz::Tz>,
    date: String,
    short_date: String,
    depart: String,
    route: String,
    purpose: String,
    rule: String,
    rule_class: &'static str,
    /// Reimbursed: the Business tick.
    business: bool,
    set_by_you: bool,
    km: String,
    rate: String,
    amount: String,
}

fn rows(m: &Shown, all: bool) -> Vec<Row> {
    m.driving
        .iter()
        .filter(|d| all || d.km > 0.0)
        .map(|d| {
            let entry = m.entries.iter().find(|e| e.start == d.trip.start);
            let (rule, rule_class) = rule_chip(d);
            let route = match entry {
                Some(e) => format!("{} → {}", e.from, e.destination()),
                None => format!("{} → {}", d.trip.from, d.trip.to),
            };
            let km_shown = if d.km > 0.0 {
                d.km
            } else {
                d.trip.distance_km.unwrap_or(0.0)
            };
            Row {
                start: d.trip.start,
                date: d.trip.start.format("%d-%m-%Y").to_string(),
                short_date: d.trip.start.format("%-d %b").to_string(),
                depart: d.trip.start.format("%H:%M").to_string(),
                route,
                purpose: if d.purpose.is_empty() {
                    reason_text(&d.private_reason()).0.to_string()
                } else {
                    d.purpose.clone()
                },
                rule,
                rule_class,
                business: d.km > 0.0,
                set_by_you: d.reason == Reason::SetByYou,
                km: km(km_shown),
                rate: entry.map(|e| rate(e.rate)).unwrap_or_default(),
                amount: entry
                    .map(|e| kroner(e.amount()).trim_end_matches(" kr").to_string())
                    .unwrap_or_else(|| "—".to_string()),
            }
        })
        .collect()
}

/// The month's logbook CSV, as `logbook -o` writes it.
fn logbook_csv(cfg: &Config, m: &MileageMonth) -> anyhow::Result<Vec<u8>> {
    use anyhow::Context;
    let mcfg = cfg.mileage.as_ref().context("no [mileage] section")?;
    let mut out = Vec::new();
    mileage::write_logbook(mcfg, m, &mut out)?;
    Ok(out)
}

/// Writes the month's logbook CSV beside the config.
#[cfg(not(target_os = "android"))]
fn save_logbook(setup: &Setup, cfg: &Config, m: &MileageMonth) -> anyhow::Result<String> {
    use anyhow::Context;
    let path = setup.dir.join(format!("logbook-{}.csv", m.month));
    std::fs::write(&path, logbook_csv(cfg, m)?)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(format!("Wrote {}", path.display()))
}

/// Shares the month's logbook CSV through the share sheet; the app's own
/// folder can't be reached from other apps.
#[cfg(target_os = "android")]
fn save_logbook(_setup: &Setup, cfg: &Config, m: &MileageMonth) -> anyhow::Result<String> {
    let csv = String::from_utf8(logbook_csv(cfg, m)?)?;
    crate::platform::share(
        &format!("Kørselsregnskab {}", m.month),
        "text/plain",
        &csv,
        None,
    )?;
    Ok("Shared the logbook CSV".into())
}

#[component]
pub fn MileageScreen(setup: Signal<Setup>, screen: Signal<Screen>) -> Element {
    // The week or month shown and Fradrag's year, kept while the app runs.
    let periods = use_context::<PagePeriods>();
    let mut period = periods.mileage;
    let mut fradrag_year = periods.fradrag;
    let months = use_memo(move || period().months());
    let mut message = use_signal(|| None::<Result<String, String>>);
    let mut printing = use_signal(|| false);
    let cal_sync = use_context::<Signal<super::CalendarSync>>();
    let mut tab = use_signal(|| Tab::Reimbursable);
    let mut copied = use_signal(|| None::<String>);
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
    let fradrag = use_resource(move || {
        let _ = cal_sync.read().version;
        let year = fradrag_year();
        let cfg = setup.read().config.clone();
        async move {
            match cfg {
                Some(cfg) => load_fradrag(cfg, year).await,
                None => None,
            }
        }
    });
    let Some(cfg) = setup.read().config.clone() else {
        return rsx! {};
    };
    if cfg.mileage.is_none() {
        return rsx! { MileageSetup { setup } };
    }
    let p = period();
    let (m, m2) = months();
    let shown = tab();
    let today = super::now(&cfg).date_naive();
    let head = if shown == Tab::Fradrag {
        let year = fradrag_year();
        let (title, sub) = calendar_year_title(year, today);
        rsx! {
            div { class: "period-head",
                YearBar {
                    title,
                    sub,
                    unit: "year",
                    note: "Whole calendar years",
                    current: year == today.year(),
                    on_step: move |by: i32| {
                        copied.set(None);
                        fradrag_year += by;
                    },
                    on_today: move |_| {
                        copied.set(None);
                        fradrag_year.set(today.year());
                    },
                }
            }
        }
    } else {
        rsx! {
            div { class: "period-head",
                PeriodPicker {
                    period: p,
                    today,
                    views: WEEK_MONTH.to_vec(),
                    on_change: move |next: Period| {
                        message.set(None);
                        printing.set(false);
                        period.set(next);
                    },
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
    let mut reload = loaded;
    let mut reload2 = loaded2;
    let failed = |e: &str| {
        rsx! {
            {head.clone()}
            div { class: "alert", role: "alert",
                span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                div {
                    b { "The logbook can't be shown: " }
                    "{e} "
                    button { class: "link-btn", onclick: move |_| screen.set(Screen::Settings), "Open Settings" }
                }
            }
        }
    };
    let loading = || {
        rsx! {
            {head.clone()}
            p { class: "placeholder", role: "status", "Loading {p.name()}…" }
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
    let cal = &l.cal;
    let compute = |l: &Loaded| match service::mileage_for(&cfg, l.month, &l.trips, &l.cal) {
        Ok(Some(mil)) => Ok(mil),
        Ok(None) => Err("no [mileage] section".to_string()),
        Err(e) => Err(format!("{e:#}")),
    };
    // The month's logbook: what the CSV and the print hold.
    let mil = match compute(l) {
        Ok(mil) => mil,
        Err(e) => {
            return rsx! {
                {head}
                div { class: "alert", role: "alert", "{e}" }
            };
        }
    };
    let mil2 = match l2.map(compute).transpose() {
        Ok(mil2) => mil2,
        Err(e) => {
            return rsx! {
                {head}
                div { class: "alert", role: "alert", "{e}" }
            };
        }
    };
    if printing() {
        let Some(mcfg) = cfg.mileage.as_ref() else {
            return rsx! {};
        };
        return rsx! {
            {head}
            PrintView {
                month: m,
                body: mileage::logbook_html_body(mcfg, &mil),
                page: mileage::logbook_html(mcfg, &mil),
                on_close: move |_| printing.set(false),
            }
        };
    }
    let mut parts = vec![&mil];
    parts.extend(mil2.as_ref());
    let last = parts[parts.len() - 1];
    let period_trips = Shown::of(&parts, &p);
    let year_km = year_km(last, &p);
    let threshold = last.rate.threshold_km;
    let low_km = year_km.min(threshold);
    let pct = if threshold > 0.0 {
        (low_km / threshold * 100.0).clamp(0.0, 100.0)
    } else {
        100.0
    };
    let show_all = shown == Tab::All;
    let rows = rows(&period_trips, show_all);
    let skipped = not_reimbursed(&period_trips);
    let fix = to_fix(&period_trips);
    let skipped_trips: usize = skipped.iter().map(|(_, n, _)| n).sum();
    let skipped_km: f64 = skipped.iter().map(|(_, _, k)| k).fold(0.0, |a, b| a + b);
    let name = p.name();
    let month_name = month_title(m);
    let tabs = move |class: &'static str| {
        rsx! {
            div { class: "{class}", role: "tablist", "aria-label": "Show",
                for (label , t) in [
                    ("Reimbursable", Tab::Reimbursable),
                    ("All driving", Tab::All),
                    ("Fradrag", Tab::Fradrag),
                ]
                {
                    button {
                        role: "tab",
                        "aria-selected": if shown == t { "true" } else { "false" },
                        onclick: move |_| {
                            copied.set(None);
                            tab.set(t);
                        },
                        "{label}"
                    }
                }
            }
        }
    };
    let mut set_business = move |start: chrono::DateTime<chrono_tz::Tz>, business: Option<bool>| {
        let Some(cfg) = setup.peek().config.clone() else {
            return;
        };
        // The list updates by itself; only a failure is worth a message.
        match service::set_trip_override(&cfg, start, business) {
            Ok(_) => message.set(None),
            Err(e) => message.set(Some(Err(format!("{e:#}")))),
        }
        reload.restart();
        if m2.is_some() {
            reload2.restart();
        }
    };

    let workplace = service::effective_workplace(&cfg, cal, m);
    rsx! {
        {head}
        match workplace {
            Some((found, service::WorkplaceSource::Detected)) => rsx! {
                div { class: "detected", role: "status",
                    span { class: "grow",
                        "Workplace from the calendar: "
                        b { "{found}" }
                    }
                    button {
                        class: "small-btn primary-sm",
                        onclick: move |_| {
                            let saved = setup::save_workplace(&setup.peek(), &found);
                            match saved {
                                Ok(()) => {
                                    let dir = setup.peek().dir.clone();
                                    setup.set(Setup::load(Ok(dir)));
                                    message.set(Some(Ok("Saved the workplace".into())));
                                }
                                Err(e) => message.set(Some(Err(format!("{e:#}")))),
                            }
                        },
                        "Save"
                    }
                }
            },
            None => rsx! {
                div { class: "alert", role: "status",
                    span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                    div {
                        "No fixed workplace: every work address counts as a client visit. "
                        button { class: "link-btn", onclick: move |_| screen.set(Screen::Settings), "Set it in Settings" }
                    }
                }
            },
            _ => rsx! {},
        }
        {tabs("mileage-tabs narrow-only")}
        if shown == Tab::Fradrag {
            match &*fradrag.read() {
                None => rsx! { p { class: "placeholder", role: "status", "Loading {fradrag_year}…" } },
                Some(f) => fradrag_view(f.as_ref(), fradrag_year(), today, copied, tabs("seg-tabs wide-only")),
            }
        } else {
        div { class: "kpis mileage-kpis",
            section { class: "kpi",
                h2 { if p.view == View::Week { "This week" } else { "This month" } }
                span { class: "kpi-value", "{kroner(period_trips.total_amount())}" }
                span { class: "kpi-sub",
                    "{km(period_trips.total_km())} km · {trips_text(period_trips.entries.len())}"
                }
            }
            section { class: "kpi span2",
                div { class: "kpi-row",
                    h2 { "Business km in {last.month.year}" }
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
                    "{rate(last.rate.low)} kr/km to {whole_km(threshold)} km, then {rate(last.rate.high)}"
                }
            }
        }

        section { class: "panel", "aria-labelledby": "biz-h",
            div { class: "panel-head",
                h2 { id: "biz-h",
                    if show_all { "All driving" } else { "Reimbursable trips" }
                }
                {tabs("seg-tabs wide-only")}
            }
            if rows.is_empty() {
                p { class: "empty",
                    if show_all {
                        "No driving in {name}."
                    } else {
                        "No reimbursable trips in {name}."
                    }
                }
            } else {
                div { class: "scroll-x wide-only",
                    table { class: "days logbook",
                        thead {
                            tr {
                                th { scope: "col", "Date" }
                                th { scope: "col", "Depart" }
                                th { scope: "col", "From → To" }
                                th { scope: "col", "Purpose" }
                                th { scope: "col", "Why it counts" }
                                th { scope: "col", "Counts" }
                                th { scope: "col", class: "r", "Km" }
                                th { scope: "col", class: "r", "Rate" }
                                th { scope: "col", class: "r", "Amount" }
                            }
                        }
                        tbody {
                            for r in rows.iter().cloned() {
                                // Keyed by the trip (and rate: a trip crossing
                                // 20.000 km is two rows), so a removed row
                                // never leaves its Business box on the next.
                                tr { key: "{r.start}{r.rate}", class: if r.business { "" } else { "quiet" },
                                    td { class: "nowrap", "{r.date}" }
                                    td { class: "mono small", "{r.depart}" }
                                    td { "{r.route}" }
                                    td { "{r.purpose}" }
                                    td {
                                        span { class: "{r.rule_class}", "{r.rule}" }
                                        if r.set_by_you {
                                            " "
                                            button {
                                                class: "link-btn small",
                                                onclick: move |_| set_business(r.start, None),
                                                "Reset"
                                            }
                                        }
                                    }
                                    td {
                                        label { class: "biz-check",
                                            input {
                                                r#type: "checkbox",
                                                checked: r.business,
                                                "aria-label": "Counts as business",
                                                onchange: move |e| set_business(r.start, Some(e.checked())),
                                            }
                                            "Business"
                                        }
                                    }
                                    td { class: "r", "{r.km}" }
                                    td { class: "r", "{r.rate}" }
                                    td { class: "r strong", "{r.amount}" }
                                }
                            }
                            tr { class: "total",
                                td { colspan: "6", "Total" }
                                td { class: "r", "{km(period_trips.total_km())}" }
                                td {}
                                td { class: "r", "{kroner(period_trips.total_amount()).trim_end_matches(\" kr\")}" }
                            }
                        }
                    }
                }
                ul { class: "plain-list narrow-only",
                    for r in rows.iter().cloned() {
                        li { key: "{r.start}{r.rate}", class: "trip-entry",
                            b { "{r.purpose}" }
                            b { class: "r", "{r.amount}" }
                            span { class: "sub", "{r.short_date} {r.depart} · {r.route}" }
                            span { class: "note r", "{r.km} km" }
                            span { class: "rule-cell",
                                span { class: "{r.rule_class}", "{r.rule}" }
                                if r.set_by_you {
                                    button {
                                        class: "link-btn small",
                                        onclick: move |_| set_business(r.start, None),
                                        "Reset"
                                    }
                                }
                            }
                            label { class: "biz-check r",
                                input {
                                    r#type: "checkbox",
                                    checked: r.business,
                                    "aria-label": "Counts as business",
                                    onchange: move |e| set_business(r.start, Some(e.checked())),
                                }
                                "Business"
                            }
                        }
                    }
                }
            }
        }

        section { class: "panel", "aria-labelledby": "skip-h",
            div { class: "panel-head",
                h2 { id: "skip-h", "Not reimbursable" }
                span { class: "note", "{trips_text(skipped_trips)} · {km(skipped_km)} km " }
                button { class: "link-btn small", onclick: move |_| screen.set(Screen::Settings), "Mileage rules" }
            }
            ul { class: "plain-list",
                for (reason , n , k) in skipped.iter() {
                    {
                        let (name, why) = reason_text(reason);
                        rsx! {
                            li { class: "skip-row",
                                span { class: "chip", "{name}" }
                                b { "{n}" }
                                span { class: "sub", "{why} · {km(*k)} km" }
                            }
                        }
                    }
                }
                li { class: "skip-row",
                    span { class: "chip warn strong", "Needs fixing" }
                    b { "{fix.len()}" }
                    span { class: "sub", "Missing purpose, route or km" }
                }
                for (trip , why) in fix.iter() {
                    li { class: "skip-row",
                        span {}
                        span {}
                        span { class: "sub", "{trip}: {why}" }
                    }
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
                Icon { d: CSV_ICON, size: 15 }
                "Logbook CSV · {month_name}"
            }
            button {
                class: "small-btn primary-sm",
                onclick: move |_| {
                    message.set(None);
                    printing.set(true);
                },
                Icon { d: PRINT_ICON, size: 15 }
                "Print {month_name} for signature"
            }
        }
    }
}

/// The Fradrag tab: the year's kørselsfradrag estimate, the two numbers
/// TastSelv asks for with Copy, and the commuting days by month.
fn fradrag_view(
    f: Option<&mileage::Fradrag>,
    year: i32,
    today: chrono::NaiveDate,
    copied: Signal<Option<String>>,
    tabs: Element,
) -> Element {
    let head = rsx! {
        div { class: "panel-head fradrag-head wide-only",
            h2 { class: "grow", "Kørselsfradrag {year}" }
            {tabs}
        }
    };
    let Some(f) = f.filter(|f| f.one_way_km.is_some()) else {
        return rsx! {
            section { class: "panel",
                {head}
                p { class: "empty",
                    match f.map(|f| f.trips_in_year) {
                        None => "Needs home and the workplace in Settings.".to_string(),
                        Some(0) => format!("No trips recorded in {year} yet."),
                        Some(n) => format!(
                            "None of your {n} trips in {year} qualify: no trip runs between home and the workplace."
                        ),
                    }
                }
            }
        };
    };
    let round_trip = format!("{:.0}", f.round_trip_km());
    let days = f.days.len().to_string();
    let estimate = format!("{} kr", whole_km(f.total()));
    let so_far = chrono::Datelike::year(&today) == f.year;
    let months = fradrag_months(f);
    let current = super::week::month_of(today);
    let copy = move |label: &'static str, text: String| {
        let mut copied = copied;
        copied.set(Some(match crate::platform::copy_text(&text) {
            Ok(()) => format!("Copied {text}"),
            Err(e) => format!("{label} can't be copied: {e:#}"),
        }));
    };
    rsx! {
        section { class: "panel fradrag", "aria-label": "Kørselsfradrag",
            {head}
            div { class: "fradrag-sum",
                b { class: "num", "{estimate}" }
                span { class: "muted", if so_far { "estimate, {f.year} so far" } else { "estimate" } }
            }
            for (label , value) in [("Km per day, round trip", round_trip), ("Commuting days", days)] {
                div { class: "fradrag-row",
                    span { class: "grow fradrag-num",
                        span { class: "pt-kicker", "{label}" }
                        b { class: "num", "{value}" }
                    }
                    button {
                        class: "small-btn",
                        "aria-label": "Copy {label}",
                        onclick: move |_| copy(label, value.clone()),
                        "Copy"
                    }
                }
            }
            if f.round_trip_km() <= 24.0 {
                span { class: "fradrag-foot", "Under 24 km a day: no deduction" }
            }
            span { class: "fradrag-foot muted", "TastSelv · rubrik 51" }
            if let Some(text) = copied() {
                div { class: "pt-status info", role: "status", "{text}" }
            }
        }
        section { class: "panel", "aria-label": "Commuting days by month",
            ul { class: "plain-list fradrag-months",
                for (m , n , note) in months {
                    li { class: "fradrag-month",
                        b { {m.first_day().format("%B").to_string()} }
                        b { class: "r num", if n == 1 { "1 day" } else { "{n} days" } }
                        span { class: "sub",
                            if m == current && !note.is_empty() {
                                "so far · {note}"
                            } else if m == current {
                                "so far"
                            } else {
                                "{note}"
                            }
                        }
                        span { class: "sub r num", "{km(n as f64 * f.round_trip_km())} km" }
                    }
                }
            }
        }
    }
}

/// A download arrow; on Android, where the CSV is shared, a share arrow.
#[cfg(not(target_os = "android"))]
const CSV_ICON: &str = "M12 3v12M7 10l5 5 5-5M4 21h16";
#[cfg(target_os = "android")]
const CSV_ICON: &str = "M4 12v8h16v-8M12 3v13M7 8l5-5 5 5";
const PRINT_ICON: &str = "M6 9V3h12v6M6 18H4v-7h16v7h-2M8 14h8v7H8z";

/// The logbook as it prints, with signature lines, and the button that
/// prints it (shares it on Android; see [`crate::platform::print_page`]).
#[component]
fn PrintView(month: Month, body: String, page: String, on_close: EventHandler<()>) -> Element {
    let mut message = use_signal(|| None::<Result<String, String>>);
    rsx! {
        style { {mileage::LOGBOOK_CSS} }
        div { class: "actions no-print print-bar",
            button { class: "small-btn", onclick: move |_| on_close.call(()), "Back to the logbook" }
            span { class: "grow" }
            match message() {
                None => rsx! {},
                Some(Ok(t)) => rsx! { span { class: "status-ok", role: "status", "{t}" } },
                Some(Err(e)) => rsx! { span { class: "status-bad", role: "status", "{e}" } },
            }
            button {
                class: "small-btn primary-sm",
                onclick: move |_| {
                    let title = format!("Kørselsregnskab {month}");
                    let result = crate::platform::print_page(&title, &page)
                        .map_err(|e| format!("{e:#}"));
                    message.set(result.transpose());
                },
                Icon { d: PRINT_ICON, size: 15 }
                "{crate::platform::PRINT_LABEL}"
            }
        }
        div { class: "print-sheet", dangerous_inner_html: "{body}" }
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
            span { class: "sub", "Your name, the car’s registration number and home." }
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
                    span { class: "note", "As in the trips’ addresses" }
                }
            }
            span { class: "note", "2026 rates: 3,94 kr/km to 20.000 km, then 2,28" }
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

    fn month(year: i32, month: u32, km_before: f64, trips: &[(&str, f64)]) -> MileageMonth {
        use chrono::TimeZone;
        let tz = chrono_tz::Europe::Copenhagen;
        MileageMonth {
            month: Month { year, month },
            rate: vw_danlon::config::MileageRate {
                year,
                low: 4.0,
                high: 2.0,
                threshold_km: 20_000.0,
            },
            km_before,
            entries: trips
                .iter()
                .map(|(day, km)| {
                    let date = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap();
                    Entry {
                        start: tz
                            .from_local_datetime(&date.and_hms_opt(8, 0, 0).unwrap())
                            .unwrap(),
                        from: "Home".into(),
                        to: "Client".into(),
                        via: Vec::new(),
                        purpose: "Client".into(),
                        km: *km,
                        rate: 4.0,
                    }
                })
                .collect(),
            skipped: Vec::new(),
            driving: Vec::new(),
        }
    }

    #[test]
    fn a_week_shows_its_trips_from_both_months() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
        let nov = month(
            2026,
            11,
            1000.0,
            &[("2026-11-02", 10.0), ("2026-11-30", 20.0)],
        );
        let dec = month(
            2026,
            12,
            1030.0,
            &[("2026-12-06", 30.0), ("2026-12-07", 40.0)],
        );
        // Week 49: Monday 30 Nov – Sunday 6 Dec.
        let w49 = Period::new(today).pick(View::Week, today).step(1);
        let shown = Shown::of(&[&nov, &dec], &w49);
        assert_eq!(shown.entries.len(), 2);
        assert_eq!(shown.total_km(), 50.0);
        assert_eq!(shown.total_amount(), 200.0);
        // The year's km up to the Sunday.
        assert_eq!(year_km(&dec, &w49), 1060.0);
        // An empty week reads 0, not -0 (a float sum starts at -0.0).
        let empty = Shown::of(&[&nov, &dec], &w49.step(2));
        assert!(empty.total_km().is_sign_positive() && empty.total_amount().is_sign_positive());
        // The month view is the whole month, as before.
        let month_view = Period::new(today);
        assert_eq!(Shown::of(&[&nov], &month_view).total_km(), 30.0);
        assert_eq!(year_km(&nov, &month_view), nov.km_year_to_date());
    }

    #[test]
    fn km_and_rates_read_like_the_mockup() {
        assert_eq!(km(384.6), "384,6");
        assert_eq!(km(3796.64), "3.796,6");
        assert_eq!(whole_km(20_000.0), "20.000");
        assert_eq!(rate(3.94), "3,94");
    }
}
