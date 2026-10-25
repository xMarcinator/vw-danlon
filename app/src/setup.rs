//! The app's data directory: `config.toml` (the CLI's format), `app.toml`
//! (theme and where secrets are kept), the state and trip files, and what
//! is missing for the screens to show anything.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use vw_danlon::config::Config;
use vw_danlon::state::State;
use vw_danlon::trip_store::TripStore;

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
            Problem::BadTrips(e) => write!(f, "The trips file can't be read: {e}"),
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
            match TripStore::load(&cfg.trip_store) {
                Ok(t) => {
                    let trips = t.trips(cfg.timezone);
                    if trips.is_empty() {
                        setup.problems.push(Problem::NoTrips);
                    }
                    setup.trips = Some(TripsSummary {
                        count: trips.len(),
                        first: trips.first().map(|t| t.start.date_naive()),
                        last: trips.last().map(|t| t.end.date_naive()),
                    });
                }
                Err(e) => setup.problems.push(Problem::BadTrips(format!("{e:#}"))),
            }
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
    let state = match State::load(&cfg.state_file) {
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
/// app keeps the link in the secret store, not in the config file.
fn with_calendar_link(text: &str, link: Option<&str>) -> Result<String> {
    let Some(link) = link else {
        return Ok(text.to_string());
    };
    let mut doc: toml_edit::DocumentMut = text.parse()?;
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
        None if !home.is_empty() => bail!(
            "home is part of the mileage logbook: add a [mileage] section to {} first",
            path.display()
        ),
        None => {}
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
        };
        save(&s, &form).unwrap();
        let cfg = Config::load(&dir.join("config.toml")).unwrap();
        assert_eq!(cfg.workplace.as_deref(), Some("Kontorvej 1"));
        assert!(cfg.calendar.is_none());
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
        };
        let err = save(&s, &form).unwrap_err();
        assert!(err.to_string().contains("[mileage]"), "{err}");
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
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            3,
            "no file written"
        );
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
}
