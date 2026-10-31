use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{Datelike, NaiveDate, NaiveTime, Weekday};
use serde::{Deserialize, Deserializer};

use crate::address::Places;
use crate::trips::Position;

#[derive(Debug, Clone, Deserialize)]
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
    /// More calendars, for private appointments only (see [`ExtraCalendar`]).
    #[serde(default)]
    pub extra_calendars: Vec<ExtraCalendar>,
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
    #[serde(default)]
    pub vacation: VacationConfig,
    #[serde(default)]
    pub home_working: HomeWorkingConfig,
    #[serde(default)]
    pub private: PrivateConfig,
    /// The Min Volkswagen account (Connected Cars) for `trips sync`.
    pub connected_cars: Option<ConnectedCarsConfig>,
    pub danlon: DanlonConfig,
    /// The database beside the config file: trips, local state and the
    /// calendar cache (see [`crate::db`]).
    #[serde(skip)]
    pub database: PathBuf,
    /// `[places]` resolved against the configured addresses.
    #[serde(skip)]
    pub known_places: Places,
}

fn default_timezone() -> chrono_tz::Tz {
    chrono_tz::Europe::Copenhagen
}

/// Norm hours per weekday, used when the calendar does not set the norm.
#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarConfig {
    /// Proton Calendar share link (https://… or webcal://…) or a local .ics file.
    pub source: String,
    /// Timed events are planned working hours and set the day's norm. When
    /// false, the norm comes from `[work_week]` and timed events are ignored.
    #[serde(default = "yes")]
    pub norm_from_calendar: bool,
    /// If non-empty, only all-day events whose title contains one of these
    /// words (case-insensitive) count as days off. Vacation and feriefridag
    /// events (`[vacation]`) always do.
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

/// `[[extra_calendars]]`: a calendar besides the work calendar. It only
/// adds private appointments; it never sets planned hours, days off or
/// vacation, and its all-day events are ignored.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtraCalendar {
    /// Proton Calendar share link (https://… or webcal://…) or a local .ics
    /// file. The app keeps the link in the secret store under `id` instead;
    /// without either (a backup from another device) the calendar is not
    /// read.
    pub source: Option<String>,
    /// What its events are.
    #[serde(rename = "use")]
    pub usage: CalendarUse,
    /// The calendar's name, as shown in the app.
    pub name: Option<String>,
    /// The app's key for the link in the secret store.
    pub id: Option<String>,
}

/// What an extra calendar's timed events are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalendarUse {
    /// Every timed event is a private appointment.
    Private,
    /// Only timed events whose title has a private word
    /// (`calendar.private_keywords`) are private appointments; the rest is
    /// ignored.
    WordsOnly,
}

impl CalendarUse {
    /// The name in `config.toml`.
    pub fn id(self) -> &'static str {
        match self {
            CalendarUse::Private => "private",
            CalendarUse::WordsOnly => "words_only",
        }
    }
}

/// Working from home: a timed work event in the calendar that is home
/// working counts its planned window as worked, without trips.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeWorkingConfig {
    /// A work event whose title contains one of these words (whole word,
    /// case-insensitive) is home working.
    #[serde(default = "default_home_working_keywords")]
    pub keywords: Vec<String>,
    /// A work event whose location is the home address (`mileage.home`) is
    /// home working.
    #[serde(default = "yes")]
    pub at_home_address: bool,
}

fn default_home_working_keywords() -> Vec<String> {
    ["Hjemmearbejde", "Hjemme"].map(String::from).to_vec()
}

impl Default for HomeWorkingConfig {
    fn default() -> Self {
        HomeWorkingConfig {
            keywords: default_home_working_keywords(),
            at_home_address: true,
        }
    }
}

/// `[private]`: private calendar appointments start automatically.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateConfig {
    /// A private appointment whose start has passed without a recording
    /// shows as started automatically, and is recorded as "calendar time"
    /// (its planned window) once `grace_minutes` after its end have passed
    /// untouched.
    #[serde(default = "yes")]
    pub auto_start: bool,
    #[serde(default = "default_grace_minutes")]
    pub grace_minutes: u32,
}

fn default_grace_minutes() -> u32 {
    60
}

impl Default for PrivateConfig {
    fn default() -> Self {
        PrivateConfig {
            auto_start: true,
            grace_minutes: default_grace_minutes(),
        }
    }
}

