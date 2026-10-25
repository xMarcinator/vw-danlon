//! The app's data directory: `config.toml` (the CLI's format), `app.toml`
//! (theme and where secrets are kept), the state and trip files, and what
//! is missing for the screens to show anything.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use vw_danlon::backup::{self, Backup};
use vw_danlon::config::Config;
use vw_danlon::connected_cars;
use vw_danlon::db;
use vw_danlon::service::{self, AccountAccess, SyncOutcome};
use vw_danlon::state::State;
use vw_danlon::trip_store;

use crate::secrets::{self, Secret, SecretStore};

/// A config for a first run: what `Config` requires and nothing else.
const STARTER_CONFIG: &str = "# Written by the vw-danlon app; the CLI reads this file too.\n\
employee_number = \"\"\n\n[danlon]\n";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Light, Theme::Dark, Theme::System];

    pub fn id(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::System => "system",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Light => "Light",
            Theme::Dark => "Dark",
            Theme::System => "System",
        }
    }
}

/// `app.toml`: the app's own settings, beside the CLI's config.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppSettings {
    pub theme: Theme,
    /// Keep secrets in this file (relative to the directory, readable only
    /// by you) instead of the platform's secret store. Off unless set.
    pub secret_file: Option<String>,
}

/// The file name used when the user opts in to keeping secrets in a file.
pub const SECRET_FILE: &str = "secrets.json";

/// Something that keeps the screens from showing data; the app opens on
/// Settings naming it.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    NoDir(String),
    BadAppSettings(String),
    NoConfig(PathBuf),
    BadConfig(String),
    SecretStore(String),
    NoCalendar,
    NoTrips,
    BadTrips(String),
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Problem::NoDir(e) => write!(f, "No data folder: {e}"),
            Problem::BadAppSettings(e) => write!(f, "App settings can't be read: {e}"),
            Problem::NoConfig(p) => write!(
                f,
                "No config file at {}. Fill in the settings below and save to create it.",
                p.display()
            ),
            Problem::BadConfig(e) => write!(f, "The config file can't be used: {e}"),
            Problem::SecretStore(e) => write!(
                f,
                "Secrets can't be read: {e}. Turn on \u{201c}Keep secrets in a file\u{201d} below to use a file only you can read."
            ),
            Problem::NoCalendar => write!(
                f,
                "No work calendar yet. Paste your Proton share link below to see your hours."
            ),
            Problem::NoTrips => write!(
                f,
                "No trips yet. Import the Min Volkswagen export or sync your account to see mileage."
            ),
            Problem::BadTrips(e) => write!(f, "The stored data can't be read: {e}"),
        }
    }
}

/// The imported trips, for the Trips source section.
#[derive(Debug, Clone, PartialEq)]
pub struct TripsSummary {
    pub count: usize,
    pub first: Option<chrono::NaiveDate>,
    pub last: Option<chrono::NaiveDate>,
}

/// The account sync, as far as it can be told without calling it.
#[derive(Debug, Clone, PartialEq)]
pub enum AccountStatus {
    NotSetUp,
    Connected {
        email: String,
        last_sync: Option<String>,
    },
    NoToken {
        email: String,
    },
    Unknown(String),
}

/// Everything the app read from its data directory at start.
pub struct Setup {
    pub dir: PathBuf,
    pub settings: AppSettings,
    pub config: Option<Config>,
    /// The config file's text, for the Settings form.
    pub config_text: Option<String>,
    /// Whether a calendar link is kept in the secret store.
    pub link_in_store: bool,
    pub trips: Option<TripsSummary>,
    /// When the trips and state were moved in from the JSON files.
    pub migrated: Option<db::Migrated>,
    pub account: AccountStatus,
    pub problems: Vec<Problem>,
}

impl Setup {
    pub fn config_path(&self) -> PathBuf {
        self.dir.join("config.toml")
    }

    pub fn app_path(&self) -> PathBuf {
        self.dir.join("app.toml")
    }

    pub fn secret_store(&self) -> Box<dyn SecretStore> {
        secrets::store(self.settings.secret_file.as_ref().map(|f| self.dir.join(f)))
    }

