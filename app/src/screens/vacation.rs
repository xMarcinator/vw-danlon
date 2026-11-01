//! The Vacation screen, with Hours' period selector: Month · Year (a
//! calendar year). Both open with number cards and the status line. Year
//! shows the 24-month chart, the year's calendar (a month's name opens
//! it), when each holiday year expires, the periods to register and the
//! year's holidays. Month shows its deadline, the chart with the month
//! shaded, one big calendar to try changes in, and the month's vacation
//! with its registration ticks. A year before this one is shown as it stood
//! on 31 December, with its periods only.
use chrono::{Datelike, NaiveDate};
use dioxus::prelude::*;
use std::collections::BTreeMap;
use vw_danlon::config::Config;
use vw_danlon::service::{self, VacationView};

use vw_danlon::month::LeaveKind;
use vw_danlon::month::Month;
use vw_danlon::state::{DanlonBalance, Registered, RegisteredPeriod};
use vw_danlon::vacation::{self, Days, Draft, HolidayYear, Period, PeriodStatus, Rules};

use super::period::{self as pages, Bar, MONTH_YEAR, PagePeriods, View, dock};
use super::vacation_plan::{self as vp, CellKind};
use crate::setup::Setup;

fn date(d: NaiveDate) -> String {
    d.format("%-d %b %Y").to_string()
}

/// The day a period is seen from: today, or 31 December of a year that is
/// over.
fn seen_on(shown: pages::Period, today: NaiveDate) -> NaiveDate {
    let year_end = NaiveDate::from_ymd_opt(shown.anchor.year(), 12, 31).expect("31 Dec exists");
    today.min(year_end)
}

/// Whether the period falls (partly) between `first` and `last`.
fn overlaps(p: &Period, first: NaiveDate, last: NaiveDate) -> bool {
    p.first <= last && p.last >= first
}