impl PrivateConfig {
    /// How long after its planned end an untouched appointment waits before
    /// its planned window is recorded.
    pub fn grace(&self) -> chrono::Duration {
        chrono::Duration::minutes(i64::from(self.grace_minutes))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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

/// Vacation (samtidighedsferie) from all-day calendar events.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VacationConfig {
    /// All-day events whose title contains one of these words
    /// (case-insensitive) are vacation, e.g. "Ferie" also matches "Juleferie".
    #[serde(default = "default_vacation_keywords")]
    pub keywords: Vec<String>,
    /// All-day events whose title contains one of these words are
    /// feriefridage; they win over `keywords`.
    #[serde(default = "default_feriefridag_keywords")]
    pub feriefridag_keywords: Vec<String>,
    /// First day of employment (YYYY-MM-DD); vacation is earned from here.
    #[serde(default, deserialize_with = "optional_date")]
    pub employment_start: Option<NaiveDate>,
    /// Feriefridage per holiday year (from the contract).
    #[serde(default = "default_feriefridage")]
    pub feriefridage: u32,
    /// Vacation days earned per full month of employment.
    #[serde(default = "default_days_per_month")]
    pub days_per_month: f64,
    /// Vacation that repeats every year (see [`Repeat`]).
    #[serde(default)]
    pub repeats: Vec<Repeat>,
    /// First day ("MM-DD") of the feriefridage period, which runs a year
    /// (from the contract; not in ferieloven).
    #[serde(default = "default_feriefridage_from")]
    pub feriefridage_from: String,
    /// Most vacation days taken before they are earned (forskud); more needs
    /// a form signed by management (from the contract; § 7 sets no maximum).
    #[serde(default = "default_max_forskud")]
    pub max_forskud: f64,
}

/// A vacation that repeats every year until a calendar event with the same
/// title in that calendar year replaces it: ISO weeks, or the week before
/// Christmas (the Monday before 24 December through 23 December).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Repeat {
    pub title: String,
    #[serde(default)]
    pub weeks: Vec<u32>,
    #[serde(default)]
    pub christmas: bool,
}

fn default_vacation_keywords() -> Vec<String> {
    vec!["Ferie".into()]
}

fn default_feriefridag_keywords() -> Vec<String> {
    vec!["Feriefridag".into()]
}

fn default_feriefridage() -> u32 {
    5
}

fn default_days_per_month() -> f64 {
    2.08
}

fn default_feriefridage_from() -> String {
    "05-01".into()
}

fn default_max_forskud() -> f64 {
    5.0
}

impl Default for VacationConfig {
    fn default() -> Self {
        VacationConfig {
            keywords: default_vacation_keywords(),
            feriefridag_keywords: default_feriefridag_keywords(),
            employment_start: None,
            feriefridage: default_feriefridage(),
            days_per_month: default_days_per_month(),
            repeats: Vec::new(),
            feriefridage_from: default_feriefridage_from(),
            max_forskud: default_max_forskud(),
        }
    }
}

impl VacationConfig {
    /// `days_per_month` in hundredths of a day (2,08 → 208).
    pub fn hundredths_per_month(&self) -> i64 {
        (self.days_per_month * 100.0).round() as i64
    }

    /// `max_forskud` in hundredths of a day.
    pub fn max_forskud_hundredths(&self) -> i64 {
        (self.max_forskud * 100.0).round() as i64
    }

    /// `feriefridage_from` as (month, day); 29 February is not allowed.
    pub fn feriefridage_start(&self) -> Result<(u32, u32)> {
        let date =
            NaiveDate::parse_from_str(&format!("2001-{}", self.feriefridage_from), "%Y-%m-%d")
                .with_context(|| {
                    format!(
                        "vacation.feriefridage_from must be MM-DD, got {:?}",
                        self.feriefridage_from
                    )
                })?;
        Ok((date.month(), date.day()))
    }
}

/// A date written as a TOML date (`2026-05-01`) or a string ("2026-05-01").
fn optional_date<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NaiveDate>, D::Error> {
    use serde::de::Error;
    let text = match toml::Value::deserialize(d)? {
        toml::Value::Datetime(dt) => dt.to_string(),
        toml::Value::String(s) => s,
        other => return Err(D::Error::custom(format!("expected a date, got {other}"))),
    };
    NaiveDate::parse_from_str(&text, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| D::Error::custom(format!("expected a date as YYYY-MM-DD, got {text:?}")))
}

/// The Min Volkswagen account, read through the Connected Cars backend. The
/// password is never stored; `account connect` keeps a device token in the
/// operating system's keychain, or in `token_file` when there is none.
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MileageConfig {
    pub driver_name: String,
    /// Registration number of the car (required on the logbook).
    pub registration_number: String,
    /// Your home address, as it appears in the trips' `from`/`to`. With
    /// `workplace` set, home ↔ workplace trips are always commuting.
    pub home: Option<String>,
    /// Your address and CPR number for the logbook header (Skat requires
    /// them on the logbook; an empty CPR prints a blank line to fill in).
    /// The address defaults to `home`.
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub cpr: Option<String>,
    /// Rates per year, as published by Skatterådet.
    pub rates: Vec<MileageRate>,
    /// You live in a yderkommune: km above 120 a day get the higher
    /// befordringsfradrag rate (see [`FradragRate`]).
    #[serde(default)]
    pub yderkommune: bool,
    /// Befordringsfradrag rates per year (LL § 9 C); 2026 is built in.
    #[serde(default)]
    pub fradrag_rates: Vec<FradragRate>,
}

