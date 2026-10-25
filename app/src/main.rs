//! The vw-danlon desktop and phone app. One set of screens; the layout
//! follows the window width: a sidebar when wide, bottom tabs when narrow.

use dioxus::prelude::*;

mod import;
mod screens;
mod secrets;
mod settings;
mod setup;

use setup::{Problem, Setup, Theme};

const CSS: Asset = asset!("/assets/app.css");
const SANS_400: Asset = asset!("/assets/fonts/IBMPlexSans-Regular.woff2");
const SANS_500: Asset = asset!("/assets/fonts/IBMPlexSans-Medium.woff2");
const SANS_600: Asset = asset!("/assets/fonts/IBMPlexSans-SemiBold.woff2");
const MONO_400: Asset = asset!("/assets/fonts/IBMPlexMono-Regular.woff2");
const MONO_500: Asset = asset!("/assets/fonts/IBMPlexMono-Medium.woff2");

fn main() {
    #[cfg(feature = "desktop")]
    {
        use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
        dioxus::LaunchBuilder::desktop()
            .with_cfg(
                Config::new().with_menu(None).with_window(
                    WindowBuilder::new()
                        .with_title("vw-danlon")
                        .with_inner_size(LogicalSize::new(1100.0, 800.0))
                        .with_min_inner_size(LogicalSize::new(320.0, 480.0)),
                ),
            )
            .launch(App);
    }
    #[cfg(not(feature = "desktop"))]
    dioxus::launch(App);
}

/// The bundled IBM Plex faces (SIL OFL, see assets/fonts/OFL.txt).
fn font_faces() -> String {
    [
        ("IBM Plex Sans", 400, SANS_400),
        ("IBM Plex Sans", 500, SANS_500),
        ("IBM Plex Sans", 600, SANS_600),
        ("IBM Plex Mono", 400, MONO_400),
        ("IBM Plex Mono", 500, MONO_500),
    ]
    .iter()
    .map(|(family, weight, src)| {
        format!(
            "@font-face{{font-family:'{family}';font-weight:{weight};font-style:normal;src:url('{src}') format('woff2');}}"
        )
    })
    .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Month,
    Vacation,
    Mileage,
    Danlon,
    Settings,
}

impl Screen {
    fn label(self) -> &'static str {
        match self {
            Screen::Month => "Month",
            Screen::Vacation => "Vacation",
            Screen::Mileage => "Mileage",
            Screen::Danlon => "Danløn",
            Screen::Settings => "Settings",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Screen::Mileage => "Mileage logbook",
            Screen::Danlon => "Danløn app checklist",
            s => s.label(),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Screen::Month => "M3 5h18v16H3zM3 10h18M8 3v4M16 3v4",
            Screen::Vacation => "M12 3a9 9 0 0 1 9 9H3a9 9 0 0 1 9-9zM12 12v8a2 2 0 0 1-4 0",
            Screen::Mileage => "M3 17h2l2-6h10l2 6h2M5 17v3M19 17v3M7.5 14h.01M16.5 14h.01M5 17h14",
            Screen::Danlon => {
                "M9 11l3 3 8-8M20 12v7a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h9"
            }
            Screen::Settings => {
                "M12 15a3 3 0 1 0 0-6a3 3 0 1 0 0 6zM12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1 7 17M17 7l2.1-2.1"
            }
        }
    }
}

/// Sidebar order; the phone tabs leave Settings to the top bar, as in the
/// mockup.
const SIDEBAR: [Screen; 5] = [
    Screen::Month,
    Screen::Vacation,
    Screen::Mileage,
    Screen::Danlon,
    Screen::Settings,
];
const TABS: [Screen; 4] = [
    Screen::Month,
    Screen::Mileage,
    Screen::Danlon,
    Screen::Vacation,
];

fn theme_icon(t: Theme) -> &'static str {
    match t {
        Theme::Light => {
            "M12 16a4 4 0 1 0 0-8a4 4 0 1 0 0 8zM12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"
        }
        Theme::Dark => "M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z",
        Theme::System => "M2 3h20v14H2zM8 21h8M12 17v4",
    }
}

#[component]
fn Icon(d: &'static str, size: u32) -> Element {
    rsx! {
        svg {
            "aria-hidden": "true",
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            style: "flex-shrink: 0",
            path { d }
        }
    }
}

