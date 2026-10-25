use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Datelike;
use clap::{Parser, Subcommand};

use vw_danlon::calendar::Occurrence;
use vw_danlon::config::Config;
use vw_danlon::mileage::{self, MileageMonth};
use vw_danlon::month::{Month, MonthReport};
use vw_danlon::service::{self, AccountAccess, build_month, load_trips, mileage_for};
use vw_danlon::{checklist, danlon, state, timesheet, trip_store, vacation};

/// Turns myVolkswagen trips into work periods, a working-time record, a
/// mileage logbook and a pay-line file for your own records. It never
/// talks to Danløn.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Config file.
    #[arg(short, long, default_value = "config.toml", global = true)]
    config: PathBuf,
    /// Use the calendar kept from the last fetch instead of fetching it.
    /// Without this, a calendar link that can't be fetched falls back to
    /// that copy with a warning.
    #[arg(long, global = true)]
    offline: bool,
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
    /// What to enter in the Danløn app per day, with the days ticked off
    /// (`checklist mark|unmark <YYYY-MM-DD>`, stored in the database beside the config).
    #[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
    Checklist {
        /// Month as YYYY-MM.
        #[arg(required = true)]
        month: Option<String>,
        /// Trips file (default: the trips stored by `trips import`).
        #[arg(short, long)]
        trips: Option<PathBuf>,
        #[command(subcommand)]
        action: Option<ChecklistAction>,
    },
    /// Set or clear a day's lunch break (stored in the database beside the config).
    Lunch {
        #[command(subcommand)]
        action: LunchAction,
    },
    /// Time private time with a start/stop timer (stored in the database beside the config).
    Private {
        #[command(subcommand)]
        action: PrivateAction,
    },
    /// The Min Volkswagen account for trip sync (device token in the keychain or `token_file`).
    Account {
        #[command(subcommand)]
        action: AccountAction,
    },
    /// Imported and synced trips (stored in the database beside the config).
    Trips {
        #[command(subcommand)]
        action: TripsAction,
    },
    /// Vacation from the calendar: earned, taken, planned and left per open
    /// holiday year, and each period with the year it draws from.
    Vacation,
    /// A backup file of the config, trips and state, to move them to the app
    /// on another device. The calendar link and device token are not in it.
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
}

#[derive(Subcommand)]
enum BackupAction {
    /// Write the backup file.
    Export { file: PathBuf },
}

#[derive(Subcommand)]
enum ChecklistAction {
    /// Tick a day off as entered in the Danløn app.
    Mark {
        /// Day as YYYY-MM-DD.
        date: String,
        /// Trips file (default: the trips stored by `trips import`).
        #[arg(short, long)]
        trips: Option<PathBuf>,
    },
    /// Remove a day's tick.
    Unmark {
        /// Day as YYYY-MM-DD.
        date: String,
    },
}

#[derive(Subcommand)]
enum AccountAction {
    /// Log in once with email and password (asked here, never stored) and
    /// keep a device token in the keychain (or `token_file`).
    Connect,
    /// Delete the stored device token.
    Disconnect,
    /// Show whether an account is connected and when trips were last synced.
    Status,
    /// Fetch the trips of a period without storing them, to check the access.
    Check {
        /// First day as YYYY-MM-DD (default: 30 days ago).
        #[arg(long)]
        from: Option<String>,
        /// Last day as YYYY-MM-DD (default: today).
        #[arg(long)]
        to: Option<String>,
    },
}

#[derive(Subcommand)]
enum TripsAction {
    /// Merge a Min Volkswagen trip export (.xlsx), or a CSV/JSON trips file,
    /// into the stored trips.
    Import { file: PathBuf },
    /// Fetch trips from the connected Min Volkswagen account (unofficial,
    /// read-only) and merge them into the stored trips, month by month.
    Sync {
        /// First day as YYYY-MM-DD (default: the newest stored trip's day,
        /// else 1 January).
        #[arg(long)]
        from: Option<String>,
    },
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
    let mode = if cli.offline {
        service::CalendarMode::Offline
    } else {
        service::CalendarMode::Fresh
    };
    let calendar = |cfg: &Config, month: Month| calendar(cfg, month, mode);

