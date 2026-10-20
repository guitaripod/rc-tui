use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use chrono::{DateTime, Local, TimeZone};
use serde::Deserialize;

const FEED_POLL: Duration = Duration::from_secs(2);
const LIVE_AFTER_MS: i64 = 180_000;
const STALE_AFTER_MS: i64 = 600_000;
const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Money,
    Reviews,
}

impl Filter {
    pub const ALL: [Self; 3] = [Self::All, Self::Money, Self::Reviews];

    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Money,
            Self::Money => Self::Reviews,
            Self::Reviews => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Money => "MONEY",
            Self::Reviews => "REVIEWS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    H24,
    D7,
    D28,
}

impl Range {
    pub const ALL: [Self; 3] = [Self::H24, Self::D7, Self::D28];

    pub fn next(self) -> Self {
        match self {
            Self::H24 => Self::D7,
            Self::D7 => Self::D28,
            Self::D28 => Self::H24,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::H24 => Self::D28,
            Self::D7 => Self::H24,
            Self::D28 => Self::D7,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::H24 => "24H",
            Self::D7 => "7D",
            Self::D28 => "28D",
        }
    }

    pub fn millis(self) -> i64 {
        match self {
            Self::H24 => DAY_MS,
            Self::D7 => 7 * DAY_MS,
            Self::D28 => 28 * DAY_MS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CelebrationTone {
    Money,
    Stars,
    Alert,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CelebrationLevel {
    Minor,
    Major,
}

#[derive(Debug, Clone)]
pub struct Celebration {
    pub entry: Entry,
    pub level: CelebrationLevel,
    pub tone: CelebrationTone,
    pub headline: &'static str,
    pub started: Instant,
    pub duration: Duration,
}

impl Celebration {
    pub fn progress(&self, now: Instant) -> Option<f32> {
        let elapsed = now.saturating_duration_since(self.started);
        if elapsed >= self.duration {
            return None;
        }
        Some((elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0))
    }

    pub fn active(&self, now: Instant) -> bool {
        self.progress(now).is_some()
    }

    pub fn major(&self) -> bool {
        self.level == CelebrationLevel::Major
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Purchase,
    Review,
    Rating,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    NotConnected,
    Live,
    Stale,
    Offline,
    Error,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FeedDocument {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Entry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub received_at: i64,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub backfilled: bool,
    #[serde(default)]
    pub purchase: Option<Purchase>,
    #[serde(default)]
    pub review: Option<Review>,
    #[serde(default)]
    pub rating: Option<Rating>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Purchase {
    #[serde(default)]
    pub amount: f64,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub first_ever: bool,
    #[serde(default)]
    pub first_today: bool,
    #[serde(default)]
    pub estimated: bool,
    #[serde(default)]
    pub transaction_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Review {
    #[serde(default)]
    pub stars: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub author: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Rating {
    #[serde(default)]
    pub stars: Option<i64>,
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub average: f64,
}

impl Entry {
    pub fn kind(&self) -> EntryKind {
        match self.kind.as_str() {
            "purchase" if self.purchase.is_some() => EntryKind::Purchase,
            "review" if self.review.is_some() => EntryKind::Review,
            "rating" if self.rating.is_some() => EntryKind::Rating,
            _ => EntryKind::Unknown,
        }
    }

    pub fn is_money(&self) -> bool {
        self.kind() == EntryKind::Purchase
    }

    pub fn is_review(&self) -> bool {
        matches!(self.kind(), EntryKind::Review | EntryKind::Rating)
    }

    pub fn stars(&self) -> Option<i64> {
        match self.kind() {
            EntryKind::Review => self.review.as_ref().map(|review| review.stars),
            EntryKind::Rating => self.rating.as_ref().and_then(|rating| rating.stars),
            _ => None,
        }
    }

    pub fn timestamp(&self) -> i64 {
        if self.at > 0 {
            self.at
        } else {
            self.received_at
        }
    }

    pub fn datetime(&self) -> Option<DateTime<Local>> {
        Local.timestamp_millis_opt(self.timestamp()).single()
    }

    pub fn scope_matches(&self, scope_id: Option<&str>, scope_label: &str) -> bool {
        let Some(scope_id) = scope_id else {
            return true;
        };
        if !self.project_id.is_empty() {
            return self.project_id == scope_id;
        }
        let wanted = name_key(scope_label);
        if wanted.is_empty() {
            return false;
        }
        let app = name_key(&self.app);
        let project = name_key(&self.project);
        if app == wanted || project == wanted {
            return true;
        }
        let wanted_brand = brand_key(scope_label);
        wanted_brand.len() >= 4
            && (wanted_brand == brand_key(&self.app) || wanted_brand == brand_key(&self.project))
    }
}

#[derive(Debug, Clone)]
pub struct Activity {
    pub entries: Vec<Entry>,
    pub path: PathBuf,
    pub health: Health,
    pub last_heartbeat_ms: Option<i64>,
    pub seen_at: i64,
    pub filter: Filter,
    pub range: Range,
    pub selected_id: Option<String>,
    pub open: bool,
    pub error: Option<String>,
    pub demo: bool,
    pub last_arrival: Option<Instant>,
    pub celebration: Option<Celebration>,
    next_poll: Instant,
    seen_was_set: bool,
    loaded_once: bool,
    last_mtime: Option<std::time::SystemTime>,
    last_len: u64,
}

impl Activity {
    pub fn new(path: Option<PathBuf>, demo: bool, seen_at: Option<i64>, now: Instant) -> Self {
        let now_ms = now_ms();
        let mut activity = Self {
            entries: Vec::new(),
            path: path.unwrap_or_else(default_path),
            health: if demo {
                Health::Live
            } else {
                Health::NotConnected
            },
            last_heartbeat_ms: if demo { Some(now_ms) } else { None },
            seen_at: seen_at.unwrap_or(0),
            filter: Filter::All,
            range: Range::D7,
            selected_id: None,
            open: false,
            error: None,
            demo,
            last_arrival: None,
            celebration: None,
            next_poll: now,
            seen_was_set: seen_at.is_some(),
            loaded_once: false,
            last_mtime: None,
            last_len: 0,
        };
        if demo {
            activity.entries = demo_entries(now_ms);
            activity.seen_was_set = true;
            activity.loaded_once = true;
            activity.seen_at = now_ms - 6 * 3_600_000;
            activity.selected_id = activity.entries.first().map(|entry| entry.id.clone());
        } else {
            activity.refresh(now);
        }
        activity
    }

    pub fn refresh(&mut self, now: Instant) -> bool {
        if self.demo || now < self.next_poll {
            return false;
        }
        self.next_poll = now + FEED_POLL;
        let now_ms = now_ms();
        let feed_meta = fs::metadata(&self.path).ok();
        let feed_exists = feed_meta.is_some();
        let mtime = feed_meta.as_ref().and_then(|meta| meta.modified().ok());
        let len = feed_meta.as_ref().map_or(0, |meta| meta.len());
        let mut changed = false;

        if mtime != self.last_mtime || len != self.last_len {
            self.last_mtime = mtime;
            self.last_len = len;
            match fs::read_to_string(&self.path) {
                Ok(text) => match serde_json::from_str::<FeedDocument>(&text) {
                    Ok(document) => {
                        if document.version > 1 {
                            self.error = Some("feed requires a newer rc-tui".to_string());
                        } else {
                            let previous = self.entries.first().map(|entry| entry.id.clone());
                            let previous_max = self.max_received();
                            let previous_ids: HashSet<String> =
                                self.entries.iter().map(|entry| entry.id.clone()).collect();
                            let first_load = !self.loaded_once;
                            self.entries = document
                                .entries
                                .into_iter()
                                .filter_map(sanitize_entry)
                                .collect();
                            self.sort_and_dedupe();
                            self.error = None;
                            let new_max = self.max_received();
                            changed = previous
                                != self.entries.first().map(|entry| entry.id.clone())
                                || new_max > previous_max;
                            if !first_load {
                                let fresh = self
                                    .entries
                                    .iter()
                                    .filter(|entry| {
                                        !entry.backfilled && !previous_ids.contains(&entry.id)
                                    })
                                    .max_by_key(|entry| entry.received_at);
                                if let Some(entry) = fresh {
                                    self.celebration = Some(celebration_for(entry));
                                }
                            }
                            self.loaded_once = true;
                            if !self.seen_was_set && !self.entries.is_empty() {
                                self.seen_at = new_max;
                                self.seen_was_set = true;
                            }
                        }
                    }
                    Err(err) => {
                        self.error = Some(format!("feed unreadable: {err}"));
                    }
                },
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    self.error = None;
                }
                Err(err) => {
                    self.error = Some(format!("feed unreadable: {err}"));
                }
            }
        }

        self.last_heartbeat_ms = fs::read_to_string(alive_path(&self.path))
            .ok()
            .and_then(|text| text.trim().parse::<i64>().ok());
        self.health = if self.error.is_some() {
            Health::Error
        } else if !feed_exists {
            Health::NotConnected
        } else {
            match self.last_heartbeat_ms {
                Some(heartbeat) if now_ms - heartbeat <= LIVE_AFTER_MS => Health::Live,
                Some(heartbeat) if now_ms - heartbeat <= STALE_AFTER_MS => Health::Stale,
                Some(_) => Health::Offline,
                None => Health::Offline,
            }
        };
        if changed {
            self.last_arrival = Some(now);
        }
        changed
    }

    pub fn current_celebration(&self, now: Instant) -> Option<&Celebration> {
        self.celebration
            .as_ref()
            .filter(|celebration| celebration.active(now))
    }

    pub fn is_celebrating(&self, now: Instant) -> bool {
        self.current_celebration(now).is_some()
    }

    pub fn mark_seen(&mut self) -> bool {
        let newest = self.max_received();
        if newest > self.seen_at {
            self.seen_at = newest;
            true
        } else {
            false
        }
    }

    pub fn entries_for(
        &self,
        scope_id: Option<&str>,
        scope_label: &str,
        filter: Filter,
        range: Range,
        now_ms: i64,
    ) -> Vec<&Entry> {
        let cutoff = now_ms.saturating_sub(range.millis());
        self.entries
            .iter()
            .filter(|entry| entry.timestamp() >= cutoff)
            .filter(|entry| entry.scope_matches(scope_id, scope_label))
            .filter(|entry| match filter {
                Filter::All => true,
                Filter::Money => entry.is_money(),
                Filter::Reviews => entry.is_review(),
            })
            .collect()
    }

    pub fn unread_count(&self, scope_id: Option<&str>, scope_label: &str, now_ms: i64) -> usize {
        let cutoff = now_ms.saturating_sub(7 * DAY_MS);
        self.entries
            .iter()
            .filter(|entry| !entry.backfilled)
            .filter(|entry| entry.received_at > self.seen_at)
            .filter(|entry| entry.timestamp() >= cutoff)
            .filter(|entry| entry.scope_matches(scope_id, scope_label))
            .count()
    }

    pub fn attention(
        &self,
        scope_id: Option<&str>,
        scope_label: &str,
        now_ms: i64,
    ) -> Option<&Entry> {
        let cutoff = now_ms.saturating_sub(7 * DAY_MS);
        self.entries
            .iter()
            .filter(|entry| !entry.backfilled)
            .filter(|entry| entry.received_at > self.seen_at)
            .filter(|entry| entry.timestamp() >= cutoff)
            .filter(|entry| entry.scope_matches(scope_id, scope_label))
            .filter(|entry| {
                entry.kind() == EntryKind::Review
                    && entry
                        .review
                        .as_ref()
                        .is_some_and(|review| (1..=2).contains(&review.stars))
            })
            .max_by_key(|entry| entry.received_at)
    }

    pub fn selected_index(&self, entries: &[&Entry]) -> usize {
        self.selected_id
            .as_ref()
            .and_then(|id| entries.iter().position(|entry| &entry.id == id))
            .unwrap_or(0)
    }

    pub fn move_selection_ids(&mut self, ids: &[String], delta: isize) {
        if ids.is_empty() {
            self.selected_id = None;
            return;
        }
        let current = self
            .selected_id
            .as_ref()
            .and_then(|id| ids.iter().position(|candidate| candidate == id))
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(ids.len() as isize) as usize;
        self.selected_id = Some(ids[next].clone());
    }

    pub fn health_label(&self, now_ms: i64) -> String {
        let mut label = match self.health {
            Health::Live => "LIVE",
            Health::Stale => "STALE",
            Health::Offline => "OFFLINE",
            Health::Error => "FEED ERROR",
            Health::NotConnected => "NOT CONNECTED",
        }
        .to_string();
        if matches!(self.health, Health::Stale | Health::Offline)
            && let Some(heartbeat) = self.last_heartbeat_ms
        {
            label.push(' ');
            label.push_str(&age(now_ms - heartbeat));
        }
        label
    }

    fn max_received(&self) -> i64 {
        self.entries
            .iter()
            .map(|entry| entry.received_at)
            .max()
            .unwrap_or(0)
    }

    fn sort_and_dedupe(&mut self) {
        self.entries.sort_by(|left, right| {
            right
                .timestamp()
                .cmp(&left.timestamp())
                .then_with(|| right.received_at.cmp(&left.received_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        let mut seen = HashSet::new();
        self.entries.retain(|entry| seen.insert(entry.id.clone()));
    }
}

pub fn default_path() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"));
    state.join("storebot").join("feed.json")
}

fn alive_path(feed: &Path) -> PathBuf {
    feed.with_file_name("feed.alive")
}

fn sanitize_entry(mut entry: Entry) -> Option<Entry> {
    entry.id = entry.id.trim().to_string();
    if entry.id.is_empty() {
        return None;
    }
    entry.app = one_line(&entry.app);
    entry.project = one_line(&entry.project);
    entry.project_id = entry.project_id.trim().to_string();
    entry.app_id = entry.app_id.trim().to_string();
    entry.country = entry.country.trim().to_ascii_uppercase();
    entry.source = entry.source.trim().to_ascii_lowercase();
    entry.text = one_line(&entry.text);
    if entry.received_at == 0 {
        entry.received_at = entry.at;
    }
    if entry.at == 0 {
        entry.at = entry.received_at;
    }
    if let Some(purchase) = &mut entry.purchase {
        purchase.currency = purchase.currency.trim().to_ascii_uppercase();
        purchase.product = one_line(&purchase.product);
        purchase.event = purchase.event.trim().to_ascii_lowercase();
        purchase.environment = purchase.environment.trim().to_ascii_lowercase();
    }
    if let Some(review) = &mut entry.review {
        review.title = one_line(&review.title);
        review.body = clean_body(&review.body, 2000);
        review.author = one_line(&review.author);
    }
    Some(entry)
}

fn one_line(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn clean_body(value: &str, limit: usize) -> String {
    let mut text = String::new();
    for character in value.chars() {
        if character == '\n' || !character.is_control() {
            text.push(character);
        } else {
            text.push(' ');
        }
    }
    text.chars().take(limit).collect()
}

fn name_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn brand_key(value: &str) -> String {
    value
        .chars()
        .take_while(|character| !character.is_whitespace())
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

fn age(milliseconds: i64) -> String {
    let seconds = milliseconds.max(0) / 1000;
    match seconds {
        0..=2 => "now".to_string(),
        3..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        _ => format!("{}h", seconds / 3600),
    }
}

fn celebration_for(entry: &Entry) -> Celebration {
    let (headline, tone, level) = match entry.kind() {
        EntryKind::Purchase => {
            let purchase = entry.purchase.as_ref();
            if purchase.is_some_and(|purchase| purchase.event == "revenue") {
                (
                    "REVENUE MOVED",
                    CelebrationTone::Quiet,
                    CelebrationLevel::Minor,
                )
            } else if purchase.is_some_and(|purchase| purchase.first_ever) {
                (
                    "FIRST EVER SALE",
                    CelebrationTone::Money,
                    CelebrationLevel::Major,
                )
            } else if purchase.is_some_and(|purchase| purchase.first_today) {
                (
                    "FIRST SALE TODAY",
                    CelebrationTone::Money,
                    CelebrationLevel::Major,
                )
            } else if purchase.is_some_and(|purchase| purchase.amount >= 25.0) {
                ("BIG SALE", CelebrationTone::Money, CelebrationLevel::Major)
            } else if purchase.is_some_and(|purchase| purchase.event == "renewal") {
                ("RENEWAL", CelebrationTone::Money, CelebrationLevel::Minor)
            } else {
                ("NEW SALE", CelebrationTone::Money, CelebrationLevel::Minor)
            }
        }
        EntryKind::Review => match entry.stars().unwrap_or(0) {
            stars if stars <= 2 => (
                "NEEDS ATTENTION",
                CelebrationTone::Alert,
                CelebrationLevel::Major,
            ),
            5 => (
                "FIVE STARS",
                CelebrationTone::Stars,
                CelebrationLevel::Major,
            ),
            _ => (
                "NEW REVIEW",
                CelebrationTone::Stars,
                CelebrationLevel::Minor,
            ),
        },
        EntryKind::Rating => match entry.stars().unwrap_or(0) {
            stars if stars <= 2 => (
                "RATING DROPPED",
                CelebrationTone::Alert,
                CelebrationLevel::Major,
            ),
            _ => (
                "NEW RATING",
                CelebrationTone::Quiet,
                CelebrationLevel::Minor,
            ),
        },
        EntryKind::Unknown => ("ACTIVITY", CelebrationTone::Quiet, CelebrationLevel::Minor),
    };
    Celebration {
        entry: entry.clone(),
        level,
        tone,
        headline,
        started: Instant::now(),
        duration: match level {
            CelebrationLevel::Major => Duration::from_millis(2600),
            CelebrationLevel::Minor => Duration::from_millis(1400),
        },
    }
}

fn demo_entries(now: i64) -> Vec<Entry> {
    let project = |id: &str, app: &str| (id.to_string(), app.to_string());
    let purchase = |id: &str,
                    project_id: &str,
                    app: &str,
                    product: &str,
                    amount: f64,
                    currency: &str,
                    country: &str,
                    event: &str,
                    offset: i64| {
        let (project_id, app) = project(project_id, app);
        Entry {
            id: id.to_string(),
            kind: "purchase".to_string(),
            at: now - offset,
            received_at: now - offset,
            app: app.clone(),
            project: app,
            project_id,
            country: country.to_string(),
            source: "demo".to_string(),
            text: format!("{product} {amount:.2} {currency}"),
            purchase: Some(Purchase {
                amount,
                currency: currency.to_string(),
                product: product.to_string(),
                event: event.to_string(),
                environment: "production".to_string(),
                first_ever: false,
                first_today: false,
                estimated: false,
                transaction_id: id.to_string(),
            }),
            ..Entry::default()
        }
    };
    let review = |id: &str,
                  project_id: &str,
                  app: &str,
                  stars: i64,
                  title: &str,
                  body: &str,
                  author: &str,
                  country: &str,
                  offset: i64| {
        let (project_id, app) = project(project_id, app);
        Entry {
            id: id.to_string(),
            kind: "review".to_string(),
            at: now - offset,
            received_at: now - offset,
            app: app.clone(),
            project: app,
            project_id,
            country: country.to_string(),
            source: "asc_reviews".to_string(),
            text: title.to_string(),
            review: Some(Review {
                stars,
                title: title.to_string(),
                body: body.to_string(),
                author: author.to_string(),
            }),
            ..Entry::default()
        }
    };
    let rating = |id: &str, project_id: &str, app: &str, stars: i64, country: &str, offset: i64| {
        let (project_id, app) = project(project_id, app);
        Entry {
            id: id.to_string(),
            kind: "rating".to_string(),
            at: now - offset,
            received_at: now - offset,
            app: app.clone(),
            project: app,
            project_id,
            country: country.to_string(),
            source: "itunes".to_string(),
            text: "new rating".to_string(),
            rating: Some(Rating {
                stars: Some(stars),
                count: 1,
                average: 4.9,
            }),
            ..Entry::default()
        }
    };
    let mut entries = vec![
        purchase(
            "demo:sale:1",
            "proj_demo_1",
            "MagicWeather",
            "Pro Monthly",
            14.99,
            "USD",
            "US",
            "renewal",
            2 * 60_000,
        ),
        review(
            "demo:review:1",
            "proj_demo_2",
            "Notes",
            5,
            "Finally an app that respects battery",
            "The best notes app I have used. It shows the forecast clearly, explains the assumptions and stays out of the way.",
            "nick",
            "FI",
            18 * 60_000,
        ),
        purchase(
            "demo:sale:2",
            "proj_demo_3",
            "Habits",
            "Pro Annual",
            39.99,
            "EUR",
            "DE",
            "new",
            55 * 60_000,
        ),
        rating(
            "demo:rating:1",
            "proj_demo_4",
            "Focus",
            4,
            "SE",
            2 * 3_600_000,
        ),
        review(
            "demo:review:2",
            "proj_demo_5",
            "Budget",
            2,
            "Needs one more update",
            "The new sync is unreliable. Please fix the duplicate transactions before I can recommend this.",
            "traveler",
            "GB",
            5 * 3_600_000,
        ),
        purchase(
            "demo:sale:3",
            "proj_demo_7",
            "Orbit",
            "Lifetime",
            9.99,
            "EUR",
            "ES",
            "purchase",
            26 * 3_600_000,
        ),
        review(
            "demo:review:3",
            "proj_demo_8",
            "Ledger",
            4,
            "Clean and fast",
            "The monthly view is exactly what I needed.",
            "mira",
            "CA",
            30 * 3_600_000,
        ),
        purchase(
            "demo:sale:4",
            "proj_demo_9",
            "Sketch",
            "Pro Monthly",
            4.99,
            "USD",
            "CA",
            "renewal",
            7 * 3_600_000,
        ),
        review(
            "demo:review:4",
            "proj_demo_10",
            "Pulse",
            5,
            "A quiet favorite",
            "The haptics and the dark theme make this feel considered.",
            "alex",
            "NL",
            9 * 3_600_000,
        ),
        rating(
            "demo:rating:2",
            "proj_demo_11",
            "Garden",
            5,
            "JP",
            11 * 3_600_000,
        ),
        purchase(
            "demo:sale:5",
            "proj_demo_12",
            "Atlas",
            "Lifetime",
            19.99,
            "EUR",
            "FR",
            "purchase",
            14 * 3_600_000,
        ),
        review(
            "demo:review:5",
            "proj_demo_13",
            "Ember",
            4,
            "Warm and quick",
            "The new timer presets are a small but welcome touch.",
            "sam",
            "AU",
            20 * 3_600_000,
        ),
    ];
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.timestamp()));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(project_id: &str, app: &str) -> Entry {
        Entry {
            id: "test".to_string(),
            kind: "purchase".to_string(),
            app: app.to_string(),
            project: app.to_string(),
            project_id: project_id.to_string(),
            purchase: Some(Purchase::default()),
            ..Entry::default()
        }
    }

    #[test]
    fn all_projects_match_every_entry() {
        assert!(entry("proj1", "One").scope_matches(None, "All projects"));
    }

    #[test]
    fn project_ids_win_over_names() {
        assert!(entry("proj1", "One").scope_matches(Some("proj1"), "Different"));
        assert!(!entry("proj1", "One").scope_matches(Some("proj2"), "One"));
    }

    #[test]
    fn names_and_brands_match_app_store_titles() {
        assert!(
            entry("", "Flaccy – Lossless FLAC Player").scope_matches(Some("proj1"), "Flaccy iOS")
        );
        assert!(entry("", "Master of Flags").scope_matches(Some("proj1"), "Master of Flags: Quiz"));
    }

    #[test]
    fn review_text_loses_terminal_control_characters() {
        let mut entry = Entry {
            id: "review".to_string(),
            kind: "review".to_string(),
            app: "App".to_string(),
            review: Some(Review {
                title: "Hi\u{1b}[31m".to_string(),
                body: "line\u{7}\nnext".to_string(),
                ..Review::default()
            }),
            ..Entry::default()
        };
        entry = sanitize_entry(entry).unwrap();
        assert!(!entry.review.as_ref().unwrap().title.contains('\u{1b}'));
        assert!(entry.review.as_ref().unwrap().body.contains('\n'));
    }

    #[test]
    fn demo_activity_has_both_money_and_reviews() {
        let entries = demo_entries(1_760_000_000_000);
        assert!(entries.iter().any(Entry::is_money));
        assert!(entries.iter().any(Entry::is_review));
        assert!(
            entries
                .windows(2)
                .all(|pair| pair[0].timestamp() >= pair[1].timestamp())
        );
    }

    #[test]
    fn significant_events_get_a_full_celebration() {
        let mut first = entry("proj1", "One");
        first.purchase = Some(Purchase {
            amount: 3.99,
            first_ever: true,
            first_today: true,
            ..Purchase::default()
        });
        let first = celebration_for(&first);
        assert_eq!(first.level, CelebrationLevel::Major);
        assert_eq!(first.headline, "FIRST EVER SALE");

        let mut review = entry("proj1", "One");
        review.kind = "review".to_string();
        review.purchase = None;
        review.review = Some(Review {
            stars: 5,
            ..Review::default()
        });
        let review = celebration_for(&review);
        assert_eq!(review.level, CelebrationLevel::Major);
        assert_eq!(review.headline, "FIVE STARS");

        let mut warning = entry("proj1", "One");
        warning.kind = "review".to_string();
        warning.purchase = None;
        warning.review = Some(Review {
            stars: 2,
            ..Review::default()
        });
        let warning = celebration_for(&warning);
        assert_eq!(warning.level, CelebrationLevel::Major);
        assert_eq!(warning.headline, "NEEDS ATTENTION");
    }

    #[test]
    fn ranges_filter_by_event_time() {
        let now = 1_760_000_000_000;
        let mut item = entry("proj1", "One");
        item.at = now - 2 * DAY_MS;
        item.received_at = now;
        let activity = Activity {
            entries: vec![item],
            path: PathBuf::new(),
            health: Health::Live,
            last_heartbeat_ms: Some(now),
            seen_at: 0,
            filter: Filter::All,
            range: Range::D7,
            selected_id: None,
            open: false,
            error: None,
            demo: true,
            last_arrival: None,
            celebration: None,
            next_poll: Instant::now(),
            seen_was_set: true,
            loaded_once: true,
            last_mtime: None,
            last_len: 0,
        };
        assert!(
            activity
                .entries_for(None, "All projects", Filter::All, Range::H24, now)
                .is_empty()
        );
        assert_eq!(
            activity
                .entries_for(None, "All projects", Filter::All, Range::D7, now)
                .len(),
            1
        );
    }
}
