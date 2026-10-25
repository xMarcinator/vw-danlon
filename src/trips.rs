//! Trips driven, read from CSV or JSON.
//!
//! Fields: `start`, `end` (required), `distance_km`, `from`, `to`, `purpose`,
//! `business` (optional). Timestamps are RFC 3339
//! (`2026-10-05T07:42:00+02:00`) or local time without offset
//! (`2026-10-05 07:42`), interpreted in the configured zone.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Trip {
    pub start: DateTime<Tz>,
    pub end: DateTime<Tz>,
    pub distance_km: Option<f64>,
    pub from: String,
    pub to: String,
    /// Business purpose (formål). Required for mileage allowance.
    pub purpose: String,
    /// Explicit business/private flag; when absent, a trip with a purpose
    /// counts as business.
    pub business: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawTrip {
    start: String,
    end: String,
    #[serde(default)]
    distance_km: Option<f64>,
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    purpose: Option<String>,
    #[serde(default)]
    business: Option<bool>,
}

pub fn load(path: &Path, tz: Tz) -> Result<Vec<Trip>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading trips {}", path.display()))?;
    let raw: Vec<RawTrip> = match path.extension().and_then(|e| e.to_str()) {
        Some("json") => serde_json::from_str(&text).context("parsing trips JSON")?,
        _ => csv::Reader::from_reader(text.as_bytes())
            .deserialize()
            .collect::<Result<_, _>>()
            .context("parsing trips CSV")?,
    };
    let mut trips = raw
        .into_iter()
        .map(|r| {
            let start = parse_time(&r.start, tz)?;
            let end = parse_time(&r.end, tz)?;
            if end < start {
                return Err(anyhow!(
                    "trip ends before it starts: {} → {}",
                    r.start,
                    r.end
                ));
            }
            let text = |s: Option<String>| s.map(|s| s.trim().to_string()).unwrap_or_default();
            Ok(Trip {
                start,
                end,
                distance_km: r.distance_km,
                from: text(r.from),
                to: text(r.to),
                purpose: text(r.purpose),
                business: r.business,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    trips.sort_by_key(|t| t.start);
    Ok(trips)
}

pub fn parse_time(s: &str, tz: Tz) -> Result<DateTime<Tz>> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&tz));
    }
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            return tz
                .from_local_datetime(&naive)
                .earliest()
                .ok_or_else(|| anyhow!("time {s:?} does not exist in {tz}"));
        }
    }
    Err(anyhow!("unrecognised time {s:?}"))
}
