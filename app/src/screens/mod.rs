//! The data screens and what they share: number formats, the month's data
//! loaded off the UI thread, and the changes the Month and Day screens make
//! (private time, lunch, trips added by hand).

pub mod danlon;
pub mod day;
pub mod mileage;
pub mod month;
pub mod period;
pub mod vacation;
pub mod week;

use std::collections::BTreeSet;

use anyhow::Result;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use chrono_tz::Tz;
use dioxus::prelude::*;
use vw_danlon::calendar::Occurrence;
use vw_danlon::config::Config;
use vw_danlon::service::{self, LunchChoice, ManualTripForm, PrivateItem, PrivateState};
use vw_danlon::state::{self, EventChoice, IntervalKind};
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

/// The short English weekday: "Mon".
pub fn weekday(d: NaiveDate) -> String {
    d.format("%a").to_string()
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

/// How long a calendar refresh may take before it is given up.
const REFRESH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Refreshes the calendar every [`REFRESH_EVERY`] while the app runs. A
/// tick missed while the app was paused (the phone asleep, the app in the
/// background) is not caught up: coming back gives at most one refresh, and
/// none while one is still running.
pub async fn refresh_calendar_now_and_then(
    setup: Signal<crate::setup::Setup>,
    sync: Signal<CalendarSync>,
) {
    let mut every = tokio::time::interval(REFRESH_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        every.tick().await;
        refresh_calendar(setup, sync).await;
    }
}

/// Fetches the calendar link and keeps the copy, off the UI thread.
pub async fn refresh_calendar(setup: Signal<crate::setup::Setup>, mut sync: Signal<CalendarSync>) {
    let Some(cfg) = setup.peek().config.clone() else {
        return;
    };
    if sync.peek().refreshing {
        return;
    }
    sync.write().refreshing = true;
    // The download has its own timeouts; this one also ends a refresh stuck
    // anywhere else, so `refreshing` never stays set.
    let job = tokio::task::spawn_blocking(move || service::refresh_calendar(&cfg));
    let result = match tokio::time::timeout(REFRESH_TIMEOUT, job).await {
        Ok(joined) => joined
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| format!("{e:#}"))),
        Err(_) => Err("no answer in time".to_string()),
    };
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

/// Bumped each time the app comes back to the foreground, so the screens
/// compute today again at once.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Resumed(pub u32);

impl Resumed {
    /// The shortest time between two bumps: coming back gives both a
    /// resume and a focus event (Android), counted once.
    const SETTLE: std::time::Duration = std::time::Duration::from_secs(1);

    /// Whether a lifecycle event means the app is in front again: Android's
    /// resume, or the window getting the focus (desktop and Android).
    pub fn is_back<T>(event: &crate::webview::tao::event::Event<'_, T>) -> bool {
        use crate::webview::tao::event::{Event, WindowEvent};
        matches!(
            event,
            Event::Resumed
                | Event::WindowEvent {
                    event: WindowEvent::Focused(true),
                    ..
                }
        )
    }

    /// Whether coming back at `now` counts: not within [`Self::SETTLE`] of
    /// the last one counted (kept in `last`).
    pub fn due(last: &mut Option<std::time::Instant>, now: std::time::Instant) -> bool {
        if last.is_some_and(|l| now.duration_since(l) < Self::SETTLE) {
            return false;
        }
        *last = Some(now);
        true
    }
}

/// The scrolling page content (`main.content`), so a screen can scroll it
/// to the top.
#[derive(Clone, Copy)]
pub struct Content(pub Signal<Option<std::rc::Rc<MountedData>>>);

impl Content {
    /// Scrolls the page content to the top.
    pub fn scroll_to_top(self) {
        let Some(el) = self.0.peek().clone() else {
            return;
        };
        spawn(async move {
            let _ = el
                .scroll(
                    dioxus::html::geometry::PixelsVector2D::new(0.0, 0.0),
                    ScrollBehavior::Instant,
                )
                .await;
        });
    }
}

/// The account trip sync the app runs by itself, shared with Settings so
/// Sync now and the automatic sync never run at once.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TripSync {
    pub syncing: bool,
}

/// How often the app syncs trips from the account while it runs.
pub const TRIP_SYNC_EVERY: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// How long an automatic trip sync may take before it is given up.
const TRIP_SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Syncs trips from a connected account when the app starts and every
/// [`TRIP_SYNC_EVERY`] while it runs. As with the calendar, a tick missed
/// while the app was in the background is not caught up: coming back gives
/// one sync at once.
pub async fn sync_trips_now_and_then(setup: Signal<crate::setup::Setup>, sync: Signal<TripSync>) {
    let mut every = tokio::time::interval(TRIP_SYNC_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        every.tick().await;
        sync_trips(setup, sync).await;
    }
}

