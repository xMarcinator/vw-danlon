//! Writes the import file for Danløn (Lønberegning → Indlæs fra fil).
//!
//! The exact layout must match what your Danløn account expects; it is
//! configured through `[danlon]` in the config file.

use std::io::Write;

use anyhow::Result;

use crate::config::{Column, Config, HoursReporting};
use crate::mileage::MileageMonth;
use crate::month::MonthReport;

#[derive(Debug)]
pub struct Line {
    pub lonart: String,
    pub quantity: f64,
    pub rate: Option<f64>,
    pub text: &'static str,
}

impl Line {
    pub fn amount(&self) -> Option<f64> {
        self.rate
            .map(|r| (self.quantity * r * 100.0).round() / 100.0)
    }
}

pub fn lines(cfg: &Config, report: &MonthReport, mileage: Option<&MileageMonth>) -> Vec<Line> {
    let mut out = Vec::new();
    if cfg.danlon.hours == HoursReporting::Actual
        && let Some(lonart) = &cfg.danlon.hours_lonart
    {
        out.push(Line {
            lonart: lonart.clone(),
            quantity: report.worked_total(),
            rate: None,
            text: "Timer",
        });
    }
    if let (Some(m), Some(lonart)) = (mileage, &cfg.danlon.mileage_lonart) {
        for (rate, text) in [
            (m.rate.low, "Kørsel (lav sats)"),
            (m.rate.high, "Kørsel (høj sats)"),
        ] {
            out.push(Line {
                lonart: lonart.clone(),
                quantity: m.km_at(rate),
                rate: Some(rate),
                text,
            });
        }
    }
    out.retain(|l| l.quantity > 0.0);
    out
}

pub fn write(cfg: &Config, report: &MonthReport, lines: &[Line], mut w: impl Write) -> Result<()> {
    let d = &cfg.danlon;
    let sep = d.delimiter.to_string();
    if d.header {
        let names: Vec<_> = d.columns.iter().map(column_name).collect();
        writeln!(w, "{}", names.join(&sep))?;
    }
    // Without a rate or amount column, a second line with the same lønart
    // could replace the first in Danløn, so merge them (e.g. 39
    // Kilometerpenge, where Danløn applies its own rate).
    let mut merged: Vec<Line> = Vec::new();
    let per_rate = d
        .columns
        .iter()
        .any(|c| matches!(c, Column::Rate | Column::Amount));
    for l in lines {
        match merged.iter_mut().find(|m| m.lonart == l.lonart) {
            Some(m) if !per_rate => m.quantity += l.quantity,
            _ => merged.push(Line {
                lonart: l.lonart.clone(),
                quantity: l.quantity,
                rate: if per_rate { l.rate } else { None },
                text: l.text,
            }),
        }
    }
    for line in &merged {
        let num = |v: Option<f64>| v.map(|v| number(v, d.decimal_comma)).unwrap_or_default();
        let fields: Vec<String> = d
            .columns
            .iter()
            .map(|c| match c {
                Column::Employee => cfg.employee_number.clone(),
                Column::Lonart => line.lonart.clone(),
                Column::Quantity => num(Some(line.quantity)),
                Column::Rate => num(line.rate),
                Column::Amount => num(line.amount()),
                Column::Text => line.text.to_string(),
                Column::PeriodStart => report.month.first_day().format("%d-%m-%Y").to_string(),
                Column::PeriodEnd => report.month.last_day().format("%d-%m-%Y").to_string(),
                Column::Empty => String::new(),
            })
            .collect();
        writeln!(w, "{}", fields.join(&sep))?;
    }
    Ok(())
}

fn column_name(c: &Column) -> &'static str {
    match c {
        Column::Employee => "Medarbejdernr",
        Column::Lonart => "Lønart",
        Column::Quantity => "Antal",
        Column::Rate => "Sats",
        Column::Amount => "Beløb",
        Column::Text => "Tekst",
        Column::PeriodStart => "Fra",
        Column::PeriodEnd => "Til",
        Column::Empty => "",
    }
}

fn number(v: f64, decimal_comma: bool) -> String {
    let s = format!("{v:.2}");
    if decimal_comma {
        s.replace('.', ",")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mileage;
    use crate::month::{self, Month};
    use crate::trips::{Trip, parse_time};

    #[test]
    fn hours_and_mileage_export() {
        let cfg: Config = toml::from_str(
            r#"
            employee_number = "7"
            [mileage]
            driver_name = "T"
            registration_number = "AB12345"
            home = "Hjem"
            rates = [{ year = 2026, low = 3.94, high = 2.28 }]
            [danlon]
            hours = "actual"
            hours_lonart = "H"
            mileage_lonart = "K"
            columns = ["employee", "lonart", "quantity", "rate", "amount"]
            "#,
        )
        .unwrap();
        cfg.validate().unwrap();
        let tz = cfg.timezone;
        let trips = vec![Trip {
            start: parse_time("2026-10-05 07:00", tz).unwrap(),
            end: parse_time("2026-10-05 15:30", tz).unwrap(),
            distance_km: Some(42.0),
            from: "Hjem".into(),
            to: "Kunde A".into(),
            purpose: "Møde".into(),
            business: None,
        }];
        let m = Month::parse("2026-10").unwrap();
        let report = month::build(&cfg, m, &trips, &[], &Default::default());
        let mil = mileage::compute(cfg.mileage.as_ref().unwrap(), None, m, &trips, &[]).unwrap();
        let mut buf = Vec::new();
        write(&cfg, &report, &lines(&cfg, &report, Some(&mil)), &mut buf).unwrap();
        assert_eq!(
            String::from_utf8(buf).unwrap(),
            "7;H;8,00;;\n7;K;42,00;3,94;165,48\n"
        );
    }

    #[test]
    fn same_lonart_merged_without_rate_column() {
        let cfg: Config = toml::from_str(
            r#"
            employee_number = "7"
            [danlon]
            mileage_lonart = "39"
            columns = ["employee", "lonart", "quantity"]
            "#,
        )
        .unwrap();
        let report = month::build(
            &cfg,
            Month::parse("2026-10").unwrap(),
            &[],
            &[],
            &Default::default(),
        );
        let lines = [
            Line {
                lonart: "39".into(),
                quantity: 100.0,
                rate: Some(3.94),
                text: "lav",
            },
            Line {
                lonart: "39".into(),
                quantity: 50.5,
                rate: Some(2.28),
                text: "høj",
            },
        ];
        let mut buf = Vec::new();
        write(&cfg, &report, &lines, &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "7;39;150,50\n");
    }
}
