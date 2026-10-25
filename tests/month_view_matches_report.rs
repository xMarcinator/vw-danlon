//! The app's Month screen shows what `report` prints: the same norm, worked
//! hours and difference per day for the example config, calendar and trips.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{NaiveDate, NaiveDateTime, TimeZone};
use vw_danlon::config::Config;
use vw_danlon::month::Month;
use vw_danlon::service;

/// The example config and calendar in a fresh folder, with a state file
/// holding a lunch override and recorded private time.
fn example_dir() -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("vw-danlon-month-view-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("examples")).unwrap();
    std::fs::copy(repo.join("config.example.toml"), dir.join("config.toml")).unwrap();
    std::fs::copy(
        repo.join("examples/arbejdstid.ics"),
        dir.join("examples/arbejdstid.ics"),
    )
    .unwrap();
    std::fs::write(
        dir.join("state.json"),
        r#"{
  "lunch": { "2026-10-05": { "start": "12:30:00", "minutes": 45 } },
  "private": {
    "intervals": [
      { "id": 1, "start": "2026-10-07T13:00:00", "end": "2026-10-07T13:40:00",
        "label": "Bank", "replaces": null }
    ],
    "next_id": 2
  }
}"#,
    )
    .unwrap();
    dir
}

/// A day's date, norm, worked hours and difference as `report` prints them.
type DayNumbers = (NaiveDate, String, String, String);

/// The numbers of each day row of `report`'s output, and the three totals.
fn report_numbers(out: &str) -> (Vec<DayNumbers>, Vec<String>) {
    let mut days = Vec::new();
    let mut totals = Vec::new();
    for line in out.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if let Some(date) = tokens
            .first()
            .and_then(|t| NaiveDate::parse_from_str(t, "%Y-%m-%d").ok())
        {
            let num = |t: &str| t.parse::<f64>().is_ok() && t.contains('.');
            let n = 2 + tokens[2..].iter().position(|t| num(t)).unwrap();
            // Worked is the number right before the signed difference.
            let w = n
                + 1
                + tokens[n + 1..]
                    .windows(2)
                    .position(|p| num(p[0]) && num(p[1]) && p[1].starts_with(['+', '-']))
                    .unwrap();
            days.push((
                date,
                tokens[n].to_string(),
                tokens[w].to_string(),
                tokens[w + 1].to_string(),
            ));
        } else if let Some(rest) = ["Norm hours:", "Worked hours:", "Difference:"]
            .iter()
            .find_map(|p| line.strip_prefix(p))
        {
            totals.push(rest.trim().to_string());
        }
    }
    (days, totals)
}

#[test]
fn month_view_matches_report_for_the_example_data() {
    let dir = example_dir();
    let trips = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/trips.csv");
    let config = dir.join("config.toml");
    let out = Command::new(env!("CARGO_BIN_EXE_vw-danlon"))
        .arg("-c")
        .arg(&config)
        .args(["report", "2026-10", "-t"])
        .arg(&trips)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (want, want_totals) = report_numbers(&String::from_utf8(out.stdout).unwrap());
    assert_eq!(want.len(), 31);

    let cfg = Config::load(&config).unwrap();
    let month = Month::parse("2026-10").unwrap();
    // Today late in the month: the calendar reaches into November for the
    // coming private appointments, which must not change October.
    let today = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
    let now = cfg
        .timezone
        .from_local_datetime(&NaiveDateTime::new(today, chrono::NaiveTime::MIN))
        .unwrap();
    let trips = service::load_trips(&cfg, Some(&trips)).unwrap();
    let cal = service::month_calendar(&cfg, month, today).unwrap();
    let view = service::month_view(&cfg, month, &trips, &cal.occurrences, now).unwrap();

    let got: Vec<_> = view
        .report
        .days
        .iter()
        .map(|d| {
            (
                d.date,
                format!("{:.2}", d.norm_hours),
                format!("{:.2}", d.worked_hours),
                format!("{:+.2}", d.difference()),
            )
        })
        .collect();
    assert_eq!(got, want);
    let r = &view.report;
    assert_eq!(
        [
            format!("{:.2}", r.norm_total()),
            format!("{:.2}", r.worked_total()),
            format!("{:+.2}", r.worked_total() - r.norm_total()),
        ],
        want_totals.as_slice()
    );
    // The lunch override and the recorded interval reached both.
    let day = |d: u32| &r.days[d as usize - 1];
    assert_eq!(day(5).lunch.source.to_string(), "override");
    assert_eq!(day(7).appointments.len(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}
