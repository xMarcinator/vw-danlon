//! `home start|stop|list|edit|rm` round trip, the forgotten home-working
//! timer check and the today marker in `report`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The example config and calendar in a fresh folder, with a home-working
/// interval and a home-working timer left running since 1 October.
fn example_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vw-danlon-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("examples")).unwrap();
    std::fs::copy(repo().join("config.example.toml"), dir.join("config.toml")).unwrap();
    std::fs::copy(
        repo().join("examples/arbejdstid.ics"),
        dir.join("examples/arbejdstid.ics"),
    )
    .unwrap();
    std::fs::write(
        dir.join("state.json"),
        r#"{
  "home": {
    "running": { "start": "2026-10-01T08:00:00", "label": "Hjemmearbejde", "replaces": null },
    "intervals": [
      { "id": 1, "start": "2026-10-03T09:00:00", "end": "2026-10-03T11:00:00",
        "label": "Rapport", "replaces": null }
    ],
    "next_id": 2
  }
}"#,
    )
    .unwrap();
    dir
}

fn cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vw-danlon"))
        .arg("-c")
        .arg(dir.join("config.toml"))
        .args(args)
        .output()
        .unwrap()
}

fn ok(out: Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn home_timer_round_trip_and_forgotten_check() {
    let dir = example_dir("home-cli");
    let trips = repo().join("examples/trips.csv");
    let trips = trips.to_str().unwrap();

    // Recorded home working counts on Saturday 3 October; the timer left
    // running is flagged.
    let report = ok(cli(&dir, &["report", "2026-10", "-t", trips]));
    let sat = report
        .lines()
        .find(|l| l.starts_with("2026-10-03"))
        .unwrap();
    assert!(
        sat.contains("09:00–11:00")
            && sat.contains("2.00")
            && sat.contains("Rapport 09:00–11:00 · home"),
        "{sat}"
    );
    assert!(
        report.contains(
            "check: home-working timer running since 2026-10-01 08:00 (Hjemmearbejde); stop it and fix the time with `home edit`"
        ),
        "{report}"
    );

    let err = cli(&dir, &["home", "start"]);
    assert!(!err.status.success());
    assert!(
        String::from_utf8_lossy(&err.stderr).contains("already running since 2026-10-01 08:00")
    );

    assert!(ok(cli(&dir, &["home", "stop"])).starts_with("Recorded   2  2026-10-01  08:00–"));
    let list = ok(cli(&dir, &["home", "list", "2026-10"]));
    assert!(
        list.contains("  1  2026-10-03  09:00–11:00   2.00 h  Rapport"),
        "{list}"
    );
    assert!(list.contains("  2  2026-10-01  08:00–"), "{list}");

    assert_eq!(
        ok(cli(&dir, &["home", "edit", "2", "08:00", "12:30"])),
        "Changed   2  2026-10-01  08:00–12:30   4.50 h  Hjemmearbejde\n"
    );
    let err = cli(&dir, &["home", "rm", "9"]);
    assert!(!err.status.success());
    assert!(String::from_utf8_lossy(&err.stderr).contains("no home-working interval with id 9"));
    assert_eq!(
        ok(cli(&dir, &["home", "rm", "1"])),
        "Deleted   1  2026-10-03  09:00–11:00   2.00 h  Rapport\n"
    );
    assert!(ok(cli(&dir, &["home", "start", "Rapport"])).contains("(Rapport)"));
    let report = ok(cli(&dir, &["report", "2026-10", "-t", trips]));
    assert!(!report.contains("check: home-working timer"), "{report}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn report_marks_today_and_shows_later_days_planned_only() {
    let dir = example_dir("today-cli");
    let trips = repo().join("examples/trips.csv");
    let cfg = vw_danlon::config::Config::load(&dir.join("config.toml")).unwrap();
    let today = chrono::Utc::now().with_timezone(&cfg.timezone).date_naive();
    let month = today.format("%Y-%m").to_string();
    let report = ok(cli(
        &dir,
        &["report", &month, "-t", trips.to_str().unwrap()],
    ));
    let row = |date: chrono::NaiveDate| {
        report
            .lines()
            .find(|l| l.starts_with(&date.to_string()))
            .unwrap()
            .to_string()
    };
    assert!(row(today).contains("◀ today"), "{report}");
    let tomorrow = today.succ_opt().unwrap();
    if tomorrow.format("%Y-%m").to_string() == month {
        let later = row(tomorrow);
        assert!(
            !later.contains("◀ today") && !later.contains('+') && !later.contains("-0"),
            "{later}"
        );
    }
    for total in [
        "Norm hours to date:",
        "Worked hours to date:",
        "Difference to date:",
        "Norm hours in month:",
    ] {
        assert!(report.contains(total), "{report}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
