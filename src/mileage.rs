//! Tax-free mileage allowance (skattefri kørselsgodtgørelse).
//!
//! Follows Skattestyrelsen's requirements: each reimbursed trip must state
//! date, destination (mål og delmål), business purpose and km, and the
//! allowance is calculated at the year's rates — the low rate up to
//! 20,000 km per year, the high rate above. Commuting between home and a
//! fixed workplace never qualifies.
//!
//! Calendar entries away from the fixed workplace are business visits: the
//! day's first trip arriving at the entry's address and the last trip
//! leaving it are business trips, with the entry's title as purpose. Trips
//! in between (a lunch errand) are not.
//!
//! Private appointments (see [`crate::month::EntryKind::Private`]) are never
//! business visits: a trip arriving at or departing from an appointment's
//! address, or starting during one, is private. Recorded private time (see
//! [`crate::month::recorded_entries`]) counts as an appointment without an
//! address.

use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use chrono::{DateTime, Datelike, NaiveDate};
use chrono_tz::Tz;

use crate::address::{self, Places};
use crate::config::{MileageConfig, MileageRate};
use crate::month::{EntryKind, Month, TimedEntry};
use crate::trips::Trip;

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Business,
    Commute,
    Private,
    /// Marked or looks like business but lacks required documentation.
    Incomplete(&'static str),
}

/// Classifies a trip. `purpose` is the trip's own purpose, or the calendar
/// entry it drives to or from (see [`calendar_purposes`]). `appointment` is
/// whether the trip goes to, leaves or starts during a private appointment
/// (see [`touches_appointment`]). `places` locates home and the workplace
/// by coordinates too.
pub fn classify(
    trip: &Trip,
    purpose: &str,
    appointment: bool,
    cfg: &MileageConfig,
    workplace: Option<&str>,
    places: &Places,
) -> Status {
    if let (Some(work), Some(home)) = (workplace, &cfg.home) {
        let from = |place| places.at(&trip.from, trip.start_pos, place);
        let to = |place| places.at(&trip.to, trip.end_pos, place);
        if (from(home) && to(work)) || (from(work) && to(home)) {
            return Status::Commute;
        }
    }
    let business = trip.business.unwrap_or(!purpose.is_empty());
    if appointment || !business {
        return Status::Private;
    }
    if purpose.is_empty() {
        return Status::Incomplete("mangler formål");
    }
    if trip.from.is_empty() || trip.to.is_empty() {
        return Status::Incomplete("mangler fra/til");
    }
    if trip.distance_km.unwrap_or(0.0) <= 0.0 {
        return Status::Incomplete("mangler km");
    }
    Status::Business
}

/// Whether a trip arrives at or departs from the address of a private
/// appointment on the trip's date, or starts during one.
pub fn touches_appointment(trip: &Trip, calendar: &[TimedEntry], places: &Places) -> bool {
    calendar
        .iter()
        .filter(|e| e.kind == EntryKind::Private)
        .any(|e| {
            let at = |place: &str, pos| {
                e.start.date_naive() == trip.start.date_naive() && places.at(place, pos, e.location)
            };
            at(&trip.to, trip.end_pos)
                || at(&trip.from, trip.start_pos)
                || (e.start <= trip.start && trip.start < e.end)
        })
}