    /// Reads the data directory; what is missing or broken becomes a
    /// [`Problem`], never an error.
    pub fn load(dir: Result<PathBuf>) -> Setup {
        let dir = match dir {
            Ok(d) => d,
            Err(e) => {
                return Setup {
                    dir: PathBuf::new(),
                    settings: AppSettings::default(),
                    config: None,
                    config_text: None,
                    link_in_store: false,
                    trips: None,
                    migrated: None,
                    account: AccountStatus::NotSetUp,
                    problems: vec![Problem::NoDir(format!("{e:#}"))],
                };
            }
        };
        let mut problems = Vec::new();
        let settings = match read_app_settings(&dir.join("app.toml")) {
            Ok(s) => s,
            Err(e) => {
                problems.push(Problem::BadAppSettings(format!("{e:#}")));
                AppSettings::default()
            }
        };
        let mut setup = Setup {
            dir,
            settings,
            config: None,
            config_text: None,
            link_in_store: false,
            trips: None,
            migrated: None,
            account: AccountStatus::NotSetUp,
            problems,
        };
        let store = setup.secret_store();
        let link = match store.get(&Secret::CalendarLink) {
            Ok(l) => l,
            Err(e) => {
                setup.problems.push(Problem::SecretStore(format!("{e:#}")));
                None
            }
        };
        setup.link_in_store = link.is_some();

        let path = setup.config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                match with_calendar_link(&text, link.as_deref())
                    .and_then(|t| Config::from_text(&t, &path))
                {
                    Ok(cfg) => setup.config = Some(cfg),
                    Err(e) => setup.problems.push(Problem::BadConfig(format!("{e:#}"))),
                }
                setup.config_text = Some(text);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                setup.problems.push(Problem::NoConfig(path));
            }
            Err(e) => setup.problems.push(Problem::BadConfig(format!(
                "reading {}: {e}",
                path.display()
            ))),
        }

        if let Some(cfg) = &setup.config {
            if cfg.calendar.is_none() {
                setup.problems.push(Problem::NoCalendar);
            }
            match trip_store::summary(&cfg.database, cfg.timezone) {
                Ok(t) => {
                    if t.count == 0 {
                        setup.problems.push(Problem::NoTrips);
                    }
                    setup.trips = Some(TripsSummary {
                        count: t.count,
                        first: t.first,
                        last: t.last_end,
                    });
                }
                Err(e) => setup.problems.push(Problem::BadTrips(format!("{e:#}"))),
            }
            setup.migrated = db::migrated(&cfg.database).ok().flatten();
            setup.account = account_status(cfg, store.as_ref());
        } else if link.is_none()
            && setup
                .problems
                .iter()
                .any(|p| matches!(p, Problem::NoConfig(_)))
        {
            setup.problems.push(Problem::NoCalendar);
        }
        setup
    }

    /// Saves the theme in `app.toml`.
    pub fn set_theme(&mut self, theme: Theme) -> Result<()> {
        self.settings.theme = theme;
        write_app_settings(&self.app_path(), &self.settings)
    }

    /// Turns the opt-in secrets file on or off in `app.toml`.
    pub fn set_secret_file(&mut self, on: bool) -> Result<()> {
        self.settings.secret_file = on.then(|| SECRET_FILE.to_string());
        write_app_settings(&self.app_path(), &self.settings)
    }
}

fn account_status(cfg: &Config, store: &dyn SecretStore) -> AccountStatus {
    let state = match State::load(&cfg.database) {
        Ok(s) => s,
        Err(e) => return AccountStatus::Unknown(format!("{e:#}")),
    };
    let email = state
        .account
        .as_ref()
        .map(|a| a.email.clone())
        .or_else(|| cfg.connected_cars.as_ref().and_then(|c| c.email.clone()));
    let Some(email) = email else {
        return AccountStatus::NotSetUp;
    };
    match store.get(&Secret::DeviceToken(email.clone())) {
        Ok(Some(_)) => AccountStatus::Connected {
            last_sync: state
                .account
                .and_then(|a| a.last_sync)
                .map(|t| t.format("%-d %b %Y %H:%M").to_string()),
            email,
        },
        Ok(None) => AccountStatus::NoToken { email },
        Err(e) => AccountStatus::Unknown(format!("{e:#}")),
    }
}

