use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use chrono::{Datelike, Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};

use crate::activity::{Entry, EntryKind};
use crate::countries;
use crate::ui::{grouped, grouped_money};

const PEOPLE_STEPS: [f64; 13] = [
    100.0,
    250.0,
    500.0,
    1_000.0,
    2_500.0,
    5_000.0,
    10_000.0,
    25_000.0,
    50_000.0,
    100_000.0,
    250_000.0,
    500_000.0,
    1_000_000.0,
];
const APP_PEOPLE_STEPS: [f64; 4] = [1_000.0, 10_000.0, 100_000.0, 1_000_000.0];
const WINDOW_DAYS: usize = 28;
const RECORD_FLOOR: f64 = 1.0;
const RECORD_MARGIN: f64 = 0.5;
const BEST_MONTH_EVERY_DAYS: i64 = 28;
const ENOUGH_REARM: f64 = 0.9;
const QUIET_AFTER: Duration = Duration::from_secs(15 * 60);
const BACK_WITHIN_MS: i64 = 3 * 60_000;
const RECENT_DAYS: usize = 7;
const SHELF_POLL: Duration = Duration::from_secs(2);
const QUOTE_LIMIT: usize = 280;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    FirstSale,
    NewCountry,
    FirstYearly,
    BestDay,
    BestMonth,
    Enough,
    People,
    AppPeople,
    FiveStars,
    Attention,
    FeedQuiet,
    FeedBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Major,
    Minor,
    Alert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Money,
    People,
    World,
    Stars,
    Alert,
    Quiet,
}

impl Kind {
    pub fn level(self) -> Level {
        match self {
            Self::FirstSale | Self::BestDay | Self::BestMonth | Self::Enough | Self::People => {
                Level::Major
            }
            Self::NewCountry
            | Self::FirstYearly
            | Self::AppPeople
            | Self::FiveStars
            | Self::FeedBack => Level::Minor,
            Self::Attention | Self::FeedQuiet => Level::Alert,
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::FirstSale
            | Self::BestDay
            | Self::BestMonth
            | Self::Enough
            | Self::FirstYearly => Tone::Money,
            Self::People | Self::AppPeople => Tone::People,
            Self::NewCountry => Tone::World,
            Self::FiveStars => Tone::Stars,
            Self::Attention | Self::FeedQuiet => Tone::Alert,
            Self::FeedBack => Tone::Quiet,
        }
    }

    /// Moments worth keeping: they reach the home screen, the list, cards and the yearbook; alerts only notify.
    pub fn keeps(self) -> bool {
        !matches!(self, Self::Attention | Self::FeedQuiet | Self::FeedBack)
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Self::FirstSale => "✺",
            Self::NewCountry => "◎",
            Self::FirstYearly => "◷",
            Self::BestDay | Self::BestMonth => "✦",
            Self::Enough => "◆",
            Self::People | Self::AppPeople => "●",
            Self::FiveStars => "★",
            Self::Attention => "▲",
            Self::FeedQuiet => "◌",
            Self::FeedBack => "◉",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::FirstSale => "first-sale",
            Self::NewCountry => "new-country",
            Self::FirstYearly => "first-yearly",
            Self::BestDay => "best-day",
            Self::BestMonth => "best-28-days",
            Self::Enough => "enough",
            Self::People => "people",
            Self::AppPeople => "app-people",
            Self::FiveStars => "five-stars",
            Self::Attention => "attention",
            Self::FeedQuiet => "feed-quiet",
            Self::FeedBack => "feed-back",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Moment {
    pub id: String,
    pub kind: Kind,
    pub at: i64,
    pub title: String,
    pub value: String,
    pub line: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub series: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub announced: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub test: bool,
}

impl Moment {
    fn new(id: String, kind: Kind, at: i64, title: String, value: String, line: String) -> Self {
        Self {
            id,
            kind,
            at,
            title,
            value,
            line,
            app: None,
            country: None,
            quote: None,
            series: Vec::new(),
            mark: None,
            card: None,
            note: None,
            announced: false,
            test: false,
        }
    }

    pub fn level(&self) -> Level {
        self.kind.level()
    }

    /// The message for a phone or a chat: glyph, title, the sentence, then the note when there is one.
    pub fn message(&self) -> String {
        let mut text = format!("{} {}\n{}", self.kind.glyph(), self.title, self.line);
        if self.test {
            text.push_str("\n\nA sample from rc-tui moments --test.");
        }
        if let Some(quote) = &self.quote {
            text.push_str(&format!("\n\n“{quote}”"));
        }
        if let Some(note) = &self.note {
            text.push_str(&format!("\n\n{note}"));
        }
        text
    }

    pub fn local_time(&self) -> Option<chrono::DateTime<Local>> {
        Local.timestamp_millis_opt(self.at).single()
    }

    /// A moment picked to show what one looks like, marked as a test everywhere it lands.
    pub fn sample(currency: &str, now: i64) -> Self {
        let series: Vec<f64> = (0..57)
            .map(|day| {
                let wave = ((day as f64) / 4.0).sin() * 9.0;
                (38.0 + day as f64 * 0.9 + wave).max(4.0)
            })
            .chain([182.0])
            .collect();
        let mut moment = Self::new(
            format!("test:{now}"),
            Kind::BestDay,
            now,
            "Best day ever".to_string(),
            grouped_money(182.0, currency),
            format!(
                "{} today, your best day ever. The old record was {}.",
                grouped_money(182.0, currency),
                grouped_money(140.0, currency)
            ),
        );
        moment.series = series;
        moment.mark = Some(140.0);
        moment.test = true;
        moment
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Record {
    Moment(Box<Moment>),
    Note { id: String, text: String, at: i64 },
}

pub fn log_path() -> PathBuf {
    crate::config::state_dir()
        .join("rc-tui")
        .join("moments.jsonl")
}

pub fn state_path() -> PathBuf {
    crate::config::state_dir()
        .join("rc-tui")
        .join("moments-state.json")
}

/// Appends one moment to the log, one JSON object per line.
pub fn append(path: &Path, moment: &Moment) -> io::Result<()> {
    write_record(path, &Record::Moment(Box::new(moment.clone())))
}

/// Attaches a note (what made it happen) to a logged moment.
pub fn add_note(path: &Path, id: &str, text: &str, at: i64) -> io::Result<()> {
    write_record(
        path,
        &Record::Note {
            id: id.to_string(),
            text: text.trim().to_string(),
            at,
        },
    )
}

fn write_record(path: &Path, record: &Record) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(record).map_err(io::Error::other)?;
    line.push('\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())
}

/// Reads the log oldest first; notes land on their moment and a repeated id keeps its latest copy.
pub fn load(path: &Path) -> Vec<Moment> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    parse(&text)
}

fn parse(text: &str) -> Vec<Moment> {
    let mut moments: Vec<Moment> = Vec::new();
    let mut notes: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Record>(line) {
            Ok(Record::Moment(moment)) => {
                moments.retain(|existing| existing.id != moment.id);
                moments.push(*moment);
            }
            Ok(Record::Note { id, text, .. }) => {
                notes.insert(id, text);
            }
            Err(_) => {}
        }
    }
    for moment in &mut moments {
        if let Some(note) = notes.remove(&moment.id) {
            moment.note = Some(note).filter(|note| !note.is_empty());
        }
    }
    moments.sort_by_key(|moment| moment.at);
    moments
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Best {
    pub value: f64,
    pub day: NaiveDate,
}

/// What the watcher remembers between runs: records, milestones already crossed, and every first already seen.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub seeded: bool,
    pub apps: BTreeSet<String>,
    pub countries: BTreeSet<String>,
    pub sold: BTreeSet<String>,
    pub yearly: BTreeSet<String>,
    pub best_day: Option<Best>,
    pub best_month: Option<Best>,
    pub best_month_told: Option<NaiveDate>,
    pub people: f64,
    pub app_people: BTreeMap<String, f64>,
    pub enough: Option<f64>,
    pub enough_reached: bool,
    pub feed_seeded: bool,
    pub feed_mark: i64,
    pub feed_quiet: bool,
}

impl State {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_string_pretty(self).map_err(io::Error::other)?,
        )?;
        fs::rename(temporary, path)
    }
}

