//! The app's Vacation and Mileage screens show what `vacation` and
//! `logbook` print, for the example config, calendar and trips; the
//! Mileage screen reads the trips imported into the store, as the app does.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use vw_danlon::config::Config;
use vw_danlon::month::Month;
use vw_danlon::{mileage, service, trip_store, vacation};

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The example config and calendar in a fresh folder with no state file.
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
    dir
}

fn cli(dir: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_vw-danlon"))
        .arg("-c")
        .arg(dir.join("config.toml"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

#[test]
fn vacation_view_matches_the_vacation_command() {
    let dir = example_dir("vacation-view");
    let out = cli(&dir, &["vacation"]);
    let cfg = Config::load(&dir.join("config.toml")).unwrap();
    let today = chrono::Utc::now().with_timezone(&cfg.timezone).date_naive();
    let v = service::vacation(&cfg, today, service::CalendarMode::Fresh).unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!(
            "{}\n{}",
            vacation::render_plan(&v.plan, &vacation::status(&v.plan, &v.fully_registered())),
            vacation::render(&v.rules, &v.overview)
        )
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn mileage_from_imported_trips_matches_logbook() {
    let dir = example_dir("mileage-view");
    let trips_csv = repo().join("examples/trips.csv");
    let logbook = dir.join("logbook.csv");
    let month = "2026-10";
    let out = cli(
        &dir,
        &[
            "logbook",
            month,
            "-t",
            trips_csv.to_str().unwrap(),
            "-o",
            logbook.to_str().unwrap(),
        ],
    );
    let printed = String::from_utf8(out.stdout).unwrap();

    // The app imports the file's bytes into the store and reads the store.
    let cfg = Config::load(&dir.join("config.toml")).unwrap();
    let bytes = std::fs::read(&trips_csv).unwrap();
    trip_store::import_bytes(&cfg.database, "trips.csv", &bytes, cfg.timezone).unwrap();
    let month = Month::parse(month).unwrap();
    let trips = service::load_trips(&cfg, None, month).unwrap();
    let cal = service::calendar(&cfg, month, service::CalendarMode::Fresh).unwrap();
    let m = service::mileage_for(&cfg, month, &trips, &cal.occurrences)
        .unwrap()
        .expect("the example config has [mileage]");

    let mut ours = Vec::new();
    mileage::write_logbook(cfg.mileage.as_ref().unwrap(), &m, &mut ours).unwrap();
    assert_eq!(
        String::from_utf8(ours).unwrap(),
        std::fs::read_to_string(&logbook).unwrap()
    );
    assert!(!m.entries.is_empty(), "the example has business trips");
    assert!(
        printed.contains(&format!(
            "Total: {:.1} km, {:.2} kr",
            m.total_km(),
            m.total_amount()
        )),
        "{printed}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