/// Purpose for each trip (same order as `trips`): its own, or the title of
/// the work entry away from the workplace that it is the day's first
/// arrival at or last departure from. Empty if neither.
pub fn calendar_purposes(
    trips: &[Trip],
    calendar: &[TimedEntry],
    workplace: Option<&str>,
    places: &Places,
) -> Vec<String> {
    let mut visits: BTreeMap<NaiveDate, Vec<(&str, &str)>> = BTreeMap::new();
    for e in calendar {
        if e.kind == EntryKind::Work
            && !e.location.trim().is_empty()
            && !workplace.is_some_and(|w| address::matches(e.location, w))
        {
            visits
                .entry(e.start.date_naive())
                .or_default()
                .push((e.location, e.summary));
        }
    }

    let mut out: Vec<String> = trips.iter().map(|t| t.purpose.clone()).collect();
    let mut by_date: BTreeMap<NaiveDate, Vec<usize>> = BTreeMap::new();
    for (i, t) in trips.iter().enumerate() {
        by_date.entry(t.start.date_naive()).or_default().push(i);
    }
    for (date, idx) in by_date {
        for (location, summary) in visits.get(&date).map(Vec::as_slice).unwrap_or(&[]) {
            let first_in = idx
                .iter()
                .find(|&&i| places.at(&trips[i].to, trips[i].end_pos, location));
            let last_out = idx
                .iter()
                .rev()
                .find(|&&i| places.at(&trips[i].from, trips[i].start_pos, location));
            for &i in first_in.into_iter().chain(last_out) {
                if out[i].is_empty() {
                    out[i] = summary.to_string();
                }
            }
        }
    }
    out
}

/// One reimbursed part of a trip, at a single rate.
#[derive(Debug, Clone)]
pub struct Entry {
    pub start: DateTime<Tz>,
    pub from: String,
    pub to: String,
    pub purpose: String,
    pub km: f64,
    pub rate: f64,
}

impl Entry {
    pub fn amount(&self) -> f64 {
        round2(self.km * self.rate)
    }
}

#[derive(Debug)]
pub struct MileageMonth {
    pub month: Month,
    pub rate: MileageRate,
    /// Business km earlier in the same year, before this month.
    pub km_before: f64,
    pub entries: Vec<Entry>,
    /// Trips in the month that were not reimbursed and why.
    pub skipped: Vec<(Trip, Status)>,
}

impl MileageMonth {
    pub fn km_at(&self, rate: f64) -> f64 {
        self.entries
            .iter()
            .filter(|e| e.rate == rate)
            .map(|e| e.km)
            .fold(0.0, |a, b| a + b)
    }
    pub fn total_km(&self) -> f64 {
        self.entries.iter().map(|e| e.km).fold(0.0, |a, b| a + b)
    }
    pub fn total_amount(&self) -> f64 {
        self.entries
            .iter()
            .map(Entry::amount)
            .fold(0.0, |a, b| a + b)
    }
}

/// `calendar` should cover the year up to the end of `month`, so business
/// km earlier in the year are counted the same way.
pub fn compute(
    cfg: &MileageConfig,
    workplace: Option<&str>,
    places: &Places,
    month: Month,
    trips: &[Trip],
    calendar: &[TimedEntry],
) -> Result<MileageMonth> {
    let rate = cfg.rate(month.year)?;
    let year_start = NaiveDate::from_ymd_opt(month.year, 1, 1).unwrap();
    let km = |t: &Trip| t.distance_km.unwrap_or(0.0);
    let purposes = calendar_purposes(trips, calendar, workplace, places);
    let classified = trips.iter().zip(&purposes).map(|(t, p)| {
        let appointment = touches_appointment(t, calendar, places);
        (t, p, classify(t, p, appointment, cfg, workplace, places))
    });

    let mut ytd = 0.0;
    let mut km_before = 0.0;
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    for (t, purpose, status) in classified {
        let d = t.start.date_naive();
        if d < year_start || d > month.last_day() {
            continue;
        }
        if !month.contains(d) {
            if status == Status::Business {
                km_before += km(t);
                ytd += km(t);
            }
            continue;
        }
        match status {
            Status::Business => {
                let total = km(t);
                let low = (rate.threshold_km - ytd).clamp(0.0, total);
                let entry = |km, rate| Entry {
                    start: t.start,
                    from: t.from.clone(),
                    to: t.to.clone(),
                    purpose: purpose.clone(),
                    km,
                    rate,
                };
                if low > 0.0 {
                    entries.push(entry(low, rate.low));
                }
                if total - low > 0.0 {
                    entries.push(entry(total - low, rate.high));
                }
                ytd += total;
            }
            status => skipped.push((t.clone(), status)),
        }
    }
    debug_assert!(entries.iter().all(|e| e.start.year() == month.year));
    Ok(MileageMonth {
        month,
        rate,
        km_before,
        entries,
        skipped,
    })
}

