//! The Settings screen: what is missing, the calendars (links kept in the
//! secret store) and the private words, the addresses, the trips source,
//! where secrets are kept, and the theme.

use dioxus::prelude::*;
use vw_danlon::config::{CalendarUse, Config};
use vw_danlon::service::{self, CalendarRead, CalendarStatus, SyncOutcome};

use crate::Screen;
#[cfg(not(target_os = "android"))]
use crate::import::PickButton;
use crate::import::{self, Picked};
use crate::setup::{
    self, AccountStatus, HomeWorkingForm, NewCalendar, Problem, SettingsForm, Setup, Theme,
};

/// Switches the theme now and remembers it in `app.toml`.
pub fn apply_theme(mut setup: Signal<Setup>, theme: Theme) {
    // The theme applies even when it can't be remembered; the next start
    // then names the app settings problem.
    let _ = setup.write().set_theme(theme);
}

#[derive(Clone, PartialEq)]
enum Status {
    None,
    Saved,
    Failed(String),
}

/// "3 added, 12 already present · 1 Sep 2026 – 25 Oct 2026", as `trips sync`
/// prints it.
fn synced_text(s: &SyncOutcome) -> String {
    let mut text = format!(
        "Synced: {} added, {} already present{} · {} – {}",
        s.synced.merged.added,
        s.synced.merged.already,
        s.synced.merged.zero_km_text(),
        s.from.format("%-d %b %Y"),
        s.to.format("%-d %b %Y")
    );
    if s.synced.without_position > 0 {
        text.push_str(&format!(
            " · {} trips without position: turn on position sharing in the app",
            s.synced.without_position
        ));
    }
    text
}

/// A calendar row in Settings: its name, use, how it was read, and the
/// extra calendar's place in the config (removable).
#[derive(Debug, Clone, PartialEq)]
struct CalendarRow {
    name: String,
    usage: Option<CalendarUse>,
    status: String,
    index: Option<usize>,
}

impl CalendarRow {
    fn new(cfg: &Config, c: CalendarStatus) -> CalendarRow {
        let now = crate::screens::now(cfg);
        let events = match c.usage {
            Some(CalendarUse::WordsOnly) => format!("{} match the words", c.events),
            _ if c.events == 1 => "1 event this month".to_string(),
            _ => format!("{} events this month", c.events),
        };
        let status = match c.read {
            CalendarRead::Synced(at) => {
                let at = at.with_timezone(&cfg.timezone);
                let when = if at.date_naive() == now.date_naive() {
                    at.format("%H:%M")
                } else {
                    at.format("%-d %b %H:%M")
                };
                format!("Synced {when} · {events}")
            }
            CalendarRead::File => format!("File · {events}"),
            CalendarRead::NotSynced => "Not synced yet".to_string(),
            CalendarRead::NoLink => {
                "No link on this device: remove it and add it again".to_string()
            }
            CalendarRead::Failed(e) => format!("Can't be read: {e}"),
        };
        CalendarRow {
            name: c.name.unwrap_or_else(|| match c.usage {
                None => "Work".into(),
                Some(_) => "Calendar".into(),
            }),
            usage: c.usage,
            status,
            index: c.index,
        }
    }

    fn chip(&self) -> &'static str {
        match self.usage {
            None => "chip acc",
            Some(CalendarUse::Private) => "chip info",
            Some(CalendarUse::WordsOnly) => "chip",
        }
    }
}

/// The calendars' rows for this month, from the kept copies.
fn calendar_rows(cfg: &Config) -> Vec<CalendarRow> {
    let today = crate::screens::now(cfg).date_naive();
    let month = vw_danlon::month::Month {
        year: chrono::Datelike::year(&today),
        month: chrono::Datelike::month(&today),
    };
    service::calendar_statuses(cfg, month)
        .into_iter()
        .map(|c| CalendarRow::new(cfg, c))
        .collect()
}

/// A Data section job: what it did, as text.
type DataJob = Box<dyn FnOnce(&Setup) -> anyhow::Result<String> + Send>;

#[cfg(not(target_os = "android"))]
const IMPORT_OLD: &str = "Import old data…";