fn read_app_settings(path: &Path) -> Result<AppSettings> {
    match std::fs::read_to_string(path) {
        Ok(t) => toml::from_str(&t).with_context(|| format!("parsing {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppSettings::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn write_app_settings(path: &Path, settings: &AppSettings) -> Result<()> {
    write_atomic(path, &toml::to_string(settings)?)
}

/// `text` with `[calendar] source` set to `link`, when there is one: the
/// app keeps the link in the secret store, not in the config file. Without
/// a link, a `[calendar]` without a source (as a backup from another
/// device has it) is left out, so the app asks for the link.
fn with_calendar_link(text: &str, link: Option<&str>) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    let Some(link) = link else {
        if doc
            .get("calendar")
            .and_then(|c| c.as_table_like())
            .is_some_and(|c| !c.contains_key("source"))
        {
            doc.remove("calendar");
        }
        return Ok(doc.to_string());
    };
    let cal = doc
        .entry("calendar")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .context("calendar must be a table")?;
    cal["source"] = toml_edit::value(link);
    Ok(doc.to_string())
}

/// What the Settings screen saves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsForm {
    /// A new calendar link; empty keeps the stored one.
    pub calendar_link: String,
    pub workplace: String,
    pub home: String,
    /// `[home_working]`; none leaves it as it is.
    pub home_working: Option<HomeWorkingForm>,
}

/// The Settings screen's Working from home section.
#[derive(Debug, Clone, PartialEq)]
pub struct HomeWorkingForm {
    /// Comma-separated words, e.g. "Hjemmearbejde, Hjemme".
    pub keywords: String,
    pub at_home_address: bool,
}

impl HomeWorkingForm {
    fn from_config(cfg: Option<&Config>) -> HomeWorkingForm {
        let h = cfg.map(|c| c.home_working.clone()).unwrap_or_default();
        HomeWorkingForm {
            keywords: h.keywords.join(", "),
            at_home_address: h.at_home_address,
        }
    }

    fn keywords(&self) -> Vec<String> {
        self.keywords
            .split(',')
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect()
    }
}

impl SettingsForm {
    /// The form as the config has it now (the link is never shown).
    pub fn from_config(cfg: Option<&Config>) -> SettingsForm {
        SettingsForm {
            calendar_link: String::new(),
            workplace: cfg.and_then(|c| c.workplace.clone()).unwrap_or_default(),
            home: cfg
                .and_then(|c| c.mileage.as_ref())
                .and_then(|m| m.home.clone())
                .unwrap_or_default(),
            home_working: Some(HomeWorkingForm::from_config(cfg)),
        }
    }
}

/// Saves the form: the calendar link to the secret store, the addresses to
/// `config.toml` (created when missing, comments kept). Nothing is written
/// unless the resulting config can be read.
pub fn save(setup: &Setup, form: &SettingsForm) -> Result<()> {
    let path = setup.config_path();
    let old = setup.config_text.as_deref().unwrap_or(STARTER_CONFIG);
    let mut doc: toml_edit::DocumentMut = old
        .parse()
        .with_context(|| format!("parsing {}", path.display()))?;

    let workplace = form.workplace.trim();
    if workplace.is_empty() {
        doc.remove("workplace");
    } else {
        doc["workplace"] = toml_edit::value(workplace);
    }
    let home = form.home.trim();
    match doc.get_mut("mileage").and_then(|m| m.as_table_like_mut()) {
        Some(m) if home.is_empty() => {
            m.remove("home");
        }
        Some(m) => {
            m.insert("home", toml_edit::value(home));
        }
        None if !home.is_empty() => {
            bail!("home is part of the mileage logbook: set it up on the Mileage screen first")
        }
        None => {}
    }
    if let Some(h) = &form.home_working {
        // The defaults need no section; an existing one is kept up to date.
        let default = vw_danlon::config::HomeWorkingConfig::default();
        let words = h.keywords();
        if doc.contains_key("home_working")
            || words != default.keywords
            || h.at_home_address != default.at_home_address
        {
            if !doc.contains_key("home_working") {
                let mut t = toml_edit::Table::new();
                t.decor_mut()
                    .set_prefix("\n# Working from home, set in the app.\n");
                doc.insert("home_working", toml_edit::Item::Table(t));
            }
            let t = doc["home_working"]
                .as_table_like_mut()
                .context("[home_working] is not a table")?;
            t.insert(
                "keywords",
                toml_edit::value(words.iter().collect::<toml_edit::Array>()),
            );
            t.insert("at_home_address", toml_edit::value(h.at_home_address));
        }
    }
    let new = doc.to_string();

    let store = setup.secret_store();
    let link = form.calendar_link.trim();
    let link = if link.is_empty() {
        store.get(&Secret::CalendarLink)?
    } else {
        Some(link.to_string())
    };
    Config::from_text(&with_calendar_link(&new, link.as_deref())?, &path).context("not saved")?;

    if !form.calendar_link.trim().is_empty() {
        store
            .set(&Secret::CalendarLink, form.calendar_link.trim())
            .context("calendar link not saved")?;
    }
    if setup.config_text.as_deref() != Some(new.as_str()) {
        std::fs::create_dir_all(&setup.dir)
            .with_context(|| format!("creating {}", setup.dir.display()))?;
        write_atomic(&path, &new)?;
    }
    Ok(())
}

/// What the Mileage screen's setup asks for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MileageForm {
    pub driver_name: String,
    pub registration_number: String,
    pub home: String,
}