/// Writes the kørselsregnskab for the month as semicolon-separated text,
/// ready to print and sign as employer control.
pub fn write_logbook(cfg: &MileageConfig, m: &MileageMonth, mut w: impl Write) -> Result<()> {
    let n = |v: f64| format!("{v:.2}").replace('.', ",");
    writeln!(w, "Kørselsregnskab;{}", m.month)?;
    writeln!(w, "Navn;{}", cfg.driver_name)?;
    writeln!(w, "Reg.nr.;{}", cfg.registration_number)?;
    writeln!(w, "Erhvervskørte km i år før perioden;{}", n(m.km_before))?;
    writeln!(
        w,
        "Satser {};{} kr/km op til {} km;{} kr/km derover",
        m.rate.year,
        n(m.rate.low),
        m.rate.threshold_km,
        n(m.rate.high)
    )?;
    writeln!(w)?;
    writeln!(w, "Dato;Afgang;Fra;Til;Formål;Km;Sats;Beløb")?;
    for e in &m.entries {
        writeln!(
            w,
            "{};{};{};{};{};{};{};{}",
            e.start.format("%d-%m-%Y"),
            e.start.format("%H:%M"),
            csv_field(&e.from),
            csv_field(&e.to),
            csv_field(&e.purpose),
            n(e.km),
            n(e.rate),
            n(e.amount())
        )?;
    }
    writeln!(w)?;
    writeln!(w, "I alt;;;;;{};;{}", n(m.total_km()), n(m.total_amount()))?;
    writeln!(w)?;
    writeln!(
        w,
        "Kontrolleret og godkendt af arbejdsgiver;Dato:;;Underskrift:"
    )?;
    Ok(())
}

/// The month's logbook (kørselsregnskab) as an HTML fragment for printing
/// and signing: the same lines as [`write_logbook`], then signature lines
/// for the driver and the employer. Classes start with `lb-`; see
/// [`LOGBOOK_CSS`].
pub fn logbook_html_body(cfg: &MileageConfig, m: &MileageMonth) -> String {
    let n = |v: f64| format!("{v:.2}").replace('.', ",");
    let mut rows = String::new();
    for e in &m.entries {
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"lb-r\">{}</td><td class=\"lb-r\">{}</td><td class=\"lb-r\">{}</td></tr>\n",
            e.start.format("%d-%m-%Y"),
            e.start.format("%H:%M"),
            html_escape(&e.from),
            html_escape(&e.to),
            html_escape(&e.purpose),
            n(e.km),
            n(e.rate),
            n(e.amount())
        ));
    }
    if m.entries.is_empty() {
        rows.push_str("<tr><td colspan=\"8\">Ingen erhvervskørsel i perioden</td></tr>\n");
    }
    let sign = |who: &str| {
        format!(
            "<div class=\"lb-sign\"><h2>{who}</h2><div class=\"lb-lines\">\
             <div><span class=\"lb-line\"></span>Navn</div>\
             <div><span class=\"lb-line\"></span>Dato</div>\
             <div><span class=\"lb-line\"></span>Underskrift</div></div></div>\n"
        )
    };
    format!(
        "<article class=\"lb-sheet\">\n<h1>Kørselsregnskab {month}</h1>\n\
         <dl class=\"lb-facts\"><dt>Navn</dt><dd>{name}</dd><dt>Reg.nr.</dt><dd>{plate}</dd>\
         <dt>Erhvervskørte km i år før perioden</dt><dd>{before}</dd>\
         <dt>Satser {year}</dt><dd>{low} kr/km op til {threshold} km, {high} kr/km derover</dd></dl>\n\
         <table class=\"lb-table\"><thead><tr><th>Dato</th><th>Afgang</th><th>Fra</th><th>Til</th><th>Formål</th>\
         <th class=\"lb-r\">Km</th><th class=\"lb-r\">Sats</th><th class=\"lb-r\">Beløb</th></tr></thead>\n<tbody>\n{rows}</tbody>\n\
         <tfoot><tr><td colspan=\"5\">I alt</td><td class=\"lb-r\">{km}</td><td></td><td class=\"lb-r\">{amount}</td></tr></tfoot></table>\n\
         {driver}{employer}</article>\n",
        month = m.month,
        name = html_escape(&cfg.driver_name),
        plate = html_escape(&cfg.registration_number),
        before = n(m.km_before),
        year = m.rate.year,
        low = n(m.rate.low),
        threshold = m.rate.threshold_km,
        high = n(m.rate.high),
        km = n(m.total_km()),
        amount = n(m.total_amount()),
        driver = sign("Medarbejder"),
        employer = sign("Kontrolleret og godkendt af arbejdsgiver"),
    )
}

