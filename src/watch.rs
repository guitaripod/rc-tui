use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{Local, Timelike, Utc};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;

use crate::activity::Activity;
use crate::api::{self, Client, Settings};
use crate::config::{self, Config, MomentsConfig};
use crate::moments::{self, AppReading, Kind, Level, Moment, Reading, State};
use crate::notify::{Action, Notice, Notifier};
use crate::{card, chime, countries, lights::Lights};

const DEFAULT_MINUTES: u64 = 15;
const RETRY_SEED: Duration = Duration::from_secs(120);
const HISTORY_DAYS: u64 = 1830;
const RECENT_DAYS: u64 = 57;
const SEGMENT_DAYS: u64 = 3;
const CONCURRENT_PROJECTS: usize = 4;
const TICK: Duration = Duration::from_secs(2);
const STALE_AFTER_MS: i64 = 3 * 60_000;
const LIGHTS_HOLD: Duration = Duration::from_secs(14);
const MAJOR_TIMEOUT_MS: i32 = 30_000;
const DEFAULT_QUIET: &str = "23-08";
const TEST_LINGER: Duration = Duration::from_secs(90);

pub struct Options {
    pub base_url: String,
    pub currency: String,
    pub dry_run: bool,
}

/// `rc-tui watch`: learns your history once, then turns every record, milestone and first into a moment.
pub fn run(options: Options) -> color_eyre::Result<ExitCode> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(watch(options))
}

/// `rc-tui moments --test`: sends a sample moment through every channel, then lingers to catch a click or a note.
pub fn test(currency: &str) -> color_eyre::Result<ExitCode> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let config = Config::load();
        let (sender, mut actions) = mpsc::unbounded_channel();
        let mut delivery = Delivery::new(&config.moments, false, sender).await;
        let moment = Moment::sample(currency, Utc::now().timestamp_millis());
        delivery.deliver(moment).await;
        eprintln!("{}", delivery.describe());
        eprintln!("Sent a sample moment. Click it or add a note in the next 90 seconds to try the actions.");
        let deadline = tokio::time::Instant::now() + TEST_LINGER;
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                action = actions.recv() => match action {
                    Some(action) => delivery.act(action),
                    None => tokio::time::sleep_until(deadline).await,
                },
            }
            delivery.reap();
        }
        Ok(ExitCode::SUCCESS)
    })
}

async fn watch(options: Options) -> color_eyre::Result<ExitCode> {
    let config = Config::load();
    if config.projects.is_empty() {
        eprintln!("rc-tui watch: no projects yet. Run rc-tui once to add one.");
        return Ok(ExitCode::FAILURE);
    }
    let Some(_lock) = lock() else {
        eprintln!("rc-tui watch: already running (another watcher holds the lock).");
        return Ok(ExitCode::FAILURE);
    };
    let state_path = moments::state_path();
    let mut state = State::load(&state_path);
    let mut saved = serde_json::to_string(&state).unwrap_or_default();
    let http = api::http_client();
    let clients: Vec<(String, Client)> = config
        .projects
        .iter()
        .map(|project| {
            (
                project.label().to_string(),
                Client::with_http(
                    Settings::new(
                        options.base_url.clone(),
                        project.api_key.clone(),
                        project.id.clone(),
                        options.currency.clone(),
                    ),
                    http.clone(),
                ),
            )
        })
        .collect();
    let enough = config.enough.map(|amount| amount as f64);
    let interval = Duration::from_secs(
        config
            .moments
            .minutes
            .unwrap_or(DEFAULT_MINUTES)
            .clamp(5, 240)
            * 60,
    );
    let (sender, mut actions) = mpsc::unbounded_channel();
    let mut delivery = Delivery::new(&config.moments, options.dry_run, sender).await;
    let feed_path = config.feed_path.as_ref().map(PathBuf::from);
    let mut activity = Activity::new(feed_path, false, None, Instant::now());
    eprintln!(
        "rc-tui watch: {} projects, every {} min. {}",
        clients.len(),
        interval.as_secs() / 60,
        delivery.describe()
    );
    if state.seeded {
        eprintln!("rc-tui watch: {}", summary(&state, &options.currency));
    } else {
        eprintln!("rc-tui watch: learning your history first; nothing fires until that is done.");
    }

    let mut next_reading = Instant::now();
    let mut stale_since: Option<Instant> = None;
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            Some(action) = actions.recv() => {
                delivery.act(action);
                continue;
            }
        }
        let now = Instant::now();
        let now_ms = Utc::now().timestamp_millis();
        activity.refresh(now);
        let feed_live = activity.path.exists();
        let heartbeat = activity.last_heartbeat_ms.filter(|_| feed_live);
        match heartbeat {
            Some(beat) if now_ms - beat > STALE_AFTER_MS => {
                stale_since.get_or_insert(now);
            }
            _ => stale_since = None,
        }
        let quiet_for = stale_since.map(|since| since.elapsed()).unwrap_or_default();
        let mut found =
            moments::observe_feed(&mut state, &activity.entries, heartbeat, quiet_for, now_ms);

        if now >= next_reading {
            let was_seeded = state.seeded;
            let mut reading = read(&clients, state.seeded, &options.currency, enough).await;
            reading.feed = feed_live;
            found.extend(moments::observe(&mut state, &reading, now_ms));
            if !reading.complete {
                eprintln!("rc-tui watch: some projects did not answer; trying again next round.");
            }
            if !was_seeded && state.seeded {
                eprintln!(
                    "rc-tui watch: history learned. {}",
                    summary(&state, &options.currency)
                );
            }
            next_reading = now + if state.seeded { interval } else { RETRY_SEED };
        }

        let snapshot = serde_json::to_string(&state).unwrap_or_default();
        if snapshot != saved && !options.dry_run {
            if let Err(err) = state.save(&state_path) {
                eprintln!(
                    "rc-tui watch: could not save {}: {err}",
                    state_path.display()
                );
            } else {
                saved = snapshot;
            }
        }
        for moment in found {
            delivery.deliver(moment).await;
        }
        delivery.reap();
        if options.dry_run && state.seeded {
            eprintln!(
                "rc-tui watch: dry run done. {}",
                summary(&state, &options.currency)
            );
            return Ok(ExitCode::SUCCESS);
        }
    }
}