/// One app as RevenueCat reports it: the 28-day overview plus daily revenue and what sold recently.
#[derive(Debug, Clone, Default)]
pub struct AppReading {
    pub name: String,
    pub people: Option<f64>,
    pub revenue: Option<f64>,
    pub days: Vec<(NaiveDate, f64)>,
    pub countries: Vec<String>,
    pub yearly: bool,
    pub sold: bool,
}

/// Every app at one moment in time; `complete` is false when any request failed.
#[derive(Debug, Clone)]
pub struct Reading {
    pub apps: Vec<AppReading>,
    pub complete: bool,
    pub today: NaiveDate,
    pub currency: String,
    pub enough: Option<f64>,
    pub feed: bool,
}

/// Compares a reading with what the watcher remembers and returns what is new; until seeded it only learns.
pub fn observe(state: &mut State, reading: &Reading, now: i64) -> Vec<Moment> {
    let speak = state.seeded;
    let mut moments = Vec::new();
    firsts(state, reading, now, speak, &mut moments);
    for app in &reading.apps {
        state.apps.insert(app_key(&app.name));
    }
    records(state, reading, now, speak, &mut moments);
    people(state, reading, now, speak, &mut moments);
    enough(state, reading, now, speak, &mut moments);
    if !state.seeded && reading.complete {
        state.seeded = true;
    }
    moments
}

fn firsts(state: &mut State, reading: &Reading, now: i64, speak: bool, out: &mut Vec<Moment>) {
    for app in &reading.apps {
        let key = app_key(&app.name);
        let name = short_name(&app.name).to_string();
        let speak = speak && state.apps.contains(&key);
        if app.sold && state.sold.insert(key.clone()) && speak && !reading.feed {
            out.push(first_sale(&key, &name, None, now));
        }
        for code in &app.countries {
            if state.countries.insert(code.clone()) && speak {
                out.push(new_country(code, &name, state.countries.len(), now));
            }
        }
        if app.yearly && state.yearly.insert(key.clone()) && speak {
            out.push(first_yearly(&key, &name, now));
        }
    }
}

fn first_sale(key: &str, name: &str, detail: Option<String>, now: i64) -> Moment {
    let line = match detail {
        Some(detail) => {
            format!("{name} made its first sale: {detail}. Someone paid for something you made.")
        }
        None => format!("{name} made its first sale. Someone paid for something you made."),
    };
    let mut moment = Moment::new(
        format!("first-sale:{key}"),
        Kind::FirstSale,
        now,
        "First sale".to_string(),
        name.to_string(),
        line,
    );
    moment.app = Some(name.to_string());
    moment
}

fn new_country(code: &str, app: &str, total: usize, now: i64) -> Moment {
    let place = countries::name(code).unwrap_or(code).to_string();
    let mut moment = Moment::new(
        format!("country:{code}"),
        Kind::NewCountry,
        now,
        format!("First purchase from {place}"),
        countries::label(code),
        format!(
            "Someone in {place} bought {app}. Your apps have now sold in {total} {}.",
            if total == 1 { "country" } else { "countries" }
        ),
    );
    moment.app = Some(app.to_string());
    moment.country = Some(code.to_string());
    moment
}

fn first_yearly(key: &str, name: &str, now: i64) -> Moment {
    let mut moment = Moment::new(
        format!("first-yearly:{key}"),
        Kind::FirstYearly,
        now,
        "First yearly subscriber".to_string(),
        name.to_string(),
        format!("Someone chose a whole year of {name}."),
    );
    moment.app = Some(name.to_string());
    moment
}

