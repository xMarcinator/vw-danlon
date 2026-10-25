mod address;
mod calendar;
mod config;
mod danlon;
mod holidays;
mod mileage;
mod month;
mod state;
mod timesheet;
mod trip_store;
mod trips;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};
use clap::{Parser, Subcommand};

use calendar::Occurrence;
use config::{Config, MileageConfig};
use mileage::MileageMonth;
use month::{Month, MonthReport};
use trips::Trip;

/// Turns myVolkswagen trips into work periods, a working-time record, a
/// mileage logbook and a pay-line file for your own records. It never
/// talks to Danløn.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Config file.
    #[arg(short, long, default_value = "config.toml", global = true)]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the work calendar and norm hours for a month.
    Norm {
        /// Month as YYYY-MM.
        month: String,
    },
    /// Show work periods, norm and difference per day, and mileage.
    Report {
        month: String,
        /// Trips file (.csv, .json or a Min Volkswagen .xlsx export), may
        /// contain the whole year. Default: the trips stored by `trips import`.
        #[arg(short, long)]
        trips: Option<PathBuf>,
    },
    /// Write the working-time record (arbejdstidsregistrering) for a month.
    Timesheet {
        month: String,
        /// Trips file (default: the trips stored by `trips import`).
        #[arg(short, long)]
        trips: Option<PathBuf>,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Write the mileage logbook (kørselsregnskab) for a month.
    Logbook {
        month: String,
        /// Trips file (default: the trips stored by `trips import`).
        #[arg(short, long)]
        trips: Option<PathBuf>,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Write the month's pay lines (lønart, quantity, rate) to a file.
    Export {
        month: String,
        /// Trips file (default: the trips stored by `trips import`).
        #[arg(short, long)]
        trips: Option<PathBuf>,
        /// File to write.
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Set or clear a day's lunch break (stored in state.json beside the config).
    Lunch {
        #[command(subcommand)]
        action: LunchAction,
    },
    /// Time private time with a start/stop timer (stored in state.json).
    Private {
        #[command(subcommand)]
        action: PrivateAction,
    },
    /// Imported trips (stored in trips.json beside the config).
    Trips {
        #[command(subcommand)]
        action: TripsAction,
    },
}

#[derive(Subcommand)]
enum TripsAction {
    /// Merge a Min Volkswagen trip export (.xlsx) into the stored trips.
    Import { file: PathBuf },
}

#[derive(Subcommand)]
enum PrivateAction {
    /// Start the timer; it links to the current or next private calendar
    /// appointment today and takes its title as the label.
    Start {
        /// Label (default: the linked appointment's title, else "Privat").
        label: Option<String>,
        /// Link the private appointment today with this title.
        #[arg(long, conflicts_with = "no_event")]
        event: Option<String>,
        /// Do not link a calendar appointment.
        #[arg(long)]
        no_event: bool,
    },
    /// Stop the timer and record the interval.
    Stop,
    /// Show the recorded intervals of a month.
    List {
        /// Month as YYYY-MM.
        month: String,
    },
    /// Change a recorded interval's start and end.
    Edit {
        id: u32,
        /// Start as HH:MM.
        start: String,
        /// End as HH:MM.
        end: String,
    },
    /// Delete a recorded interval.
    Rm { id: u32 },
}

#[derive(Subcommand)]
enum LunchAction {
    /// Use this lunch break on a day instead of the default or the calendar.
    Set {
        /// Day as YYYY-MM-DD.
        date: String,
        /// Lunch start as HH:MM.
        start: String,
        /// Lunch length in minutes; 0 means no lunch that day.
        minutes: u32,
    },
    /// Remove a day's lunch override.
    Clear {
        /// Day as YYYY-MM-DD.
        date: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;

    match cli.command {
        Command::Norm { month } => {
            let month = Month::parse(&month)?;
            let report = month::build(
                &cfg,
                month,
                &[],
                &calendar(&cfg, month)?,
                &BTreeMap::new(),
                &[],
            );
            print_days(&report, false);
        }
        Command::Report { month, trips } => {
            let month = Month::parse(&month)?;
            let trips = load_trips(&cfg, trips.as_deref())?;
            let cal = calendar(&cfg, month)?;
            let report = build_month(&cfg, month, &trips, &cal)?;
            print_days(&report, true);
            if let Some(m) = mileage_for(&cfg, month, &trips, &cal)? {
                print_mileage(&m);
            }
            let now = chrono::Utc::now()
                .with_timezone(&cfg.timezone)
                .naive_local();
            if let Some(r) = state::State::load(&cfg.state_file)?.private.running
                && r.forgotten(now)
            {
                println!(
                    "\ncheck: private timer running since {} ({}); stop it and fix the time with `private edit`",
                    r.start.format("%Y-%m-%d %H:%M"),
                    r.label
                );
            }
        }
        Command::Timesheet { month, trips, out } => {
            let month = Month::parse(&month)?;
            let trips = load_trips(&cfg, trips.as_deref())?;
            let report = build_month(&cfg, month, &trips, &calendar(&cfg, month)?)?;
            timesheet::write(&cfg, &report, create(&out)?)?;
            println!("Wrote {}", out.display());
        }
        Command::Logbook { month, trips, out } => {
            let mcfg = cfg
                .mileage
                .as_ref()
                .context("no [mileage] section in config")?;
            let month = Month::parse(&month)?;
            let trips = load_trips(&cfg, trips.as_deref())?;
            let cal = calendar(&cfg, month)?;
            let m = compute_mileage(&cfg, mcfg, month, &trips, &cal)?;
            mileage::write_logbook(mcfg, &m, create(&out)?)?;
            print_mileage(&m);
            println!("Wrote {}", out.display());
        }
        Command::Export { month, trips, out } => {
            let month = Month::parse(&month)?;
            let trips = load_trips(&cfg, trips.as_deref())?;
            let cal = calendar(&cfg, month)?;
            let report = build_month(&cfg, month, &trips, &cal)?;
            let mil = mileage_for(&cfg, month, &trips, &cal)?;
            if let Some(m) = &mil {
                warn_skipped(m);
            }
            let lines = danlon::lines(&cfg, &report, mil.as_ref());
            for l in &lines {
                let rate = l.rate.map(|r| format!("à {r:.2}")).unwrap_or_default();
                println!(
                    "{:<18} lønart {:<6} {:>9.2} {}",
                    l.text, l.lonart, l.quantity, rate
                );
            }
            if lines.is_empty() {
                println!("Nothing to export for {}", report.month);
            }
            danlon::write(&cfg, &report, &lines, create(&out)?)?;
            println!("Wrote {}", out.display());
        }
        Command::Lunch { action } => lunch(&cfg, action)?,
        Command::Private { action } => private(&cfg, action)?,
        Command::Trips {
            action: TripsAction::Import { file },
        } => import_trips(&cfg, &file)?,
    }
    Ok(())
}

/// Trips from `-t`, else from the trip store.
fn load_trips(cfg: &Config, file: Option<&Path>) -> Result<Vec<Trip>> {
    if let Some(file) = file {
        return trips::load(file, cfg.timezone);
    }
    trip_store::stored_trips(&cfg.trip_store, cfg.timezone)
}

fn import_trips(cfg: &Config, file: &Path) -> Result<()> {
    let (merged, period) = trip_store::import(&cfg.trip_store, file, cfg.timezone)?;
    println!(
        "{} added, {} already imported",
        merged.added, merged.already
    );
    if let Some((first, last)) = period {
        println!("Period: {first} – {last}");
    }
    Ok(())
}

fn lunch(cfg: &Config, action: LunchAction) -> Result<()> {
    let path = &cfg.state_file;
    match action {
        LunchAction::Set {
            date,
            start,
            minutes,
        } => {
            let date = state::parse_date(&date)?;
            let start = state::parse_time(&start)?;
            state::State::update(path, |s| {
                s.lunch
                    .insert(date, state::LunchOverride { start, minutes })
            })?;
            if minutes == 0 {
                println!("{date}: no lunch");
            } else {
                println!("{date}: lunch {} for {minutes} min", start.format("%H:%M"));
            }
        }
        LunchAction::Clear { date } => {
            let date = state::parse_date(&date)?;
            if state::State::update(path, |s| s.lunch.remove(&date))?.is_some() {
                println!("{date}: lunch override cleared");
            } else {
                println!("{date}: no lunch override");
            }
        }
    }
    Ok(())
}

fn private(cfg: &Config, action: PrivateAction) -> Result<()> {
    let path = &cfg.state_file;
    let now = chrono::Utc::now().with_timezone(&cfg.timezone);
    match action {
        PrivateAction::Start {
            label,
            event,
            no_event,
        } => {
            let choice = match (event, no_event) {
                (_, true) => state::EventChoice::None,
                (Some(title), _) => state::EventChoice::Title(title),
                (None, false) => state::EventChoice::Auto,
            };
            let window = match choice {
                state::EventChoice::None => None,
                _ => {
                    let today = Month {
                        year: now.year(),
                        month: now.month(),
                    };
                    let cal = calendar(cfg, today)?;
                    state::link_event(&timed_entries(cfg, &cal), now, &choice)?
                }
            };
            let r = state::State::update(path, |s| {
                s.private.start(now.naive_local(), label, window).cloned()
            })??;
            print!("Timer started {} ({})", r.start.format("%H:%M"), r.label);
            match &r.replaces {
                Some(w) => println!(
                    ", replacing {} {}–{}",
                    w.title,
                    w.start.format("%H:%M"),
                    w.end.format("%H:%M")
                ),
                None => println!(),
            }
        }
        PrivateAction::Stop => {
            let i = state::State::update(path, |s| s.private.stop(now.naive_local()).cloned())??;
            println!("Recorded {}", interval_text(&i));
        }
        PrivateAction::List { month } => {
            let month = Month::parse(&month)?;
            let p = state::State::load(path)?.private;
            let mut any = false;
            for i in p
                .intervals
                .iter()
                .filter(|i| month.contains(i.start.date()))
            {
                println!("{}", interval_text(i));
                any = true;
            }
            if let Some(r) = p.running.filter(|r| month.contains(r.start.date())) {
                println!(
                    "running  since {}  {}",
                    r.start.format("%Y-%m-%d %H:%M"),
                    r.label
                );
                any = true;
            }
            if !any {
                println!("No private time recorded in {month}");
            }
        }
        PrivateAction::Edit { id, start, end } => {
            let start = state::parse_time(&start)?;
            let end = state::parse_time(&end)?;
            let i = state::State::update(path, |s| s.private.edit(id, start, end).cloned())??;
            println!("Changed {}", interval_text(&i));
        }
        PrivateAction::Rm { id } => {
            let i = state::State::update(path, |s| s.private.remove(id))??;
            println!("Deleted {}", interval_text(&i));
        }
    }
    Ok(())
}

/// One recorded interval as a line: id, day, times, hours, label and the
/// appointment it replaces.
fn interval_text(i: &state::PrivateInterval) -> String {
    let end = if i.end.date() == i.start.date() {
        i.end.format("%H:%M").to_string()
    } else {
        i.end.format("%Y-%m-%d %H:%M").to_string()
    };
    let hours = (i.end - i.start).num_minutes() as f64 / 60.0;
    let replaces = i
        .replaces
        .as_ref()
        .map(|w| {
            format!(
                "  (replaces {} {}–{})",
                w.title,
                w.start.format("%H:%M"),
                w.end.format("%H:%M")
            )
        })
        .unwrap_or_default();
    format!(
        "{:>3}  {}  {}–{}  {:>5.2} h  {}{}",
        i.id,
        i.start.format("%Y-%m-%d"),
        i.start.format("%H:%M"),
        end,
        hours,
        i.label,
        replaces
    )
}

/// Calendar occurrences from 1 January to the end of `month`; the earlier
/// months are needed for the year's mileage total.
fn calendar(cfg: &Config, month: Month) -> Result<Vec<Occurrence>> {
    let year_start = NaiveDate::from_ymd_opt(month.year, 1, 1).unwrap();
    Ok(match &cfg.calendar {
        Some(cal) => {
            let text = calendar::fetch(&cal.source)?;
            if let Some(expected) = &cal.name {
                match calendar::name(&text) {
                    Some(actual) if actual.trim().eq_ignore_ascii_case(expected.trim()) => {}
                    Some(actual) => anyhow::bail!(
                        "calendar is {actual:?}, expected {expected:?} — check calendar.source"
                    ),
                    None => {
                        eprintln!("warning: calendar has no name; cannot check it is {expected:?}")
                    }
                }
            }
            calendar::occurrences(&text, cfg.timezone, year_start, month.last_day())?
        }
        None => Vec::new(),
    })
}

fn mileage_for(
    cfg: &Config,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
) -> Result<Option<MileageMonth>> {
    cfg.mileage
        .as_ref()
        .map(|m| compute_mileage(cfg, m, month, trips, cal))
        .transpose()
}

/// The month's mileage; trips starting during recorded private time are
/// private, like those during a private appointment.
fn compute_mileage(
    cfg: &Config,
    mcfg: &MileageConfig,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
) -> Result<MileageMonth> {
    let recorded = state::State::load(&cfg.state_file)?.private.intervals;
    let mut entries = timed_entries(cfg, cal);
    entries.extend(month::recorded_entries(&recorded, cfg.timezone));
    mileage::compute(
        mcfg,
        cfg.workplace.as_deref(),
        &cfg.known_places,
        month,
        trips,
        &entries,
    )
}

/// The month's days, with the lunch overrides and recorded private time
/// from the state file.
fn build_month(
    cfg: &Config,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
) -> Result<MonthReport> {
    let state = state::State::load(&cfg.state_file)?;
    Ok(month::build(
        cfg,
        month,
        trips,
        cal,
        &state.lunch,
        &state.private.intervals,
    ))
}

/// The calendar's timed entries, classified with the configured private
/// words and lunch title.
fn timed_entries<'a>(cfg: &Config, cal: &'a [Occurrence]) -> Vec<month::TimedEntry<'a>> {
    let private_words = cfg
        .calendar
        .as_ref()
        .map(|c| c.private_keywords.as_slice())
        .unwrap_or_default();
    month::timed_entries(cal, private_words, &cfg.lunch.title)
}

fn create(path: &Path) -> Result<BufWriter<File>> {
    let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    Ok(BufWriter::new(file))
}

fn print_days(r: &MonthReport, with_trips: bool) {
    if with_trips {
        println!(
            "{:<10}  {:<3}  {:<11}  {:>5}  {:<24}  {:<23}  {:>6}  {:>6}  note",
            "date", "day", "planned", "norm", "at work address", "lunch", "worked", "diff"
        );
    } else {
        println!(
            "{:<10}  {:<3}  {:<11}  {:>5}  {:<30}  note",
            "date", "day", "planned", "norm", "work address"
        );
    }
    for d in &r.days {
        let day = timesheet::weekday_da(d.date.weekday());
        let planned = d
            .planned
            .map(|(s, e)| format!("{}–{}", s.format("%H:%M"), e.format("%H:%M")))
            .unwrap_or_default();
        if with_trips {
            let lunch = if d.first_start.is_some() {
                d.lunch_text()
            } else {
                String::new()
            };
            println!(
                "{}  {}  {:<11}  {:>5.2}  {:<24}  {:<23}  {:>6.2}  {:>+6.2}  {}",
                d.date,
                day,
                planned,
                d.norm_hours,
                d.periods_text(),
                lunch,
                d.worked_hours,
                d.difference(),
                d.remark()
            );
        } else {
            let location = if d.off_reason.is_some() {
                String::new()
            } else {
                d.work_locations.join(" | ")
            };
            println!(
                "{}  {}  {:<11}  {:>5.2}  {:<30}  {}",
                d.date,
                day,
                planned,
                d.norm_hours,
                location,
                d.remark()
            );
        }
    }
    println!();
    println!("Norm hours:     {:>8.2}", r.norm_total());
    if with_trips {
        println!("Worked hours:   {:>8.2}", r.worked_total());
        println!(
            "Difference:     {:>+8.2}",
            r.worked_total() - r.norm_total()
        );
    }
}

fn print_mileage(m: &MileageMonth) {
    println!();
    println!(
        "Mileage allowance {} (km earlier this year: {:.1})",
        m.month, m.km_before
    );
    for e in &m.entries {
        println!(
            "  {}  {} → {}  {:<24} {:>7.1} km à {:.2} = {:>8.2}",
            e.start.format("%d-%m %H:%M"),
            e.from,
            e.to,
            e.purpose,
            e.km,
            e.rate,
            e.amount()
        );
    }
    println!(
        "  Total: {:.1} km, {:.2} kr",
        m.total_km(),
        m.total_amount()
    );
    warn_skipped(m);
}

fn warn_skipped(m: &MileageMonth) {
    for (t, status) in &m.skipped {
        if let mileage::Status::Incomplete(why) = status {
            eprintln!(
                "warning: {} {} → {} not reimbursed: {}",
                t.start.format("%d-%m %H:%M"),
                t.from,
                t.to,
                why
            );
        }
    }
}