/// The records and memory the watcher holds, in one line.
fn summary(state: &State, currency: &str) -> String {
    let money = |value: f64| crate::ui::grouped_money(value, currency);
    let mut parts = Vec::new();
    if let Some(best) = state.best_day {
        parts.push(format!("best day {} ({})", money(best.value), best.day));
    }
    if let Some(best) = state.best_month {
        parts.push(format!(
            "best 28 days {} (to {})",
            money(best.value),
            best.day
        ));
    }
    parts.push(format!("{} countries", state.countries.len()));
    parts.push(format!("{} apps sold", state.sold.len()));
    if state.people > 0.0 {
        parts.push(format!("past {} people", crate::ui::grouped(state.people)));
    }
    if let Some(target) = state.enough {
        parts.push(format!(
            "enough {} ({})",
            money(target),
            if state.enough_reached {
                "reached"
            } else {
                "not yet"
            }
        ));
    }
    parts.join(", ")
}

/// Holds an exclusive lock for this process's life so only one watcher fires each moment.
fn lock() -> Option<File> {
    let dir = config::state_dir().join("rc-tui");
    std::fs::create_dir_all(&dir).ok()?;
    let file = File::create(dir.join("watch.lock")).ok()?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let taken = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if taken != 0 {
            return None;
        }
    }
    Some(file)
}

/// Reads every project: the overview, daily revenue, and which countries and plan lengths sold recently.
async fn read(
    clients: &[(String, Client)],
    seeded: bool,
    currency: &str,
    enough: Option<f64>,
) -> Reading {
    let days = if seeded { RECENT_DAYS } else { HISTORY_DAYS };
    let segment_days = if seeded { SEGMENT_DAYS } else { HISTORY_DAYS };
    let gate = Arc::new(Semaphore::new(CONCURRENT_PROJECTS));
    let mut set = JoinSet::new();
    for (order, (name, client)) in clients.iter().enumerate() {
        let client = client.clone();
        let name = name.clone();
        let gate = gate.clone();
        set.spawn(async move {
            let _permit = gate.acquire_owned().await;
            (order, read_app(name, &client, days, segment_days).await)
        });
    }
    let mut apps: Vec<(usize, AppReading, bool)> = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok((order, (app, ok))) = joined {
            apps.push((order, app, ok));
        }
    }
    apps.sort_by_key(|(order, _, _)| *order);
    let complete = apps.len() == clients.len() && apps.iter().all(|(_, _, ok)| *ok);
    Reading {
        apps: apps.into_iter().map(|(_, app, _)| app).collect(),
        complete,
        today: Utc::now().date_naive(),
        currency: currency.to_string(),
        enough,
        feed: false,
    }
}

