//! The Settings screen: what is missing, the calendar link (kept in the
//! secret store), the addresses, the trips source, where secrets are kept,
//! and the theme.

use dioxus::prelude::*;

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

#[component]
pub fn Settings(setup: Signal<Setup>) -> Element {
    let initial = SettingsForm::from_config(setup.peek().config.as_ref());
    let mut link = use_signal(String::new);
    let mut workplace = use_signal(|| initial.workplace.clone());
    let mut home = use_signal(|| initial.home.clone());
    let mut status = use_signal(|| Status::None);

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
                            "Part of the mileage logbook ([mileage] in config.toml), which isn't set up"
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
                span { class: "note", "In Min Volkswagen: Ture → Historik → share icon. Overlapping exports are merged." }
            }
            div { class: "box",
                span {
                    b { "Sync from your Min Volkswagen account" }
                    span { class: "note", " optional · unofficial" }
                }
                span { class: "sub", "{account}" }
                span { class: "note", "The device token is kept in {store}; your password is never stored." }
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