#[cfg(not(target_os = "android"))]
const DATA_NOTE: &str =
    "Backups leave out the calendar links and account token. Importing replaces this app's data.";
#[cfg(target_os = "android")]
const DATA_NOTE: &str = "Without calendar links and token. To import a backup, share its file to vw-danlon; importing replaces this phone's data.";

/// Where the trips file comes from.
#[cfg(not(target_os = "android"))]
const TRIPS_NOTE: &str = "Min Volkswagen: Trips → History → share. Overlapping exports are merged.";
#[cfg(target_os = "android")]
const TRIPS_NOTE: &str =
    "Min Volkswagen: Trips → History → share → vw-danlon. Overlapping exports are merged.";

/// Keeps the backup where the user can take it: a file in Downloads on the
/// desktop; the share sheet on Android.
#[cfg(not(target_os = "android"))]
fn save_backup(name: &str, json: &str) -> anyhow::Result<String> {
    use anyhow::Context;
    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .context("no Downloads folder")?;
    let path = dir.join(name);
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(format!(
        "Saved {}. The calendar links and account token are not in it.",
        path.display()
    ))
}

#[cfg(target_os = "android")]
fn save_backup(name: &str, json: &str) -> anyhow::Result<String> {
    crate::platform::share(name, "application/json", json, None)?;
    Ok("Shared the backup. The calendar links and account token are not in it.".into())
}

/// Reads the data directory again after a change.
fn reload(mut setup: Signal<Setup>) {
    let dir = setup.peek().dir.clone();
    setup.set(Setup::load(Ok(dir)));
}