/// Befordringsfradrag rates for one year: kr/km of the daily round trip
/// from 25 to 120 km, above 120 km, and above 120 km in a yderkommune.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FradragRate {
    pub year: i32,
    pub low: f64,
    pub high: f64,
    pub yderkommune_high: f64,
}

/// The 2026 befordringsfradrag rates (skat.dk).
pub const FRADRAG_2026: FradragRate = FradragRate {
    year: 2026,
    low: 3.17,
    high: 1.59,
    yderkommune_high: 3.51,
};

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

    /// The befordringsfradrag rates of `year`: the config's, else the
    /// built-in ones for 2026.
    pub fn fradrag_rate(&self, year: i32) -> Result<FradragRate> {
        self.fradrag_rates
            .iter()
            .find(|r| r.year == year)
            .copied()
            .or((year == FRADRAG_2026.year).then_some(FRADRAG_2026))
            .with_context(|| {
                format!(
                    "no [[mileage.fradrag_rates]] entry for {year} — add the rates from skat.dk"
                )
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

#[derive(Debug, Clone, Deserialize)]
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
        Config::from_text(&text, path)
    }

    /// The config in `text`, read as if from the file at `path`: relative
    /// paths and the database are resolved beside it.
    pub fn from_text(text: &str, path: &Path) -> Result<Self> {
        let mut cfg =
            Config::parse(text).with_context(|| format!("parsing config {}", path.display()))?;
        // Resolve a local calendar path relative to the config file.
        if let Some(cal) = &mut cfg.calendar
            && !cal.source.contains("://")
            && Path::new(&cal.source).is_relative()
        {
            let base = path.parent().unwrap_or(Path::new("."));
            cal.source = base.join(&cal.source).to_string_lossy().into_owned();
        }
        let config_dir = path.parent().unwrap_or(Path::new("."));
        for source in cfg
            .extra_calendars
            .iter_mut()
            .filter_map(|c| c.source.as_mut())
        {
            if !source.contains("://") && Path::new(source.as_str()).is_relative() {
                *source = config_dir.join(&*source).to_string_lossy().into_owned();
            }
        }
        if let Some(file) = cfg
            .connected_cars
            .as_mut()
            .and_then(|c| c.token_file.as_mut())
            && Path::new(file.as_str()).is_relative()
        {
            *file = config_dir.join(&*file).to_string_lossy().into_owned();
        }
        cfg.database = config_dir.join(crate::db::FILE_NAME);
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
        // The app keeps the work calendar's link in its secret store, so its
        // `[calendar]` (the private words) may have no source: that reads as
        // no work calendar.
        let sourceless = raw
            .get("calendar")
            .and_then(|c| c.as_table())
            .is_some_and(|c| !c.contains_key("source"));
        let mut cfg: Config = if sourceless {
            let mut raw = raw;
            raw.remove("calendar");
            toml::Value::Table(raw).try_into()?
        } else {
            toml::from_str(text)?
        };
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

    /// Timed events whose title has one of these words are private
    /// appointments, in every calendar (`calendar.private_keywords`; none
    /// without a work calendar).
    pub fn private_words(&self) -> &[String] {
        self.calendar
            .as_ref()
            .map(|c| c.private_keywords.as_slice())
            .unwrap_or_default()
    }

    pub fn validate(&self) -> Result<()> {
        if self.danlon.hours == HoursReporting::Actual && self.danlon.hours_lonart.is_none() {
            bail!("danlon.hours = \"actual\" requires danlon.hours_lonart");
        }
        if self.mileage.is_some() && self.danlon.mileage_lonart.is_none() {
            bail!("[mileage] requires danlon.mileage_lonart");
        }
        if self.vacation.days_per_month.is_nan() || self.vacation.days_per_month <= 0.0 {
            bail!("vacation.days_per_month must be a positive number of days");
        }
        if self.vacation.max_forskud.is_nan() || self.vacation.max_forskud < 0.0 {
            bail!("vacation.max_forskud must be zero or more days");
        }
        self.vacation.feriefridage_start()?;
        for r in &self.vacation.repeats {
            if r.title.trim().is_empty() {
                bail!("vacation.repeats: every repeat needs a title");
            }
            if r.weeks.is_empty() == !r.christmas {
                bail!(
                    "vacation.repeats {:?}: set either weeks or christmas = true",
                    r.title
                );
            }
            if let Some(w) = r.weeks.iter().find(|w| !(1..=53).contains(*w)) {
                bail!("vacation.repeats {:?}: week {w} is not 1–53", r.title);
            }
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
    fn vacation_defaults_and_employment_start() {
        let base = "employee_number = \"1\"\n[danlon]\n";
        let v = Config::parse(base).unwrap().vacation;
        assert_eq!(
            (v.keywords.clone(), v.feriefridag_keywords.clone()),
            (vec!["Ferie".to_string()], vec!["Feriefridag".to_string()])
        );
        assert_eq!(
            (v.employment_start, v.feriefridage, v.hundredths_per_month()),
            (None, 5, 208)
        );

        let may = NaiveDate::from_ymd_opt(2026, 5, 1);
        for start in ["2026-05-01", "\"2026-05-01\""] {
            let cfg =
                Config::parse(&format!("{base}[vacation]\nemployment_start = {start}\n")).unwrap();
            assert_eq!(cfg.vacation.employment_start, may);
        }
        let err = Config::parse(&format!("{base}[vacation]\nemployment_start = \"1 May\"\n"))
            .unwrap_err();
        assert!(format!("{err:#}").contains("YYYY-MM-DD"), "{err:#}");
    }

    #[test]
    fn vacation_plan_keys_default_parse_and_are_checked() {
        let base = "employee_number = \"1\"\n[danlon]\n";
        let v = Config::parse(base).unwrap().vacation;
        assert_eq!(
            (
                v.repeats.len(),
                v.feriefridage_start().unwrap(),
                v.max_forskud_hundredths()
            ),
            (0, (5, 1), 500)
        );

        let cfg = Config::parse(&format!(
            "{base}[vacation]\nferiefridage_from = \"01-01\"\nmax_forskud = 3\n\
             repeats = [{{ title = \"Sommerferie\", weeks = [28, 29, 30] }}, \
             {{ title = \"Juleferie\", christmas = true }}]\n"
        ))
        .unwrap();
        cfg.validate().unwrap();
        let v = cfg.vacation;
        assert_eq!(v.feriefridage_start().unwrap(), (1, 1));
        assert_eq!(v.max_forskud_hundredths(), 300);
        assert_eq!(v.repeats[0].weeks, [28, 29, 30]);
        assert!(v.repeats[1].christmas && v.repeats[1].weeks.is_empty());

        for (bad, says) in [
            ("feriefridage_from = \"02-30\"", "MM-DD"),
            ("feriefridage_from = \"02-29\"", "MM-DD"),
            ("max_forskud = -1", "max_forskud"),
            ("repeats = [{ title = \"X\" }]", "either weeks or christmas"),
            (
                "repeats = [{ title = \"X\", weeks = [1], christmas = true }]",
                "either weeks or christmas",
            ),
            ("repeats = [{ title = \"X\", weeks = [54] }]", "not 1–53"),
            (
                "repeats = [{ title = \" \", weeks = [1] }]",
                "needs a title",
            ),
        ] {
            let err = Config::parse(&format!("{base}[vacation]\n{bad}\n"))
                .and_then(|c| c.validate())
                .unwrap_err();
            assert!(format!("{err:#}").contains(says), "{bad}: {err:#}");
        }
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
    fn extra_calendars_are_optional_and_read_with_their_use() {
        let base = "employee_number = \"1\"\n[danlon]\n";
        let cfg = Config::from_text(base, Path::new("/cfg/config.toml")).unwrap();
        assert!(cfg.extra_calendars.is_empty());

        let text = format!(
            "{base}[[extra_calendars]]\nsource = \"private.ics\"\nuse = \"private\"\nname = \"Personal\"\n\
             [[extra_calendars]]\nid = \"c1\"\nuse = \"words_only\"\n"
        );
        let cfg = Config::from_text(&text, Path::new("/cfg/config.toml")).unwrap();
        assert_eq!(
            cfg.extra_calendars,
            [
                ExtraCalendar {
                    source: Some("/cfg/private.ics".into()),
                    usage: CalendarUse::Private,
                    name: Some("Personal".into()),
                    id: None,
                },
                ExtraCalendar {
                    source: None,
                    usage: CalendarUse::WordsOnly,
                    name: None,
                    id: Some("c1".into()),
                },
            ]
        );
        let err = Config::parse(&format!(
            "{base}[[extra_calendars]]\nsource = \"x.ics\"\nuse = \"working_hours\"\n"
        ))
        .unwrap_err();
        assert!(format!("{err:#}").contains("working_hours"), "{err:#}");
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
