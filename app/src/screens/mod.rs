//! The data screens and what they share: number formats, the month's data
//! loaded off the UI thread, and the changes the Month and Day screens make
//! (private time, lunch, trips added by hand).

pub mod danlon;
pub mod day;
pub mod mileage;
pub mod month;
pub mod vacation;

use std::collections::BTreeSet;

use anyhow::Result;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use chrono_tz::Tz;
use dioxus::prelude::*;
use vw_danlon::calendar::Occurrence;
use vw_danlon::config::Config;
use vw_danlon::service::{self, LunchChoice, ManualTripForm, PrivateItem, PrivateState};
use vw_danlon::state::{self, EventChoice};
use vw_danlon::trips::Trip;

/// Hours as the mockup writes them: "7,58".
pub fn hours(h: f64) -> String {
    format!("{h:.2}").replace('.', ",")
}

/// A difference with its sign: "+0,68", "−6,90", "0,00".
pub fn signed(h: f64) -> String {
    let text = hours(h.abs());
    if text == "0,00" {
        text
    } else if h < 0.0 {
        format!("−{text}")
    } else {
        format!("+{text}")
    }
}

/// The class colouring a difference.
pub fn diff_class(h: f64) -> &'static str {
    if h <= -0.005 {
        "bad"
    } else if h >= 0.005 {
        "good"
    } else {
        "quiet"
    }
}

pub fn kroner(amount: f64) -> String {
    let text = format!("{amount:.2}");
    let (int, dec) = text.split_once('.').unwrap_or((&text, "00"));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(c);
    }
    format!("{grouped},{dec} kr")
}

pub fn hm(t: NaiveDateTime) -> String {
    t.format("%H:%M").to_string()
}

/// Now in the config's timezone.
pub fn now(cfg: &Config) -> DateTime<Tz> {
    Utc::now().with_timezone(&cfg.timezone)
}

/// The calendar link's refresh, shared by the screens: they show the
/// copy kept from the last fetch at once, and load again when a refresh
/// (in the background every [`REFRESH_EVERY`], or on demand) changed it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CalendarSync {
    /// Bumped when a refresh changed the kept calendar.
    pub version: u32,
    pub refreshing: bool,
    /// Why the last refresh failed; the kept copy is used meanwhile.
    pub error: Option<String>,
}

/// How often the app refreshes the calendar while it runs.
pub const REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Fetches the calendar link and keeps the copy, off the UI thread.
pub async fn refresh_calendar(setup: Signal<crate::setup::Setup>, mut sync: Signal<CalendarSync>) {
    let Some(cfg) = setup.peek().config.clone() else {
        return;
    };
    if sync.peek().refreshing {
        return;
    }
    sync.write().refreshing = true;
    let result = tokio::task::spawn_blocking(move || service::refresh_calendar(&cfg))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| format!("{e:#}")));
    let mut s = sync.write();
    s.refreshing = false;
    match result {
        Ok(Some((_, true))) => {
            s.version += 1;
            s.error = None;
        }
        Ok(_) => s.error = None,
        Err(e) => s.error = Some(e),
    }
}

/// How old the kept calendar is, for the screens: "fetched 14:02", or
/// "fetched 3 days ago".
pub fn calendar_age(fetched_at: DateTime<Utc>, now: DateTime<Tz>) -> String {
    let local = fetched_at.with_timezone(&now.timezone());
    let days = (now.date_naive() - local.date_naive()).num_days();
    match days {
        ..=0 => format!("fetched {}", local.format("%H:%M")),
        1 => format!("fetched yesterday {}", local.format("%H:%M")),
        d => format!("fetched {d} days ago"),
    }
}

/// What the Month and Day screens are built from: the trips and the
/// calendar, loaded off the UI thread.
#[derive(Clone)]
pub struct Loaded {
    pub trips: Vec<Trip>,
    /// The start times of the trips added by hand.
    pub manual: BTreeSet<DateTime<Tz>>,
    pub occurrences: Vec<Occurrence>,
    pub calendar_warning: Option<String>,
    /// When the calendar shown was fetched; none for a calendar file.
    pub calendar_fetched_at: Option<DateTime<Utc>>,
}