async fn read_app(
    name: String,
    client: &Client,
    days: u64,
    segment_days: u64,
) -> (AppReading, bool) {
    let (overview, revenue, places, durations) = tokio::join!(
        client.fetch_overview(),
        client.revenue_days(days),
        client.purchase_segments("country", segment_days),
        client.purchase_segments("product_duration", segment_days),
    );
    let ok = overview.is_ok() && revenue.is_ok() && places.is_ok() && durations.is_ok();
    let metric = |ids: &[&str]| {
        overview.as_ref().ok().and_then(|overview| {
            ids.iter().find_map(|id| {
                overview
                    .metrics
                    .iter()
                    .find(|metric| metric.id == *id)
                    .map(|metric| metric.value)
            })
        })
    };
    let people = metric(&crate::app::PEOPLE);
    let money = metric(&["revenue"]);
    let revenue = revenue.unwrap_or_default();
    let mut unknown = Vec::new();
    let countries = places
        .unwrap_or_default()
        .into_iter()
        .filter_map(|place| match countries::code(&place) {
            Some(code) => Some(code.to_string()),
            None => {
                unknown.push(place);
                None
            }
        })
        .collect();
    if !unknown.is_empty() {
        eprintln!(
            "rc-tui watch: {name}: no ISO code for {}",
            unknown.join(", ")
        );
    }
    let app = AppReading {
        people,
        revenue: money,
        sold: revenue.iter().any(|(_, _, purchases)| *purchases > 0.0),
        days: revenue
            .iter()
            .map(|(day, value, _)| (*day, *value))
            .collect(),
        countries,
        yearly: durations
            .unwrap_or_default()
            .iter()
            .any(|name| moments::is_yearly_duration(name)),
        name,
    };
    (app, ok)
}

/// Everywhere a moment can land: the log, a card, the desktop, the speakers, the lights and a command.
struct Delivery {
    log: PathBuf,
    cards: Option<PathBuf>,
    icons: PathBuf,
    notifier: Option<Notifier>,
    notify: bool,
    actions: mpsc::UnboundedSender<Action>,
    chime: Option<PathBuf>,
    lights: Option<Lights>,
    command: Vec<String>,
    terminal: Option<Vec<String>>,
    quiet: Option<(u32, u32)>,
    shown: HashMap<u32, Moment>,
    children: Vec<Child>,
    dry_run: bool,
}

impl Delivery {
    async fn new(
        settings: &MomentsConfig,
        dry_run: bool,
        actions: mpsc::UnboundedSender<Action>,
    ) -> Self {
        let cache = config::cache_dir().join("rc-tui");
        let cards = card::available().then(|| {
            settings
                .cards
                .as_deref()
                .map(expand)
                .unwrap_or_else(|| config::pictures_dir().join("rc-tui"))
        });
        let chime = settings
            .sound
            .unwrap_or(true)
            .then(|| write_chime(&cache))
            .flatten();
        let lights = settings
            .lights
            .unwrap_or(true)
            .then(|| Lights::detect(settings.lights_profile.clone()))
            .flatten();
        let mut delivery = Self {
            log: moments::log_path(),
            cards,
            icons: cache.join("icons"),
            notifier: None,
            notify: settings.notify.unwrap_or(true),
            actions,
            chime,
            lights,
            command: settings.command.clone(),
            terminal: terminal(&settings.terminal),
            quiet: parse_quiet(settings.quiet.as_deref().unwrap_or(DEFAULT_QUIET)),
            shown: HashMap::new(),
            children: Vec::new(),
            dry_run,
        };
        delivery.ensure_notifier().await;
        delivery
    }

    /// Connects to the notification server, again on every moment until it answers: at login the watcher can start before the desktop does.
    async fn ensure_notifier(&mut self) {
        if !self.notify || self.notifier.is_some() {
            return;
        }
        if let Some(notifier) = Notifier::connect().await {
            notifier.listen(self.actions.clone()).await;
            self.notifier = Some(notifier);
        }
    }

    /// Which channels are live, for the first log line.
    fn describe(&self) -> String {
        let mut on = Vec::new();
        if self.notifier.is_some() {
            on.push("notifications".to_string());
        } else if self.notify {
            on.push("notifications once the desktop answers".to_string());
        }
        if let Some(cards) = &self.cards {
            on.push(format!("cards in {}", cards.display()));
        }
        if self.chime.is_some() {
            on.push("chime".to_string());
        }
        if self.lights.is_some() {
            on.push("lights".to_string());
        }
        if !self.command.is_empty() {
            on.push(format!("command {}", self.command[0]));
        }
        if let Some((start, end)) = self.quiet {
            on.push(format!(
                "quiet {:02}:{:02}-{:02}:{:02}",
                start / 60,
                start % 60,
                end / 60,
                end % 60
            ));
        }
        format!("Moments go to: {}.", on.join(", "))
    }

