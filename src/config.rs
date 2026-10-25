use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{NaiveTime, Weekday};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Your employee number in Danløn (must match exactly).
    pub employee_number: String,
    #[serde(default = "default_timezone")]
    pub timezone: chrono_tz::Tz,
    #[serde(default)]
    pub work_week: WorkWeek,
    #[serde(default)]
    pub holidays: HolidayConfig,
    pub calendar: Option<CalendarConfig>,
    /// Your fixed workplace address. Arriving here always counts as being at
    /// work (e.g. when a colleague drives you on to the day's address), and
    /// home ↔ workplace trips are commuting, never mileage allowance.
    pub workplace: Option<String>,
    #[serde(default)]
    pub periods: PeriodConfig,
    #[serde(default)]
    pub lunch: LunchConfig,
    pub mileage: Option<MileageConfig>,
    pub danlon: DanlonConfig,
    /// The local state file (`state.json` beside the config file).
    #[serde(skip)]
    pub state_file: PathBuf,
}

fn default_timezone() -> chrono_tz::Tz {
    chrono_tz::Europe::Copenhagen
}

/// Norm hours per weekday, used when the calendar does not set the norm.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkWeek {
    pub mon: f64,
    pub tue: f64,
    pub wed: f64,
    pub thu: f64,
    pub fri: f64,
    #[serde(default)]
    pub sat: f64,
    #[serde(default)]
    pub sun: f64,
}

impl Default for WorkWeek {
    /// 37 hours, Monday to Friday.
    fn default() -> Self {
        WorkWeek {
            mon: 7.4,
            tue: 7.4,
            wed: 7.4,
            thu: 7.4,
            fri: 7.4,
            sat: 0.0,
            sun: 0.0,
        }
    }
}