/// Rates written by the mileage setup, as in the example config: kr/km up
/// to the threshold, kr/km above it, threshold km. The user checks them on
/// skat.dk.
const DEFAULT_RATES: [(i32, f64, f64, i64); 1] = [(2026, 3.94, 2.28, 20_000)];

/// Adds a `[mileage]` section with the default rates to `config.toml`
/// (created when missing, comments kept). Nothing is written unless the
/// resulting config can be read.
pub fn add_mileage(setup: &Setup, form: &MileageForm) -> Result<()> {
    let path = setup.config_path();
    let driver = form.driver_name.trim();
    let plate = form.registration_number.trim();
    if driver.is_empty() || plate.is_empty() {
        bail!("the logbook needs your name and the car's registration number");
    }
    let old = setup.config_text.as_deref().unwrap_or(STARTER_CONFIG);
    let mut doc: toml_edit::DocumentMut = old
        .parse()
        .with_context(|| format!("parsing {}", path.display()))?;
    if doc.contains_key("mileage") {
        bail!("{} already has a [mileage] section", path.display());
    }
    let mut mileage = toml_edit::Table::new();
    mileage.decor_mut().set_prefix(
        "\n# Tax-free mileage allowance (skattefri kørselsgodtgørelse), set up in the app.\n",
    );
    mileage["driver_name"] = toml_edit::value(driver);
    mileage["registration_number"] = toml_edit::value(plate);
    let home = form.home.trim();
    if !home.is_empty() {
        mileage["home"] = toml_edit::value(home);
    }
    let mut rates = toml_edit::ArrayOfTables::new();
    for (i, (year, low, high, threshold)) in DEFAULT_RATES.into_iter().enumerate() {
        let mut r = toml_edit::Table::new();
        if i == 0 {
            r.decor_mut()
                .set_prefix("\n# Rates from Skatterådet. VERIFY on skat.dk before use.\n");
        }
        r["year"] = toml_edit::value(i64::from(year));
        r["low"] = toml_edit::value(low);
        r["high"] = toml_edit::value(high);
        r["threshold_km"] = toml_edit::value(threshold);
        rates.push(r);
    }
    mileage.insert("rates", toml_edit::Item::ArrayOfTables(rates));
    // The logbook's km go on lønart 39 (Kilometerpenge), as in the example
    // config, unless the config names another.
    if let Some(danlon) = doc.get_mut("danlon").and_then(|d| d.as_table_like_mut())
        && !danlon.contains_key("mileage_lonart")
    {
        danlon.insert("mileage_lonart", toml_edit::value("39"));
    }
    doc.insert("mileage", toml_edit::Item::Table(mileage));
    let new = doc.to_string();
    Config::from_text(&new, &path).context("not saved")?;
    std::fs::create_dir_all(&setup.dir)
        .with_context(|| format!("creating {}", setup.dir.display()))?;
    write_atomic(&path, &new)
}

/// The account's email, or an error saying to connect it.
fn account_email(cfg: &Config) -> Result<(State, String)> {
    let state = State::load(&cfg.database)?;
    let email = AccountAccess::new(cfg, &state)
        .email
        .context("no Min Volkswagen account is connected")?;
    Ok((state, email))
}

/// Registers this device with the account and keeps the device token in
/// `store`; the password is used once and never stored.
pub fn connect_account(
    cfg: &Config,
    store: &dyn SecretStore,
    email: &str,
    password: &str,
) -> Result<()> {
    let email = email.trim();
    if email.is_empty() || password.is_empty() {
        bail!("enter the account's email and password");
    }
    let secret = Secret::DeviceToken(email.to_string());
    // Nothing is registered that cannot be kept.
    store.get(&secret)?;
    let state = State::load(&cfg.database)?;
    let access = AccountAccess::new(cfg, &state);
    let token = connected_cars::register_device(access.namespace, email, password)?;
    store
        .set(&secret, &token)
        .context("the device token can't be kept")?;
    service::remember_account(cfg, email)
}

/// Fetches the trips since the newest stored one from the account.
pub fn sync_account(
    cfg: &Config,
    store: &dyn SecretStore,
    today: NaiveDate,
) -> Result<SyncOutcome> {
    let (state, email) = account_email(cfg)?;
    let token = store
        .get(&Secret::DeviceToken(email.clone()))?
        .with_context(|| format!("no device token for {email}: connect the account again"))?;
    service::sync_trips_with(cfg, &AccountAccess::new(cfg, &state), &token, None, today)
}