    async fn deliver(&mut self, mut moment: Moment) {
        if self.dry_run {
            eprintln!(
                "rc-tui watch: would deliver {} · {} · {}",
                moment.title, moment.value, moment.line
            );
            return;
        }
        if moment.kind.keeps()
            && let Some(dir) = &self.cards
        {
            match card::render(&moment, dir) {
                Ok(path) => moment.card = Some(path.to_string_lossy().into_owned()),
                Err(err) => eprintln!("rc-tui watch: card failed: {err}"),
            }
        }
        if let Err(err) = moments::append(&self.log, &moment) {
            eprintln!("rc-tui watch: could not log the moment: {err}");
        }
        eprintln!(
            "rc-tui watch: {} · {} · {}",
            moment.title, moment.value, moment.line
        );

        self.ensure_notifier().await;
        let hush = self.in_quiet_hours()
            || match &self.notifier {
                Some(notifier) => notifier.inhibited().await,
                None => false,
            };
        let major = moment.level() == Level::Major;
        let chime_now = major && !hush && self.chime.is_some();
        if let Some(notifier) = &self.notifier {
            let icon = card::icon(&moment, &self.icons).ok();
            let card_path = moment.card.as_ref().map(PathBuf::from);
            let summary = format!("{} {}", moment.kind.glyph(), moment.title);
            let notice = Notice {
                summary: &summary,
                body: &moment.line,
                quote: moment.quote.as_deref(),
                icon: icon.as_deref(),
                card: card_path.as_deref(),
                silent: chime_now || hush,
                reply: moment.kind.keeps(),
                timeout_ms: if major { MAJOR_TIMEOUT_MS } else { -1 },
            };
            if let Some(id) = notifier.show(&notice).await {
                self.shown.insert(id, moment.clone());
            }
        }
        if chime_now && let Some(chime) = &self.chime {
            self.play(chime.clone());
        }
        if major
            && !hush
            && let Some(lights) = &self.lights
        {
            lights.pulse(card::tone_rgb(moment.kind.tone()), LIGHTS_HOLD);
        }
        if self.wants_command(&moment) {
            self.run_command(&moment);
        }
    }

    /// The command speaks for moments nobody announced yet; the feed's own bot already told you about sales and reviews.
    fn wants_command(&self, moment: &Moment) -> bool {
        !self.command.is_empty()
            && !moment.announced
            && (moment.kind.keeps() || moment.kind == Kind::FeedQuiet)
    }

    fn run_command(&mut self, moment: &Moment) {
        let card = moment.card.clone().unwrap_or_default();
        let message = moment.message();
        let args: Vec<String> = self
            .command
            .iter()
            .map(|part| {
                part.replace("{text}", &message)
                    .replace("{title}", &moment.title)
                    .replace("{line}", &moment.line)
                    .replace("{value}", &moment.value)
                    .replace("{card}", &card)
                    .replace("{kind}", moment.kind.slug())
                    .replace("{id}", &moment.id)
            })
            .collect();
        let Some((program, rest)) = args.split_first() else {
            return;
        };
        match Command::new(expand(program))
            .args(rest)
            .env(
                "RC_TUI_MOMENT",
                serde_json::to_string(moment).unwrap_or_default(),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
        {
            Ok(child) => self.children.push(child),
            Err(err) => eprintln!("rc-tui watch: command {program} failed: {err}"),
        }
    }

    fn play(&mut self, chime: PathBuf) {
        for player in ["pw-play", "paplay", "aplay", "afplay"] {
            if let Ok(child) = Command::new(player)
                .arg(&chime)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                self.children.push(child);
                return;
            }
        }
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::Open(id) => {
                let Some(moment) = self.shown.get(&id) else {
                    return;
                };
                let Some(terminal) = &self.terminal else {
                    return;
                };
                let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rc-tui"));
                let mut parts = terminal.clone();
                parts.push(exe.to_string_lossy().into_owned());
                parts.push("--moment".to_string());
                parts.push(moment.id.clone());
                if let Some((program, rest)) = parts.split_first()
                    && let Ok(child) = Command::new(program)
                        .args(rest)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                {
                    self.children.push(child);
                }
            }
            Action::Reply(id, text) => {
                let Some(moment) = self.shown.get(&id) else {
                    return;
                };
                if text.trim().is_empty() {
                    return;
                }
                match moments::add_note(&self.log, &moment.id, &text, Utc::now().timestamp_millis())
                {
                    Ok(()) => eprintln!("rc-tui watch: noted on {}: {}", moment.title, text.trim()),
                    Err(err) => eprintln!("rc-tui watch: could not save the note: {err}"),
                }
            }
            Action::Closed(id) => {
                self.shown.remove(&id);
            }
        }
    }

    fn in_quiet_hours(&self) -> bool {
        let Some((start, end)) = self.quiet else {
            return false;
        };
        let now = Local::now();
        within(now.hour() * 60 + now.minute(), start, end)
    }

    fn reap(&mut self) {
        self.children
            .retain_mut(|child| matches!(child.try_wait(), Ok(None)));
    }
}