/// The styles of [`logbook_html_body`], for screen and print.
pub const LOGBOOK_CSS: &str = "\
.lb-sheet{font-family:'IBM Plex Sans',Arial,sans-serif;color:#111;background:#fff;font-size:12px;line-height:1.4}\
.lb-sheet h1{font-size:18px;margin:0 0 8px}\
.lb-sheet h2{font-size:12px;margin:0 0 18px;font-weight:600}\
.lb-facts{display:grid;grid-template-columns:max-content 1fr;gap:2px 12px;margin:0 0 12px}\
.lb-facts dt{color:#555}.lb-facts dd{margin:0}\
.lb-table{width:100%;border-collapse:collapse;margin-bottom:24px}\
.lb-table th,.lb-table td{border-bottom:1px solid #ccc;padding:4px 6px;text-align:left;vertical-align:top}\
.lb-table thead th{border-bottom:1.5px solid #111}\
.lb-table tfoot td{font-weight:600;border-top:1.5px solid #111;border-bottom:0}\
.lb-r,.lb-table td:nth-child(-n+2){white-space:nowrap}.lb-r{text-align:right!important}\
.lb-sign{margin-top:18px;break-inside:avoid}\
.lb-lines{display:grid;grid-template-columns:2fr 1fr 2fr;gap:16px}\
.lb-lines div{color:#555;font-size:11px}\
.lb-line{display:block;border-bottom:1px solid #111;height:28px;margin-bottom:3px}";