/// Deletes the device token and forgets the account.
pub fn disconnect_account(cfg: &Config, store: &dyn SecretStore) -> Result<()> {
    let (_, email) = account_email(cfg)?;
    store.delete(&Secret::DeviceToken(email))?;
    service::forget_account(cfg)
}

/// Moves the data of the folder `from` the CLI used into the app's folder
/// (see [`backup::import_folder`]: the folder's trips.json and state.json
/// are deleted once the data is stored). A device token the CLI kept in a
/// `token_file` is copied too and kept in the app's secret store. The text
/// says what was done.
#[cfg_attr(target_os = "android", allow(dead_code))]
pub fn import_old_data(setup: &Setup, from: &Path) -> Result<String> {
    let done = backup::import_folder(from, &setup.dir)?;
    let mut text = format!("Imported {} trips", done.trips);
    if done.zero_km > 0 {
        text.push_str(&format!(", {} skipped (0 km)", done.zero_km));
    }
    if !done.deleted.is_empty() {
        let names: Vec<String> = done
            .deleted
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        text.push_str(&format!(
            "; deleted {} from {}",
            names.join(" and "),
            from.display()
        ));
    }
    if let Some(note) = copy_token(setup, from)? {
        text.push_str(&format!("; {note}"));
    }
    Ok(text)
}

/// Copies the CLI's `token_file` (relative to its folder) and keeps its
/// device token in the app's secret store; what was done, if anything.
#[cfg_attr(target_os = "android", allow(dead_code))]
fn copy_token(setup: &Setup, from: &Path) -> Result<Option<String>> {
    let path = setup.config_path();
    let text = std::fs::read_to_string(&path)?;
    let Ok(cfg) = Config::from_text(&with_calendar_link(&text, None)?, &path) else {
        return Ok(None);
    };
    let Some(file) = cfg
        .connected_cars
        .as_ref()
        .and_then(|c| c.token_file.as_deref())
    else {
        return Ok(None);
    };
    let name = Path::new(file)
        .file_name()
        .context("token_file has no file name")?;
    let source = from.join(name);
    if !source.exists() {
        return Ok(None);
    }
    std::fs::copy(&source, file).with_context(|| format!("copying {}", source.display()))?;
    let state = State::load(&cfg.database)?;
    let access = AccountAccess::new(&cfg, &state);
    let (Some(email), Some(token)) = (access.email.clone(), access.token().ok()) else {
        return Ok(None);
    };
    let store = setup.secret_store();
    store
        .set(&Secret::DeviceToken(email.clone()), &token)
        .context("the device token can't be kept")?;
    Ok(Some(format!(
        "the device token for {email} is kept in {}",
        store.describe()
    )))
}

/// The backup of the app's data, as a file name and its text.
pub fn export_backup(setup: &Setup) -> Result<(String, String)> {
    let cfg = setup
        .config
        .as_ref()
        .context("the config can't be used yet")?;
    let backup = Backup::export(&setup.config_path(), &cfg.database)?;
    let name = format!(
        "vw-danlon-backup-{}.json",
        crate::screens::now(cfg).format("%Y-%m-%d")
    );
    Ok((name, backup.to_json()?))
}

/// Replaces the app's config, trips and state with the backup in `bytes`
/// (a file named `name`).
pub fn import_backup(setup: &Setup, name: &str, bytes: &[u8]) -> Result<String> {
    let backup = Backup::parse(name, bytes)?;
    backup.restore(&setup.dir)?;
    Ok(format!(
        "Imported the backup of {}: {} trips. Enter the calendar link and connect the account again.",
        backup.created.format("%-d %b %Y"),
        backup.trips.trips.len()
    ))
}

/// Writes `text` through a temp file and a rename.
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}

/// The data directory: `VW_DANLON_DIR` when set, else the platform's
/// config directory (the app's own files on Android).
pub fn data_dir() -> Result<PathBuf> {
    if let Some(d) = std::env::var_os("VW_DANLON_DIR") {
        return Ok(d.into());
    }
    platform_dir()
}

#[cfg(not(target_os = "android"))]
fn platform_dir() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("no config directory on this system")?
        .join("vw-danlon"))
}

