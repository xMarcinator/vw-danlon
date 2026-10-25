//! The Settings screen: what is missing, the calendar link (kept in the
//! secret store), the addresses, the trips source, where secrets are kept,
//! and the theme.

use dioxus::prelude::*;
use vw_danlon::service::SyncOutcome;

use crate::Screen;
use crate::import::{self, PickButton, Picked};
use crate::setup::{self, AccountStatus, Problem, SettingsForm, Setup, Theme};

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
        "Synced: {} added, {} already present · {} – {}",
        s.synced.merged.added,
        s.synced.merged.already,
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
    let mut status = use_signal(|| Status::None);
    let mut trips_status = use_signal(|| None::<Result<String, String>>);
    let mut account_status = use_signal(|| None::<Result<String, String>>);
    let mut busy = use_signal(|| false);
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

    let s = setup.read();
    let store = s.secret_store().describe();
    let no_calendar = s.problems.contains(&Problem::NoCalendar);
    let has_mileage = s.config.as_ref().is_some_and(|c| c.mileage.is_some());
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
    drop(s);

    let save = move |_| {
        let form = SettingsForm {
            calendar_link: link(),
            workplace: workplace(),
            home: home(),
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
        busy.set(true);
        account_status.set(Some(Ok("Syncing…".into())));
        spawn(async move {
            let result = in_background(setup, |cfg, store| {
                let today = crate::screens::now(cfg).date_naive();
                setup::sync_account(cfg, store, today)
            })
            .await;
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
                        button { class: "small-btn", disabled: busy(), onclick: sync_now, "Sync now" }
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
                    "Fetches your trips with a device token kept in {store}; your password is never stored. Uses the app’s own unofficial access, so it can stop working when the app changes; the export above always works."
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
