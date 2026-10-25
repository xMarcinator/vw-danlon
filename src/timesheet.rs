//! Working-time record (arbejdstidsregistrering).
//!
//! Since 1 July 2024 Danish employers must record each employee's daily
//! working time and keep the records for 5 years. This writes one month as
//! semicolon-separated text with every day of the month.

use std::io::Write;

use anyhow::Result;
use chrono::{Datelike, Weekday};

use crate::config::Config;
use crate::month::MonthReport;

pub fn write(cfg: &Config, r: &MonthReport, mut w: impl Write) -> Result<()> {
    let n = |v: f64| format!("{v:.2}").replace('.', ",");
    writeln!(w, "Arbejdstidsregistrering;{}", r.month)?;
    writeln!(w, "Medarbejdernr.;{}", cfg.employee_number)?;
    writeln!(w)?;
    writeln!(
        w,
        "Dato;Ugedag;Start;Slut;Perioder;Arbejdsadresse;Frokost;Arbejdstid (timer);Normtid (timer);Difference;Bemærkning"
    )?;
    for d in &r.days {
        let time = |t: Option<chrono::DateTime<chrono_tz::Tz>>| {
            t.map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
        };
        let worked = d.first_start.is_some();
        writeln!(
            w,
            "{};{};{};{};{};{};{};{};{};{};{}",
            d.date.format("%d-%m-%Y"),
            weekday_da(d.date.weekday()),
            time(d.first_start),
            time(d.last_end),
            csv_field(&d.periods_text()),
            if worked {
                csv_field(&d.work_locations.join(" | "))
            } else {
                String::new()
            },
            if worked {
                d.lunch_text()
            } else {
                String::new()
            },
            n(d.worked_hours),
            n(d.norm_hours),
            n(d.difference()),
            csv_field(&d.remark())
        )?;
    }
    writeln!(w)?;
    writeln!(
        w,
        "I alt;;;;;;;{};{};{};",
        n(r.worked_total()),
        n(r.norm_total()),
        n(r.worked_total() - r.norm_total())
    )?;
    Ok(())
}

fn csv_field(s: &str) -> String {
    if s.contains([';', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn weekday_da(d: Weekday) -> &'static str {
    match d {
        Weekday::Mon => "man",
        Weekday::Tue => "tir",
        Weekday::Wed => "ons",
        Weekday::Thu => "tor",
        Weekday::Fri => "fre",
        Weekday::Sat => "lør",
        Weekday::Sun => "søn",
    }
}