/// The overview as on `day`, fetched off the UI thread.
async fn load(cfg: Config, day: NaiveDate) -> Result<VacationView, String> {
    tokio::task::spawn_blocking(move || service::vacation(&cfg, day, service::CalendarMode::Cached))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

/// "Concurrent holiday · employed since 1 May 2026 · 2,08 days a month".
fn rules_line(rules: &Rules) -> String {
    let per_month = Days(rules.per_month);
    match rules.employment_start {
        Some(start) => format!(
            "Concurrent holiday · employed since {} · {per_month} days a month",
            date(start)
        ),
        None => format!(
            "Concurrent holiday · {per_month} days a month · earning from 1 Sep (no employment start)"
        ),
    }
}

/// "19 Dec 2026 – 3 Jan 2027 · 6 working days of 16", as `vacation` writes it.
fn period_when(p: &Period) -> String {
    let dates = if p.first == p.last {
        date(p.first)
    } else {
        format!("{} – {}", date(p.first), date(p.last))
    };
    let mut days = match p.working_days {
        0 => "no working days".to_string(),
        1 => "1 working day".to_string(),
        n => format!("{n} working days"),
    };
    if p.calendar_days() != i64::from(p.working_days) && p.working_days > 0 {
        days.push_str(&format!(" of {}", p.calendar_days()));
    }
    format!("{dates} · {days}")
}

/// What is left after the period, and of what.
fn balance_after(p: &Period, rules: &Rules) -> Option<String> {
    if let Some((year, left)) = p.feriefridage_left
        && p.draws.is_empty()
    {
        return Some(format!("{left} of {} ({year})", rules.feriefridage));
    }
    p.draws
        .last()
        .map(|d| format!("{} ({})", d.left_after, d.year))
}

/// The chips of a period: where its days come from, then its status
/// (Taken, Ongoing, Planned, Feriefridag, or a repeat not in the calendar).
fn period_chips(p: &Period, repeat: bool) -> Vec<(String, &'static str)> {
    let mut chips: Vec<(String, &'static str)> = Vec::new();
    let several = p.draws.len() > 1;
    for d in &p.draws {
        chips.push((
            if several {
                format!("{} from {}", d.days, d.year)
            } else {
                format!("From {}", d.year)
            },
            "chip",
        ));
    }
    if let Some((year, _)) = p.feriefridage_left
        && p.kind == LeaveKind::Ferie
    {
        chips.push((format!("Feriefridage {year}"), "chip"));
    }
    if p.forskud {
        chips.push(("Forskudsferie: taken before earned".into(), "chip warn"));
    }
    if p.before_employment.0 > 0 {
        chips.push((
            format!("{} before the employment start", p.before_employment),
            "chip warn",
        ));
    }
    chips.push(match p.status {
        PeriodStatus::Taken => ("Taken".into(), "chip acc"),
        PeriodStatus::Ongoing => ("Ongoing".into(), "chip warn"),
        PeriodStatus::Planned if repeat => ("Repeats · not in the calendar".into(), "chip dashed"),
        PeriodStatus::Planned if p.kind == LeaveKind::Feriefridag => {
            ("Feriefridag".into(), "chip client")
        }
        PeriodStatus::Planned => ("Planned".into(), "chip info"),
    });
    chips
}

fn quoted(words: &[String]) -> String {
    words
        .iter()
        .map(|w| format!("“{w}”"))
        .collect::<Vec<_>>()
        .join(" or ")
}

/// The draft's bar: how many changes, Discard and Copy, and what was copied.
fn draft_bar(
    mut draft: Signal<Draft>,
    mut copied: Signal<Option<Result<String, String>>>,
    class: &'static str,
) -> Element {
    let n = draft.read().len();
    if n == 0 {
        return rsx! {};
    }
    let changes = if n == 1 {
        "1 change".to_string()
    } else {
        format!("{n} changes")
    };
    rsx! {
        div { class: "vac-draft {class}",
            div { class: "vac-draft-row",
                span {
                    b { "{changes}" }
                    " in the draft · nothing is saved"
                }
                button {
                    r#type: "button",
                    class: "small-btn",
                    onclick: move |_| {
                        draft.set(Draft::default());
                        copied.set(None);
                    },
                    "Discard"
                }
                button {
                    r#type: "button",
                    class: "small-btn primary-sm",
                    onclick: move |_| {
                        let text = vp::draft_text(&draft.read());
                        copied.set(Some(match crate::platform::copy_text(&text) {
                            Ok(()) => Ok(text),
                            Err(e) => Err(format!("The dates can't be copied: {e:#}")),
                        }));
                    },
                    "Copy"
                }
            }
            match &*copied.read() {
                Some(Ok(text)) => rsx! { pre { class: "vac-copied", "aria-live": "polite", "{text}" } },
                Some(Err(e)) => rsx! { span { class: "note", role: "alert", "{e}" } },
                None => rsx! {},
            }
        }
    }
}

/// The next 24 months, laid out by `flow-next-charts` and painted on the
/// app's colour tokens.
fn chart_view(c: &vp::Chart) -> Element {
    match c.scene() {
        Ok(scene) => crate::chart::painting(&scene.drawing, "vac", "vac-chart", &c.label),
        Err(e) => rsx! { span { class: "note", role: "alert", "The chart can't be drawn: {e}" } },
    }
}

/// One month's days, Monday first with week numbers; a weekday that can be
/// tried taps into the draft. `class` sizes it (the year's small months or
/// Month's big one).
fn month_days(
    m: vp::CalMonth,
    mut draft: Signal<Draft>,
    mut copied: Signal<Option<Result<String, String>>>,
    class: &'static str,
) -> Element {
    rsx! {
        div { class: "vac-days {class}",
            span {}
            for w in vp::WEEKDAYS {
                span { class: "wd", "{w}" }
            }
            for (wk , cells) in m.weeks {
                span { class: "wd", "{wk}" }
                for c in cells {
                    match c {
                        None => rsx! { span {} },
                        Some(c) => {
                            let class = format!(
                                "{}{}{}",
                                c.kind.class(),
                                if c.past { " past" } else { "" },
                                if c.today { " today" } else { "" },
                            );
                            let day = c.date.format("%-d").to_string();
                            if c.tappable {
                                let cell = c.clone();
                                rsx! {
                                    button {
                                        r#type: "button",
                                        class: "{class}",
                                        "aria-label": "{c.label}",
                                        "aria-pressed": if matches!(c.kind, CellKind::Add | CellKind::Remove) { "true" } else { "false" },
                                        onclick: move |_| {
                                            draft.with_mut(|d| vp::toggle(d, &cell));
                                            copied.set(None);
                                        },
                                        "{day}"
                                    }
                                }
                            } else {
                                rsx! {
                                    span { class: "{class}", "aria-label": "{c.label}", "{day}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A month of the phone's year: small days to look at, no taps (the
/// month's name opens it to try changes).
fn mini_days(m: &vp::CalMonth) -> Element {
    rsx! {
        div { class: "vac-mini-days", "aria-hidden": "true",
            for (_ , cells) in m.weeks.iter() {
                for c in cells.iter() {
                    match c {
                        None => rsx! { span {} },
                        Some(c) => {
                            let class = format!(
                                "{}{}{}",
                                c.kind.class(),
                                if c.past { " past" } else { "" },
                                if c.today { " today" } else { "" },
                            );
                            rsx! { span { class: "{class}", {c.date.format("%-d").to_string()} } }
                        }
                    }
                }
            }
        }
    }
}

/// The Danløn · Calendar · Teams ticks of a period to register.
fn tick_buttons(
    g: vp::Registration,
    registered: Vec<RegisteredPeriod>,
    cfg: Config,
    mut ticked: Signal<Option<Vec<RegisteredPeriod>>>,
    mut problem: Signal<Option<String>>,
) -> Element {
    rsx! {
        span { class: "vac-ticks",
            for (i , label) in [format!("Danløn {} h", g.hours()), "Calendar".to_string(), "Teams".to_string()]
                .into_iter()
                .enumerate()
            {
                {
                    let on = [g.ticks.danlon, g.ticks.calendar, g.ticks.teams][i];
                    let g = g.clone();
                    let list = registered.clone();
                    let cfg = cfg.clone();
                    rsx! {
                        button {
                            r#type: "button",
                            class: if on { "vac-tick on" } else { "vac-tick" },
                            "aria-pressed": if on { "true" } else { "false" },
                            onclick: move |_| {
                                let mut t: Registered = g.ticks;
                                match i {
                                    0 => t.danlon = !t.danlon,
                                    1 => t.calendar = !t.calendar,
                                    _ => t.teams = !t.teams,
                                }
                                match service::set_registered(&cfg, &g.title, g.first, t) {
                                    Ok(()) => {
                                        ticked.set(Some(vp::with_ticks(&list, &g.title, g.first, t)));
                                        problem.set(None);
                                    }
                                    Err(e) => problem.set(Some(format!("The tick can't be saved: {e:#}"))),
                                }
                            },
                            if on { "✓ {label}" } else { "○ {label}" }
                        }
                    }
                }
            }
        }
    }
}

/// A period of the list: its dates, what is left after it and its chips;
/// `ticks` when it still has to be registered.
fn period_row(p: &Period, repeat: bool, rules: &Rules, ticks: Option<Element>) -> Element {
    rsx! {
        li { class: "period",
            span { class: "period-text",
                b { "{p.title}" }
                span { class: "sub", "{period_when(p)}" }
            }
            if let Some(b) = balance_after(p, rules) {
                span { class: "period-balance",
                    "left after"
                    br {}
                    b { "{b}" }
                }
            }
            span { class: "period-chips",
                for (text , class) in period_chips(p, repeat) {
                    span { class: "{class}", "{text}" }
                }
            }
            if let Some(t) = ticks {
                div { class: "period-ticks", {t} }
            }
        }
    }
}

#[component]
pub fn VacationScreen(setup: Signal<Setup>) -> Element {
    // The month or calendar year shown, kept while the app runs.
    let mut shown_period = use_context::<PagePeriods>().vacation;
    let cal_sync = use_context::<Signal<super::CalendarSync>>();
    let draft = use_signal(Draft::default);
    let copied = use_signal(|| None::<Result<String, String>>);
    // Ticks saved since the overview was loaded.
    let ticked = use_signal(|| None::<Vec<RegisteredPeriod>>);
    let mut problem = use_signal(|| None::<String>);
    // Danløn's balances saved since loading, the holiday year being typed
    // in, and its fields.
    let mut balances = use_signal(|| None::<BTreeMap<i32, DanlonBalance>>);
    let mut editing = use_signal(|| None::<HolidayYear>);
    let mut dl_days = use_signal(String::new);
    let mut dl_on = use_signal(String::new);
    let day = use_memo(move || {
        let today = super::month::today_of(&setup.read());
        seen_on(shown_period(), today)
    });
    let loaded = use_resource(move || {
        let _ = cal_sync.read().version;
        let day = day();
        let cfg = setup.read().config.clone();
        async move {
            match cfg {
                Some(cfg) => load(cfg, day).await.map(|v| (day, v)),
                None => Err("no config".to_string()),
            }
        }
    });
    let Some(cfg) = setup.read().config.clone() else {
        return rsx! {};
    };
    let shown = shown_period();
    let today = super::now(&cfg).date_naive();
    let bar = Bar::picker(
        shown,
        today,
        &MONTH_YEAR,
        Callback::new(move |p: pages::Period| shown_period.set(p)),
    );
    let head = bar.top(rsx! {});
    let docked = |actions: Element| dock(&bar, actions);
    let loaded = loaded.read();
    let v = match &*loaded {
        Some(Ok((on, v))) if *on == day() => v,
        Some(Err(e)) => {
            return rsx! {
                {head}
                div { class: "alert", role: "alert",
                    span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                    div { b { "Vacation can't be shown: " } "{e}" }
                }
                {docked(rsx! {})}
            };
        }
        _ => {
            return rsx! {
                {head}
                p { class: "placeholder", role: "status", "Loading the calendar…" }
                {docked(rsx! {})}
            };
        }
    };
    // The plan runs from today; a year that is over shows its periods only.
    let as_of_today = day() == today;
    let is_month = shown.view == View::Month;
    let month: Month = shown.month();
    let year = shown.anchor.year();
    let (first, last) = shown.days();
    let drafted = draft.read().clone();
    let plan = if drafted.is_empty() || !as_of_today {
        v.plan.clone()
    } else {
        vacation::plan(&cfg, &v.leave, today, &drafted)
    };
    let registered: Vec<RegisteredPeriod> = ticked().unwrap_or_else(|| v.registered.clone());
    let done: Vec<(String, NaiveDate)> = registered
        .iter()
        .filter(|r| r.ticks.all())
        .map(|r| (r.title.clone(), r.first))
        .collect();
    let status = vacation::status(&plan, &done);
    let danlon: BTreeMap<i32, DanlonBalance> =
        balances().unwrap_or_else(|| v.danlon_balances.clone());
    let kpis = if is_month {
        vp::month_kpis(&plan, &v.rules, &v.leave, month)
    } else {
        vp::year_kpis(&plan, &v.rules, &v.leave, year)
    };
    let deadline = is_month
        .then(|| vp::month_deadline(&plan, &v.rules, month))
        .flatten();
    let chart = vp::chart(&plan, &v.rules, is_month.then_some(month));
    let warnings = vp::warnings(&plan, &status);
    let pools = vp::pools(&plan, &v.rules);
    let regs = vp::registrations(&plan, &registered);
    let month_regs = vp::registrations_between(&plan, &registered, first, last);
    let source = if as_of_today {
        &plan.overview
    } else {
        &v.overview
    };
    let periods: Vec<(Period, bool)> = source
        .periods
        .iter()
        .filter(|p| overlaps(p, first, last))
        .map(|p| {
            (
                p.clone(),
                as_of_today && plan.repeat_days.contains(&p.first),
            )
        })
        .collect();
    let repeats = vacation::repeat_days(&cfg, &v.leave, today, vacation::horizon(today));
    let planned = vp::planned(&v.leave, &repeats);
    let months = vp::year_calendar(&cfg, year, today, &planned, &drafted);
    let repeats_note = vp::repeats_note(&cfg.vacation.repeats);
    let keywords = quoted(&cfg.vacation.keywords);
    let fridag = quoted(&cfg.vacation.feriefridag_keywords);
    let status_class = if status.warn() {
        "vac-status warn"
    } else {
        "vac-status"
    };
    let shown_name = shown.name();
    let keys = rsx! {
        div { class: "vac-keys",
            span { class: "chip key-ferie", "Vacation" }
            span { class: "chip key-repeat", "Repeats" }
            span { class: "chip client", "Feriefridag" }
            span { class: "chip info", "Holiday" }
            span { class: "chip warn", "Acies day off" }
            span { class: "chip key-we", "Weekend" }
        }
    };

    let chart_panel = rsx! {
        if as_of_today {
            section { class: "panel vac-a-chart", "aria-labelledby": "v-chart",
                div { class: "panel-head",
                    h2 { id: "v-chart", "Next 24 months" }
                    span { class: "note",
                        if is_month && plan.months.iter().any(|m| m.month == month) {
                            "{month.first_day().format(\"%b %Y\")} shaded · days"
                        } else {
                            "days"
                        }
                    }
                }
                div { class: "vac-legend",
                    span { span { class: "key k-vac", "aria-hidden": "true" } "Vacation used" }
                    span { span { class: "key k-fri", "aria-hidden": "true" } "Feriefridag" }
                    span { span { class: "key k-line", "aria-hidden": "true" } "Balance" }
                    span { span { class: "key k-deadline", "aria-hidden": "true" } "Deadline" }
                }
                div { class: "vac-chart-wrap", {chart_view(&chart)} }
                if !warnings.is_empty() {
                    ul { class: "vac-warnings",
                        for w in warnings {
                            li { "{w}" }
                        }
                    }
                }
            }
        }
    };

    let pools_panel = rsx! {
        if as_of_today {
            section { class: "panel vac-pools vac-a-side", "aria-label": "When each holiday year expires",
                for r in pools {
                    div { class: "vac-pool",
                        b { "{r.name}" }
                        b { class: if r.lost.is_some() { "bad" } else { "" },
                            {r.lost.clone().unwrap_or_else(|| "0 lost".into())}
                        }
                        span { class: "sub", "{r.sub}" }
                        span { class: "sub right", {r.paid.clone().unwrap_or_default()} }
                        if let Some(y) = r.year {
                            {
                                let saved = danlon.get(&y.0).copied();
                                let open = editing() == Some(y);
                                let start = move |_| {
                                    dl_days.set(saved.map(|b| Days(b.days).to_string()).unwrap_or_default());
                                    dl_on.set(saved.map_or(today, |b| b.read_on).to_string());
                                    editing.set(Some(y));
                                };
                                if open {
                                    let cfg = cfg.clone();
                                    let cfg2 = cfg.clone();
                                    let all = danlon.clone();
                                    let all2 = danlon.clone();
                                    rsx! {
                                        div { class: "vac-dl-form", role: "group", "aria-label": "Danløn's balance for {y}",
                                            label { class: "label",
                                                "Days left in Danløn"
                                                input {
                                                    r#type: "text",
                                                    inputmode: "decimal",
                                                    value: "{dl_days}",
                                                    oninput: move |e| dl_days.set(e.value()),
                                                }
                                            }
                                            label { class: "label",
                                                "Read on"
                                                input {
                                                    r#type: "date",
                                                    value: "{dl_on}",
                                                    oninput: move |e| dl_on.set(e.value()),
                                                }
                                            }
                                            span { class: "note vac-dl-wide",
                                                "Typed in from the Danløn app. The app's own count still drives the plan."
                                            }
                                            div { class: "vac-dl-actions vac-dl-wide",
                                                if saved.is_some() {
                                                    button {
                                                        r#type: "button",
                                                        class: "link-btn bad",
                                                        onclick: move |_| {
                                                            match service::set_danlon_balance(&cfg2, y, None) {
                                                                Ok(()) => {
                                                                    let mut m = all2.clone();
                                                                    m.remove(&y.0);
                                                                    balances.set(Some(m));
                                                                    editing.set(None);
                                                                    problem.set(None);
                                                                }
                                                                Err(e) => problem.set(Some(format!("Danløn's balance can't be removed: {e:#}"))),
                                                            }
                                                        },
                                                        "Remove"
                                                    }
                                                }
                                                button {
                                                    r#type: "button",
                                                    class: "small-btn",
                                                    onclick: move |_| editing.set(None),
                                                    "Cancel"
                                                }
                                                button {
                                                    r#type: "button",
                                                    class: "small-btn primary-sm",
                                                    onclick: move |_| {
                                                        let Some(days) = vp::parse_days(&dl_days()) else {
                                                            problem.set(Some("Type the days as in Danløn, e.g. 8,50.".into()));
                                                            return;
                                                        };
                                                        let Ok(read_on) = dl_on().parse::<NaiveDate>() else {
                                                            problem.set(Some("Pick the date you read the balance.".into()));
                                                            return;
                                                        };
                                                        let b = DanlonBalance { read_on, days };
                                                        match service::set_danlon_balance(&cfg, y, Some(b)) {
                                                            Ok(()) => {
                                                                let mut m = all.clone();
                                                                m.insert(y.0, b);
                                                                balances.set(Some(m));
                                                                editing.set(None);
                                                                problem.set(None);
                                                            }
                                                            Err(e) => problem.set(Some(format!("Danløn's balance can't be saved: {e:#}"))),
                                                        }
                                                    },
                                                    "Save"
                                                }
                                            }
                                        }
                                    }
                                } else if let Some(b) = saved {
                                    let app = vacation::balance_on(&v.rules, &v.leave, y, b.read_on);
                                    let (text, diff, apart) = vp::danlon_comparison(Days(b.days), app, b.read_on);
                                    rsx! {
                                        div { class: "vac-dl",
                                            span { class: "vac-dl-text", "{text}" }
                                            span { class: if apart { "vac-dl-diff apart" } else { "vac-dl-diff" }, "{diff}" }
                                            button { r#type: "button", class: "vac-link", onclick: start, "Change" }
                                        }
                                    }
                                } else {
                                    rsx! {
                                        button { r#type: "button", class: "vac-link vac-dl-wide", onclick: start, "Enter Danløn's balance" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    let note = rsx! {
        div { class: "vac-note vac-a-side", role: "note",
            if let Some(n) = repeats_note {
                span { "{n}" }
            }
            span {
                "Vacation is an all-day {keywords} event in the calendar ({fridag} for a feriefridag). "
                "Earned 1 Sep – 31 Aug, usable until 31 Dec the year after; feriefridage are used first."
            }
        }
    };

    let body = if is_month {
        let m = months
            .into_iter()
            .nth(month.month as usize - 1)
            .expect("a year has twelve months");
        rsx! {
            div { class: "vac-layout",
                if let Some(dl) = deadline {
                    div { class: if dl.warn { "vac-deadline warn vac-a-side" } else { "vac-deadline vac-a-side" }, role: "note",
                        span { "aria-hidden": "true", class: "vac-deadline-mark", "!" }
                        span {
                            b { "{dl.head}" }
                            " · {dl.text}"
                        }
                    }
                }
                {chart_panel}
                section { class: "panel vac-a-cal vac-mcal", "aria-label": "Calendar {shown_name}",
                    div { class: "vac-cal-head",
                        {keys}
                        span { class: "note", "Tap a weekday to try it in the draft" }
                    }
                    {month_days(m, draft, copied, "big")}
                }
                section { class: "panel vac-a-side", "aria-labelledby": "v-mlist",
                    div { class: "panel-head",
                        h2 { id: "v-mlist", "Vacation this month" }
                    }
                    if periods.is_empty() {
                        p { class: "empty", "No vacation in {shown_name}." }
                    }
                    ul { class: "plain-list",
                        for (p , repeat) in periods {
                            {
                                let ticks = month_regs
                                    .iter()
                                    .find(|g| g.title == p.title && g.first == p.first)
                                    .filter(|g| !g.ticks.all())
                                    .map(|g| tick_buttons(g.clone(), registered.clone(), cfg.clone(), ticked, problem));
                                period_row(&p, repeat, &v.rules, ticks)
                            }
                        }
                    }
                }
                {pools_panel}
                {note}
            }
        }
    } else {
        rsx! {
            div { class: "vac-layout",
                {chart_panel}
                section { class: "vac-a-cal vac-cal", "aria-label": "Calendar {year}",
                    {keys}
                    div { class: "vac-months wide-only",
                        for (i , m) in months.iter().cloned().enumerate() {
                            {
                                let name = m.name.clone();
                                rsx! {
                            section { class: "vac-month", "aria-label": "{name} {year}",
                                button {
                                    r#type: "button",
                                    class: "vac-month-open",
                                    "aria-label": "Open {name} {year}",
                                    onclick: move |_| shown_period.set(pages::Period {
                                        view: View::Month,
                                        anchor: NaiveDate::from_ymd_opt(year, i as u32 + 1, 1).expect("a month's first day"),
                                    }),
                                    "{name}"
                                    crate::Icon { d: "m9 6 6 6-6 6", size: 14 }
                                }
                                {month_days(m, draft, copied, "")}
                            }
                                }
                            }
                        }
                    }
                    div { class: "vac-minis narrow-only",
                        for (i , m) in months.iter().enumerate() {
                            div { class: "vac-mini",
                                button {
                                    r#type: "button",
                                    class: "vac-month-open",
                                    "aria-label": "Open {m.name} {year}",
                                    onclick: move |_| shown_period.set(pages::Period {
                                        view: View::Month,
                                        anchor: NaiveDate::from_ymd_opt(year, i as u32 + 1, 1).expect("a month's first day"),
                                    }),
                                    {m.name.chars().take(3).collect::<String>()}
                                    crate::Icon { d: "m9 6 6 6-6 6", size: 14 }
                                }
                                {mini_days(m)}
                            }
                        }
                    }
                }
                {pools_panel}
                if as_of_today && !regs.is_empty() {
                    section { class: "panel vac-a-side", "aria-labelledby": "v-reg",
                        div { class: "panel-head",
                            h2 { id: "v-reg", "Not registered yet" }
                            span { class: "note", "next 60 days" }
                        }
                        ul { class: "plain-list",
                            for g in regs {
                                li { class: "vac-reg",
                                    span { class: "vac-reg-head",
                                        b { "{g.title}" }
                                        span { class: "sub",
                                            if g.working_days == 1 { "1 day" } else { "{g.working_days} days" }
                                        }
                                    }
                                    span { class: "sub",
                                        if g.first == g.last {
                                            "{date(g.first)}"
                                        } else {
                                            "{date(g.first)} – {date(g.last)}"
                                        }
                                    }
                                    {tick_buttons(g.clone(), registered.clone(), cfg.clone(), ticked, problem)}
                                }
                            }
                        }
                    }
                }
                section { class: "panel vac-a-side", "aria-labelledby": "v-list",
                    div { class: "panel-head",
                        h2 { id: "v-list", "Holidays" }
                        span { class: "note", "{year} · oldest days are used first" }
                    }
                    if periods.is_empty() {
                        p { class: "empty", "No vacation in {year} in the calendar." }
                    }
                    ul { class: "plain-list",
                        for (p , repeat) in periods {
                            {period_row(&p, repeat, &v.rules, None)}
                        }
                    }
                }
                {note}
            }
        }
    };

    rsx! {
        {head}
        span { class: "sub", "{rules_line(&v.rules)}" }
        if !as_of_today {
            span { class: "note", "As on {date(day())}, the end of {year}." }
        }
        if let Some(w) = &v.calendar_warning {
            div { class: "alert", role: "status", "{w}" }
        }
        if let Some(e) = problem() {
            div { class: "alert", role: "alert", "{e}" }
        }
        if as_of_today {
            div { class: "{status_class}", role: "status",
                b { "{status.headline}" }
                span { class: "vac-next",
                    span { "aria-hidden": "true", "→" }
                    span { "{status.next_step}" }
                }
            }
        }
        div { class: "kpis vac-kpis",
            for k in kpis {
                section { class: "kpi",
                    h2 { "{k.label}" }
                    span { class: "kpi-value {k.tone}", "{k.value}" }
                    if !k.sub.is_empty() {
                        span { class: "kpi-sub", "{k.sub}" }
                    }
                }
            }
        }
        {draft_bar(draft, copied, "wide-only")}
        {body}
        {docked(draft_bar(draft, copied, "narrow-only"))}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vw_danlon::month::LeaveKind;
    use vw_danlon::vacation::Draw;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn a_year_is_seen_on_today_or_its_last_day() {
        let today = d("2026-11-26");
        let this = pages::Period::new(today).pick(View::Year, today);
        assert_eq!(seen_on(this, today), today);
        assert_eq!(seen_on(this.step(1), today), today);
        assert_eq!(seen_on(this.step(-1), today), d("2025-12-31"));
        // A month is seen like its year.
        assert_eq!(
            seen_on(pages::Period::new(today).step(-12), today),
            d("2025-12-31")
        );
        let p = |first: &str, last: &str| Period {
            kind: LeaveKind::Ferie,
            title: "Ferie".into(),
            first: d(first),
            last: d(last),
            working_days: 1,
            status: PeriodStatus::Planned,
            draws: Vec::new(),
            forskud: false,
            before_employment: Days(0),
            feriefridage_left: None,
        };
        let (jan, dec) = (d("2027-01-01"), d("2027-12-31"));
        assert!(overlaps(&p("2027-01-01", "2027-01-01"), jan, dec));
        assert!(!overlaps(&p("2026-12-31", "2026-12-31"), jan, dec));
        // Across New Year: in both years.
        let across = p("2026-12-19", "2027-01-03");
        assert!(overlaps(&across, jan, dec));
        assert!(overlaps(&across, d("2026-01-01"), d("2026-12-31")));
    }

    #[test]
    fn a_period_reads_like_the_vacation_command() {
        let rules = Rules {
            employment_start: Some(d("2026-05-01")),
            per_month: 208,
            feriefridage: 5,
            feriefridage_from: (5, 1),
            max_forskud: 500,
        };
        let p = Period {
            kind: LeaveKind::Ferie,
            title: "Juleferie".into(),
            first: d("2026-12-19"),
            last: d("2027-01-03"),
            working_days: 6,
            status: PeriodStatus::Planned,
            draws: vec![
                Draw {
                    year: HolidayYear(2025),
                    days: Days(132),
                    left_after: Days(0),
                },
                Draw {
                    year: HolidayYear(2026),
                    days: Days(468),
                    left_after: Days(2032),
                },
            ],
            forskud: true,
            before_employment: Days(0),
            feriefridage_left: None,
        };
        assert_eq!(
            period_when(&p),
            "19 Dec 2026 – 3 Jan 2027 · 6 working days of 16"
        );
        assert_eq!(
            balance_after(&p, &rules).as_deref(),
            Some("20,32 (2026/27)")
        );
        let chips: Vec<String> = period_chips(&p, false).into_iter().map(|c| c.0).collect();
        assert_eq!(
            chips,
            [
                "1,32 from 2025/26",
                "4,68 from 2026/27",
                "Forskudsferie: taken before earned",
                "Planned"
            ]
        );
        assert_eq!(
            rules_line(&rules),
            "Concurrent holiday · employed since 1 May 2026 · 2,08 days a month"
        );
    }
}