/// Runs `job` with the config and the secret store off the UI thread.
async fn in_background<T: Send + 'static>(
    setup: Signal<Setup>,
    job: impl FnOnce(&vw_danlon::config::Config, &dyn crate::secrets::SecretStore) -> anyhow::Result<T>
    + Send
    + 'static,
) -> Result<T, String> {
    let (cfg, store) = {
        let s = setup.peek();
        (s.config.clone(), s.secret_store())
    };
    let cfg = cfg.ok_or("the config can't be used yet")?;
    tokio::task::spawn_blocking(move || job(&cfg, store.as_ref()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

#[component]
pub fn Settings(setup: Signal<Setup>, screen: Signal<Screen>) -> Element {
    let initial = SettingsForm::from_config(setup.peek().config.as_ref());
    // The private words as typed; none shows the config's.
    let mut private_words = use_signal(|| None::<String>);
    // Add calendar: open, the link or file, its use, and how adding went.
    let mut adding = use_signal(|| false);
    let mut new_source = use_signal(String::new);
    let mut new_kind = use_signal(|| NewCalendar::Extra(CalendarUse::Private));
    let mut cal_status = use_signal(|| None::<Result<String, String>>);
    let cal_sync = use_context::<Signal<crate::screens::CalendarSync>>();
    // The calendars with their names and last sync, off the UI thread.
    let calendars = use_resource(move || {
        let cfg = setup.read().config.clone();
        let _ = cal_sync.read().version;
        async move {
            let cfg = cfg?;
            tokio::task::spawn_blocking(move || calendar_rows(&cfg))
                .await
                .ok()
        }
    });
    let mut workplace = use_signal(|| initial.workplace.clone());
    let mut home = use_signal(|| initial.home.clone());
    let initial_hw = initial.home_working.clone().unwrap_or(HomeWorkingForm {
        keywords: String::new(),
        at_home_address: true,
    });
    let mut hw_keywords = use_signal(|| initial_hw.keywords.clone());
    let mut hw_at_home = use_signal(|| initial_hw.at_home_address);
    let mut status = use_signal(|| Status::None);
    let mut trips_status = use_signal(|| None::<Result<String, String>>);
    let mut account_status = use_signal(|| None::<Result<String, String>>);
    let mut busy = use_signal(|| false);
    let mut trip_sync = use_context::<Signal<crate::screens::TripSync>>();
    let mut email = use_signal(|| match &setup.peek().account {
        AccountStatus::NoToken { email } => email.clone(),
        _ => setup
            .peek()
            .config
            .as_ref()
            .and_then(|c| c.connected_cars.as_ref())
            .and_then(|c| c.email.clone())
            .unwrap_or_default(),
    });
    let mut password = use_signal(String::new);
    let mut data_status = use_signal(|| None::<Result<String, String>>);
    // Without a fixed workplace: the one the calendar shows, off the UI
    // thread.
    let detected = use_resource(move || {
        let cfg = setup.read().config.clone();
        async move {
            let cfg = cfg?;
            tokio::task::spawn_blocking(move || setup::detected_workplace(&cfg))
                .await
                .ok()
                .flatten()
        }
    });
    let mut workplace_status = use_signal(|| None::<Result<String, String>>);

    let s = setup.read();
    let store = s.secret_store().describe();
    let no_calendar = s.problems.contains(&Problem::NoCalendar);
    let has_calendar = s.config.as_ref().is_some_and(|c| c.calendar.is_some());
    let words_shown = private_words().unwrap_or_else(|| {
        SettingsForm::from_config(s.config.as_ref())
            .private_words
            .unwrap_or_default()
    });
    let has_mileage = s.config.as_ref().is_some_and(|c| c.mileage.is_some());
    // The yearly rates, newest first: (year, low, high, threshold km).
    let mut rates: Vec<(i32, String, String, String)> = s
        .config
        .as_ref()
        .and_then(|c| c.mileage.as_ref())
        .map(|m| {
            m.rates
                .iter()
                .map(|r| {
                    (
                        r.year,
                        crate::screens::hours(r.low),
                        crate::screens::hours(r.high),
                        crate::screens::kroner(r.threshold_km)
                            .trim_end_matches(",00 kr")
                            .to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    rates.sort_by_key(|r| std::cmp::Reverse(r.0));
    let file_mode = s.settings.secret_file.is_some();
    let theme = s.settings.theme;
    let trips = match &s.trips {
        Some(t) if t.count > 0 => format!(
            "{} trips · {} – {}",
            t.count,
            t.first
                .map(|d| d.format("%-d %b %Y").to_string())
                .unwrap_or_default(),
            t.last
                .map(|d| d.format("%-d %b %Y").to_string())
                .unwrap_or_default()
        ),
        _ => "No trips imported yet.".to_string(),
    };
    let connected = matches!(s.account, AccountStatus::Connected { .. });
    let has_config = s.config.is_some();
    let account = match &s.account {
        AccountStatus::NotSetUp => "Not set up".to_string(),
        AccountStatus::Connected { email, last_sync } => match last_sync {
            Some(t) => format!("Connected as {email} · last sync {t}"),
            None => format!("Connected as {email} · not synced yet"),
        },
        AccountStatus::NoToken { email } => {
            format!("{email}: no device token in {store}")
        }
        AccountStatus::Unknown(e) => format!("Status unknown: {e}"),
    };
    let problems: Vec<String> = s.problems.iter().map(|p| p.to_string()).collect();
    let mut stored = format!("Stored in {} · {trips}", vw_danlon::db::FILE_NAME);
    if let Some(m) = &s.migrated {
        stored.push_str(&format!(
            " · moved from {} on {}",
            m.files.join(" and "),
            m.at.format("%-d %b %Y")
        ));
    }
    drop(s);

    let save = move |_| {
        let form = SettingsForm {
            workplace: workplace(),
            home: home(),
            home_working: Some(HomeWorkingForm {
                keywords: hw_keywords(),
                at_home_address: hw_at_home(),
            }),
            private_words: private_words(),
        };
        let result = setup::save(&setup.peek(), &form);
        match result {
            Ok(()) => {
                let dir = setup.peek().dir.clone();
                setup.set(Setup::load(Ok(dir)));
                private_words.set(None);
                status.set(Status::Saved);
            }
            Err(e) => status.set(Status::Failed(format!("{e:#}"))),
        }
    };

    let mut open_add = move |source: String| {
        new_source.set(source);
        new_kind.set(if has_calendar {
            NewCalendar::Extra(CalendarUse::Private)
        } else {
            NewCalendar::WorkingHours
        });
        cal_status.set(None);
        adding.set(true);
    };
    // Fetches the calendar once to check it, then keeps it.
    let add_calendar = move |_| {
        let (source, kind) = (new_source(), new_kind());
        busy.set(true);
        cal_status.set(Some(Ok("Checking the calendar…".into())));
        let dir = setup.peek().dir.clone();
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                setup::add_calendar(&Setup::load(Ok(dir)), &source, kind)
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| format!("Not added: {e:#}")));
            if result.is_ok() {
                adding.set(false);
                new_source.set(String::new());
                reload(setup);
            }
            cal_status.set(Some(result));
            busy.set(false);
        });
    };
    let mut remove_calendar = move |index: usize, name: String| {
        let result = setup::remove_calendar(&setup.peek(), index);
        if result.is_ok() {
            reload(setup);
        }
        cal_status.set(Some(
            result
                .map(|()| format!("Removed {name}"))
                .map_err(|e| format!("Not removed: {e:#}")),
        ));
    };

    let on_picked = move |picked: Result<Picked, String>| {
        let picked = match picked {
            Ok(p) => p,
            Err(e) => {
                trips_status.set(Some(Err(e)));
                return;
            }
        };
        let Some(cfg) = setup.peek().config.clone() else {
            trips_status.set(Some(Err("the config can't be used yet".into())));
            return;
        };
        busy.set(true);
        trips_status.set(Some(Ok(format!("Importing {}…", picked.name))));
        spawn(async move {
            let result = import::import(cfg, picked).await;
            if result.is_ok() {
                reload(setup);
            }
            trips_status.set(Some(result));
            busy.set(false);
        });
    };
    let sync_now = move |_| {
        if trip_sync.peek().syncing {
            return;
        }
        busy.set(true);
        trip_sync.write().syncing = true;
        account_status.set(Some(Ok("Syncing…".into())));
        spawn(async move {
            let result = in_background(setup, |cfg, store| {
                let today = crate::screens::now(cfg).date_naive();
                setup::sync_account(cfg, store, today)
            })
            .await;
            trip_sync.write().syncing = false;
            reload(setup);
            account_status.set(Some(result.map(|s| synced_text(&s))));
            busy.set(false);
        });
    };
    let connect = move |_| {
        let (address, secret) = (email(), password());
        busy.set(true);
        account_status.set(Some(Ok("Connecting…".into())));
        spawn(async move {
            let result = in_background(setup, move |cfg, store| {
                setup::connect_account(cfg, store, &address, &secret)
            })
            .await;
            password.set(String::new());
            reload(setup);
            account_status.set(Some(result.map(|()| "Connected".into())));
            busy.set(false);
        });
    };
    let disconnect = move |_| {
        busy.set(true);
        spawn(async move {
            let result = in_background(setup, setup::disconnect_account).await;
            reload(setup);
            account_status.set(Some(
                result.map(|()| "Disconnected; the device token is deleted".into()),
            ));
            busy.set(false);
        });
    };

    // Runs a Data job off the UI thread, then reads the folder again.
    let mut data_job = move |busy_text: &str, job: DataJob| {
        busy.set(true);
        data_status.set(Some(Ok(busy_text.to_string())));
        let dir = setup.peek().dir.clone();
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let setup = Setup::load(Ok(dir));
                job(&setup)
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| format!("Not done: {e:#}")));
            reload(setup);
            data_status.set(Some(result));
            busy.set(false);
        });
    };
    let on_backup = move |picked: Result<Picked, String>| match picked {
        Ok(p) => data_job(
            "Importing the backup…",
            Box::new(move |s| setup::import_backup(s, &p.name, &p.bytes)),
        ),
        Err(e) => data_status.set(Some(Err(e))),
    };
    // A file shared to the app (Android): a backup waits for Import backup
    // (it replaces the data), anything else is imported as trips. A shared
    // calendar link opens Add calendar with it.
    let mut shared = use_context::<Signal<Option<import::Shared>>>();
    let mut shared_backup = use_signal(|| None::<Picked>);
    use_effect(move || {
        if shared.read().is_none() {
            return;
        }
        let picked = match shared.write().take() {
            Some(import::Shared::File(picked)) => picked,
            Some(import::Shared::Link(link)) => {
                let mut open_add = open_add;
                open_add(link);
                return;
            }
            None => return,
        };
        let mut on_picked = on_picked;
        match picked {
            Ok(p) if vw_danlon::backup::Backup::parse(&p.name, &p.bytes).is_ok() => {
                shared_backup.set(Some(p));
            }
            other => on_picked(other),
        }
    });
    #[cfg(not(target_os = "android"))]
    let on_folder = move |folder: std::path::PathBuf| {
        data_job(
            "Importing…",
            Box::new(move |s| setup::import_old_data(s, &folder)),
        )
    };
    let export = move |_| {
        let result =
            setup::export_backup(&setup.peek()).and_then(|(name, json)| save_backup(&name, &json));
        data_status.set(Some(result.map_err(|e| format!("Not exported: {e:#}"))));
    };

    #[cfg(not(target_os = "android"))]
    let folder_button = rsx! {
        crate::import::FolderButton { label: IMPORT_OLD, class: "small-btn", on_picked: on_folder }
    };
    #[cfg(not(target_os = "android"))]
    let folder_busy = rsx! {
        button { class: "small-btn", disabled: true, "{IMPORT_OLD}" }
    };
    #[cfg(target_os = "android")]
    let (folder_button, folder_busy) = (rsx! {}, rsx! {});

    // Android has no file picker: files are shared to the app.
    #[cfg(not(target_os = "android"))]
    let (trips_button, backup_button) = (
        rsx! {
            PickButton { label: "Import trips file", class: "small-btn", accept: ".xlsx,.csv,.json", on_picked }
        },
        rsx! {
            PickButton { label: "Import backup…", class: "small-btn", accept: ".json", on_picked: on_backup }
        },
    );
    #[cfg(target_os = "android")]
    let (trips_button, backup_button) = (rsx! {}, rsx! {});

    let toggle_file = move |e: Event<FormData>| {
        let on = e.checked();
        let result = setup.write().set_secret_file(on);
        match result {
            Ok(()) => {
                let dir = setup.peek().dir.clone();
                setup.set(Setup::load(Ok(dir)));
                status.set(Status::None);
            }
            Err(e) => status.set(Status::Failed(format!("{e:#}"))),
        }
    };

    rsx! {
        if !problems.is_empty() {
            div { class: "alert", role: "alert",
                span { "aria-hidden": "true", style: "font-weight: 700", "!" }
                div {
                    b { "Setup needed:" }
                    ul {
                        for p in problems {
                            li { "{p}" }
                        }
                    }
                }
            }
        }

        section { class: "card cal-card", "aria-labelledby": "s-cal",
            h2 { id: "s-cal", "Calendars" }
            match calendars.read().clone().flatten() {
                Some(rows) if !rows.is_empty() => rsx! {
                    ul { class: "cal-list",
                        for row in rows {
                            li {
                                span { class: "cal-name",
                                    b { "{row.name}" }
                                    span { class: row.chip(), "{setup::use_label(row.usage)}" }
                                }
                                if let Some(index) = row.index {
                                    button {
                                        class: "cal-remove",
                                        "aria-label": "Remove {row.name}",
                                        disabled: busy(),
                                        onclick: {
                                            let name = row.name.clone();
                                            move |_| remove_calendar(index, name.clone())
                                        },
                                        "Remove"
                                    }
                                }
                                span { class: "note", "{row.status}" }
                            }
                        }
                    }
                },
                _ => rsx! {},
            }
            if adding() {
                div { class: "cal-add",
                    label { class: "label", r#for: "cal-new", "Proton share link or .ics file" }
                    input {
                        id: "cal-new",
                        r#type: "url",
                        class: "mono",
                        placeholder: "https://calendar.proton.me/api/calendar/v1/url/…",
                        autocomplete: "off",
                        value: "{new_source}",
                        oninput: move |e| new_source.set(e.value()),
                    }
                    span { class: "note",
                        "In Proton Calendar on the web: the calendar's Settings → Share with anyone → Create link."
                        if cfg!(target_os = "android") {
                            " Or share the link to vw-danlon."
                        }
                    }
                    span { class: "label", id: "cal-use", "Use" }
                    div { class: "segmented", role: "radiogroup", "aria-labelledby": "cal-use",
                        for k in NewCalendar::ALL {
                            button {
                                role: "radio",
                                "aria-checked": if k == new_kind() { "true" } else { "false" },
                                onclick: move |_| new_kind.set(k),
                                "{k.label()}"
                            }
                        }
                    }
                    span { class: "sub",
                        if new_kind() == NewCalendar::WorkingHours {
                            if has_calendar {
                                "Replaces the work calendar: planned hours, days off and vacation."
                            } else {
                                "Planned hours, days off and vacation."
                            }
                        } else {
                            "Private: every timed event in it is private time. Words only: only titles with the private words below."
                        }
                    }
                    div { class: "cal-add-actions",
                        button {
                            class: "small-btn",
                            onclick: move |_| {
                                adding.set(false);
                                cal_status.set(None);
                            },
                            "Cancel"
                        }
                        button {
                            class: "small-btn primary-sm",
                            disabled: busy() || new_source().trim().is_empty(),
                            onclick: add_calendar,
                            "Add"
                        }
                    }
                }
            } else {
                button {
                    class: if no_calendar { "cal-add-btn needs" } else { "cal-add-btn" },
                    onclick: move |_| open_add(String::new()),
                    crate::Icon { d: "M12 5v14M5 12h14", size: 16 }
                    "Add calendar"
                }
            }
            match cal_status() {
                None => rsx! {},
                Some(Ok(t)) => rsx! { span { class: "status-ok", role: "status", "{t}" } },
                Some(Err(e)) => rsx! { span { class: "status-bad", role: "status", "{e}" } },
            }
            span { class: "note",
                "The links are kept in {store}. One calendar is for working hours."
            }
        }

        section { class: "card", "aria-labelledby": "s-priv",
            h2 { id: "s-priv", "Private appointments" }
            span { class: "sub", "Not company time." }
            div { class: "field",
                label { r#for: "priv-kw", "Event titles containing" }
                input {
                    id: "priv-kw",
                    r#type: "text",
                    disabled: !has_calendar,
                    value: "{words_shown}",
                    oninput: move |e| private_words.set(Some(e.value())),
                }
                span { class: "note",
                    if has_calendar {
                        "Whole words, comma-separated. Checked in every calendar."
                    } else {
                        "Add the work calendar first."
                    }
                }
            }
        }

        section { class: "card", "aria-labelledby": "s-addr",
            h2 { id: "s-addr", "Addresses" }
            div { class: "grid2",
                div { class: "field",
                    label { r#for: "a-work", "Fixed workplace" }
                    input {
                        id: "a-work",
                        r#type: "text",
                        value: "{workplace}",
                        oninput: move |e| workplace.set(e.value()),
                    }
                    span { class: "note", "Always counts as work" }
                    match detected.read().clone().flatten() {
                        Some(Some(found)) => rsx! {
                            div { class: "detected", role: "status",
                                span { class: "grow",
                                    "From the calendar: "
                                    b { "{found}" }
                                }
                                button {
                                    class: "small-btn primary-sm",
                                    onclick: move |_| {
                                        let result = setup::save_workplace(&setup.peek(), &found);
                                        if result.is_ok() {
                                            workplace.set(found.clone());
                                            reload(setup);
                                        }
                                        workplace_status
                                            .set(Some(result.map(|()| "Saved".into()).map_err(|e| format!("{e:#}"))));
                                    },
                                    "Save"
                                }
                            }
                        },
                        Some(None) => rsx! {
                            div { class: "detected warn", role: "status",
                                "No fixed workplace: every work address counts as a client visit."
                            }
                        },
                        None => rsx! {},
                    }
                    match workplace_status() {
                        Some(Ok(t)) => rsx! { span { class: "note good", "{t}" } },
                        Some(Err(e)) => rsx! { span { class: "note bad", "{e}" } },
                        None => rsx! {},
                    }
                }
                div { class: "field",
                    label { r#for: "a-home", "Home" }
                    input {
                        id: "a-home",
                        r#type: "text",
                        disabled: !has_mileage,
                        value: "{home}",
                        oninput: move |e| home.set(e.value()),
                    }
                    span { class: "note",
                        if has_mileage {
                            "Home ↔ workplace is commuting"
                        } else {
                            "Needs the mileage logbook. "
                            button {
                                class: "link-btn",
                                onclick: move |_| screen.set(Screen::Mileage),
                                "Set it up"
                            }
                        }
                    }
                }
            }
        }

        section { class: "card", "aria-labelledby": "s-home",
            h2 { id: "s-home", "Working from home" }
            span { class: "sub",
                "Counts the planned hours as worked."
            }
            div { class: "grid2",
                div { class: "field",
                    label { r#for: "h-kw", "Event titles containing" }
                    input {
                        id: "h-kw",
                        r#type: "text",
                        value: "{hw_keywords}",
                        oninput: move |e| hw_keywords.set(e.value()),
                    }
                    span { class: "note", "Whole words, comma-separated" }
                }
                label { class: "check hw-check",
                    input {
                        r#type: "checkbox",
                        checked: hw_at_home(),
                        onchange: move |e| hw_at_home.set(e.checked()),
                    }
                    span { "An event at the home address is home working" }
                }
            }
        }

        section { class: "card", "aria-labelledby": "s-rules",
            h2 { id: "s-rules", "Mileage rules" }
            span { class: "sub",
                "All driving is logged; Skat’s rules decide what is reimbursed."
            }
            ul { class: "rules",
                li {
                    b { "Client visits" }
                    ": reimbursed · home ↔ client 60 days per address a year"
                }
                li {
                    b { "Home ↔ fixed workplace" }
                    ": commute, not reimbursed"
                }
                li {
                    b { "Errands on the way home" }
                    ": private"
                }
                li {
                    b { "Between workplaces" }
                    ": reimbursed"
                }
            }
            span { class: "note",
                "LL § 9 B · fixed by Skat."
            }
            if rates.is_empty() {
                span { class: "note",
                    "No rates: the mileage logbook isn't set up. "
                    button {
                        class: "link-btn",
                        onclick: move |_| screen.set(Screen::Mileage),
                        "Set it up"
                    }
                }
            } else {
                table { class: "days rates",
                    thead {
                        tr {
                            th { scope: "col", "Year" }
                            th { scope: "col", class: "r", "kr/km" }
                            th { scope: "col", class: "r", "up to" }
                            th { scope: "col", class: "r", "then kr/km" }
                        }
                    }
                    tbody {
                        for (year , low , high , threshold) in rates {
                            tr {
                                td { "{year}" }
                                td { class: "r", "{low}" }
                                td { class: "r", "{threshold} km" }
                                td { class: "r", "{high}" }
                            }
                        }
                    }
                }
                span { class: "note",
                    "Skatterådet’s rates, in config.toml"
                }
            }
        }

        section { class: "card", "aria-labelledby": "s-trips",
            h2 { id: "s-trips", "Trips source" }
            div { class: "box",
                b { "Min Volkswagen export (.xlsx) or CSV/JSON file" }
                span { class: "sub", "{trips}" }
                span { class: "note", "{TRIPS_NOTE}" }
                if !cfg!(target_os = "android") {
                    div { class: "box-actions",
                        if has_config && !busy() {
                            {trips_button}
                        } else {
                            button { class: "small-btn", disabled: true, "Import trips file" }
                        }
                    }
                }
                match trips_status() {
                    None => rsx! {},
                    Some(Ok(t)) => rsx! { span { class: "status-ok", role: "status", "{t}" } },
                    Some(Err(e)) => rsx! { span { class: "status-bad", role: "status", "{e}" } },
                }
            }
            div { class: "box",
                div { class: "box-row",
                    span { class: "grow",
                        b { "Sync from your Min Volkswagen account" }
                        span { class: "note", " optional · unofficial" }
                        br {}
                        span { class: "sub",
                            span { class: if connected { "dot" } else { "dot warn" }, "aria-hidden": "true" }
                            " {account}"
                        }
                    }
                    if connected {
                        button {
                            class: "small-btn",
                            disabled: busy() || trip_sync.read().syncing,
                            onclick: sync_now,
                            if trip_sync.read().syncing { "Syncing…" } else { "Sync now" }
                        }
                        button { class: "small-btn", disabled: busy(), onclick: disconnect, "Disconnect" }
                    }
                }
                if !connected && has_config {
                    div { class: "grid2",
                        div { class: "field",
                            label { r#for: "acc-email", "Account email" }
                            input {
                                id: "acc-email",
                                r#type: "text",
                                autocomplete: "username",
                                value: "{email}",
                                oninput: move |e| email.set(e.value()),
                            }
                        }
                        div { class: "field",
                            label { r#for: "acc-pw", "Password (used once, never stored)" }
                            input {
                                id: "acc-pw",
                                r#type: "password",
                                autocomplete: "current-password",
                                value: "{password}",
                                oninput: move |e| password.set(e.value()),
                            }
                        }
                    }
                    div { class: "box-actions",
                        button { class: "small-btn", disabled: busy(), onclick: connect, "Connect" }
                    }
                }
                match account_status() {
                    None => rsx! {},
                    Some(Ok(t)) => rsx! { span { class: "status-ok", role: "status", "{t}" } },
                    Some(Err(e)) => rsx! { span { class: "status-bad", role: "status", "{e}" } },
                }
                span { class: "note",
                    "Auto-syncs every 15 min · unofficial, may stop working · password never stored"
                }
            }
        }

        section { class: "card", "aria-labelledby": "s-secrets",
            h2 { id: "s-secrets", "Secrets" }
            label { class: "check",
                input {
                    r#type: "checkbox",
                    checked: file_mode,
                    onchange: toggle_file,
                }
                span {
                    "Keep secrets in a file readable only by you"
                    br {}
                    span { class: "note",
                        "For systems without a keychain."
                    }
                }
            }
            span { class: "sub", "Secrets are kept in {store}." }
        }

        section { class: "card", "aria-labelledby": "s-data",
            h2 { id: "s-data", "Data" }
            span { class: "sub", "{stored}" }
            div { class: "box-actions",
                if busy() {
                    {folder_busy}
                    button { class: "small-btn", disabled: true, "Export backup…" }
                    button { class: "small-btn", disabled: true, "Import backup…" }
                } else {
                    {folder_button}
                    button { class: "small-btn", disabled: !has_config, onclick: export, "Export backup…" }
                    {backup_button}
                }
            }
            if let Some(p) = shared_backup() {
                div { class: "box",
                    b { "{p.name} is a backup" }
                    span { class: "note", "Importing it replaces this phone's data." }
                    div { class: "box-actions",
                        button {
                            class: "small-btn",
                            disabled: busy(),
                            onclick: move |_| {
                                if let Some(p) = shared_backup.take() {
                                    let mut on_backup = on_backup;
                                    on_backup(Ok(p));
                                }
                            },
                            "Import backup"
                        }
                        button { class: "small-btn", onclick: move |_| shared_backup.set(None), "Cancel" }
                    }
                }
            }
            match data_status() {
                None => rsx! {},
                Some(Ok(t)) => rsx! { span { class: "status-ok", role: "status", "{t}" } },
                Some(Err(e)) => rsx! { span { class: "status-bad", role: "status", "{e}" } },
            }
            span { class: "note", "{DATA_NOTE}" }
        }

        section { class: "card", "aria-labelledby": "s-theme",
            h2 { id: "s-theme", "Theme" }
            div { class: "segmented", role: "radiogroup", "aria-labelledby": "s-theme",
                for t in Theme::ALL {
                    button {
                        role: "radio",
                        "aria-checked": if t == theme { "true" } else { "false" },
                        onclick: move |_| apply_theme(setup, t),
                        "{t.label()}"
                    }
                }
            }
        }

        div { class: "actions",
            match status() {
                Status::None => rsx! {},
                Status::Saved => rsx! { span { class: "status-ok", role: "status", "Saved" } },
                Status::Failed(e) => rsx! { span { class: "status-bad", role: "status", "{e}" } },
            }
            button { class: "primary", onclick: save, "Save settings" }
        }
    }
}