fn records(state: &mut State, reading: &Reading, now: i64, speak: bool, out: &mut Vec<Moment>) {
    let mut totals: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for app in &reading.apps {
        for (day, value) in &app.days {
            *totals.entry(*day).or_default() += value;
        }
    }
    if totals.is_empty() {
        return;
    }
    let days: Vec<(NaiveDate, f64)> = totals.into_iter().collect();
    let recent = reading.today.pred_opt().unwrap_or(reading.today);

    let mut celebrated: Option<(Best, Option<Best>)> = None;
    for (day, value) in &days {
        let previous = state.best_day;
        if *value < RECORD_FLOOR
            || previous.is_some_and(|best| *value <= best.value + RECORD_MARGIN)
        {
            continue;
        }
        state.best_day = Some(Best {
            value: *value,
            day: *day,
        });
        if speak
            && *day >= recent
            && let Some(previous) = previous
            && previous.day != *day
        {
            celebrated = Some((
                Best {
                    value: *value,
                    day: *day,
                },
                Some(previous),
            ));
        }
    }
    if let Some((best, previous)) = celebrated {
        out.push(best_day(best, previous, reading, &days, now));
    }

    let mut sums: Vec<(NaiveDate, f64)> = Vec::new();
    for end in WINDOW_DAYS.saturating_sub(1)..days.len() {
        let sum: f64 = days[end + 1 - WINDOW_DAYS..=end]
            .iter()
            .map(|(_, value)| value)
            .sum();
        sums.push((days[end].0, sum));
    }
    let mut celebrated: Option<(Best, Best)> = None;
    for (day, sum) in &sums {
        let previous = state.best_month;
        if *sum < RECORD_FLOOR || previous.is_some_and(|best| *sum <= best.value + RECORD_MARGIN) {
            continue;
        }
        state.best_month = Some(Best {
            value: *sum,
            day: *day,
        });
        let rested = state
            .best_month_told
            .is_none_or(|told| (*day - told).num_days() >= BEST_MONTH_EVERY_DAYS);
        if speak
            && *day >= recent
            && rested
            && let Some(previous) = previous
        {
            celebrated = Some((
                Best {
                    value: *sum,
                    day: *day,
                },
                previous,
            ));
        }
    }
    if let Some((best, previous)) = celebrated {
        state.best_month_told = Some(best.day);
        let mut moment = Moment::new(
            format!("best-28-days:{}", best.day),
            Kind::BestMonth,
            now,
            "Best 28 days ever".to_string(),
            grouped_money(best.value, &reading.currency),
            format!(
                "{} in the last 28 days, more than any 28 days before. The previous best was {}.",
                grouped_money(best.value, &reading.currency),
                grouped_money(previous.value, &reading.currency)
            ),
        );
        moment.series = sums
            .iter()
            .rev()
            .take(57)
            .rev()
            .map(|(_, sum)| *sum)
            .collect();
        moment.mark = Some(previous.value);
        out.push(moment);
    }
}

fn best_day(
    best: Best,
    previous: Option<Best>,
    reading: &Reading,
    days: &[(NaiveDate, f64)],
    now: i64,
) -> Moment {
    let money = |value: f64| grouped_money(value, &reading.currency);
    let when = if best.day == reading.today {
        "today"
    } else {
        "yesterday"
    };
    let mut line = format!("{} {when}, your best day ever.", money(best.value));
    if let Some(previous) = previous {
        line.push_str(&format!(
            " The old record was {} on {}.",
            money(previous.value),
            short_date(previous.day, reading.today)
        ));
    }
    let leader = reading
        .apps
        .iter()
        .filter_map(|app| {
            app.days
                .iter()
                .find(|(day, _)| *day == best.day)
                .map(|(_, value)| (short_name(&app.name), *value))
        })
        .max_by(|left, right| left.1.total_cmp(&right.1));
    if let Some((name, value)) = leader
        && value >= best.value * 0.5
        && reading.apps.len() > 1
    {
        line.push_str(&format!(" {name} brought {} of it.", money(value)));
    }
    let mut moment = Moment::new(
        format!("best-day:{}", best.day),
        Kind::BestDay,
        now,
        "Best day ever".to_string(),
        money(best.value),
        line,
    );
    moment.series = days
        .iter()
        .rev()
        .take(57)
        .rev()
        .map(|(_, value)| *value)
        .collect();
    moment.mark = previous.map(|previous| previous.value);
    moment
}

fn short_date(day: NaiveDate, today: NaiveDate) -> String {
    if day.year() == today.year() {
        day.format("%-d %b").to_string()
    } else {
        day.format("%-d %b %Y").to_string()
    }
}

fn people(state: &mut State, reading: &Reading, now: i64, speak: bool, out: &mut Vec<Moment>) {
    for app in &reading.apps {
        let Some(count) = app.people else {
            continue;
        };
        let key = app_key(&app.name);
        let step = highest_step(&APP_PEOPLE_STEPS, count).unwrap_or(0.0);
        let Some(crossed) = state.app_people.get(&key).copied() else {
            state.app_people.insert(key, step);
            continue;
        };
        if step > crossed {
            state.app_people.insert(key.clone(), step);
            if speak {
                let name = short_name(&app.name).to_string();
                let mut moment = Moment::new(
                    format!("app-people:{key}:{}", step as u64),
                    Kind::AppPeople,
                    now,
                    format!("{name} reached {} people", grouped(step)),
                    grouped(step),
                    format!(
                        "For the first time, more than {} people used {name} in 28 days.",
                        grouped(step)
                    ),
                );
                moment.app = Some(name);
                out.push(moment);
            }
        }
    }
    if !reading.complete {
        return;
    }
    let total: f64 = reading.apps.iter().filter_map(|app| app.people).sum();
    if let Some(step) = highest_step(&PEOPLE_STEPS, total).filter(|step| *step > state.people) {
        state.people = step;
        if speak {
            out.push(Moment::new(
                format!("people:{}", step as u64),
                Kind::People,
                now,
                format!("{} people", grouped(step)),
                grouped(step),
                format!(
                    "For the first time, more than {} people used your apps in 28 days.",
                    grouped(step)
                ),
            ));
        }
    }
}

fn highest_step(steps: &[f64], count: f64) -> Option<f64> {
    steps.iter().copied().rev().find(|step| count >= *step)
}

