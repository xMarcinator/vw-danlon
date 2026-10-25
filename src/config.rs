use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{NaiveTime, Weekday};
use serde::Deserialize;

use crate::address::Places;
use crate::trips::Position;

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
    /// Coordinates of home and the workplace (see [`PlacesConfig`]).
    pub places: Option<PlacesConfig>,
    #[serde(default)]
    pub periods: PeriodConfig,
    #[serde(default)]
    pub lunch: LunchConfig,
    pub mileage: Option<MileageConfig>,
    #[serde(default)]
    pub checklist: ChecklistConfig,
    /// The Min Volkswagen account (Connected Cars) for `trips sync`.
    pub connected_cars: Option<ConnectedCarsConfig>,
    pub danlon: DanlonConfig,
    /// The local state file (`state.json` beside the config file).
    #[serde(skip)]
    pub state_file: PathBuf,
    /// The imported trips (`trips.json` beside the config file).
    #[serde(skip)]
    pub trip_store: PathBuf,
    /// `[places]` resolved against the configured addresses.
    #[serde(skip)]
    pub known_places: Places,
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

/// Coordinates ("56.1694, 9.5432", as copied from a map) of home and the
/// workplace. A trip ending within `radius_m` of one counts as arriving
/// there, whatever address the trip export shows.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacesConfig {
    pub home: Option<String>,
    pub workplace: Option<String>,
    #[serde(default = "default_radius")]
    pub radius_m: f64,
}

fn default_radius() -> f64 {
    150.0
}

/// A "lat, lon" coordinate pair.
fn parse_position(key: &str, s: &str) -> Result<Position> {
    let parsed = s.split_once(',').and_then(|(lat, lon)| {
        Some(Position {
            lat: lat.trim().parse().ok()?,
            lon: lon.trim().parse().ok()?,
        })
    });
    match parsed {
        Some(p) if (-90.0..=90.0).contains(&p.lat) && (-180.0..=180.0).contains(&p.lon) => Ok(p),
        _ => bail!(
            "{key} = {s:?} is not a coordinate; expected \"lat, lon\", e.g. \"56.1694, 9.5432\""
        ),
    }
}

/// The Danløn app checklist.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChecklistConfig {
    /// Name of home as used in the Danløn app's routes.
    #[serde(default = "default_home_name")]
    pub home_name: String,
    /// Name of the fixed workplace as used in the Danløn app's routes.
    #[serde(default = "default_workplace_name")]
    pub workplace_name: String,
}

fn default_home_name() -> String {
    "Hjem".into()
}

fn default_workplace_name() -> String {
    "Kontoret".into()
}

impl Default for ChecklistConfig {
    fn default() -> Self {
        ChecklistConfig {
            home_name: default_home_name(),
            workplace_name: default_workplace_name(),
        }
    }
}

/// The Min Volkswagen account, read through the Connected Cars backend. The
/// password is never stored; `account connect` keeps a device token in the
/// operating system's keychain, or in `token_file` when there is none.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectedCarsConfig {
    /// Account email, offered as the default by `account connect`.
    pub email: Option<String>,
    /// Licence plate or vehicle id; needed when the account has several cars.
    pub vehicle: Option<String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    /// Keep the device token in this file (relative to the config file,
    /// readable only by you) instead of the keychain.
    pub token_file: Option<String>,
}

fn default_namespace() -> String {
    "semler:minvolkswagen".into()
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
        if let Some(file) = cfg
            .connected_cars
            .as_mut()
            .and_then(|c| c.token_file.as_mut())
            && Path::new(file.as_str()).is_relative()
        {
            *file = config_dir.join(&*file).to_string_lossy().into_owned();
        }
        cfg.state_file = config_dir.join("state.json");
        cfg.trip_store = config_dir.join("trips.json");
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
        let mut cfg: Config = toml::from_str(text)?;
        cfg.known_places = cfg.resolve_places()?;
        Ok(cfg)
    }

    /// The `[places]` coordinates paired with the addresses they belong to.
    fn resolve_places(&self) -> Result<Places> {
        let Some(p) = &self.places else {
            return Ok(Places::default());
        };
        if p.radius_m.is_nan() || p.radius_m <= 0.0 {
            bail!("places.radius_m must be a positive number of metres");
        }
        let home = self.mileage.as_ref().and_then(|m| m.home.as_ref());
        let mut known = Vec::new();
        for (key, coord, address, needs) in [
            ("places.home", &p.home, home, "mileage.home"),
            (
                "places.workplace",
                &p.workplace,
                self.workplace.as_ref(),
                "workplace",
            ),
        ] {
            let Some(coord) = coord else { continue };
            let pos = parse_position(key, coord)?;
            let Some(address) = address else {
                bail!("{key} requires {needs} to be set");
            };
            known.push((address.clone(), pos));
        }
        Ok(Places {
            known,
            radius_m: p.radius_m,
        })
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

    #[test]
    fn old_api_section_is_refused() {
        let err = Config::parse("employee_number = \"1\"\n[danlon]\n[danlon.api]\nurl = \"x\"\n")
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("[danlon.api]") && msg.contains("unknown field `api`"),
            "{msg}"
        );
    }

    #[test]
    fn places_coordinates() {
        let base =
            "employee_number = \"1\"\nworkplace = \"Kontorvej 1, 8000 Aarhus C\"\n[danlon]\n";
        let cfg = Config::parse(&format!(
            "{base}[places]\nworkplace = \"56.1694, 9.5432\"\n"
        ))
        .unwrap();
        assert_eq!(cfg.known_places.radius_m, 150.0);
        assert_eq!(
            cfg.known_places.known,
            [(
                "Kontorvej 1, 8000 Aarhus C".to_string(),
                Position {
                    lat: 56.1694,
                    lon: 9.5432
                }
            )]
        );

        for bad in ["\"56.1694\"", "\"95, 9.5\"", "\"nord, øst\""] {
            let err = Config::parse(&format!("{base}[places]\nworkplace = {bad}\n")).unwrap_err();
            assert!(err.to_string().contains("places.workplace"), "{err}");
        }
        let err = Config::parse(&format!("{base}[places]\nhome = \"56.1, 9.5\"\n")).unwrap_err();
        assert!(err.to_string().contains("mileage.home"), "{err}");
    }
}
