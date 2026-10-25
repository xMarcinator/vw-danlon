//! Importing a trips file: the Min Volkswagen .xlsx export, or a CSV/JSON
//! trips file. Desktop picks it with the native file dialog (Dioxus opens
//! one for a file input); Android with the system document picker that the
//! webview opens for a file input outside the Dioxus tree, read in the page
//! and handed over as base64.

use dioxus::prelude::*;
use vw_danlon::config::Config;
use vw_danlon::trip_store;

use crate::Icon;

/// A picked file's name and content.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    pub name: String,
    pub bytes: Vec<u8>,
}

const UPLOAD_ICON: &str = "M4 14v5a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-5M12 15V3M7 8l5-5 5 5";

/// Merges the picked file into the trip store off the UI thread; the text
/// says what was added, or why nothing was.
pub async fn import(cfg: Config, picked: Picked) -> Result<String, String> {
    let name = picked.name.clone();
    tokio::task::spawn_blocking(move || {
        trip_store::import_bytes(&cfg.trip_store, &picked.name, &picked.bytes, cfg.timezone)
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
            "{name}: {} added, {} already imported{period}",
            merged.added, merged.already
        )
    })
    .map_err(|e| format!("Not imported: {e:#}"))
}

/// A button that picks one trips file.
#[component]
pub fn PickButton(
    label: String,
    class: String,
    on_picked: EventHandler<Result<Picked, String>>,
) -> Element {
    picker(label, class, on_picked)
}

#[cfg(not(target_os = "android"))]
fn picker(
    label: String,
    class: String,
    on_picked: EventHandler<Result<Picked, String>>,
) -> Element {
    rsx! {
        label { class: "{class}",
            input {
                class: "visually-hidden",
                r#type: "file",
                accept: ".xlsx,.csv,.json",
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

/// Opens the webview's own file chooser (the Android document picker): a
/// file input Dioxus does not know is not routed to its desktop dialog.
#[cfg(target_os = "android")]
const PICK_JS: &str = r#"
const input = document.createElement('input');
input.type = 'file';
input.style.display = 'none';
document.body.appendChild(input);
input.addEventListener('change', () => {
  const f = input.files && input.files[0];
  input.remove();
  if (!f) { dioxus.send(null); return; }
  const r = new FileReader();
  r.onload = () => dioxus.send({ name: f.name, data: String(r.result).split(',')[1] || '' });
  r.onerror = () => dioxus.send({ name: f.name, error: String(r.error) });
  r.readAsDataURL(f);
});
input.click();
"#;

#[cfg(target_os = "android")]
#[derive(serde::Deserialize)]
struct FromPage {
    name: String,
    #[serde(default)]
    data: String,
    error: Option<String>,
}

#[cfg(target_os = "android")]
fn picker(
    label: String,
    class: String,
    on_picked: EventHandler<Result<Picked, String>>,
) -> Element {
    use base64::Engine;
    rsx! {
        button {
            class: "{class}",
            onclick: move |_| {
                spawn(async move {
                    let mut eval = document::eval(PICK_JS);
                    let page: Option<FromPage> = match eval.recv().await {
                        Ok(p) => p,
                        Err(e) => {
                            on_picked.call(Err(format!("the file picker failed: {e}")));
                            return;
                        }
                    };
                    let Some(page) = page else { return };
                    let picked = match page.error {
                        Some(e) => Err(format!("{} can't be read: {e}", page.name)),
                        None => base64::engine::general_purpose::STANDARD
                            .decode(page.data.as_bytes())
                            .map(|bytes| Picked { name: page.name.clone(), bytes })
                            .map_err(|e| format!("{} can't be read: {e}", page.name)),
                    };
                    on_picked.call(picked);
                });
            },
            Icon { d: UPLOAD_ICON, size: 16 }
            "{label}"
        }
    }
}