#[cfg(target_os = "android")]
fn platform_dir() -> Result<PathBuf> {
    use jni::objects::{JObject, JString};
    let ctx = ndk_context::android_context();
    // SAFETY: the pointers come from ndk-context, which the Android
    // launcher initialises with the process's VM and the app context.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };
    let dir = env
        .call_method(&context, "getFilesDir", "()Ljava/io/File;", &[])?
        .l()?;
    let path = env
        .call_method(&dir, "getAbsolutePath", "()Ljava/lang/String;", &[])?
        .l()?;
    let path: String = env.get_string(&JString::from(path))?.into();
    Ok(PathBuf::from(path))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::secrets::FileStore;

    /// A fresh scratch directory for one test.
    pub fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vwd-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A scratch directory using the opt-in secrets file, so tests never
    /// touch the keychain.
    fn file_dir(name: &str) -> PathBuf {
        let dir = scratch(name);
        std::fs::write(dir.join("app.toml"), "secret_file = \"secrets.json\"\n").unwrap();
        dir
    }

    const MILEAGE: &str = "employee_number = \"1\"\n# keep me\n[danlon]\nmileage_lonart = \"39\"\n\n[mileage]\ndriver_name = \"A\"\nregistration_number = \"AB1\"\n[[mileage.rates]]\nyear = 2026\nlow = 3.94\nhigh = 2.28\nthreshold_km = 20000\n";

    #[test]
    fn missing_config_names_the_file() {
        let dir = file_dir("noconfig");
        let s = Setup::load(Ok(dir.clone()));
        assert_eq!(
            s.problems,
            [
                Problem::NoConfig(dir.join("config.toml")),
                Problem::NoCalendar
            ]
        );
        assert!(s.problems[0].to_string().contains("config.toml"));
    }

    #[test]
    fn bad_config_and_no_calendar_and_no_trips_are_named() {
        let dir = file_dir("bad");
        std::fs::write(dir.join("config.toml"), "employee_number = 1\n").unwrap();
        let s = Setup::load(Ok(dir.clone()));
        assert!(
            matches!(&s.problems[..], [Problem::BadConfig(e)] if e.contains("employee_number")),
            "{:?}",
            s.problems
        );

        std::fs::write(dir.join("config.toml"), MILEAGE).unwrap();
        let s = Setup::load(Ok(dir));
        assert_eq!(s.problems, [Problem::NoCalendar, Problem::NoTrips]);
    }

    #[test]
    fn calendar_link_from_the_store_completes_the_config() {
        let dir = file_dir("link");
        std::fs::write(dir.join("config.toml"), MILEAGE).unwrap();
        let s = Setup::load(Ok(dir.clone()));
        let form = SettingsForm {
            calendar_link: " https://calendar.example/x.ics ".into(),
            workplace: "Kontorvej 1, 8000 Aarhus C".into(),
            home: "Hjemvej 3, 8200 Aarhus N".into(),
            ..SettingsForm::from_config(None)
        };
        save(&s, &form).unwrap();

        let text = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(text.contains("# keep me"), "{text}");
        assert!(
            !text.contains("calendar.example"),
            "link must not reach the file: {text}"
        );
        let s = Setup::load(Ok(dir.clone()));
        assert_eq!(s.problems, [Problem::NoTrips]);
        assert!(s.link_in_store);
        let cfg = s.config.as_ref().unwrap();
        assert_eq!(
            cfg.calendar.as_ref().unwrap().source,
            "https://calendar.example/x.ics"
        );
        assert_eq!(
            SettingsForm::from_config(Some(cfg)),
            SettingsForm {
                calendar_link: String::new(),
                ..form
            }
        );
        let store = FileStore(dir.join("secrets.json"));
        assert_eq!(
            store.get(&Secret::CalendarLink).unwrap().as_deref(),
            Some("https://calendar.example/x.ics")
        );
    }

    #[test]
    fn first_save_creates_a_config_the_cli_reads() {
        let dir = file_dir("first");
        let s = Setup::load(Ok(dir.clone()));
        let form = SettingsForm {
            calendar_link: "https://calendar.example/y.ics".into(),
            workplace: "Kontorvej 1".into(),
            home: String::new(),
            home_working: None,
        };
        save(&s, &form).unwrap();
        let cfg = Config::load(&dir.join("config.toml")).unwrap();
        assert_eq!(cfg.workplace.as_deref(), Some("Kontorvej 1"));
        assert!(cfg.calendar.is_none());
    }

    #[test]
    fn home_working_words_are_saved_and_defaults_add_nothing() {
        let dir = file_dir("home-working");
        let s = Setup::load(Ok(dir.clone()));
        save(&s, &SettingsForm::from_config(None)).unwrap();
        let text = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(!text.contains("home_working"), "{text}");

        let s = Setup::load(Ok(dir.clone()));
        let form = SettingsForm {
            home_working: Some(HomeWorkingForm {
                keywords: " Hjemme , WFH,, ".into(),
                at_home_address: false,
            }),
            ..SettingsForm::from_config(s.config.as_ref())
        };
        save(&s, &form).unwrap();
        let cfg = Config::load(&dir.join("config.toml")).unwrap();
        assert_eq!(cfg.home_working.keywords, ["Hjemme", "WFH"]);
        assert!(!cfg.home_working.at_home_address);
        assert_eq!(
            SettingsForm::from_config(Some(&cfg)).home_working.unwrap(),
            HomeWorkingForm {
                keywords: "Hjemme, WFH".into(),
                at_home_address: false,
            }
        );
    }

    #[test]
    fn invalid_result_writes_nothing() {
        let dir = file_dir("invalid");
        std::fs::write(
            dir.join("config.toml"),
            "employee_number = \"1\"\n[danlon]\n",
        )
        .unwrap();
        let s = Setup::load(Ok(dir.clone()));
        let form = SettingsForm {
            calendar_link: "https://calendar.example/z.ics".into(),
            workplace: String::new(),
            home: "Hjemvej 3".into(),
            home_working: None,
        };
        let err = save(&s, &form).unwrap_err();
        assert!(err.to_string().contains("Mileage screen"), "{err}");
        assert_eq!(
            FileStore(dir.join("secrets.json"))
                .get(&Secret::CalendarLink)
                .unwrap(),
            None
        );
    }

    #[test]
    fn store_failure_is_named_and_nothing_falls_back() {
        // A secrets "file" that is a directory: the store fails to read.
        let dir = scratch("broken");
        std::fs::create_dir(dir.join("vault")).unwrap();
        std::fs::write(dir.join("app.toml"), "secret_file = \"vault\"\n").unwrap();
        std::fs::write(dir.join("config.toml"), MILEAGE).unwrap();
        let s = Setup::load(Ok(dir.clone()));
        let msg = s.problems[0].to_string();
        assert!(
            matches!(s.problems[0], Problem::SecretStore(_)) && msg.contains("vault"),
            "{msg}"
        );
        let form = SettingsForm {
            calendar_link: "https://calendar.example/w.ics".into(),
            ..SettingsForm::default()
        };
        assert!(save(&s, &form).is_err());
        // Only the database, created when the app first reads the data.
        let files = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name() != db::FILE_NAME)
            .count();
        assert_eq!(files, 3, "no file written");
    }

    #[test]
    fn old_data_and_a_backup_move_in() {
        let app = file_dir("import-app");
        let cli = scratch("import-cli");
        let config = "employee_number = \"1\"\n[calendar]\nsource = \"https://cal.example/SECRET.ics\"\n\
            [connected_cars]\nemail = \"a@example.com\"\ntoken_file = \"connected-cars-token\"\n[danlon]\n";
        std::fs::write(cli.join("config.toml"), config).unwrap();
        std::fs::write(
            cli.join("connected-cars-token"),
            r#"{"email": "a@example.com", "device_token": "dev-1"}"#,
        )
        .unwrap();
        std::fs::write(
            cli.join("trips.json"),
            r#"{"trips": [{"start": "2026-10-05T07:32:00+02:00", "end": "2026-10-05T08:05:00+02:00",
                "start_odometer": null, "distance_km": 24.1, "from": "Hjem", "to": "Kontor",
                "start_pos": null, "end_pos": null}]}"#,
        )
        .unwrap();

        let setup = Setup::load(Ok(app.clone()));
        let text = import_old_data(&setup, &cli).unwrap();
        assert!(
            text.starts_with(&format!(
                "Imported 1 trips; deleted trips.json from {}; the device token for a@example.com is kept in",
                cli.display()
            )),
            "{text}"
        );
        let setup = Setup::load(Ok(app.clone()));
        assert_eq!(setup.trips.as_ref().unwrap().count, 1);
        assert!(matches!(setup.account, AccountStatus::Connected { .. }));
        assert!(cli.join("config.toml").exists() && !cli.join("trips.json").exists());

        // The backup has no calendar link: the phone asks for one.
        let (_, json) = export_backup(&setup).unwrap();
        assert!(!json.contains("SECRET") && !json.contains("dev-1"));
        let phone = file_dir("import-phone");
        let text =
            import_backup(&Setup::load(Ok(phone.clone())), "b.json", json.as_bytes()).unwrap();
        assert!(text.contains(": 1 trips."), "{text}");
        let phone = Setup::load(Ok(phone));
        assert!(
            phone.problems.contains(&Problem::NoCalendar),
            "{:?}",
            phone.problems
        );
        assert_eq!(phone.trips.as_ref().unwrap().count, 1);
    }

    #[test]
    fn app_settings_round_trip() {
        let dir = file_dir("appsettings");
        let mut s = Setup::load(Ok(dir.clone()));
        assert_eq!(s.settings.secret_file.as_deref(), Some(SECRET_FILE));
        s.set_theme(Theme::Dark).unwrap();
        let s = Setup::load(Ok(dir.clone()));
        assert_eq!(s.settings.theme, Theme::Dark);
        assert_eq!(s.settings.secret_file.as_deref(), Some(SECRET_FILE));

        let mut settings = s.settings.clone();
        settings.secret_file = None;
        let mut s = s;
        s.set_secret_file(false).unwrap();
        assert_eq!(read_app_settings(&dir.join("app.toml")).unwrap(), settings);

        std::fs::write(
            dir.join("app.toml"),
            "theme = \"pink\"\nsecret_file = \"secrets.json\"\n",
        )
        .unwrap();
        assert!(
            read_app_settings(&dir.join("app.toml"))
                .unwrap_err()
                .to_string()
                .contains("app.toml")
        );
    }

    #[test]
    fn mileage_setup_adds_a_section_the_cli_reads_and_unblocks_home() {
        let dir = file_dir("mileage-setup");
        std::fs::write(
            dir.join("config.toml"),
            "employee_number = \"1\"\n# keep me\n[danlon]\n",
        )
        .unwrap();
        let s = Setup::load(Ok(dir.clone()));
        let form = |driver: &str| MileageForm {
            driver_name: driver.into(),
            registration_number: " AB12345 ".into(),
            home: "Hjemvej 3".into(),
        };
        let err = add_mileage(&s, &form(" ")).unwrap_err();
        assert!(err.to_string().contains("your name"), "{err}");

        add_mileage(&s, &form("Test Testesen")).unwrap();
        let text = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(
            text.contains("# keep me") && text.contains("skat.dk"),
            "{text}"
        );
        let cfg = Config::load(&dir.join("config.toml")).unwrap();
        let m = cfg.mileage.as_ref().unwrap();
        assert_eq!(
            (m.driver_name.as_str(), m.registration_number.as_str()),
            ("Test Testesen", "AB12345")
        );
        assert_eq!(m.home.as_deref(), Some("Hjemvej 3"));
        assert_eq!(m.rate(2026).unwrap().low, 3.94);
        assert_eq!(cfg.danlon.mileage_lonart.as_deref(), Some("39"));

        let s = Setup::load(Ok(dir.clone()));
        let err = add_mileage(&s, &form("Again")).unwrap_err();
        assert!(err.to_string().contains("already has"), "{err}");
        // Home can now be changed in Settings.
        let settings = SettingsForm {
            home: "Nyvej 1".into(),
            ..SettingsForm::from_config(s.config.as_ref())
        };
        save(&s, &settings).unwrap();
        let cfg = Config::load(&dir.join("config.toml")).unwrap();
        assert_eq!(cfg.mileage.unwrap().home.as_deref(), Some("Nyvej 1"));
    }

    #[test]
    fn sync_without_an_account_or_token_says_so() {
        let dir = file_dir("sync-none");
        std::fs::write(dir.join("config.toml"), MILEAGE).unwrap();
        let s = Setup::load(Ok(dir.clone()));
        let cfg = s.config.as_ref().unwrap();
        let store = s.secret_store();
        let today = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        let err = sync_account(cfg, store.as_ref(), today).unwrap_err();
        assert!(
            err.to_string().contains("no Min Volkswagen account"),
            "{err}"
        );

        service::remember_account(cfg, "a@example.com").unwrap();
        let err = sync_account(cfg, store.as_ref(), today).unwrap_err();
        assert!(
            err.to_string()
                .contains("no device token for a@example.com"),
            "{err}"
        );
        let err = connect_account(cfg, store.as_ref(), "a@example.com", "").unwrap_err();
        assert!(err.to_string().contains("password"), "{err}");

        store
            .set(&Secret::DeviceToken("a@example.com".into()), "dev-1")
            .unwrap();
        assert!(matches!(
            Setup::load(Ok(dir.clone())).account,
            AccountStatus::Connected { .. }
        ));
        disconnect_account(cfg, store.as_ref()).unwrap();
        assert_eq!(Setup::load(Ok(dir)).account, AccountStatus::NotSetUp);
        assert_eq!(
            store
                .get(&Secret::DeviceToken("a@example.com".into()))
                .unwrap(),
            None
        );
    }
}