#[component]
fn App() -> Element {
    let setup = use_signal(|| Setup::load(setup::data_dir()));
    let screen = use_signal(|| {
        if setup.peek().problems.is_empty() {
            Screen::Month
        } else {
            Screen::Settings
        }
    });
    let theme = setup.read().settings.theme;
    let current = screen();

    rsx! {
        document::Stylesheet { href: CSS }
        style { {font_faces()} }
        div { class: "app", "data-theme": theme.id(),
            Sidebar { screen, setup }
            div { class: "main-col",
                header { class: if current == Screen::Month { "topbar hidden" } else { "topbar" },
                    if current == Screen::Settings {
                        button {
                            class: "icon-btn",
                            "aria-label": "Back to Month",
                            onclick: move |_| { let mut s = screen; s.set(Screen::Month) },
                            Icon { d: "m15 6-6 6 6 6", size: 22 }
                        }
                    }
                    h1 { "{current.title()}" }
                    if current != Screen::Settings {
                        button {
                            class: "icon-btn",
                            "aria-label": "Settings",
                            onclick: move |_| { let mut s = screen; s.set(Screen::Settings) },
                            Icon { d: Screen::Settings.icon(), size: 22 }
                        }
                    }
                }
                main { class: if current == Screen::Month { "content full" } else { "content" },
                    if current != Screen::Month {
                        h1 { "{current.title()}" }
                    }
                    if current == Screen::Settings {
                        settings::Settings { setup, screen }
                    } else if current == Screen::Danlon {
                        Placeholder { screen: current }
                    } else if setup.read().config.is_none() {
                        NeedsSetup { screen }
                    } else if current == Screen::Month {
                        screens::month::MonthScreen { setup, screen }
                    } else if current == Screen::Vacation {
                        screens::vacation::VacationScreen { setup }
                    } else {
                        screens::mileage::MileageScreen { setup, screen }
                    }
                }
            }
            nav { class: "tabbar", "aria-label": "Main",
                for s in TABS {
                    button {
                        class: "tab",
                        "aria-current": if s == current { "page" } else { "false" },
                        onclick: move |_| { let mut sc = screen; sc.set(s) },
                        span { class: "pill", Icon { d: s.icon(), size: 20 } }
                        "{s.label()}"
                    }
                }
            }
        }
    }
}

#[component]
fn Sidebar(screen: Signal<Screen>, setup: Signal<Setup>) -> Element {
    let current = screen();
    let s = setup.read();
    let calendar_ok = s.config.as_ref().is_some_and(|c| c.calendar.is_some());
    let trips_ok = !s
        .problems
        .iter()
        .any(|p| matches!(p, Problem::NoTrips | Problem::BadTrips(_)))
        && s.trips.is_some();
    let theme = s.settings.theme;
    rsx! {
        nav { class: "sidebar", "aria-label": "Main",
            div { class: "brand",
                svg { "aria-hidden": "true", width: "26", height: "26", view_box: "0 0 256 256",
                    rect { width: "256", height: "256", rx: "56", fill: "var(--acc)" }
                    circle { cx: "128", cy: "128", r: "70", fill: "none", stroke: "var(--logo-ink)", stroke_width: "22" }
                    path { d: "M128 82v50l32 20", fill: "none", stroke: "var(--logo-ink)", stroke_width: "22", stroke_linecap: "round" }
                }
                "vw-danlon"
            }
            for item in SIDEBAR {
                button {
                    class: "nav-item",
                    "aria-current": if item == current { "page" } else { "false" },
                    onclick: move |_| screen.set(item),
                    Icon { d: item.icon(), size: 16 }
                    "{item.label()}"
                }
            }
            div { class: "nav-head", "Sources" }
            div { class: "source",
                span { class: if calendar_ok { "dot" } else { "dot warn" }, "aria-hidden": "true" }
                "Calendar"
            }
            div { class: "source",
                span { class: if trips_ok { "dot" } else { "dot warn" }, "aria-hidden": "true" }
                "Trips file"
            }
            div { class: "grow" }
            div { class: "theme-switch", role: "group", "aria-label": "Color theme",
                for t in Theme::ALL {
                    button {
                        "aria-label": "{t.label()} theme",
                        title: "{t.label()} theme",
                        "aria-pressed": if t == theme { "true" } else { "false" },
                        onclick: move |_| settings::apply_theme(setup, t),
                        Icon { d: theme_icon(t), size: 15 }
                    }
                }
            }
        }
    }
}

/// A data screen without a usable config.
#[component]
fn NeedsSetup(screen: Signal<Screen>) -> Element {
    rsx! {
        div { class: "alert", role: "alert",
            span { "aria-hidden": "true", style: "font-weight: 700", "!" }
            div {
                "The config can't be used yet. "
                button { class: "link-btn", onclick: move |_| screen.set(Screen::Settings), "Open Settings" }
            }
        }
    }
}

/// Screens that later tasks fill in.
#[component]
fn Placeholder(screen: Screen) -> Element {
    rsx! {
        p { class: "placeholder", "The {screen.title()} screen comes in a later step." }
    }
}