impl WorkWeek {
    pub fn hours(&self, day: Weekday) -> f64 {
        match day {
            Weekday::Mon => self.mon,
            Weekday::Tue => self.tue,
            Weekday::Wed => self.wed,
            Weekday::Thu => self.thu,
            Weekday::Fri => self.fri,
            Weekday::Sat => self.sat,
            Weekday::Sun => self.sun,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolidayConfig {
    #[serde(default = "yes")]
    pub danish_public: bool,
    #[serde(default = "yes")]
    pub grundlovsdag: bool,
    #[serde(default = "yes")]
    pub christmas_eve: bool,
    #[serde(default = "yes")]
    pub new_years_eve: bool,
}

fn yes() -> bool {
    true
}

fn default_private_keywords() -> Vec<String> {
    ["Læge", "Tandlæge", "Fysioterapeut"]
        .map(String::from)
        .to_vec()
}

impl Default for HolidayConfig {
    fn default() -> Self {
        HolidayConfig {
            danish_public: true,
            grundlovsdag: true,
            christmas_eve: true,
            new_years_eve: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarConfig {
    /// Proton Calendar share link (https://… or webcal://…) or a local .ics file.
    pub source: String,
    /// Timed events are planned working hours and set the day's norm. When
    /// false, the norm comes from `[work_week]` and timed events are ignored.
    #[serde(default = "yes")]
    pub norm_from_calendar: bool,
    /// If non-empty, only all-day events whose title contains one of these
    /// words (case-insensitive) count as days off.
    #[serde(default)]
    pub days_off_keywords: Vec<String>,
    /// Timed events whose title contains one of these words (whole word,
    /// case-insensitive) are private appointments: not company time, no work
    /// address and no planned hours. An empty list disables this.
    #[serde(default = "default_private_keywords")]
    pub private_keywords: Vec<String>,
    /// Expected calendar name (X-WR-CALNAME). Guards against pointing the
    /// share link at the wrong calendar.
    pub name: Option<String>,
    /// Work address for days whose calendar entry has none (or no entry).
    pub default_location: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodConfig {
    /// Round each day's worked time to the nearest N minutes (0 = no rounding).
    #[serde(default)]
    pub round_to_minutes: u32,
    /// Count the drive to and from the work address as working time.
    #[serde(default)]
    pub count_travel_time: bool,
}

/// The daily lunch break. It is deducted only where it overlaps time at work.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LunchConfig {
    /// Default start of lunch.
    #[serde(default = "default_lunch_start")]
    pub start: NaiveTime,
    /// Default length in minutes (0 = no lunch break).
    #[serde(default = "default_lunch_minutes")]
    pub minutes: u32,
    /// Timed calendar events with this title (whole word, case-insensitive)
    /// set the day's lunch window.
    #[serde(default = "default_lunch_title")]
    pub title: String,
}

fn default_lunch_start() -> NaiveTime {
    NaiveTime::from_hms_opt(12, 0, 0).unwrap()
}

fn default_lunch_minutes() -> u32 {
    30
}

fn default_lunch_title() -> String {
    "Frokost".into()
}

impl Default for LunchConfig {
    fn default() -> Self {
        LunchConfig {
            start: default_lunch_start(),
            minutes: default_lunch_minutes(),
            title: default_lunch_title(),
        }
    }
}

/// Tax-free mileage allowance (skattefri kørselsgodtgørelse).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MileageConfig {
    pub driver_name: String,
    /// Registration number of the car (required on the logbook).
    pub registration_number: String,
    /// Your home address, as it appears in the trips' `from`/`to`. With
    /// `workplace` set, home ↔ workplace trips are always commuting.
    pub home: Option<String>,
    /// Rates per year, as published by Skatterådet.
    pub rates: Vec<MileageRate>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub struct MileageRate {
    pub year: i32,
    /// kr/km up to `threshold_km` per year.
    pub low: f64,
    /// kr/km above `threshold_km`.
    pub high: f64,
    #[serde(default = "default_threshold")]
    pub threshold_km: f64,
}

fn default_threshold() -> f64 {
    20_000.0
}

impl MileageConfig {
    pub fn rate(&self, year: i32) -> Result<MileageRate> {
        self.rates
            .iter()
            .find(|r| r.year == year)
            .copied()
            .with_context(|| {
                format!("no [[mileage.rates]] entry for {year} — add the rates from skat.dk")
            })
    }
}

#[derive(Debug, Default, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HoursReporting {
    /// No hours line: Danløn reports its own Normtimer (160,33 for 37 h/week),
    /// which is what Skat expects for full-time monthly salaried employees.
    #[default]
    None,
    /// Report the actual worked hours from the trips.
    Actual,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DanlonConfig {
    #[serde(default)]
    pub hours: HoursReporting,
    /// Lønart for hours (needed when `hours = "actual"`).
    pub hours_lonart: Option<String>,
    /// Lønart for tax-free mileage allowance (needed when `[mileage]` is set).
    pub mileage_lonart: Option<String>,
    #[serde(default = "default_delimiter")]
    pub delimiter: char,
    #[serde(default = "yes")]
    pub decimal_comma: bool,
    #[serde(default)]
    pub header: bool,
    /// Column layout. Available fields: employee, lonart, quantity, rate,
    /// amount, text, period_start, period_end, empty.
    #[serde(default = "default_columns")]
    pub columns: Vec<Column>,
    #[serde(default)]
    pub api: ApiConfig,
}

/// Danløn GraphQL API, used by `export --send` and `danlon …`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiConfig {
    #[serde(default = "default_api_url")]
    pub url: String,
    /// File holding the refresh token, used when DANLON_REFRESH_TOKEN is not
    /// set. Relative to the config file.
    pub refresh_token_file: Option<PathBuf>,
    /// Danløn's company id. Default: the company the token belongs to.
    pub company_id: Option<String>,
    /// Danløn's employee id. Default: the employee whose number is
    /// `employee_number`.
    pub employee_id: Option<String>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        ApiConfig {
            url: default_api_url(),
            refresh_token_file: None,
            company_id: None,
            employee_id: None,
        }
    }
}

fn default_api_url() -> String {
    "https://api.danlon.dk/graphql".into()
}

fn default_delimiter() -> char {
    ';'
}

fn default_columns() -> Vec<Column> {
    vec![
        Column::Employee,
        Column::Lonart,
        Column::Quantity,
        Column::Rate,
    ]
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    Employee,
    Lonart,
    Quantity,
    Rate,
    Amount,
    Text,
    PeriodStart,
    PeriodEnd,
    Empty,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg =
            Config::parse(&text).with_context(|| format!("parsing config {}", path.display()))?;
        // Resolve a local calendar path relative to the config file.
        if let Some(cal) = &mut cfg.calendar
            && !cal.source.contains("://")
            && Path::new(&cal.source).is_relative()
        {
            let base = path.parent().unwrap_or(Path::new("."));
            cal.source = base.join(&cal.source).to_string_lossy().into_owned();
        }
        let config_dir = path.parent().unwrap_or(Path::new("."));
        crate::danlon_api::resolve_token_file(&mut cfg.danlon.api.refresh_token_file, config_dir);
        cfg.state_file = config_dir.join("state.json");
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn parse(text: &str) -> Result<Self> {
        let raw: toml::Table = toml::from_str(text)?;
        if raw
            .get("periods")
            .and_then(|p| p.get("unpaid_break_minutes"))
            .is_some()
        {
            bail!(
                "periods.unpaid_break_minutes is no longer supported; set the lunch break in [lunch] (start, minutes, title)"
            );
        }
        Ok(toml::from_str(text)?)
    }

    pub fn validate(&self) -> Result<()> {
        if self.danlon.hours == HoursReporting::Actual && self.danlon.hours_lonart.is_none() {
            bail!("danlon.hours = \"actual\" requires danlon.hours_lonart");
        }
        if self.mileage.is_some() && self.danlon.mileage_lonart.is_none() {
            bail!("[mileage] requires danlon.mileage_lonart");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lunch_defaults_and_old_break_key_names_lunch() {
        let base = "employee_number = \"1\"\n[danlon]\n";
        let cfg = Config::parse(base).unwrap();
        assert_eq!(cfg.lunch.start, NaiveTime::from_hms_opt(12, 0, 0).unwrap());
        assert_eq!(
            (cfg.lunch.minutes, cfg.lunch.title.as_str()),
            (30, "Frokost")
        );

        let cfg =
            Config::parse(&format!("{base}[lunch]\nstart = \"11:30\"\nminutes = 45\n")).unwrap();
        assert_eq!(cfg.lunch.start, NaiveTime::from_hms_opt(11, 30, 0).unwrap());
        assert_eq!(cfg.lunch.minutes, 45);

        let err =
            Config::parse(&format!("{base}[periods]\nunpaid_break_minutes = 30\n")).unwrap_err();
        assert!(err.to_string().contains("[lunch]"), "{err}");
    }
}
