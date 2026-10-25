//! Work calendar from an iCalendar source (Proton Calendar share link, or a
//! local .ics export).
//!
//! - All-day events mark days off (ferie, fridage).
//! - Timed events are planned working hours; with `norm_from_calendar` they
//!   set the norm for their day. Their LOCATION is the work address used to
//!   match trips.
//!
//! Recurring events (RRULE, EXDATE) and moved/cancelled single occurrences
//! (RECURRENCE-ID) are handled.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

/// One concrete occurrence of an event.
#[derive(Debug, Clone, PartialEq)]
pub enum Occurrence {
    AllDay {
        date: NaiveDate,
        summary: String,
    },
    Timed {
        start: DateTime<Tz>,
        end: DateTime<Tz>,
        summary: String,
        location: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum When {
    Date(NaiveDate),
    Time(DateTime<Tz>),
}

impl When {
    fn date(self) -> NaiveDate {
        match self {
            When::Date(d) => d,
            When::Time(t) => t.date_naive(),
        }
    }
}

#[derive(Debug, Default)]
struct Event {
    uid: String,
    summary: String,
    location: String,
    start: Option<When>,
    end: Option<When>,
    rrule: Option<String>,
    exdates: Vec<When>,
    recurrence_id: Option<When>,
    cancelled: bool,
}

/// Reads the calendar from a URL (http/https/webcal) or a file path.
pub fn fetch(source: &str) -> Result<String> {
    let url = source
        .strip_prefix("webcal://")
        .map(|rest| format!("https://{rest}"));
    let url = url.as_deref().unwrap_or(source);
    if url.starts_with("https://") || url.starts_with("http://") {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(crate::root_certs())
                    .build(),
            )
            .build()
            .into();
        agent
            .get(url)
            .call()
            .context("downloading calendar")?
            .body_mut()
            .read_to_string()
            .context("reading calendar response")
    } else {
        std::fs::read_to_string(source).with_context(|| format!("reading calendar {source}"))
    }
}

/// All occurrences overlapping `from..=to` (dates in `tz`).
pub fn occurrences(text: &str, tz: Tz, from: NaiveDate, to: NaiveDate) -> Result<Vec<Occurrence>> {
    let events = parse(text, tz)?;

    // Occurrences replaced by a RECURRENCE-ID override, per UID.
    let mut overridden: HashMap<&str, HashSet<NaiveDateTime>> = HashMap::new();
    for e in &events {
        if let Some(rid) = e.recurrence_id {
            overridden.entry(&e.uid).or_default().insert(naive(rid, tz));
        }
    }

    let mut out = Vec::new();
    for e in events.iter().filter(|e| !e.cancelled) {
        let Some(start) = e.start else { continue };
        let length = match (start, e.end) {
            (When::Date(s), Some(When::Date(en))) if en > s => en - s,
            (When::Date(_), _) => Duration::days(1),
            (When::Time(s), Some(When::Time(en))) if en > s => en - s,
            (When::Time(_), _) => Duration::zero(),
        };
        let starts = match (&e.rrule, e.recurrence_id) {
            (Some(rule), None) => expand(rule, start, &e.exdates, tz, from - length, to)?,
            _ => vec![start],
        };
        let skip = overridden.get(e.uid.as_str());
        for s in starts {
            if e.recurrence_id.is_none()
                && e.rrule.is_some()
                && skip.is_some_and(|set| set.contains(&naive(s, tz)))
            {
                continue;
            }
            match s {
                When::Date(first) => {
                    let mut d = first;
                    while d < first + length {
                        if d >= from && d <= to {
                            out.push(Occurrence::AllDay {
                                date: d,
                                summary: e.summary.clone(),
                            });
                        }
                        d += Duration::days(1);
                    }
                }
                When::Time(t) => {
                    let end = t + length;
                    if t.date_naive() <= to && end.date_naive() >= from {
                        out.push(Occurrence::Timed {
                            start: t,
                            end,
                            summary: e.summary.clone(),
                            location: e.location.clone(),
                        });
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The calendar's name (X-WR-CALNAME), if the source states it.
pub fn name(text: &str) -> Option<String> {
    unfold(text).into_iter().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        let key = key.split(';').next()?;
        key.eq_ignore_ascii_case("X-WR-CALNAME")
            .then(|| unescape(value))
    })
}

fn naive(w: When, tz: Tz) -> NaiveDateTime {
    match w {
        When::Date(d) => d.and_hms_opt(0, 0, 0).unwrap(),
        When::Time(t) => t.with_timezone(&tz).naive_local(),
    }
}

/// Expands an RRULE into start times between `from` and `to` (inclusive dates).
fn expand(
    rule: &str,
    start: When,
    exdates: &[When],
    tz: Tz,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<When>> {
    // UNTIL is applied here rather than by the rrule crate, which is strict
    // about UNTIL and DTSTART having matching value types.
    let mut until: Option<NaiveDate> = None;
    let rule_without_until: Vec<&str> = rule
        .split(';')
        .filter(|part| match part.strip_prefix("UNTIL=") {
            Some(v) => {
                until = parse_when(v, None, tz).ok().map(When::date);
                false
            }
            None => true,
        })
        .collect();
    let rule = rule_without_until.join(";");

    let rtz = rrule::Tz::Tz(tz);
    let dt_start = match start {
        When::Date(d) => rtz
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
            .earliest(),
        When::Time(t) => Some(t.with_timezone(&rtz)),
    }
    .ok_or_else(|| anyhow!("bad DTSTART"))?;

    let parsed: rrule::RRule<rrule::Unvalidated> =
        rule.parse().map_err(|e| anyhow!("RRULE {rule:?}: {e}"))?;
    let mut set = parsed
        .build(dt_start)
        .map_err(|e| anyhow!("RRULE {rule:?}: {e}"))?;
    for ex in exdates {
        let ex = match *ex {
            When::Date(d) => rtz
                .from_local_datetime(&d.and_time(dt_start.time()))
                .earliest(),
            When::Time(t) => Some(t.with_timezone(&rtz)),
        };
        if let Some(ex) = ex {
            set = set.exdate(ex);
        }
    }

    let last = until.map_or(to, |u| u.min(to));
    let window_start = rtz
        .from_local_datetime(&from.and_hms_opt(0, 0, 0).unwrap())
        .earliest();
    let window_end = rtz
        .from_local_datetime(&last.and_hms_opt(23, 59, 59).unwrap())
        .earliest();
    if let Some(ws) = window_start {
        set = set.after(ws);
    }
    if let Some(we) = window_end {
        set = set.before(we);
    }

    Ok(set
        .all(u16::MAX)
        .dates
        .into_iter()
        .map(|dt| match start {
            When::Date(_) => When::Date(dt.date_naive()),
            When::Time(_) => When::Time(dt.with_timezone(&tz)),
        })
        .collect())
}

fn parse(text: &str, tz: Tz) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    let mut current: Option<Event> = None;

    for line in unfold(text) {
        let Some((name_params, value)) = line.split_once(':') else {
            continue;
        };
        let mut parts = name_params.split(';');
        let name = parts.next().unwrap_or("").to_ascii_uppercase();
        let tzid = parts
            .find_map(|p| p.strip_prefix("TZID="))
            .map(|t| t.trim_matches('"'));

        if name == "BEGIN" && value.eq_ignore_ascii_case("VEVENT") {
            current = Some(Event::default());
            continue;
        }
        let Some(e) = current.as_mut() else { continue };
        match name.as_str() {
            "END" if value.eq_ignore_ascii_case("VEVENT") => events.extend(current.take()),
            "UID" => e.uid = value.to_string(),
            "SUMMARY" => e.summary = unescape(value),
            "LOCATION" => e.location = unescape(value),
            "DTSTART" => e.start = Some(parse_when(value, tzid, tz)?),
            "DTEND" => e.end = Some(parse_when(value, tzid, tz)?),
            "RRULE" => e.rrule = Some(value.to_string()),
            "EXDATE" => {
                for v in value.split(',') {
                    e.exdates.push(parse_when(v, tzid, tz)?);
                }
            }
            "RECURRENCE-ID" => e.recurrence_id = Some(parse_when(value, tzid, tz)?),
            "STATUS" => e.cancelled = value.eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    Ok(events)
}

fn parse_when(value: &str, tzid: Option<&str>, default_tz: Tz) -> Result<When> {
    let v = value.trim();
    if v.len() == 8 {
        let d = NaiveDate::parse_from_str(v, "%Y%m%d")
            .with_context(|| format!("bad ICS date {v:?}"))?;
        return Ok(When::Date(d));
    }
    if let Some(utc) = v.strip_suffix('Z') {
        let n = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S")
            .with_context(|| format!("bad ICS time {v:?}"))?;
        return Ok(When::Time(
            Utc.from_utc_datetime(&n).with_timezone(&default_tz),
        ));
    }
    let n = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S")
        .with_context(|| format!("bad ICS time {v:?}"))?;
    // Unknown TZIDs (e.g. Windows zone names) fall back to the configured zone.
    let zone: Tz = tzid.and_then(|t| t.parse().ok()).unwrap_or(default_tz);
    let t = zone
        .from_local_datetime(&n)
        .earliest()
        .ok_or_else(|| anyhow!("time {v:?} does not exist in {zone}"))?;
    Ok(When::Time(t.with_timezone(&default_tz)))
}

fn unescape(v: &str) -> String {
    v.replace("\\n", " ")
        .replace("\\N", " ")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

/// Joins continuation lines (RFC 5545 §3.1).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        if let Some(rest) = raw.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        lines.push(raw.to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const CPH: Tz = chrono_tz::Europe::Copenhagen;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn all_day_range_and_folded_summary() {
        let ics = "BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Sommer\r\n ferie\r\nDTSTART;VALUE=DATE:20260706\r\nDTEND;VALUE=DATE:20260709\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";
        let occ = occurrences(ics, CPH, d("2026-07-01"), d("2026-07-31")).unwrap();
        assert_eq!(occ.len(), 3);
        assert_eq!(
            occ[0],
            Occurrence::AllDay {
                date: d("2026-07-06"),
                summary: "Sommerferie".into()
            }
        );
    }

    #[test]
    fn weekly_work_schedule_with_exception_and_override() {
        let ics = "BEGIN:VCALENDAR
BEGIN:VEVENT
UID:work
SUMMARY:Arbejde
DTSTART;TZID=Europe/Copenhagen:20260105T080000
DTEND;TZID=Europe/Copenhagen:20260105T152400
RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;UNTIL=20261231T225959Z
EXDATE;TZID=Europe/Copenhagen:20261007T080000
END:VEVENT
BEGIN:VEVENT
UID:work
RECURRENCE-ID;TZID=Europe/Copenhagen:20261008T080000
SUMMARY:Arbejde
DTSTART;TZID=Europe/Copenhagen:20261008T090000
DTEND;TZID=Europe/Copenhagen:20261008T130000
END:VEVENT
END:VCALENDAR
";
        let occ = occurrences(ics, CPH, d("2026-10-05"), d("2026-10-09")).unwrap();
        let mut days: Vec<_> = occ
            .iter()
            .map(|o| match o {
                Occurrence::Timed { start, end, .. } => {
                    (start.date_naive(), (*end - *start).num_minutes())
                }
                _ => panic!("expected timed"),
            })
            .collect();
        days.sort();
        assert_eq!(
            days,
            [
                (d("2026-10-05"), 444),
                (d("2026-10-06"), 444),
                (d("2026-10-08"), 240), // moved and shortened
                (d("2026-10-09"), 444),
            ]
        );
    }

    #[test]
    fn utc_times_converted() {
        let ics =
            "BEGIN:VEVENT\nUID:x\nDTSTART:20261030T070000Z\nDTEND:20261030T150000Z\nEND:VEVENT\n";
        let occ = occurrences(ics, CPH, d("2026-10-01"), d("2026-10-31")).unwrap();
        let Occurrence::Timed { start, .. } = &occ[0] else {
            panic!()
        };
        assert_eq!(start.format("%H:%M").to_string(), "08:00");
    }
}