fn enough(state: &mut State, reading: &Reading, now: i64, speak: bool, out: &mut Vec<Moment>) {
    let Some(target) = reading.enough.filter(|target| *target > 0.0) else {
        state.enough = None;
        return;
    };
    if !reading.complete {
        return;
    }
    let total: f64 = reading.apps.iter().filter_map(|app| app.revenue).sum();
    if state.enough != Some(target) {
        state.enough = Some(target);
        state.enough_reached = total >= target;
        return;
    }
    if state.enough_reached {
        if total < target * ENOUGH_REARM {
            state.enough_reached = false;
        }
        return;
    }
    if total >= target {
        state.enough_reached = true;
        if speak {
            let money = |value: f64| grouped_money(value, &reading.currency);
            let mut moment = Moment::new(
                format!("enough:{}", reading.today),
                Kind::Enough,
                now,
                "Enough".to_string(),
                money(total),
                format!(
                    "The last 28 days paid {}, past the {} you call enough. Everything beyond it is room to breathe.",
                    money(total),
                    money(target)
                ),
            );
            moment.mark = Some(target);
            out.push(moment);
        }
    }
}

/// Reads storebot's feed for firsts, reviews worth keeping and a feed that stopped breathing.
pub fn observe_feed(
    state: &mut State,
    entries: &[Entry],
    heartbeat: Option<i64>,
    quiet_for: Duration,
    now: i64,
) -> Vec<Moment> {
    let mut out = Vec::new();
    let mark = entries
        .iter()
        .map(|entry| entry.received_at)
        .max()
        .unwrap_or(0);
    if !state.feed_seeded {
        for entry in entries {
            learn_purchase(state, entry);
        }
        state.feed_mark = mark;
        state.feed_seeded = true;
        return out;
    }
    let mut fresh: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.received_at > state.feed_mark && !entry.backfilled)
        .collect();
    fresh.sort_by_key(|entry| entry.received_at);
    let speak = state.seeded;
    for entry in fresh {
        match entry.kind() {
            EntryKind::Purchase => purchase_moments(state, entry, speak, now, &mut out),
            EntryKind::Review => {
                if let Some(moment) = review_moment(entry, now) {
                    out.push(moment);
                }
            }
            _ => {}
        }
    }
    state.feed_mark = state.feed_mark.max(mark);

    if let Some(heartbeat) = heartbeat {
        let age = now - heartbeat;
        if !state.feed_quiet && quiet_for >= QUIET_AFTER {
            state.feed_quiet = true;
            out.push(Moment::new(
                format!("feed-quiet:{now}"),
                Kind::FeedQuiet,
                now,
                "The activity feed went quiet".to_string(),
                format!("{} min", age.max(0) / 60_000),
                format!(
                    "No heartbeat from storebot for {} minutes. Live sales and reviews pause until it is back.",
                    age.max(0) / 60_000
                ),
            ));
        } else if state.feed_quiet && age <= BACK_WITHIN_MS {
            state.feed_quiet = false;
            out.push(Moment::new(
                format!("feed-back:{now}"),
                Kind::FeedBack,
                now,
                "The activity feed is back".to_string(),
                "live".to_string(),
                "storebot is breathing again. Live sales and reviews are flowing.".to_string(),
            ));
        }
    }
    out
}

fn counts_as_sale(entry: &Entry) -> bool {
    entry.purchase.as_ref().is_some_and(|purchase| {
        !matches!(
            purchase.event.as_str(),
            "revenue" | "refund" | "cancellation" | "expiration" | "billing_issue"
        ) && purchase.environment != "sandbox"
    })
}

fn learn_purchase(state: &mut State, entry: &Entry) {
    if entry.kind() != EntryKind::Purchase || !counts_as_sale(entry) {
        return;
    }
    state.sold.insert(app_key(entry_app(entry)));
    if !entry.country.is_empty() {
        state.countries.insert(entry.country.clone());
    }
}

fn purchase_moments(
    state: &mut State,
    entry: &Entry,
    speak: bool,
    now: i64,
    out: &mut Vec<Moment>,
) {
    let Some(purchase) = entry.purchase.as_ref().filter(|_| counts_as_sale(entry)) else {
        return;
    };
    let key = app_key(entry_app(entry));
    let name = short_name(entry_app(entry)).to_string();
    let place = (!entry.country.is_empty()).then(|| countries::label(&entry.country));
    if state.sold.insert(key.clone()) && speak && purchase.first_ever {
        let amount = if purchase.amount > 0.0 {
            Some(crate::ui::format_metric(
                purchase.amount,
                &purchase.currency,
                &purchase.currency,
            ))
        } else {
            None
        };
        let detail = match (amount, &place) {
            (Some(amount), Some(place)) => Some(format!("{amount} from {place}")),
            (Some(amount), None) => Some(amount),
            (None, Some(place)) => Some(format!("from {place}")),
            (None, None) => None,
        };
        let mut moment = first_sale(&key, &name, detail, now);
        moment.announced = true;
        out.push(moment);
    }
    let paid = purchase.amount > 0.0;
    if paid && !entry.country.is_empty() && state.countries.insert(entry.country.clone()) && speak {
        out.push(new_country(
            &entry.country,
            &name,
            state.countries.len(),
            now,
        ));
    }
    let initial = matches!(
        purchase.event.as_str(),
        "" | "purchase" | "initial_purchase"
    );
    if initial
        && paid
        && is_yearly(&format!("{} {}", purchase.product, purchase.product_id))
        && state.yearly.insert(key.clone())
        && speak
    {
        out.push(first_yearly(&key, &name, now));
    }
}

fn review_moment(entry: &Entry, now: i64) -> Option<Moment> {
    let review = entry.review.as_ref()?;
    let app = short_name(entry_app(entry)).to_string();
    let title = review.title.trim();
    let body = review.body.trim();
    let mut moment = match review.stars {
        5 if body.chars().count() >= 12 => Moment::new(
            format!("review:{}", entry.id),
            Kind::FiveStars,
            now,
            "Five stars".to_string(),
            "★★★★★".to_string(),
            if title.is_empty() {
                format!("A five-star review of {app}.")
            } else {
                format!("A five-star review of {app}: “{title}”")
            },
        ),
        1 | 2 => Moment::new(
            format!("review:{}", entry.id),
            Kind::Attention,
            now,
            "A review needs you".to_string(),
            stars(review.stars),
            if title.is_empty() {
                format!("A {}-star review of {app}.", review.stars)
            } else {
                format!("A {}-star review of {app}: “{title}”", review.stars)
            },
        ),
        _ => return None,
    };
    if !body.is_empty() {
        moment.quote = Some(clip(body, QUOTE_LIMIT));
    }
    moment.app = Some(app);
    moment.country = (!entry.country.is_empty()).then(|| entry.country.clone());
    moment.announced = true;
    Some(moment)
}

