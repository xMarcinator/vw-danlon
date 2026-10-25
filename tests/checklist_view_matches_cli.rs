//! The app's Danløn screen shows what `checklist <month>` prints: the same
//! registrations, statuses and ticks for the example config, calendar and
//! trips, and ticking from the screen is refused where `checklist mark` is.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chrono::NaiveDate;
use vw_danlon::checklist;
use vw_danlon::config::Config;
use vw_danlon::month::Month;
use vw_danlon::service;

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The example config and calendar in a fresh folder, with 5 October ticked.
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
    std::fs::write(dir.join("state.json"), r#"{ "entered": ["2026-10-05"] }"#).unwrap();
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

fn view(dir: &Path, trips: &Path) -> (Config, service::ChecklistView) {
    let cfg = Config::load(&dir.join("config.toml")).unwrap();
    let month = Month::parse("2026-10").unwrap();
    let trips = service::load_trips(&cfg, Some(trips)).unwrap();
    let cal = service::calendar(&cfg, month).unwrap();
    let view = service::checklist_view(&cfg, month, &trips, &cal.occurrences).unwrap();
    (cfg, view)
}

#[test]
fn checklist_view_matches_the_checklist_command() {
    let dir = example_dir("checklist-view");
    let trips = repo().join("examples/trips.csv");
    let trips_arg = trips.to_str().unwrap();
    let out = cli(&dir, &["checklist", "2026-10", "-t", trips_arg]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = String::from_utf8(out.stdout).unwrap();

    let (cfg, v) = view(&dir, &trips);
    assert_eq!(checklist::render(&v.days, &v.entered), printed);
    // The example month has both kinds of registration and a day to fix.
    assert!(printed.contains("2026-10-05  man  [x] entered"));
    assert!(printed.contains("Kørsel"));
    assert!(printed.contains("fix first: "));

    // Ticking a day from the screen shows up in `checklist`.
    let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
    assert!(service::set_entered(&cfg, &v.days, d("2026-10-06"), true).unwrap());
    assert!(service::set_entered(&cfg, &v.days, d("2026-10-05"), false).unwrap());
    let out = cli(&dir, &["checklist", "2026-10", "-t", trips_arg]);
    let printed = String::from_utf8(out.stdout).unwrap();
    assert!(
        printed.contains("2026-10-06  tir  [x] entered"),
        "{printed}"
    );
    assert!(
        printed.contains("2026-10-05  man  [ ] to enter"),
        "{printed}"
    );
    assert!(printed.ends_with("Days entered: 1 of 3 · Trips entered: 1 of 1 · 1 to fix first\n"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn days_that_cannot_be_ticked_are_refused_like_checklist_mark() {
    let dir = example_dir("checklist-refused");
    let trips = repo().join("examples/trips.csv");
    let (cfg, v) = view(&dir, &trips);
    for (date, reason) in [
        ("2026-10-03", "nothing to enter (weekend)"),
        ("2026-10-08", "fix first: "),
    ] {
        let day = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
        let err = service::set_entered(&cfg, &v.days, day, true).unwrap_err();
        assert!(format!("{err:#}").contains(reason), "{err:#}");
        let out = cli(
            &dir,
            &["checklist", "mark", date, "-t", trips.to_str().unwrap()],
        );
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains(reason));
    }
    let (_, after) = view(&dir, &trips);
    assert_eq!(after.entered, v.entered);
    std::fs::remove_dir_all(&dir).unwrap();
}
