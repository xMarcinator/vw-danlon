//! Importing a trips file: the Min Volkswagen .xlsx export, or a CSV/JSON
//! trips file. Desktop picks it with the native file dialog (Dioxus opens
//! one for a file input). Android has no picker: the file is shared to the
//! app ("Share to vw-danlon"), see [`crate::platform::take_shared_file`].

#[cfg(not(target_os = "android"))]
use dioxus::prelude::*;
use vw_danlon::config::Config;
use vw_danlon::trip_store;

#[cfg(not(target_os = "android"))]
use crate::Icon;

/// A picked file's name and content.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// What was shared to the app (Android): a file, or why it couldn't be
/// read, which Settings imports; or a calendar link, which Settings offers
/// to add.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub enum Shared {
    File(Result<Picked, String>),
    Link(String),
}

/// The calendar link in text shared to the app: its first http(s) or
/// webcal link, e.g. from "Share" on a Proton Calendar link.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn shared_link(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| matches!(c, '<' | '>' | '"' | '\'' | '(' | ')')))
        .find(|w| {
            ["https://", "http://", "webcal://"].iter().any(|p| {
                w.len() > p.len() && w.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p))
            })
        })
        .map(String::from)
}

#[cfg(not(target_os = "android"))]
const UPLOAD_ICON: &str = "M4 14v5a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-5M12 15V3M7 8l5-5 5 5";

/// Merges the picked file into the trip store off the UI thread; the text
/// says what was added, or why nothing was.
pub async fn import(cfg: Config, picked: Picked) -> Result<String, String> {
    let name = picked.name.clone();
    tokio::task::spawn_blocking(move || {
        trip_store::import_bytes(&cfg.database, &picked.name, &picked.bytes, cfg.timezone)
    })
    .await
    .map_err(|e| e.to_string())?
    .map(|(merged, period)| {
        let period = period
            .map(|(first, last)| {
                format!(
                    " · {} – {}",
                    first.format("%-d %b %Y"),
                    last.format("%-d %b %Y")
                )
            })
            .unwrap_or_default();
        format!(
            "{name}: {} added, {} already imported{}{period}",
            merged.added,
            merged.already,
            merged.zero_km_text()
        )
    })
    .map_err(|e| format!("Not imported: {e:#}"))
}

/// The name a shared file is imported under: its display name, with the
/// extension its MIME type gives when the name has none the import knows
/// (the import tells the formats apart by the extension).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn shared_name(display: Option<&str>, mime: Option<&str>) -> String {
    let name = display
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or("shared file");
    let known = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["xlsx", "csv", "json"].contains(&e.to_ascii_lowercase().as_str()));
    let ext = match mime.unwrap_or_default() {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "text/csv" | "text/comma-separated-values" => "csv",
        "application/json" => "json",
        _ => "",
    };
    if known || ext.is_empty() {
        name.to_string()
    } else {
        format!("{name}.{ext}")
    }
}

#[cfg(not(target_os = "android"))]
const FOLDER_ICON: &str =
    "M3 6a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z";

/// A button that picks a folder with the desktop's folder dialog.
#[cfg(not(target_os = "android"))]
#[component]
pub fn FolderButton(
    label: String,
    class: String,
    on_picked: EventHandler<std::path::PathBuf>,
) -> Element {
    rsx! {
        label { class: "{class}",
            input {
                class: "visually-hidden",
                r#type: "file",
                directory: true,
                onchange: move |e: Event<FormData>| {
                    if let Some(folder) = e.files().into_iter().next() {
                        on_picked.call(folder.path());
                    }
                },
            }
            Icon { d: FOLDER_ICON, size: 16 }
            "{label}"
        }
    }
}

/// A button that picks one file; `accept` lists its extensions (the
/// desktop dialog's filter).
#[cfg(not(target_os = "android"))]
#[component]
pub fn PickButton(
    label: String,
    class: String,
    accept: String,
    on_picked: EventHandler<Result<Picked, String>>,
) -> Element {
    rsx! {
        label { class: "{class}",
            input {
                class: "visually-hidden",
                r#type: "file",
                accept: "{accept}",
                onchange: move |e: Event<FormData>| {
                    let Some(file) = e.files().into_iter().next() else {
                        return;
                    };
                    spawn(async move {
                        let name = file.name();
                        let picked = file
                            .read_bytes()
                            .await
                            .map(|b| Picked { name: name.clone(), bytes: b.to_vec() })
                            .map_err(|e| format!("{name} can't be read: {e}"));
                        on_picked.call(picked);
                    });
                },
            }
            Icon { d: UPLOAD_ICON, size: 16 }
            "{label}"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{shared_link, shared_name};

    #[test]
    fn a_shared_file_is_named_for_the_import() {
        let xlsx = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
        for (display, mime, want) in [
            (Some("Trips.xlsx"), Some(xlsx), "Trips.xlsx"),
            (Some("Trips.XLSX"), None, "Trips.XLSX"),
            (Some("export"), Some(xlsx), "export.xlsx"),
            (Some("trips"), Some("text/csv"), "trips.csv"),
            (Some("backup"), Some("application/json"), "backup.json"),
            (None, Some("text/csv"), "shared file.csv"),
            (Some("  "), None, "shared file"),
            (Some("notes.txt"), Some("text/plain"), "notes.txt"),
        ] {
            assert_eq!(shared_name(display, mime), want, "{display:?} {mime:?}");
        }
    }

    #[test]
    fn a_shared_calendar_link_is_found_in_the_text() {
        let link = "https://calendar.proton.me/api/calendar/v1/url/KEY/calendar.ics?CacheKey=x";
        for (text, want) in [
            (link.to_string(), Some(link)),
            (format!("Kalender: <{link}>\n"), Some(link)),
            (
                "webcal://cal.example/x.ics".into(),
                Some("webcal://cal.example/x.ics"),
            ),
            ("Møde på æøå https:// og ingen link".into(), None),
            ("".into(), None),
        ] {
            assert_eq!(shared_link(&text).as_deref(), want, "{text:?}");
        }
    }
}
