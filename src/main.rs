mod address;
mod calendar;
mod config;
mod danlon;
mod danlon_api;
mod holidays;
mod mileage;
mod month;
mod state;
mod timesheet;
mod trips;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{Datelike, NaiveDate};
use clap::{Parser, Subcommand};

use calendar::Occurrence;
use config::Config;
use mileage::MileageMonth;
use month::{Month, MonthReport};
use trips::Trip;

/// Turns myVolkswagen trips into work periods, a working-time record, a
/// mileage logbook and a Danløn import file.
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
        /// Trips file (.csv or .json), may contain the whole year.
        #[arg(short, long)]
        trips: PathBuf,
    },
    /// Write the working-time record (arbejdstidsregistrering) for a month.
    Timesheet {
        month: String,
        #[arg(short, long)]
        trips: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Write the mileage logbook (kørselsregnskab) for a month.
    Logbook {
        month: String,
        #[arg(short, long)]
        trips: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Write the Danløn import file for a month, or send it with --send.
    Export {
        month: String,
        #[arg(short, long)]
        trips: PathBuf,
        /// Import file to write.
        #[arg(short, long, required_unless_present = "send")]
        out: Option<PathBuf>,
        /// Send the pay lines to Danløn through its API.
        #[arg(long)]
        send: bool,
        /// Send without asking (refuses if lines with the same code are
        /// already waiting in Danløn).
        #[arg(long, requires = "send")]
        yes: bool,
    },
    /// Danløn API: show ids and pay codes, or delete pay parts.
    Danlon {
        #[command(subcommand)]
        action: DanlonAction,
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

#[derive(Subcommand)]
enum DanlonAction {
    /// Show the company, employees, pay codes and your waiting pay parts.
    Info,
    /// Delete pay parts that have not been through payroll yet.
    Delete {
        /// Pay part ids, as shown by `export --send` or `danlon info`.
        #[arg(required = true)]
        ids: Vec<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;

    match cli.command {
        Command::Norm { month } => {
            let month = Month::parse(&month)?;
            let report = month::build(&cfg, month, &[], &calendar(&cfg, month)?, &BTreeMap::new());
            print_days(&report, false);
        }
        Command::Report { month, trips } => {
            let month = Month::parse(&month)?;
            let trips = trips::load(&trips, cfg.timezone)?;
            let cal = calendar(&cfg, month)?;
            let report = build_month(&cfg, month, &trips, &cal)?;
            print_days(&report, true);
            if let Some(m) = mileage_for(&cfg, month, &trips, &cal)? {
                print_mileage(&m);
            }
        }
        Command::Timesheet { month, trips, out } => {
            let month = Month::parse(&month)?;
            let trips = trips::load(&trips, cfg.timezone)?;
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
            let trips = trips::load(&trips, cfg.timezone)?;
            let cal = calendar(&cfg, month)?;
            let entries = timed_entries(&cfg, &cal);
            let m = mileage::compute(mcfg, cfg.workplace.as_deref(), month, &trips, &entries)?;
            mileage::write_logbook(mcfg, &m, create(&out)?)?;
            print_mileage(&m);
            println!("Wrote {}", out.display());
        }
        Command::Export {
            month,
            trips,
            out,
            send,
            yes,
        } => {
            let month = Month::parse(&month)?;
            let trips = trips::load(&trips, cfg.timezone)?;
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
            if let Some(out) = out {
                danlon::write(&cfg, &report, &lines, create(&out)?)?;
                println!("Wrote {}", out.display());
            }
            if send && !lines.is_empty() {
                send_to_danlon(&cfg, &lines, yes)?;
            }
        }
        Command::Danlon { action } => {
            let api = danlon_api::Client::connect(&cfg.danlon.api)?;
            let company = company_id(&cfg, &api)?;
            match action {
                DanlonAction::Info => danlon_info(&cfg, &api, &company)?,
                DanlonAction::Delete { ids } => {
                    api.delete(&company, &ids)?;
                    println!("Deleted {} pay part(s)", ids.len());
                }
            }
        }
        Command::Lunch { action } => lunch(&cfg, action)?,
        Command::Private { action } => private(&cfg, action)?,
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

fn company_id(cfg: &Config, api: &danlon_api::Client) -> Result<String> {
    match &cfg.danlon.api.company_id {
        Some(id) => Ok(id.clone()),
        None => api.current_company(),
    }
}

fn danlon_info(cfg: &Config, api: &danlon_api::Client, company: &str) -> Result<()> {
    use danlon_api::show;
    println!("Company id: {company}");
    println!();
    println!("Employees (number, id, name):");
    let employees = api.employees(company)?;
    for e in &employees {
        let inactive = if e.active { "" } else { " (inactive)" };
        println!("  {:<6} {:<16} {}{}", e.domain_id, e.id, e.name, inactive);
    }
    println!();
    println!("Pay codes (units / rate / amount allowed):");
    for c in api.pay_codes()? {
        let yn = |b: bool| if b { "ja " } else { "nej" };
        println!(
            "  {:<6} {} {} {}  {}",
            c.code,
            yn(c.units_allowed),
            yn(c.rate_allowed),
            yn(c.amount_allowed),
            c.description
        );
    }
    let employee = danlon_api::find_employee(
        &employees,
        cfg.danlon.api.employee_id.as_deref(),
        &cfg.employee_number,
    )?;
    println!();
    println!(
        "Waiting pay parts for {} ({}):",
        employee.name, employee.domain_id
    );
    let parts = api.pay_parts(company, &employee.id)?;
    for p in &parts {
        println!(
            "  {:<16} {:<6} units {:>9}  rate {:>9}  amount {:>10}",
            p.id,
            p.code,
            show(p.units),
            show(p.rate),
            show(p.amount)
        );
    }
    if parts.is_empty() {
        println!("  none");
    }
    Ok(())
}

/// Sends the export lines as pay parts after showing them and asking.
fn send_to_danlon(cfg: &Config, lines: &[danlon::Line], yes: bool) -> Result<()> {
    use danlon_api::show;
    let api = danlon_api::Client::connect(&cfg.danlon.api)?;
    let company = company_id(cfg, &api)?;
    let employees = api.employees(&company)?;
    let employee = danlon_api::find_employee(
        &employees,
        cfg.danlon.api.employee_id.as_deref(),
        &cfg.employee_number,
    )?;
    let codes = api.pay_codes()?;
    let parts = danlon_api::to_pay_parts(lines, &codes)?;

    println!();
    println!(
        "To send to Danløn for {} ({}):",
        employee.name, employee.domain_id
    );
    for p in &parts {
        let description = codes
            .iter()
            .find(|c| c.code == p.code)
            .map(|c| c.description.as_str())
            .unwrap_or_default();
        println!(
            "  {:<6} {:<28} units {:>9}  rate {:>9}  amount {:>10}",
            p.code,
            description,
            show(p.units),
            show(p.rate),
            show(p.amount)
        );
    }
    let waiting: Vec<_> = api
        .pay_parts(&company, &employee.id)?
        .into_iter()
        .filter(|w| parts.iter().any(|p| p.code == w.code))
        .collect();
    if !waiting.is_empty() {
        eprintln!();
        eprintln!("warning: Danløn already has waiting pay parts with these codes:");
        for w in &waiting {
            eprintln!("  {}  {}  units {}", w.id, w.code, show(w.units));
        }
        if yes {
            bail!("not sending twice; check in Danløn or run `danlon delete` first");
        }
    }
    if !yes {
        print!("Send to Danløn? [y/N] ");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().lock().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "j" | "J") {
            println!("Not sent.");
            return Ok(());
        }
    }
    let created = api.create(&company, &employee.id, &parts)?;
    println!("Sent. Created pay parts:");
    for p in &created {
        println!("  {}  {}  units {}", p.id, p.code, show(p.units));
    }
    println!("Undo with: danlon delete <id>…  (before running payroll)");
    Ok(())
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
        .map(|m| {
            let entries = timed_entries(cfg, cal);
            mileage::compute(m, cfg.workplace.as_deref(), month, trips, &entries)
        })
        .transpose()
}

/// The month's days, with the lunch overrides from the state file.
fn build_month(
    cfg: &Config,
    month: Month,
    trips: &[Trip],
    cal: &[Occurrence],
) -> Result<MonthReport> {
    let overrides = state::State::load(&cfg.state_file)?.lunch;
    Ok(month::build(cfg, month, trips, cal, &overrides))
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