/// One automatic sync, off the UI thread. Only a connected account is
/// synced; the screens reload only when trips were added. A failure is
/// left for Sync now in Settings to show: the account line there says when
/// the last sync was.
pub async fn sync_trips(mut setup: Signal<crate::setup::Setup>, mut sync: Signal<TripSync>) {
    let (cfg, store, dir) = {
        let s = setup.peek();
        if !matches!(s.account, crate::setup::AccountStatus::Connected { .. }) {
            return;
        }
        let Some(cfg) = s.config.clone() else {
            return;
        };
        (cfg, s.secret_store(), s.dir.clone())
    };
    if sync.peek().syncing {
        return;
    }
    sync.write().syncing = true;
    let job = tokio::task::spawn_blocking(move || {
        let today = now(&cfg).date_naive();
        crate::setup::sync_account(&cfg, store.as_ref(), today)
    });
    let added = match tokio::time::timeout(TRIP_SYNC_TIMEOUT, job).await {
        Ok(Ok(Ok(outcome))) => outcome.synced.merged.added,
        _ => 0,
    };
    sync.write().syncing = false;
    if added > 0 {
        setup.set(crate::setup::Setup::load(Ok(dir)));
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
    pub month: vw_danlon::month::Month,
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
            month,
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

/// The month as the screens show it at `now`, with the state it was built
/// from: computed off the UI thread (it reads and may write the database,
/// see [`service::month_view`]).
pub struct Computed {
    pub month: vw_danlon::month::Month,
    pub now: DateTime<Tz>,
    pub view: service::MonthView,
    pub state: state::State,
}

/// Builds [`Computed`] from `loaded` at the current time, off the UI thread.
pub async fn compute(cfg: Config, loaded: Loaded) -> Result<Computed, String> {
    tokio::task::spawn_blocking(move || -> Result<Computed> {
        let now = now(&cfg);
        let view =
            service::month_view(&cfg, loaded.month, &loaded.trips, &loaded.occurrences, now)?;
        let state = state::State::load(&cfg.database)?;
        Ok(Computed {
            month: loaded.month,
            now,
            view,
            state,
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
    /// The real start, as typed, of the appointment started automatically.
    StartedAt(String),
    /// Private time without an event that started earlier today, as typed.
    StartAt(String),
    /// At work without a departure: you left now without a trip.
    LeaveWork,
    /// Take back "left work" on that day.
    UndoLeaveWork(NaiveDate),
    /// You did not go to the private appointment on that day with this
    /// title: it counts no private time.
    Skip(NaiveDate, String),
    /// Start or stop the working-from-home timer.
    HomeStart,
    HomeStop,
    /// Start the working-from-home timer earlier today, as typed.
    HomeStartAt(String),
    /// Record private time (`home` false) or home working on a day, as
    /// typed.
    AddTime {
        date: NaiveDate,
        home: bool,
        label: String,
        start: String,
        end: String,
    },
    /// Relabel and retime a recorded private (or home-working) interval.
    Change {
        home: bool,
        id: u32,
        label: String,
        start: String,
        end: String,
    },
    Delete {
        home: bool,
        id: u32,
    },
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
    Home,
    Lunch,
    Trips,
}

impl Action {
    pub fn panel(&self) -> Panel {
        match self {
            Action::Lunch(..) | Action::LunchAt { .. } => Panel::Lunch,
            Action::AddTrip(..) | Action::DeleteTrip(_) => Panel::Trips,
            Action::HomeStart
            | Action::HomeStop
            | Action::HomeStartAt(_)
            | Action::LeaveWork
            | Action::UndoLeaveWork(_) => Panel::Home,
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
        Action::StartedAt(at) => {
            let at = state::parse_time(at.trim())?;
            let r = service::start_private(cfg, cal, now, &EventChoice::Auto, Some(at), None)?;
            format!("Timer running since {} ({})", hm(r.start), r.label)
        }
        Action::StartAt(at) => {
            let at = state::parse_time(at.trim())?;
            let r = service::start_private(cfg, cal, now, &EventChoice::None, Some(at), None)?;
            format!("Timer running since {} ({})", hm(r.start), r.label)
        }
        Action::LeaveWork => {
            service::set_left_work(cfg, now.date_naive(), Some(now.naive_local()))?;
            format!(
                "Left work at {} · the time at work ends then",
                hm(now.naive_local())
            )
        }
        Action::UndoLeaveWork(date) => {
            service::set_left_work(cfg, *date, None)?;
            "Left work taken back".to_string()
        }
        Action::Skip(date, title) => {
            let w = service::skip_private(cfg, cal, *date, Some(title), now.naive_local())?;
            format!(
                "Skipped {} {}–{}: no private time",
                w.title,
                hm(w.start),
                hm(w.end)
            )
        }
        Action::HomeStart => {
            let r = service::start_home(cfg, now, None)?;
            format!("Working from home since {}", hm(r.start))
        }
        Action::HomeStartAt(at) => {
            let at = state::parse_time(at.trim())?;
            let r = service::start_home(cfg, now, Some(at))?;
            format!("Working from home since {}", hm(r.start))
        }
        Action::HomeStop => {
            let i = service::stop_home(cfg, now.naive_local())?;
            format!("Recorded {} {}–{}", i.label, hm(i.start), hm(i.end))
        }
        Action::AddTime {
            date,
            home,
            label,
            start,
            end,
        } => {
            let start = state::parse_time(start.trim())?;
            let end = state::parse_time(end.trim())?;
            let now = now.naive_local();
            let i = if *home {
                service::add_home(cfg, *date, start, end, label, now)?
            } else {
                service::add_private(cfg, *date, start, end, label, now)?
            };
            format!("Added {} {}–{}", i.label, hm(i.start), hm(i.end))
        }
        Action::Change {
            home,
            id,
            label,
            start,
            end,
        } => {
            let start = state::parse_time(start.trim())?;
            let end = state::parse_time(end.trim())?;
            if *home {
                service::change_home(cfg, *id, label, start, end)?;
            } else {
                service::change_private(cfg, *id, label, start, end)?;
            }
            format!("Changed {}", label.trim())
        }
        Action::Delete { home, id } => {
            let i = if *home {
                service::delete_home(cfg, *id)?
            } else {
                service::delete_private(cfg, *id)?
            };
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

/// The note, chip text and chip class of an item; `auto_start` when
/// private appointments start automatically.
fn describe(
    item: &PrivateItem,
    current: bool,
    auto_start: bool,
) -> (String, &'static str, &'static str) {
    match &item.state {
        PrivateState::Planned if current && auto_start => {
            ("Starts by itself".into(), "Next", "chip acc")
        }
        PrivateState::Planned if current => (String::new(), "Next", "chip acc"),
        PrivateState::Planned => (String::new(), "Planned", "chip"),
        PrivateState::Running { since } => {
            (format!("Since {}", hm(*since)), "Running", "chip info")
        }
        PrivateState::StartedAutomatically { since } => (
            format!("Started by itself at {}", hm(*since)),
            "Running",
            "chip info",
        ),
        PrivateState::Recorded {
            kind: IntervalKind::CalendarTime,
            ..
        } => ("Not timed".into(), "Calendar time", "chip warn"),
        PrivateState::Recorded { .. } => (String::new(), "Recorded", "chip"),
    }
}

/// Private-time items: when, what they are, and Skip for an appointment
/// not recorded (planned or started automatically): you did not go.
#[component]
pub fn PrivateList(
    items: Vec<PrivateItem>,
    current: Option<usize>,
    today: NaiveDate,
    auto_start: bool,
    on_action: Callback<Action, bool>,
) -> Element {
    rsx! {
        for (i , item) in items.into_iter().enumerate() {
            {
                let (note, chip, chip_class) = describe(&item, current == Some(i), auto_start);
                let (start, end) = match &item.state {
                    PrivateState::Recorded { start, end, .. } => (*start, Some(*end)),
                    PrivateState::Running { since } => (*since, None),
                    PrivateState::StartedAutomatically { since } => {
                        (*since, item.planned.map(|p| p.1))
                    }
                    PrivateState::Planned => (item.start(), item.planned.map(|p| p.1)),
                };
                let skip = matches!(
                    item.state,
                    PrivateState::Planned | PrivateState::StartedAutomatically { .. }
                )
                .then(|| item.planned.map(|(s, _)| (s.date(), item.label.clone())))
                .flatten();
                rsx! {
                    li { class: "pt-item",
                        div { class: "pt-row",
                            span { class: "pt-text",
                                span {
                                    b { "{item.label}" }
                                    " · "
                                    span { class: "num", "{when(start, end, today)}" }
                                }
                                if !note.is_empty() {
                                    span { class: "pt-note", "{note}" }
                                }
                            }
                            span { class: "{chip_class}", "{chip}" }
                            if let Some((date, title)) = skip {
                                button {
                                    class: "small-btn",
                                    title: "I didn't go: no private time",
                                    onclick: move |_| {
                                        on_action.call(Action::Skip(date, title.clone()));
                                    },
                                    "Skip"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod resume_tests {
    use super::Resumed;
    use std::time::{Duration, Instant};

    #[test]
    fn coming_back_twice_at_once_counts_once() {
        let t = Instant::now();
        let mut last = None;
        assert!(Resumed::due(&mut last, t));
        assert!(!Resumed::due(&mut last, t + Duration::from_millis(300)));
        assert!(Resumed::due(&mut last, t + Duration::from_secs(2)));
    }
}