fn stars(count: i64) -> String {
    let count = count.clamp(0, 5) as usize;
    format!("{}{}", "★".repeat(count), "☆".repeat(5 - count))
}

fn clip(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    let cut: String = flat.chars().take(limit).collect();
    let cut = match cut.rfind(' ') {
        Some(space) => &cut[..space],
        None => &cut,
    };
    format!("{cut}…")
}

fn entry_app(entry: &Entry) -> &str {
    if entry.app.is_empty() {
        &entry.project
    } else {
        &entry.app
    }
}

/// Whether a product name or identifier reads as a yearly plan.
pub fn is_yearly(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    ["year", "annual", "p1y", "12m", "12 month", "1y"]
        .iter()
        .any(|word| {
            text.match_indices(word).any(|(index, _)| {
                let before = text[..index].chars().next_back();
                let starts_word = before.is_none_or(|character| !character.is_ascii_alphanumeric());
                *word != "1y" || starts_word
            })
        })
}

/// RevenueCat names a product duration segment like `P1Y`; twelve months counts too.
pub fn is_yearly_duration(name: &str) -> bool {
    matches!(name.trim().to_ascii_uppercase().as_str(), "P1Y" | "P12M")
}

/// The app's name before any subtitle: "Embr — Live Streams" is Embr.
pub fn short_name(name: &str) -> &str {
    let mut cut = name.len();
    for separator in [":", " — ", " – ", " - ", " | "] {
        if let Some(index) = name.find(separator) {
            cut = cut.min(index);
        }
    }
    let short = name[..cut].trim();
    if short.is_empty() { name.trim() } else { short }
}