/// Loads the trips and the kept calendar for `month` without blocking
/// the UI; the calendar is fetched only when no copy is kept yet.
pub async fn load(cfg: Config, month: vw_danlon::month::Month) -> Result<Loaded, String> {
    tokio::task::spawn_blocking(move || -> Result<Loaded> {
        let now = now(&cfg);
        let trips = service::load_trips(&cfg, None, month)?;
        let manual = service::manual_trip_starts(&cfg)?;
        let cal =
            service::month_calendar(&cfg, month, now.date_naive(), service::CalendarMode::Cached)?;
        Ok(Loaded {
            trips,
            manual,
            occurrences: cal.occurrences,
            calendar_warning: cal.warning,
            calendar_fetched_at: cal.fetched_at,
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

/// A change made on the Month or Day screen.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Start the timer linked to that day's private appointment with this
    /// title, or without one.
    Start(Option<String>),
    Stop,
    Change {
        id: u32,
        label: String,
        start: String,
        end: String,
    },
    Delete(u32),
    /// No lunch that day, or back to the default.
    Lunch(NaiveDate, LunchChoice),
    /// Lunch from `start` for `minutes`, as typed.
    LunchAt {
        date: NaiveDate,
        start: String,
        minutes: String,
    },
    AddTrip(NaiveDate, ManualTripForm),
    /// Delete the trip added by hand starting then.
    DeleteTrip(DateTime<Tz>),
}

/// The part of a screen an action's message belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    Private,
    Lunch,
    Trips,
}

impl Action {
    pub fn panel(&self) -> Panel {
        match self {
            Action::Lunch(..) | Action::LunchAt { .. } => Panel::Lunch,
            Action::AddTrip(..) | Action::DeleteTrip(_) => Panel::Trips,
            _ => Panel::Private,
        }
    }
}

/// What the last action said, and where.
pub type Message = Option<(Panel, Result<String, String>)>;

/// The message for `panel`, if the last action was there.
pub fn message_for(message: &Message, panel: Panel) -> Option<Result<String, String>> {
    message
        .as_ref()
        .filter(|(p, _)| *p == panel)
        .map(|(_, m)| m.clone())
}

/// The status line of a panel.
pub fn status(message: Option<Result<String, String>>) -> Element {
    match message {
        Some(Ok(text)) => rsx! { div { class: "pt-status info", role: "status", "{text}" } },
        Some(Err(text)) => rsx! { div { class: "pt-status bad", role: "alert", "{text}" } },
        None => rsx! {},
    }
}

/// Runs `action`; the text says what was done.
pub fn apply(cfg: &Config, cal: &[Occurrence], action: &Action) -> Result<String> {
    let now = now(cfg);
    Ok(match action {
        Action::Start(title) => {
            let choice = match title {
                Some(t) => EventChoice::Title(t.clone()),
                None => EventChoice::None,
            };
            let r = service::start_private(cfg, cal, now, &choice, None, None)?;
            format!("Timer started {} ({})", hm(r.start), r.label)
        }
        Action::Stop => {
            let i = service::stop_private(cfg, cal, now.naive_local())?;
            let replaces = i
                .replaces
                .as_ref()
                .map(|w| format!(" · replaces the planned {}–{}", hm(w.start), hm(w.end)))
                .unwrap_or_default();
            format!(
                "Recorded {} {}–{}{replaces}",
                i.label,
                hm(i.start),
                hm(i.end)
            )
        }
        Action::Change {
            id,
            label,
            start,
            end,
        } => {
            let start = state::parse_time(start)?;
            let end = state::parse_time(end)?;
            service::change_private(cfg, *id, label, start, end)?;
            format!("Changed {label}")
        }
        Action::Delete(id) => {
            let i = service::delete_private(cfg, *id)?;
            format!("Deleted {} {}–{}", i.label, hm(i.start), hm(i.end))
        }
        Action::Lunch(date, choice) => service::set_lunch(cfg, *date, *choice)?,
        Action::LunchAt {
            date,
            start,
            minutes,
        } => {
            let start = state::parse_time(start.trim())?;
            let minutes: u32 = minutes
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid minutes '{}'", minutes.trim()))?;
            service::set_lunch(cfg, *date, LunchChoice::At { start, minutes })?
        }
        Action::AddTrip(date, form) => {
            let t = service::add_manual_trip(cfg, *date, form)?;
            format!(
                "Added {}–{} {} → {}",
                t.start.format("%H:%M"),
                t.end.format("%H:%M"),
                t.from,
                t.to
            )
        }
        Action::DeleteTrip(start) => {
            service::delete_manual_trip(cfg, *start)?;
            format!("Deleted the trip at {}", start.format("%H:%M"))
        }
    })
}

/// "Today 13:00–14:00", "Thu 27 Nov 08:30–09:15".
fn when(start: NaiveDateTime, end: Option<NaiveDateTime>, today: NaiveDate) -> String {
    let day = if start.date() == today {
        "Today".to_string()
    } else {
        start.format("%a %-d %b").to_string()
    };
    match end {
        Some(e) => format!("{day} {}–{}", hm(start), hm(e)),
        None => format!("{day} {}", hm(start)),
    }
}

/// The note, chip text and chip class of an item.
fn describe(item: &PrivateItem, current: bool) -> (String, &'static str, &'static str) {
    let replaces = |planned: Option<(NaiveDateTime, NaiveDateTime)>| {
        planned
            .map(|(s, e)| format!(" · replaces {}–{}", hm(s), hm(e)))
            .unwrap_or_default()
    };
    match &item.state {
        PrivateState::Planned if current => ("Next · from the calendar".into(), "Next", "chip acc"),
        PrivateState::Planned => ("From the calendar".into(), "Planned", "chip"),
        PrivateState::Running { since } => (
            format!(
                "Timer running since {}{}",
                hm(*since),
                replaces(item.planned)
            ),
            "Running",
            "chip warn",
        ),
        PrivateState::StartedAutomatically { since } => (
            format!(
                "Started automatically at {}{}",
                hm(*since),
                replaces(item.planned)
            ),
            "Started",
            "chip warn",
        ),
        PrivateState::Recorded { start, end, .. } => (
            format!(
                "Recorded {}–{}{}",
                hm(*start),
                hm(*end),
                replaces(item.planned)
            ),
            "Recorded",
            "chip info",
        ),
    }
}

/// Private-time items, each recorded one with an editor for its label and
/// times and a Delete button.
#[component]
pub fn PrivateList(
    items: Vec<PrivateItem>,
    current: Option<usize>,
    today: NaiveDate,
    on_action: Callback<Action, bool>,
) -> Element {
    let mut editing = use_signal(|| None::<u32>);
    rsx! {
        for (i , item) in items.into_iter().enumerate() {
            {
                let (note, chip, chip_class) = describe(&item, current == Some(i));
                let (start, end) = match &item.state {
                    PrivateState::Recorded { start, end, .. } => (*start, Some(*end)),
                    PrivateState::Running { since }
                    | PrivateState::StartedAutomatically { since } => (*since, None),
                    PrivateState::Planned => (item.start(), item.planned.map(|p| p.1)),
                };
                let id = match item.state {
                    PrivateState::Recorded { id, .. } => Some(id),
                    _ => None,
                };
                let label = item.label.clone();
                rsx! {
                    li { class: "pt-item",
                        div { class: "pt-row",
                            span { class: "pt-text",
                                span {
                                    b { "{item.label}" }
                                    " · "
                                    span { class: "num", "{when(start, end, today)}" }
                                }
                                span { class: "pt-note", "{note}" }
                            }
                            span { class: "{chip_class}", "{chip}" }
                            if let Some(id) = id {
                                button {
                                    class: "small-btn",
                                    "aria-expanded": if editing() == Some(id) { "true" } else { "false" },
                                    onclick: move |_| {
                                        let open = editing() == Some(id);
                                        editing.set(if open { None } else { Some(id) });
                                    },
                                    "Edit"
                                }
                            }
                        }
                        if let (Some(id), Some(end)) = (id, end) {
                            if editing() == Some(id) {
                                IntervalEditor {
                                    id,
                                    label,
                                    start: hm(start),
                                    end: hm(end),
                                    // A refused change keeps the editor open
                                    // with what was typed.
                                    on_action: move |a| {
                                        if on_action.call(a) {
                                            editing.set(None);
                                        }
                                    },
                                    on_cancel: move |_| editing.set(None),
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Relabel, retime or delete one recorded interval.
#[component]
fn IntervalEditor(
    id: u32,
    label: String,
    start: String,
    end: String,
    on_action: EventHandler<Action>,
    on_cancel: EventHandler<()>,
) -> Element {
    let mut label = use_signal(|| label);
    let mut start = use_signal(|| start);
    let mut end = use_signal(|| end);
    rsx! {
        div { class: "pt-edit",
            div { class: "field",
                label { r#for: "pt-label-{id}", "Label" }
                input {
                    id: "pt-label-{id}",
                    r#type: "text",
                    value: "{label}",
                    oninput: move |e| label.set(e.value()),
                }
            }
            div { class: "field",
                label { r#for: "pt-start-{id}", "Start" }
                input {
                    id: "pt-start-{id}",
                    r#type: "time",
                    value: "{start}",
                    oninput: move |e| start.set(e.value()),
                }
            }
            div { class: "field",
                label { r#for: "pt-end-{id}", "End" }
                input {
                    id: "pt-end-{id}",
                    r#type: "time",
                    value: "{end}",
                    oninput: move |e| end.set(e.value()),
                }
            }
            div { class: "pt-edit-actions",
                button {
                    class: "small-btn danger",
                    onclick: move |_| on_action.call(Action::Delete(id)),
                    "Delete"
                }
                span { class: "grow" }
                button { class: "small-btn", onclick: move |_| on_cancel.call(()), "Cancel" }
                button {
                    class: "small-btn primary-sm",
                    onclick: move |_| {
                        on_action
                            .call(Action::Change {
                                id,
                                label: label(),
                                start: start(),
                                end: end(),
                            })
                    },
                    "Save"
                }
            }
        }
    }
}