/// The logbook as a standalone HTML page, to share or print.
pub fn logbook_html(cfg: &MileageConfig, m: &MileageMonth) -> String {
    format!(
        "<!doctype html>\n<html lang=\"da\"><head><meta charset=\"utf-8\"><title>Kørselsregnskab {}</title>\
         <style>body{{margin:24px}}{LOGBOOK_CSS}</style></head><body>\n{}</body></html>\n",
        m.month,
        logbook_html_body(cfg, m)
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn csv_field(s: &str) -> String {
    if s.contains([';', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::Occurrence;
    use crate::month::timed_entries;
    use crate::trips::parse_time;

    fn cfg() -> MileageConfig {
        MileageConfig {
            driver_name: "Test".into(),
            registration_number: "AB12345".into(),
            home: Some("Hjem".into()),
            rates: vec![MileageRate {
                year: 2026,
                low: 3.94,
                high: 2.28,
                threshold_km: 20_000.0,
            }],
        }
    }

    fn trip(start: &str, from: &str, to: &str, purpose: &str, km: f64) -> Trip {
        let tz = chrono_tz::Europe::Copenhagen;
        let start = parse_time(start, tz).unwrap();
        Trip {
            start,
            end: start,
            distance_km: Some(km),
            from: from.into(),
            to: to.into(),
            purpose: purpose.into(),
            business: None,
            start_pos: None,
            end_pos: None,
        }
    }

    const WORK: Option<&str> = Some("Kontor");

    fn classify_own(t: &Trip, c: &MileageConfig) -> Status {
        classify(t, &t.purpose, false, c, WORK, &Places::default())
    }

    #[test]
    fn classification() {
        let c = cfg();
        assert_eq!(
            classify_own(
                &trip("2026-10-05 08:00", "Hjem", "kontor", "Arbejde", 20.0),
                &c
            ),
            Status::Commute
        );
        assert_eq!(
            classify_own(
                &trip("2026-10-05 08:00", "Hjem", "Kunde A", "Møde", 20.0),
                &c
            ),
            Status::Business
        );
        assert_eq!(
            classify_own(&trip("2026-10-05 08:00", "Hjem", "Netto", "", 3.0), &c),
            Status::Private
        );
        let mut t = trip("2026-10-05 08:00", "Hjem", "Kunde A", "", 20.0);
        t.business = Some(true);
        assert_eq!(classify_own(&t, &c), Status::Incomplete("mangler formål"));
    }

    fn visit(start: &str, summary: &str, location: &str) -> Occurrence {
        appointment(start, start, summary, location)
    }

    fn appointment(start: &str, end: &str, summary: &str, location: &str) -> Occurrence {
        let tz = chrono_tz::Europe::Copenhagen;
        let start = parse_time(start, tz).unwrap();
        Occurrence::Timed {
            start,
            end: parse_time(end, tz).unwrap(),
            summary: summary.into(),
            location: location.into(),
        }
    }

    #[test]
    fn calendar_visits_are_business() {
        let c = cfg();
        let odense = "Vestergade 12, 5000 Odense C";
        let cal = vec![
            visit("2026-10-05 08:00", "Kontor", "Kontor"),
            visit("2026-10-06 09:00", "Service Kunde A", odense),
            visit("2026-10-07 09:00", "Service Kunde A", odense),
        ];
        let trips = vec![
            // Monday at the workplace: commuting only.
            trip("2026-10-05 07:30", "Hjem", "Kontor", "", 25.0),
            trip("2026-10-05 16:00", "Kontor", "Hjem", "", 25.0),
            // Tuesday: home → customer, a lunch errand, customer → office.
            trip(
                "2026-10-06 07:45",
                "Hjem",
                "Vestergade 12, 5000 Odense",
                "",
                98.0,
            ),
            trip(
                "2026-10-06 11:30",
                "Vestergade 12, 5000 Odense",
                "Bager",
                "",
                3.0,
            ),
            trip(
                "2026-10-06 12:00",
                "Bager",
                "Vestergade 12, 5000 Odense",
                "",
                3.0,
            ),
            trip(
                "2026-10-06 15:00",
                "Vestergade 12, 5000 Odense",
                "Kontor",
                "",
                75.0,
            ),
            trip("2026-10-06 17:00", "Kontor", "Hjem", "", 25.0),
            // Wednesday: a colleague drives to the customer; the car stays at work.
            trip("2026-10-07 07:30", "Hjem", "Kontor", "", 25.0),
            trip("2026-10-07 16:00", "Kontor", "Hjem", "", 25.0),
        ];
        let cal = timed_entries(&cal, &[], "Frokost");
        let m = compute(
            &c,
            WORK,
            &Places::default(),
            Month::parse("2026-10").unwrap(),
            &trips,
            &cal,
        )
        .unwrap();
        let reimbursed: Vec<_> = m
            .entries
            .iter()
            .map(|e| (e.km, e.purpose.as_str()))
            .collect();
        assert_eq!(
            reimbursed,
            [(98.0, "Service Kunde A"), (75.0, "Service Kunde A")]
        );
        let commutes = m.skipped.iter().filter(|s| s.1 == Status::Commute).count();
        assert_eq!(commutes, 5);
    }

    #[test]
    fn private_appointment_trips_are_private() {
        let c = cfg();
        let doctor = "Torvet 3, 8000 Aarhus C, Danmark";
        let occurrences = vec![
            visit("2026-10-08 08:00", "Kontor", "Kontor"),
            appointment("2026-10-08 10:00", "2026-10-08 11:15", "Læge", doctor),
        ];
        let cal = timed_entries(&occurrences, &["Læge".to_string()], "Frokost");
        let trips = vec![
            trip(
                "2026-10-08 09:45",
                "Kontoret",
                "Torvet 3, 8000 Aarhus",
                "",
                4.0,
            ),
            // Starts during the appointment: private even with its own purpose.
            trip("2026-10-08 10:05", "Kontoret", "Apotek", "Indkøb", 1.0),
            trip(
                "2026-10-08 11:20",
                "Torvet 3, 8000 Aarhus",
                "Kontoret",
                "",
                4.0,
            ),
            // Business trips the same day are unaffected.
            trip("2026-10-08 13:00", "Kontoret", "Kunde B", "Service", 10.0),
        ];

        // A located appointment never becomes a business visit purpose.
        let purposes = calendar_purposes(&trips, &cal, WORK, &Places::default());
        assert_eq!(purposes, ["", "Indkøb", "", "Service"]);

        let m = compute(
            &c,
            WORK,
            &Places::default(),
            Month::parse("2026-10").unwrap(),
            &trips,
            &cal,
        )
        .unwrap();
        let statuses: Vec<_> = m.skipped.iter().map(|s| s.1.clone()).collect();
        assert_eq!(statuses, [const { Status::Private }; 3]);
        let mut logbook = Vec::new();
        write_logbook(&c, &m, &mut logbook).unwrap();
        let logbook = String::from_utf8(logbook).unwrap();
        assert!(!logbook.contains("Torvet") && !logbook.contains("Apotek"));
        assert!(logbook.contains("Kunde B"));

        // The printable logbook has the same trips, escaped, and the
        // signature lines.
        let mut c = c;
        c.driver_name = "Test <Testesen>".into();
        let html = logbook_html(&c, &m);
        assert!(
            html.contains("<td>Kunde B</td><td>Service</td><td class=\"lb-r\">10,00</td>"),
            "{html}"
        );
        assert!(html.contains("Test &lt;Testesen&gt;") && !html.contains("Torvet"));
        assert!(html.contains("Kørselsregnskab 2026-10"));
        assert_eq!(
            html.matches("<span class=\"lb-line\"></span>Underskrift")
                .count(),
            2
        );
        assert!(html.contains("Kontrolleret og godkendt af arbejdsgiver"));
    }

    #[test]
    fn trips_during_recorded_private_time_are_private() {
        let at = |s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        let recorded = [crate::state::PrivateInterval {
            id: 1,
            start: at("2026-10-08 10:02"),
            end: at("2026-10-08 11:14"),
            label: "Privat".into(),
            replaces: None,
        }];
        let cal = crate::month::recorded_entries(&recorded, chrono_tz::Europe::Copenhagen);
        let trips = vec![
            trip("2026-10-08 10:05", "Kontoret", "Kunde A", "Service", 10.0),
            trip("2026-10-08 11:14", "Kunde A", "Kontoret", "Service", 10.0),
        ];
        let m = compute(
            &cfg(),
            WORK,
            &Places::default(),
            Month::parse("2026-10").unwrap(),
            &trips,
            &cal,
        )
        .unwrap();
        assert_eq!(m.skipped.len(), 1);
        assert_eq!(m.skipped[0].1, Status::Private);
        assert_eq!(m.skipped[0].0.start, trips[0].start);
        assert_eq!(m.total_km(), 10.0);
    }

    #[test]
    fn threshold_split_across_year() {
        let c = cfg();
        let trips = vec![
            trip("2026-09-01 08:00", "Hjem", "Kunde A", "Montage", 19_990.0),
            trip("2026-10-05 08:00", "Hjem", "Kunde B", "Service", 30.0),
            trip("2026-10-06 08:00", "Hjem", "Kontor", "Arbejde", 25.0),
        ];
        let m = compute(
            &c,
            WORK,
            &Places::default(),
            Month::parse("2026-10").unwrap(),
            &trips,
            &[],
        )
        .unwrap();
        assert_eq!(m.km_before, 19_990.0);
        assert_eq!(m.km_at(3.94), 10.0);
        assert_eq!(m.km_at(2.28), 20.0);
        assert!((m.total_amount() - (39.40 + 45.60)).abs() < 1e-9);
        assert_eq!(m.skipped.len(), 1);
        assert_eq!(m.skipped[0].1, Status::Commute);
    }
}
