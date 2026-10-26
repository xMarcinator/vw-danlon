//! The Vacation screen: the open holiday years' balances, the feriefridage,
//! days about to expire and every vacation period with the year it draws
//! from, as `vacation` prints them.

use chrono::NaiveDate;
use dioxus::prelude::*;
use vw_danlon::config::Config;
use vw_danlon::service::{self, VacationView};
use vw_danlon::vacation::{Days, Period, PeriodStatus, Rules, YearBalance};

use crate::setup::Setup;

fn date(d: NaiveDate) -> String {
    d.format("%-d %b %Y").to_string()
}

/// The overview, fetched off the UI thread.
async fn load(cfg: Config) -> Result<VacationView, String> {
    tokio::task::spawn_blocking(move || {
        let today = super::now(&cfg).date_naive();
        service::vacation(&cfg, today, service::CalendarMode::Cached)
    })
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

/// Label, value and line under it of a holiday year's card.
fn year_card(y: &YearBalance) -> (String, String, String) {
    if y.earning_done {
        (
            format!("{} · earning done", y.year),
            format!("{} left", y.left),
            format!(
                "{} earned · {} taken · {} planned · use by {}",
                y.earned,
                y.taken,
                y.planned,
                date(y.year.use_by())
            ),
        )
    } else {
        (
            format!("{} · earning now", y.year),
            format!("{} earned", y.earned),
            format!(
                "{} by {} · {} taken · {} planned · {} left · use by {}",
                y.total,
                date(y.year.last_day()),
                y.taken,
                y.planned,
                y.left,
                date(y.year.use_by())
            ),
        )
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
    if let Some((year, left)) = p.feriefridage_left {
        return Some(format!("{left} of {} ({year})", rules.feriefridage));
    }
    p.draws
        .last()
        .map(|d| format!("{} ({})", d.left_after, d.year))
}

/// The chips of a period: where its days come from, then its status.
fn period_chips(p: &Period) -> Vec<(String, &'static str)> {
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
    if let Some((year, _)) = p.feriefridage_left {
        chips.push((format!("Feriefridag {year}"), "chip"));
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

#[component]
pub fn VacationScreen(setup: Signal<Setup>) -> Element {
    let cal_sync = use_context::<Signal<super::CalendarSync>>();
    let loaded = use_resource(move || {
        let _ = cal_sync.read().version;
        let cfg = setup.read().config.clone();
        async move {
            match cfg {
                Some(cfg) => load(cfg).await,
                None => Err("no config".to_string()),
            }
        }
    });
    let Some(cfg) = setup.read().config.clone() else {
        return rsx! {};
    };
    let loaded = loaded.read();
    let v = match &*loaded {
        None => return rsx! { p { class: "placeholder", role: "status", "Loading the calendar…" } },
        Some(Err(e)) => {
            return rsx! {
                div { class: "alert", role: "alert",
                    span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                    div { b { "Vacation can't be shown: " } "{e}" }
                }
            };
        }
        Some(Ok(v)) => v,
    };
    let o = &v.overview;
    let mut cards: Vec<(String, String, String)> = o.years.iter().map(year_card).collect();
    let taken: i64 = o.years.iter().map(|y| y.taken.0).sum();
    let planned: i64 = o.years.iter().map(|y| y.planned.0).sum();
    cards.push((
        "Taken and planned".into(),
        format!("{} + {}", Days(taken), Days(planned)),
        "days in the open holiday years".into(),
    ));
    for f in &o.feriefridage {
        cards.push((
            format!("Feriefridage {}", f.year),
            format!("{} of {}", f.left(), f.allowance),
            format!("per contract · {} taken · {} planned", f.taken, f.planned),
        ));
    }
    let keywords = quoted(&cfg.vacation.keywords);
    let fridag = quoted(&cfg.vacation.feriefridag_keywords);

    rsx! {
        span { class: "sub", "{rules_line(&v.rules)}" }
        if let Some(w) = &v.calendar_warning {
            div { class: "alert", role: "status", "{w}" }
        }
        for e in o.expiring.iter() {
            div { class: "alert", role: "alert",
                span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                span {
                    b { "{e.days} days from {e.year} expire on {date(e.date)}." }
                    " Plan or transfer them."
                }
            }
        }
        div { class: "kpis",
            for (label , value , sub) in cards {
                section { class: "kpi",
                    h2 { "{label}" }
                    span { class: "kpi-value", "{value}" }
                    span { class: "kpi-sub", "{sub}" }
                }
            }
        }
        div { class: "two-col",
            section { class: "panel", "aria-labelledby": "v-list",
                div { class: "panel-head",
                    h2 { id: "v-list", "Holidays" }
                    span { class: "note", "oldest days are used first" }
                }
                if o.periods.is_empty() {
                    p { class: "empty", "No vacation in the calendar." }
                }
                ul { class: "plain-list",
                    for p in o.periods.iter() {
                        li { class: "period",
                            span { class: "period-text",
                                b { "{p.title}" }
                                span { class: "sub", "{period_when(p)}" }
                            }
                            if let Some(b) = balance_after(p, &v.rules) {
                                span { class: "period-balance",
                                    "left after"
                                    br {}
                                    b { "{b}" }
                                }
                            }
                            span { class: "period-chips",
                                for (text , class) in period_chips(p) {
                                    span { class: "{class}", "{text}" }
                                }
                            }
                        }
                    }
                }
            }
            section { class: "card", "aria-labelledby": "v-how",
                h2 { id: "v-how", "How vacation is counted" }
                span { class: "sub",
                    "An all-day {keywords} event in the calendar ({fridag} for a feriefridag)."
                }
                span { class: "note",
                    "Earned 1 Sep – 31 Aug, usable until 31 Dec the year after."
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vw_danlon::month::LeaveKind;
    use vw_danlon::vacation::{Draw, HolidayYear};

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn a_period_reads_like_the_vacation_command() {
        let rules = Rules {
            employment_start: Some(d("2026-05-01")),
            per_month: 208,
            feriefridage: 5,
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
        let chips: Vec<String> = period_chips(&p).into_iter().map(|c| c.0).collect();
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