/// Whether a minute of the day falls in a window that may wrap past midnight.
fn within(minute: u32, start: u32, end: u32) -> bool {
    if start <= end {
        minute >= start && minute < end
    } else {
        minute >= start || minute < end
    }
}

/// Parses quiet hours like `23-08` or `22:30-07:15`; `off` turns them off.
fn parse_quiet(text: &str) -> Option<(u32, u32)> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("off") || text.is_empty() {
        return None;
    }
    let (start, end) = text.split_once('-')?;
    let minute = |part: &str| -> Option<u32> {
        let (hour, minute) = part.trim().split_once(':').unwrap_or((part.trim(), "0"));
        let hour: u32 = hour.parse().ok()?;
        let minute: u32 = minute.parse().ok()?;
        (hour < 24 && minute < 60).then_some(hour * 60 + minute)
    };
    Some((minute(start)?, minute(end)?))
}

fn write_chime(cache: &Path) -> Option<PathBuf> {
    let path = cache.join("chime.wav");
    let wav = chime::wav();
    if std::fs::read(&path).ok().as_deref() != Some(wav.as_slice()) {
        std::fs::create_dir_all(cache).ok()?;
        std::fs::write(&path, wav).ok()?;
    }
    Some(path)
}

/// The terminal that "Open rc-tui" launches: configured, else `$TERMINAL`, else the first one installed.
fn terminal(configured: &[String]) -> Option<Vec<String>> {
    if !configured.is_empty() {
        return Some(configured.to_vec());
    }
    let known: [(&str, &[&str]); 9] = [
        ("ghostty", &["-e"]),
        ("kitty", &[]),
        ("alacritty", &["-e"]),
        ("wezterm", &["start", "--"]),
        ("foot", &[]),
        ("konsole", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("xfce4-terminal", &["-x"]),
        ("xterm", &["-e"]),
    ];
    let preferred = std::env::var("TERMINAL").ok();
    let mut candidates: Vec<(&str, &[&str])> = Vec::new();
    if let Some(name) = preferred.as_deref()
        && let Some(entry) = known.iter().find(|(known, _)| *known == name)
    {
        candidates.push(*entry);
    }
    candidates.extend(known.iter().copied());
    candidates
        .into_iter()
        .find(|(name, _)| on_path(name))
        .map(|(name, args)| {
            std::iter::once(name.to_string())
                .chain(args.iter().map(|arg| arg.to_string()))
                .collect()
        })
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// Expands a leading `~/` to the home directory.
fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_wrap_past_midnight() {
        let quiet = parse_quiet("23-08").unwrap();
        assert_eq!(quiet, (23 * 60, 8 * 60));
        assert!(within(23 * 60 + 30, quiet.0, quiet.1));
        assert!(within(3 * 60, quiet.0, quiet.1));
        assert!(!within(12 * 60, quiet.0, quiet.1));
        assert_eq!(
            parse_quiet("22:30-07:15"),
            Some((22 * 60 + 30, 7 * 60 + 15))
        );
        assert!(within(13 * 60, 12 * 60, 14 * 60));
        assert_eq!(parse_quiet("off"), None);
        assert_eq!(parse_quiet("25-08"), None);
    }

    #[test]
    fn expands_home_in_paths() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand("~/Pictures"), PathBuf::from(home).join("Pictures"));
        assert_eq!(expand("/abs"), PathBuf::from("/abs"));
    }

    #[test]
    fn a_configured_terminal_wins() {
        assert_eq!(
            terminal(&["myterm".to_string(), "-x".to_string()]),
            Some(vec!["myterm".to_string(), "-x".to_string()])
        );
    }
}
