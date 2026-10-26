//! The Settings screen: what is missing, the calendar link (kept in the
//! secret store), the addresses, the trips source, where secrets are kept,
//! and the theme.

use dioxus::prelude::*;
use vw_danlon::service::SyncOutcome;

use crate::Screen;
use crate::import::{self, PickButton, Picked};
use crate::setup::{self, AccountStatus, HomeWorkingForm, Problem, SettingsForm, Setup, Theme};

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

/// A Data section job: what it did, as text.
type DataJob = Box<dyn FnOnce(&Setup) -> anyhow::Result<String> + Send>;

#[cfg(not(target_os = "android"))]
const IMPORT_OLD: &str = "Import old data…";

#[cfg(not(target_os = "android"))]
const DATA_NOTE: &str = "Import old data: pick the folder you used with the command-line tool (config.toml, trips.json, state.json). Its settings, trips and state replace this app's; then trips.json and state.json are deleted there, config.toml stays. A backup holds settings, trips, ticks, lunch, private and home-working time — not the calendar link or account token, which you enter again on the other device. Importing a backup replaces this app's data.";
#[cfg(target_os = "android")]
const DATA_NOTE: &str = "Move your data from the computer: export a backup there, then import it here; it replaces the data on this phone. The calendar link and account token are not in the backup.";

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
        "Saved {}. The calendar link and account token are not in it.",
        path.display()
    ))
}

#[cfg(target_os = "android")]
fn save_backup(name: &str, json: &str) -> anyhow::Result<String> {
    crate::platform::share(name, "application/json", json, None)?;
    Ok("Shared the backup. The calendar link and account token are not in it.".into())
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
    let mut link = use_signal(String::new);
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
    let link_hint = if s.link_in_store {
        "A link is saved. Paste a new one to replace it."
    } else if s.config.as_ref().is_some_and(|c| c.calendar.is_some()) {
        "The config file names a calendar. Paste the link here to keep it in the secret store instead."
    } else {
        "https://calendar.proton.me/…"
    };
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
            calendar_link: link(),
            workplace: workplace(),
            home: home(),
            home_working: Some(HomeWorkingForm {
                keywords: hw_keywords(),
                at_home_address: hw_at_home(),
            }),
        };
        let result = setup::save(&setup.peek(), &form);
        match result {
            Ok(()) => {
                let dir = setup.peek().dir.clone();
                setup.set(Setup::load(Ok(dir)));
                link.set(String::new());
                status.set(Status::Saved);
            }
            Err(e) => status.set(Status::Failed(format!("{e:#}"))),
        }
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

        section { class: "card", "aria-labelledby": "s-cal",
            h2 { id: "s-cal", "Work calendar" }
            label { class: "label", r#for: "cal-src", "Proton share link or .ics file" }
            input {
                id: "cal-src",
                r#type: "url",
                class: if no_calendar { "mono needs" } else { "mono" },
                placeholder: "{link_hint}",
                autocomplete: "off",
                value: "{link}",
                oninput: move |e| link.set(e.value()),
            }
            span { class: "note",
                "Timed events are working hours at the event’s address. All-day events are days off. The link holds the key to your calendar, so it is kept in {store}, never in the config file."
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
                    span { class: "note", "Always counts as work, also when a colleague drives on" }
                    match detected.read().clone().flatten() {
                        Some(Some(found)) => rsx! {
                            div { class: "detected", role: "status",
                                span { class: "grow",
                                    "Detected from your calendar: "
                                    b { "{found}" }
                                    ". Used until you save a workplace."
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
                                "No fixed workplace set and none found in the calendar: every work event with an address counts as a client visit, so drives there are reimbursed."
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
                            "Home ↔ workplace is commuting, never reimbursed"
                        } else {
                            "Part of the mileage logbook, which isn't set up. "
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
                "A work event titled like this, or placed at your home address, counts its planned hours as worked (minus lunch and private time). The Start/Stop timer on Month counts the time you measure instead. Days at home are not commuting days."
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
                    span { class: "note", "Whole words, separated by commas" }
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
                "All driving is logged. Only trips that count under Skat’s rules for tax-free kørselsgodtgørelse are reimbursed; the rest are listed as “not reimbursable” with the reason."
            }
            ul { class: "rules",
                li {
                    b { "Client visits" }
                    " (work events at other addresses): reimbursed, and the time is work hours. Home ↔ client counts for 60 working days per address within 12 months; work ↔ client always."
                }
                li {
                    b { "Home ↔ fixed workplace" }
                    ": private (commute). Logged as driving, never reimbursed, not work hours."
                }
                li {
                    b { "Errands on the way home" }
                    ": private. After a client visit only the direct km home are reimbursed."
                }
                li {
                    b { "Between workplaces" }
                    " during the day: reimbursed (tick Business on the trip if the tool can’t see it)."
                }
            }
            span { class: "note",
                "These follow Skat (LL § 9 B, skat.dk) and can’t be switched off. Kørselsfradrag (LL § 9 C) is separate; the Mileage screen shows the estimate. Nothing is sent to Skat; godtgørelse reaches Skat through Danløn."
            }
            if rates.is_empty() {
                span { class: "note",
                    "No yearly rates: the mileage logbook isn't set up. "
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
                    "Rates are Skatterådet’s for each year, in [[mileage.rates]] in config.toml; add next year’s when published."
                }
            }
        }

        section { class: "card", "aria-labelledby": "s-trips",
            h2 { id: "s-trips", "Trips source" }
            div { class: "box",
                b { "Min Volkswagen export (.xlsx) or CSV/JSON file" }
                span { class: "sub", "{trips}" }
                span { class: "note",
                    "In Min Volkswagen: Ture → Historik → share icon. Exports can overlap: a trip already imported (same start time and odometer) is skipped. A CSV/JSON file has start, end, distance_km, from, to, purpose."
                }
                div { class: "box-actions",
                    if has_config && !busy() {
                        PickButton {
                            label: "Import trips file",
                            class: "small-btn",
                            accept: ".xlsx,.csv,.json",
                            on_picked,
                        }
                    } else {
                        button { class: "small-btn", disabled: true, "Import trips file" }
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
                    "Syncs by itself when the app opens or comes back, and every 15 minutes while it is open; Sync now fetches at once. Uses a device token kept in {store}; your password is never stored. Uses the app’s own unofficial access, so it can stop working when the app changes; the export above always works."
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
                        "For systems without a keychain. Secrets already saved stay where they are."
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
                    PickButton {
                        label: "Import backup…",
                        class: "small-btn",
                        accept: ".json",
                        on_picked: on_backup,
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