/// A stable key for one app across RevenueCat projects and App Store names.
pub fn app_key(name: &str) -> String {
    short_name(name)
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Text without flag emoji, for places that cannot draw them: terminals disagree on their width and librsvg paints them grey.
pub fn plain(text: &str) -> String {
    text.chars()
        .filter(|character| !('\u{1F1E6}'..='\u{1F1FF}').contains(character))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Moments in the last week, newest first.
pub fn recent(moments: &[Moment], now: i64) -> Vec<&Moment> {
    let cutoff = now - RECENT_DAYS as i64 * 86_400_000;
    let mut recent: Vec<&Moment> = moments
        .iter()
        .filter(|moment| moment.kind.keeps() && !moment.test && moment.at >= cutoff)
        .collect();
    recent.sort_by_key(|moment| std::cmp::Reverse(moment.at));
    recent
}

/// "2h ago", "3 days ago": how long since a moment.
pub fn ago(moment_ms: i64, now: i64) -> String {
    let minutes = (now - moment_ms).max(0) / 60_000;
    match minutes {
        0 => "just now".to_string(),
        1..=59 => format!("{minutes}m ago"),
        60..=1439 => format!("{}h ago", minutes / 60),
        1440..=2879 => "yesterday".to_string(),
        _ => format!("{} days ago", minutes / 1440),
    }
}

/// The dashboard's live view of the moments log: reloads when the file changes and flags new arrivals.
#[derive(Debug, Clone)]
pub struct Shelf {
    pub path: PathBuf,
    pub moments: Vec<Moment>,
    pub open: bool,
    pub selected: usize,
    next_poll: Instant,
    stamp: Option<(SystemTime, u64)>,
    loaded: bool,
}

impl Shelf {
    pub fn new(path: PathBuf, demo: bool, now: Instant) -> Self {
        let mut shelf = Self {
            path,
            moments: Vec::new(),
            open: false,
            selected: 0,
            next_poll: now,
            stamp: None,
            loaded: false,
        };
        if demo {
            shelf.moments = demo_moments(chrono::Utc::now().timestamp_millis());
            shelf.loaded = true;
        } else {
            shelf.refresh(now);
        }
        shelf
    }

    /// Reloads the log when it changed and returns the moments that arrived since the last look.
    pub fn refresh(&mut self, now: Instant) -> Vec<Moment> {
        if now < self.next_poll {
            return Vec::new();
        }
        self.next_poll = now + SHELF_POLL;
        let stamp = fs::metadata(&self.path)
            .ok()
            .and_then(|meta| Some((meta.modified().ok()?, meta.len())));
        if stamp == self.stamp && self.loaded {
            return Vec::new();
        }
        self.stamp = stamp;
        let before: BTreeSet<String> = self
            .moments
            .iter()
            .map(|moment| moment.id.clone())
            .collect();
        self.moments = load(&self.path);
        let first = !self.loaded;
        self.loaded = true;
        if first {
            return Vec::new();
        }
        self.moments
            .iter()
            .filter(|moment| !before.contains(&moment.id) && moment.kind.keeps())
            .cloned()
            .collect()
    }

    /// Kept moments, newest first, as the list shows them.
    pub fn listed(&self) -> Vec<&Moment> {
        let mut listed: Vec<&Moment> = self
            .moments
            .iter()
            .filter(|moment| moment.kind.keeps() && !moment.test)
            .collect();
        listed.sort_by_key(|moment| std::cmp::Reverse(moment.at));
        listed
    }

    pub fn select_id(&mut self, id: &str) {
        if let Some(index) = self.listed().iter().position(|moment| moment.id == id) {
            self.selected = index;
        }
    }

    pub fn selected_moment(&self) -> Option<&Moment> {
        let listed = self.listed();
        listed
            .get(self.selected.min(listed.len().saturating_sub(1)))
            .copied()
    }

    pub fn move_selection(&mut self, delta: isize) {
        let count = self.listed().len();
        if count == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + delta).clamp(0, count as isize - 1) as usize;
    }
}

fn demo_moments(now: i64) -> Vec<Moment> {
    let hour = 3_600_000;
    let mut best = Moment::sample("EUR", now - 2 * hour);
    best.id = "demo:best-day".to_string();
    best.test = false;
    let mut country = new_country("JP", "Solar Beam", 16, now - 26 * hour);
    country.id = "demo:country".to_string();
    let mut stars = Moment::new(
        "demo:stars".to_string(),
        Kind::FiveStars,
        now - 3 * 24 * hour,
        "Five stars".to_string(),
        "★★★★★".to_string(),
        "A five-star review of Psywave: “Finally a calm one”".to_string(),
    );
    stars.quote = Some("I use it every night before sleep. Thank you for making it.".to_string());
    stars.note = Some("The night-mode update shipped on Monday.".to_string());
    let people = Moment::new(
        "demo:people".to_string(),
        Kind::People,
        now - 5 * 24 * hour,
        "1,000 people".to_string(),
        "1,000".to_string(),
        "For the first time, more than 1,000 people used your apps in 28 days.".to_string(),
    );
    vec![people, stars, country, best]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{Purchase, Review};

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn series(end: &str, values: &[f64]) -> Vec<(NaiveDate, f64)> {
        let end = day(end);
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let back = (values.len() - 1 - index) as u64;
                (
                    end.checked_sub_days(chrono::Days::new(back)).unwrap(),
                    *value,
                )
            })
            .collect()
    }

    fn reading(today: &str, apps: Vec<AppReading>) -> Reading {
        Reading {
            apps,
            complete: true,
            today: day(today),
            currency: "EUR".to_string(),
            enough: None,
            feed: false,
        }
    }

    fn app(name: &str, days: Vec<(NaiveDate, f64)>) -> AppReading {
        AppReading {
            name: name.to_string(),
            people: Some(10.0),
            revenue: Some(days.iter().rev().take(28).map(|(_, value)| value).sum()),
            days,
            ..AppReading::default()
        }
    }

    fn seeded(history: Reading) -> State {
        let mut state = State::default();
        assert!(observe(&mut state, &history, 0).is_empty());
        assert!(state.seeded);
        state
    }

    #[test]
    fn learns_quietly_until_seeded_then_celebrates_a_best_day() {
        let history = reading(
            "2026-10-20",
            vec![app("Solar Beam", series("2026-10-20", &[10.0, 40.0, 12.0]))],
        );
        let mut state = seeded(history);
        assert_eq!(state.best_day.unwrap().value, 40.0);

        let today = reading(
            "2026-10-21",
            vec![app("Solar Beam", series("2026-10-21", &[40.0, 12.0, 55.0]))],
        );
        let moments = observe(&mut state, &today, 1);
        assert_eq!(moments.len(), 1);
        assert_eq!(moments[0].kind, Kind::BestDay);
        assert_eq!(moments[0].value, "€55");
        assert!(moments[0].line.contains("today, your best day ever"));
        assert!(moments[0].line.contains("€40 on 19 Oct"));
        assert_eq!(moments[0].mark, Some(40.0));

        let later = reading(
            "2026-10-21",
            vec![app("Solar Beam", series("2026-10-21", &[40.0, 12.0, 70.0]))],
        );
        assert!(observe(&mut state, &later, 2).is_empty());
        assert_eq!(state.best_day.unwrap().value, 70.0);
    }

    #[test]
    fn an_old_revision_updates_the_record_without_a_moment() {
        let history = reading(
            "2026-10-20",
            vec![app(
                "A",
                series("2026-10-20", &[10.0, 40.0, 12.0, 8.0, 9.0]),
            )],
        );
        let mut state = seeded(history);
        let revised = reading(
            "2026-10-20",
            vec![app(
                "A",
                series("2026-10-20", &[10.0, 50.0, 12.0, 8.0, 9.0]),
            )],
        );
        assert!(observe(&mut state, &revised, 1).is_empty());
        assert_eq!(state.best_day.unwrap().value, 50.0);
    }

    #[test]
    fn names_the_app_that_carried_a_record_day() {
        let history = reading(
            "2026-10-20",
            vec![
                app("Solar Beam", series("2026-10-20", &[5.0, 10.0])),
                app("Psywave", series("2026-10-20", &[5.0, 10.0])),
            ],
        );
        let mut state = seeded(history);
        let today = reading(
            "2026-10-21",
            vec![
                app(
                    "Solar Beam: Sun Tracker",
                    series("2026-10-21", &[10.0, 60.0]),
                ),
                app("Psywave", series("2026-10-21", &[10.0, 4.0])),
            ],
        );
        let moments = observe(&mut state, &today, 1);
        assert!(
            moments[0].line.ends_with("Solar Beam brought €60 of it."),
            "{}",
            moments[0].line
        );
    }

    #[test]
    fn best_28_days_is_celebrated_at_most_once_a_month() {
        let base = vec![10.0; 40];
        let mut state = seeded(reading(
            "2026-10-01",
            vec![app("A", series("2026-10-01", &base))],
        ));
        let mut values = base.clone();
        values.push(30.0);
        let moments = observe(
            &mut state,
            &reading("2026-10-02", vec![app("A", series("2026-10-02", &values))]),
            1,
        );
        let month: Vec<_> = moments
            .iter()
            .filter(|moment| moment.kind == Kind::BestMonth)
            .collect();
        assert_eq!(month.len(), 1);
        assert_eq!(month[0].value, "€300");
        values.push(30.0);
        let again = observe(
            &mut state,
            &reading("2026-10-03", vec![app("A", series("2026-10-03", &values))]),
            2,
        );
        assert!(again.iter().all(|moment| moment.kind != Kind::BestMonth));
        assert!(state.best_month.unwrap().value > 300.0);
    }

    #[test]
    fn people_milestones_cross_once_and_never_on_partial_data() {
        let mut first = app("A", series("2026-10-20", &[1.0]));
        first.people = Some(834.0);
        let mut state = seeded(reading("2026-10-20", vec![first.clone()]));
        assert_eq!(state.people, 500.0);

        first.people = Some(1_020.0);
        let mut partial = reading("2026-10-21", vec![first.clone()]);
        partial.complete = false;
        assert!(
            observe(&mut state, &partial, 1)
                .iter()
                .all(|moment| moment.kind != Kind::People)
        );

        let moments = observe(&mut state, &reading("2026-10-21", vec![first.clone()]), 2);
        let people: Vec<_> = moments
            .iter()
            .filter(|moment| moment.kind == Kind::People)
            .collect();
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].title, "1,000 people");
        assert!(observe(&mut state, &reading("2026-10-22", vec![first]), 3).is_empty());
    }

    #[test]
    fn an_app_crossing_a_thousand_people_gets_its_own_moment() {
        let mut one = app("Embr — Live Streams", series("2026-10-20", &[1.0]));
        one.people = Some(400.0);
        let mut state = seeded(reading("2026-10-20", vec![one.clone()]));
        one.people = Some(1_200.0);
        let moments = observe(&mut state, &reading("2026-10-21", vec![one]), 1);
        let app_people: Vec<_> = moments
            .iter()
            .filter(|moment| moment.kind == Kind::AppPeople)
            .collect();
        assert_eq!(app_people.len(), 1);
        assert_eq!(app_people[0].title, "Embr reached 1,000 people");
    }

    #[test]
    fn enough_fires_on_the_way_up_and_rearms_below_ninety_percent() {
        let mut history = reading(
            "2026-10-20",
            vec![app("A", series("2026-10-20", &[50.0; 28]))],
        );
        history.enough = Some(3_000.0);
        let mut state = seeded(history);
        assert!(!state.enough_reached);

        let mut up = reading(
            "2026-10-21",
            vec![app("A", series("2026-10-21", &[110.0; 28]))],
        );
        up.enough = Some(3_000.0);
        let moments = observe(&mut state, &up, 1);
        assert!(moments.iter().any(|moment| moment.kind == Kind::Enough));
        assert!(
            observe(&mut state, &up, 2)
                .iter()
                .all(|moment| moment.kind != Kind::Enough)
        );

        let mut dip = reading(
            "2026-10-22",
            vec![app("A", series("2026-10-22", &[100.0; 28]))],
        );
        dip.enough = Some(3_000.0);
        observe(&mut state, &dip, 3);
        assert!(state.enough_reached);
        let mut low = reading(
            "2026-10-23",
            vec![app("A", series("2026-10-23", &[90.0; 28]))],
        );
        low.enough = Some(3_000.0);
        observe(&mut state, &low, 4);
        assert!(!state.enough_reached);
    }

    #[test]
    fn firsts_from_revenuecat_need_a_seeded_memory() {
        let mut quiet = app("Solar Beam", series("2026-10-20", &[1.0]));
        quiet.countries = vec!["GB".to_string()];
        quiet.sold = true;
        let unsold = app("Embr", series("2026-10-20", &[0.0]));
        let mut state = seeded(reading("2026-10-20", vec![quiet.clone(), unsold]));
        quiet.countries = vec!["GB".to_string(), "JP".to_string()];
        quiet.yearly = true;
        let mut fresh = app("Embr", series("2026-10-21", &[1.0]));
        fresh.sold = true;
        let moments = observe(&mut state, &reading("2026-10-21", vec![quiet, fresh]), 1);
        let kinds: Vec<Kind> = moments.iter().map(|moment| moment.kind).collect();
        assert!(kinds.contains(&Kind::NewCountry));
        assert!(kinds.contains(&Kind::FirstYearly));
        assert!(kinds.contains(&Kind::FirstSale));
        let country = moments
            .iter()
            .find(|moment| moment.kind == Kind::NewCountry)
            .unwrap();
        assert_eq!(country.title, "First purchase from Japan");
        assert!(country.line.contains("2 countries"));
    }

    #[test]
    fn with_a_feed_the_first_sale_is_the_feeds_to_announce() {
        let unsold = app("Embr", series("2026-10-20", &[0.0]));
        let mut state = seeded(reading("2026-10-20", vec![unsold]));
        let mut sold = app("Embr", series("2026-10-21", &[3.0]));
        sold.sold = true;
        let mut with_feed = reading("2026-10-21", vec![sold]);
        with_feed.feed = true;
        assert!(
            observe(&mut state, &with_feed, 1)
                .iter()
                .all(|moment| moment.kind != Kind::FirstSale)
        );
        assert!(state.sold.contains("embr"));
    }

    #[test]
    fn a_free_trial_is_not_a_purchase_from_a_new_country() {
        let mut state = seeded(reading(
            "2026-10-20",
            vec![app("A", series("2026-10-20", &[1.0]))],
        ));
        observe_feed(&mut state, &[], None, Duration::ZERO, 0);
        let mut trial = sale("t", "Psywave", "KR", 50, false);
        if let Some(purchase) = trial.purchase.as_mut() {
            purchase.amount = 0.0;
            purchase.product = "Psywave Pro Yearly".to_string();
        }
        assert!(observe_feed(&mut state, &[trial], None, Duration::ZERO, 1).is_empty());
        assert!(!state.countries.contains("KR"));
    }

    fn sale(id: &str, app: &str, country: &str, received_at: i64, first_ever: bool) -> Entry {
        Entry {
            id: id.to_string(),
            kind: "purchase".to_string(),
            at: received_at,
            received_at,
            app: app.to_string(),
            project: app.to_string(),
            country: country.to_string(),
            purchase: Some(Purchase {
                amount: 4.99,
                currency: "USD".to_string(),
                product: "Pro".to_string(),
                event: "purchase".to_string(),
                environment: "production".to_string(),
                first_ever,
                ..Purchase::default()
            }),
            ..Entry::default()
        }
    }

    #[test]
    fn the_feed_seeds_its_mark_then_reports_new_countries_and_first_sales() {
        let mut state = seeded(reading(
            "2026-10-20",
            vec![app("Solar Beam", series("2026-10-20", &[1.0]))],
        ));
        let old = vec![sale("a", "Solar Beam", "GB", 100, false)];
        assert!(observe_feed(&mut state, &old, None, Duration::ZERO, 1_000).is_empty());
        assert!(state.countries.contains("GB"));

        let mut feed = old.clone();
        feed.push(sale("b", "Embr — Live Streams", "JP", 200, true));
        feed.push(sale("c", "Solar Beam", "GB", 300, false));
        let moments = observe_feed(&mut state, &feed, None, Duration::ZERO, 2_000);
        let kinds: Vec<Kind> = moments.iter().map(|moment| moment.kind).collect();
        assert_eq!(kinds, vec![Kind::FirstSale, Kind::NewCountry]);
        assert!(moments[0].announced);
        assert!(
            moments[0].line.contains("$4.99 from 🇯🇵 Japan"),
            "{}",
            moments[0].line
        );
        assert!(!moments[1].announced);
        assert!(observe_feed(&mut state, &feed, None, Duration::ZERO, 3_000).is_empty());
    }

    #[test]
    fn a_feed_purchase_before_revenuecat_is_seeded_only_learns() {
        let mut state = State::default();
        observe_feed(&mut state, &[], None, Duration::ZERO, 0);
        let feed = vec![sale("b", "Embr", "BR", 200, true)];
        assert!(observe_feed(&mut state, &feed, None, Duration::ZERO, 1).is_empty());
        assert!(state.countries.contains("BR"));
        assert!(state.sold.contains("embr"));
    }

    #[test]
    fn reviews_keep_the_kind_ones_and_flag_the_hard_ones() {
        let mut state = seeded(reading(
            "2026-10-20",
            vec![app("A", series("2026-10-20", &[1.0]))],
        ));
        observe_feed(&mut state, &[], None, Duration::ZERO, 0);
        let review = |id: &str, stars: i64, body: &str| Entry {
            id: id.to_string(),
            kind: "review".to_string(),
            at: 10,
            received_at: 10,
            app: "Psywave".to_string(),
            review: Some(Review {
                stars,
                title: "Calm".to_string(),
                body: body.to_string(),
                author: "someone".to_string(),
            }),
            ..Entry::default()
        };
        let feed = vec![
            review("r1", 5, "I use it every night before I sleep."),
            review("r2", 5, "ok"),
            review("r3", 1, "Crashes on launch."),
            review("r4", 4, "Pretty good overall, a few rough edges."),
        ];
        let moments = observe_feed(&mut state, &feed, None, Duration::ZERO, 1);
        let kinds: Vec<Kind> = moments.iter().map(|moment| moment.kind).collect();
        assert_eq!(kinds, vec![Kind::FiveStars, Kind::Attention]);
        assert_eq!(
            moments[0].quote.as_deref(),
            Some("I use it every night before I sleep.")
        );
        assert_eq!(moments[1].value, "★☆☆☆☆");
    }

    #[test]
    fn a_quiet_feed_alerts_once_and_says_when_it_is_back() {
        let mut state = State::default();
        observe_feed(&mut state, &[], Some(0), Duration::ZERO, 0);
        let now = 20 * 60_000;
        let quiet = observe_feed(&mut state, &[], Some(0), Duration::from_secs(16 * 60), now);
        assert_eq!(quiet.len(), 1);
        assert_eq!(quiet[0].kind, Kind::FeedQuiet);
        assert!(
            observe_feed(
                &mut state,
                &[],
                Some(0),
                Duration::from_secs(17 * 60),
                now + 60_000
            )
            .is_empty()
        );
        let back = observe_feed(
            &mut state,
            &[],
            Some(now + 120_000),
            Duration::ZERO,
            now + 130_000,
        );
        assert_eq!(back[0].kind, Kind::FeedBack);
    }

    #[test]
    fn the_log_round_trips_moments_and_notes() {
        let mut moment = Moment::sample("EUR", 5);
        moment.test = false;
        moment.id = "best-day:2026-10-21".to_string();
        let text = format!(
            "{}\n{}\nnot json\n",
            serde_json::to_string(&Record::Moment(Box::new(moment.clone()))).unwrap(),
            serde_json::to_string(&Record::Note {
                id: moment.id.clone(),
                text: "Launched on Product Hunt".to_string(),
                at: 9,
            })
            .unwrap()
        );
        let moments = parse(&text);
        assert_eq!(moments.len(), 1);
        assert_eq!(moments[0].note.as_deref(), Some("Launched on Product Hunt"));
        assert_eq!(moments[0].series, moment.series);
    }

    #[test]
    fn names_and_keys_ignore_subtitles() {
        assert_eq!(short_name("Master of Flags: World Quiz"), "Master of Flags");
        assert_eq!(short_name("Embr — Live Streams"), "Embr");
        assert_eq!(
            app_key("Tailscode: Coding Agent Remote"),
            app_key("Tailscode")
        );
        assert_ne!(
            app_key("Master of Flags"),
            app_key("Master of Inventory: Warranty")
        );
    }

    #[test]
    fn spots_yearly_plans() {
        assert!(is_yearly("Solar Beam Pro Yearly com.example.pro.annual"));
        assert!(is_yearly("com.example.p1y"));
        assert!(is_yearly("Premium 1y"));
        assert!(!is_yearly("Premium Lifetime com.example.lifetime"));
        assert!(!is_yearly("Monthly com.example.monthly.11y"));
        assert!(is_yearly_duration("P1Y"));
        assert!(!is_yearly_duration("lifetime"));
    }

    #[test]
    fn recent_skips_alerts_tests_and_old_moments() {
        let now = 10 * 86_400_000;
        let mut kept = Moment::sample("EUR", now - 3_600_000);
        kept.test = false;
        let mut old = kept.clone();
        old.id = "old".to_string();
        old.at = 0;
        let mut alert = kept.clone();
        alert.id = "alert".to_string();
        alert.kind = Kind::FeedQuiet;
        let test = Moment::sample("EUR", now);
        let all = vec![kept.clone(), old, alert, test];
        let recent = recent(&all, now);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, kept.id);
        assert_eq!(ago(now - 2 * 3_600_000, now), "2h ago");
    }
}