    match cli.command {
        Command::Norm { month } => {
            let month = Month::parse(&month)?;
            let report = service::norm_month(&cfg, month, &calendar(&cfg, month)?);
            print_days(&report, false);
        }
        Command::Report { month, trips } => {
            let month = Month::parse(&month)?;
            let trips = load_trips(&cfg, trips.as_deref(), month)?;
            let cal = calendar(&cfg, month)?;
            let report = build_month(&cfg, month, &trips, &cal)?;
            print_days(&report, true);
            if let Some(m) = mileage_for(&cfg, month, &trips, &cal)? {
                print_mileage(&m);
            }
            let now = chrono::Utc::now()
                .with_timezone(&cfg.timezone)
                .naive_local();
            if let Some(r) = service::forgotten_timer(&cfg, now)? {
                println!(
                    "\ncheck: private timer running since {} ({}); stop it and fix the time with `private edit`",
                    r.start.format("%Y-%m-%d %H:%M"),
                    r.label
                );
            }
        }
        Command::Timesheet { month, trips, out } => {
            let month = Month::parse(&month)?;
            let trips = load_trips(&cfg, trips.as_deref(), month)?;
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
            let trips = load_trips(&cfg, trips.as_deref(), month)?;
            let cal = calendar(&cfg, month)?;
            let m = service::compute_mileage(&cfg, mcfg, month, &trips, &cal)?;
            mileage::write_logbook(mcfg, &m, create(&out)?)?;
            print_mileage(&m);
            println!("Wrote {}", out.display());
        }
        Command::Export { month, trips, out } => {
            let month = Month::parse(&month)?;
            let trips = load_trips(&cfg, trips.as_deref(), month)?;
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
        Command::Checklist {
            month,
            trips,
            action,
        } => checklist(&cfg, mode, month, trips, action)?,
        Command::Lunch { action } => lunch(&cfg, action)?,
        Command::Private { action } => private(&cfg, mode, action)?,
        Command::Account { action } => account(&cfg, action)?,
        Command::Trips { action } => match action {
            TripsAction::Import { file } => import_trips(&cfg, &file)?,
            TripsAction::Sync { from } => sync_trips(&cfg, from.as_deref())?,
        },
        Command::Vacation => print_vacation(&cfg, mode)?,
        Command::Backup {
            action: BackupAction::Export { file },
        } => {
            let backup = vw_danlon::backup::Backup::export(&cli.config, &cfg.database)?;
            std::fs::write(&file, backup.to_json()?)
                .with_context(|| format!("writing {}", file.display()))?;
            println!(
                "Wrote {}: {} trips; the calendar link and device token are not in it",
                file.display(),
                backup.trips.trips.len()
            );
        }
    }
    Ok(())
}

/// The calendar from 1 January to the end of `month`, printing its warnings.
fn calendar(cfg: &Config, month: Month, mode: service::CalendarMode) -> Result<Vec<Occurrence>> {
    let cal = service::calendar(cfg, month, mode)?;
    warn_calendar(
        cfg,
        cal.warning.as_deref(),
        cal.fetched_at,
        cal.fetch_error.as_deref(),
    );
    Ok(cal.occurrences)
}

fn warn_calendar(
    cfg: &Config,
    warning: Option<&str>,
    fetched_at: Option<chrono::DateTime<chrono::Utc>>,
    fetch_error: Option<&str>,
) {
    if let Some(w) = warning {
        eprintln!("warning: {w}");
    }
    let fetched = fetched_at.map(|t| {
        t.with_timezone(&cfg.timezone)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    });
    if let (Some(e), Some(at)) = (fetch_error, &fetched) {
        eprintln!("warning: the calendar can't be fetched ({e}); using the copy fetched {at}");
    }
}

fn import_trips(cfg: &Config, file: &Path) -> Result<()> {
    let (merged, period) = trip_store::import(&cfg.database, file, cfg.timezone)?;
    println!(
        "{} added, {} already imported{}",
        merged.added,
        merged.already,
        merged.zero_km_text()
    );
    if let Some((first, last)) = period {
        println!("Period: {first} – {last}");
    }
    Ok(())
}

fn sync_trips(cfg: &Config, from: Option<&str>) -> Result<()> {
    let today = chrono::Utc::now().with_timezone(&cfg.timezone).date_naive();
    let s = service::sync_trips(cfg, from, today)?;
    println!(
        "{} added, {} already present{}",
        s.synced.merged.added,
        s.synced.merged.already,
        s.synced.merged.zero_km_text()
    );
    if s.synced.without_position > 0 {
        println!(
            "{} trips without position: turn on position sharing in the app",
            s.synced.without_position
        );
    }
    println!("Period: {} – {}", s.from, s.to);
    Ok(())
}

fn account(cfg: &Config, action: AccountAction) -> Result<()> {
    let state = state::State::load(&cfg.database)?;
    let access = AccountAccess::new(cfg, &state);
    let (tokens, email) = (&access.tokens, access.email.clone());
    match action {
        AccountAction::Connect => {
            let email = prompt_email(email.as_deref())?;
            tokens.check(&email)?;
            let password = rpassword::prompt_password("Password (not shown, never stored): ")
                .context("reading the password")?;
            access.connect(cfg, &email, &password)?;
            drop(password);
            println!(
                "Connected {email}; the device token is in {}",
                tokens.describe()
            );
        }
        AccountAction::Disconnect => {
            let Some(email) = email else {
                println!("Not connected");
                return Ok(());
            };
            if access.disconnect(cfg, &email)? {
                println!("Disconnected {email}; the device token is deleted");
            } else {
                println!(
                    "Not connected ({email} has no device token in {})",
                    tokens.describe()
                );
            }
        }
        AccountAction::Status => {
            match (&email, access.connected()?) {
                (Some(e), true) => {
                    println!("Connected: {e} (device token in {})", tokens.describe())
                }
                _ => println!("Not connected: run `account connect`, or use `trips import`"),
            }
            let last = state.account.as_ref().and_then(|a| a.last_sync);
            match last {
                Some(t) => println!(
                    "Last sync: {}",
                    t.with_timezone(&cfg.timezone).format("%Y-%m-%d %H:%M")
                ),
                None => println!("Last sync: never"),
            }
        }
        AccountAction::Check { from, to } => {
            let today = chrono::Utc::now().with_timezone(&cfg.timezone).date_naive();
            let c = service::check_account(cfg, &access, from.as_deref(), to.as_deref(), today)?;
            let (from, to) = (c.from, c.to);
            println!("Vehicle {} (id {})", c.vehicle.plate, c.vehicle.id);
            println!("{} trips {from} – {to} (nothing stored)", c.trips.len());
            let km: f64 = c.trips.iter().filter_map(|t| t.trip.distance_km).sum();
            println!("{km:.1} km");
            let missing = vw_danlon::connected_cars::without_position(&c.trips);
            if missing > 0 {
                println!("{missing} trips without position: turn on position sharing in the app");
            }
            for t in c.trips.iter().rev().take(3) {
                let t = &t.trip;
                println!(
                    "  {} – {}  {:>6} km  {} → {}",
                    t.start.format("%Y-%m-%d %H:%M"),
                    t.end.format("%H:%M"),
                    t.distance_km.map_or("?".into(), |k| format!("{k:.1}")),
                    t.from,
                    t.to
                );
            }
        }
    }
    Ok(())
}

/// Asks for the account email; Enter keeps `default`.
fn prompt_email(default: Option<&str>) -> Result<String> {
    use std::io::Write;
    match default {
        Some(d) => print!("Email [{d}]: "),
        None => print!("Email: "),
    }
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .context("reading the email")?;
    let email = match line.trim() {
        "" => default.unwrap_or_default().to_string(),
        e => e.to_string(),
    };
    if !email.contains('@') {
        anyhow::bail!("an email address is needed");
    }
    Ok(email)
}

fn checklist(
    cfg: &Config,
    mode: service::CalendarMode,
    month: Option<String>,
    trips: Option<PathBuf>,
    action: Option<ChecklistAction>,
) -> Result<()> {
    let path = &cfg.database;
    match action {
        None => {
            let month = Month::parse(month.as_deref().unwrap_or_default())?;
            let days = checklist_days(cfg, mode, month, trips.as_deref())?;
            let entered = state::State::load(path)?.entered;
            print!("{}", checklist::render(&days, &entered));
        }
        Some(ChecklistAction::Mark { date, trips }) => {
            let date = state::parse_date(&date)?;
            let month = Month {
                year: date.year(),
                month: date.month(),
            };
            let days = checklist_days(cfg, mode, month, trips.as_deref())?;
            if service::set_entered(cfg, &days, date, true)? {
                println!("{date}: entered");
            } else {
                println!("{date}: already entered");
            }
        }
        Some(ChecklistAction::Unmark { date }) => {
            let date = state::parse_date(&date)?;
            if service::set_entered(cfg, &[], date, false)? {
                println!("{date}: tick removed");
            } else {
                println!("{date}: not ticked");
            }
        }
    }
    Ok(())
}

/// The month's Danløn app registrations per day, printing the warnings.
fn checklist_days(
    cfg: &Config,
    mode: service::CalendarMode,
    month: Month,
    trips: Option<&Path>,
) -> Result<Vec<checklist::ChecklistDay>> {
    let trips = load_trips(cfg, trips, month)?;
    let cal = calendar(cfg, month, mode)?;
    let report = build_month(cfg, month, &trips, &cal)?;
    let mileage = mileage_for(cfg, month, &trips, &cal)?;
    if let Some(m) = &mileage {
        warn_skipped(m);
    }
    Ok(service::checklist_days(
        cfg,
        &report,
        &trips,
        mileage.as_ref(),
        &cal,
    ))
}

fn lunch(cfg: &Config, action: LunchAction) -> Result<()> {
    let (date, choice) = match action {
        LunchAction::Set {
            date,
            start,
            minutes,
        } => (
            date,
            service::LunchChoice::At {
                start: state::parse_time(&start)?,
                minutes,
            },
        ),
        LunchAction::Clear { date } => (date, service::LunchChoice::Default),
    };
    println!(
        "{}",
        service::set_lunch(cfg, state::parse_date(&date)?, choice)?
    );
    Ok(())
}

fn private(cfg: &Config, mode: service::CalendarMode, action: PrivateAction) -> Result<()> {
    let path = &cfg.database;
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
                    let cal = calendar(cfg, today, mode)?;
                    state::link_event(&service::timed_entries(cfg, &cal), now, &choice)?
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

/// The vacation overview of the open holiday years.
fn print_vacation(cfg: &Config, mode: service::CalendarMode) -> Result<()> {
    let today = chrono::Utc::now().with_timezone(&cfg.timezone).date_naive();
    let v = service::vacation(cfg, today, mode)?;
    warn_calendar(
        cfg,
        v.calendar_warning.as_deref(),
        v.calendar_fetched_at,
        v.calendar_fetch_error.as_deref(),
    );
    print!("{}", vacation::render(&v.rules, &v.overview));
    Ok(())
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
