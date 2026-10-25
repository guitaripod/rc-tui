use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use chrono::Days;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use tachyonfx::{
    CellFilter, Duration as FxDuration, EffectManager, Interpolation, Motion as FxMotion, fx,
};

use crate::activity::{Activity, Celebration, Entry, EntryKind, Filter, Range};
use crate::api::{self, Client, Failure, OverviewMetric, Snapshot};
use crate::config;
use crate::moments::{self, Shelf};
use crate::theme;

const VALUE_TWEEN: Duration = Duration::from_millis(400);
const SERIES_TWEEN: Duration = Duration::from_millis(700);
const FLASH_MS: u32 = 500;
const FOCUS_VISIBLE: Duration = Duration::from_secs(5);
const NOTICE_TTL: Duration = Duration::from_secs(10);

pub const PEOPLE: [&str; 2] = ["active_users", "active_customers"];

#[derive(Clone)]
pub struct Source {
    pub label: String,
    pub id: String,
    pub client: Option<Client>,
}

impl Source {
    pub fn live(label: impl Into<String>, id: impl Into<String>, client: Client) -> Self {
        Self {
            label: label.into(),
            id: id.into(),
            client: Some(client),
        }
    }

    pub fn demo(label: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            id: id.into(),
            client: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Full,
    Minimal,
    Off,
}

impl Motion {
    pub fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or_default().to_ascii_lowercase().as_str() {
            "minimal" => Self::Minimal,
            "off" | "none" => Self::Off,
            _ => Self::Full,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    None,
    Quit,
    Setup,
    AddProject,
    Import,
    EditProject(usize),
    RemoveProject(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    D7,
    D28,
    D90,
}

impl Window {
    pub fn days(self) -> u64 {
        match self {
            Self::D7 => 7,
            Self::D28 => 28,
            Self::D90 => 90,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::D7 => "7 days",
            Self::D28 => "28 days",
            Self::D90 => "90 days",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::D7 => Self::D28,
            Self::D28 => Self::D90,
            Self::D90 => Self::D7,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Detail {
    pub metric_id: String,
    pub window: Window,
    pub range: Option<Vec<f64>>,
    pub loading: bool,
    pub error: Option<Failure>,
    pub selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Live,
    Stale,
    Failed,
}

#[derive(Debug, Clone)]
pub struct PickerEntry {
    pub scope: usize,
    pub view: Option<usize>,
    pub label: String,
    pub detail: String,
    pub state: RowState,
}

#[derive(Debug, Clone)]
pub struct HealthRow {
    pub label: String,
    pub detail: String,
    pub state: RowState,
    pub failure: Option<Failure>,
    pub last_success: Option<Instant>,
}

#[derive(Debug, Clone)]
pub struct DetailRow {
    pub view: usize,
    pub label: String,
    pub value: f64,
    pub share: f64,
    pub trend: Option<Trend>,
    pub state: RowState,
}

#[derive(Debug, Clone)]
pub struct GardenRow {
    pub label: String,
    pub people: Option<f64>,
    pub revenue: Option<f64>,
    pub state: RowState,
}

#[derive(Debug, Clone)]
pub struct ProjectView {
    pub label: String,
    pub id: String,
    pub snapshot: Option<Snapshot>,
    pub failure: Option<Failure>,
    pub last_success: Option<Instant>,
}

impl ProjectView {
    pub fn live(&self) -> bool {
        self.failure.is_none() && self.snapshot.is_some()
    }

    pub fn state(&self, now: Instant, stale_after: Duration) -> RowState {
        if self.failure.is_some() {
            RowState::Failed
        } else if self
            .last_success
            .is_some_and(|then| now.saturating_duration_since(then) > stale_after)
        {
            RowState::Stale
        } else if self.snapshot.is_some() {
            RowState::Live
        } else {
            RowState::Failed
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Trend {
    pub ratio: f64,
    pub change: f64,
    pub previous: f64,
    pub days: usize,
}

#[derive(Debug, Clone)]
pub enum Status {
    Idle,
    Fetching,
    Error(Failure),
}

#[derive(Debug, Clone)]
pub struct Tween {
    from: f64,
    to: f64,
    start: Instant,
    duration: Duration,
}

impl Tween {
    fn new(value: f64, now: Instant) -> Self {
        Self {
            from: value,
            to: value,
            start: now,
            duration: Duration::ZERO,
        }
    }

    fn set(&mut self, target: f64, now: Instant, duration: Duration) {
        self.from = self.value(now);
        self.to = target;
        self.start = now;
        self.duration = duration;
    }

    pub fn value(&self, now: Instant) -> f64 {
        if self.duration.is_zero() {
            return self.to;
        }
        let progress = (now.duration_since(self.start).as_secs_f64() / self.duration.as_secs_f64())
            .clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        self.from + (self.to - self.from) * eased
    }

    pub fn target(&self) -> f64 {
        self.to
    }
}

#[derive(Debug, Clone)]
pub struct Morph {
    from: Vec<f64>,
    to: Vec<f64>,
    start: Instant,
    duration: Duration,
}

impl Morph {
    fn new(values: Vec<f64>, now: Instant) -> Self {
        Self {
            from: values.clone(),
            to: values,
            start: now,
            duration: Duration::ZERO,
        }
    }

    fn set(&mut self, target: Vec<f64>, now: Instant, duration: Duration) {
        self.from = api::resample(&self.sample(now), target.len());
        self.to = target;
        self.start = now;
        self.duration = duration;
    }

    pub fn sample(&self, now: Instant) -> Vec<f64> {
        if self.duration.is_zero() {
            return self.to.clone();
        }
        let progress = (now.duration_since(self.start).as_secs_f64() / self.duration.as_secs_f64())
            .clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        let from = api::resample(&self.from, self.to.len());
        from.iter()
            .zip(&self.to)
            .map(|(a, b)| a + (b - a) * eased)
            .collect()
    }
}

pub struct Card {
    pub id: String,
    pub name: String,
    pub unit: String,
    pub period: String,
    pub value: Tween,
    pub series: Morph,
    pub updated: Option<Instant>,
    pub trend: Option<Trend>,
}

impl Card {
    fn new(metric: &OverviewMetric, series: Vec<f64>, now: Instant) -> Self {
        Self {
            id: metric.id.clone(),
            name: metric.name.clone(),
            unit: metric.unit.clone(),
            period: metric.period.clone(),
            value: Tween::new(metric.value, now),
            trend: trend_for(&metric.id, &series),
            series: Morph::new(series, now),
            updated: Some(now),
        }
    }
}

struct ProjectOutcome {
    order: usize,
    result: Result<Snapshot, Failure>,
}

struct SourceRuntime {
    source: Source,
    cache: HashMap<String, Vec<f64>>,
    charts_at: Option<Instant>,
}

enum Command {
    Fetch {
        only: Option<usize>,
        force_charts: bool,
    },
    Range {
        only: Option<usize>,
        chart: String,
        metric_id: String,
        days: u64,
        monetary: bool,
        measure: u64,
    },
}

enum Response {
    Project(ProjectOutcome),
    Done {
        full: bool,
    },
    Range {
        metric_id: String,
        days: u64,
        result: Result<Vec<f64>, Failure>,
    },
}

struct Worker {
    requests: tokio::sync::mpsc::UnboundedSender<Command>,
    responses: mpsc::Receiver<Response>,
}

impl Worker {
    fn spawn(sources: Vec<Source>, currency: String, chart_ttl: Duration) -> Self {
        let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let (response_tx, response_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    let _ = response_tx.send(Response::Project(ProjectOutcome {
                        order: 0,
                        result: Err(Failure::local(format!("runtime: {err}"))),
                    }));
                    let _ = response_tx.send(Response::Done { full: true });
                    return;
                }
            };
            runtime.block_on(async move {
                let mut runtimes: Vec<SourceRuntime> = sources
                    .into_iter()
                    .map(|source| SourceRuntime {
                        source,
                        cache: HashMap::new(),
                        charts_at: None,
                    })
                    .collect();

                while let Some(command) = request_rx.recv().await {
                    match command {
                        Command::Fetch { only, force_charts } => {
                            let full = only.is_none();
                            let indices: Vec<usize> = match only {
                                Some(index) if index < runtimes.len() => vec![index],
                                Some(_) => Vec::new(),
                                None => (0..runtimes.len()).collect(),
                            };
                            let mut set = tokio::task::JoinSet::new();
                            for index in indices {
                                let runtime = &mut runtimes[index];
                                let source = runtime.source.clone();
                                let cache = std::mem::take(&mut runtime.cache);
                                let charts_at = runtime.charts_at;
                                let currency = currency.clone();
                                set.spawn(async move {
                                    let (result, cache, charts_at) = fetch_source(
                                        &source,
                                        cache,
                                        charts_at,
                                        force_charts,
                                        chart_ttl,
                                        &currency,
                                    )
                                    .await;
                                    (index, result, cache, charts_at)
                                });
                            }
                            while let Some(joined) = set.join_next().await {
                                let outcome = match joined {
                                    Ok((index, result, cache, charts_at)) => {
                                        runtimes[index].cache = cache;
                                        runtimes[index].charts_at = charts_at;
                                        ProjectOutcome {
                                            order: index,
                                            result,
                                        }
                                    }
                                    Err(err) => ProjectOutcome {
                                        order: usize::MAX,
                                        result: Err(Failure::local(format!(
                                            "worker task failed: {err}"
                                        ))),
                                    },
                                };
                                if response_tx.send(Response::Project(outcome)).is_err() {
                                    break;
                                }
                            }
                            if response_tx.send(Response::Done { full }).is_err() {
                                break;
                            }
                        }
                        Command::Range {
                            only,
                            chart,
                            metric_id,
                            days,
                            monetary,
                            measure,
                        } => {
                            let indices: Vec<usize> = match only {
                                Some(index) if index < runtimes.len() => vec![index],
                                Some(_) => Vec::new(),
                                None => (0..runtimes.len()).collect(),
                            };
                            let mut set = tokio::task::JoinSet::new();
                            for index in indices {
                                let source = runtimes[index].source.clone();
                                let chart = chart.clone();
                                let currency = currency.clone();
                                let metric = metric_id.clone();
                                set.spawn(async move {
                                    match &source.client {
                                        Some(client) => {
                                            client
                                                .fetch_range(&chart, days, monetary, measure)
                                                .await
                                        }
                                        None => {
                                            let snapshot =
                                                api::demo_snapshot(&currency, index as u64 + 1);
                                            snapshot
                                                .series
                                                .get(&metric)
                                                .cloned()
                                                .map(|values| api::resample(&values, days as usize))
                                                .ok_or_else(|| {
                                                    Failure::local("no demo series for this metric")
                                                })
                                        }
                                    }
                                });
                            }
                            let mut series = Vec::new();
                            let mut first_failure = None;
                            while let Some(joined) = set.join_next().await {
                                match joined {
                                    Ok(Ok(values)) if !values.is_empty() => series.push(values),
                                    Ok(Ok(_)) => {}
                                    Ok(Err(failure)) => {
                                        first_failure.get_or_insert(failure);
                                    }
                                    Err(err) => {
                                        first_failure.get_or_insert_with(|| {
                                            Failure::local(format!("worker task failed: {err}"))
                                        });
                                    }
                                }
                            }
                            let result = if series.is_empty() {
                                Err(first_failure.unwrap_or_else(|| {
                                    Failure::local("no chart data for this window")
                                }))
                            } else {
                                Ok(api::aggregate_series(&series))
                            };
                            if response_tx
                                .send(Response::Range {
                                    metric_id,
                                    days,
                                    result,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            });
        });
        Self {
            requests: request_tx,
            responses: response_rx,
        }
    }

    fn send(&self, command: Command) {
        let _ = self.requests.send(command);
    }

    fn drain(&self) -> Vec<Response> {
        self.responses.try_iter().collect()
    }
}

async fn fetch_source(
    source: &Source,
    mut cache: HashMap<String, Vec<f64>>,
    charts_at: Option<Instant>,
    force_charts: bool,
    chart_ttl: Duration,
    currency: &str,
) -> (
    Result<Snapshot, Failure>,
    HashMap<String, Vec<f64>>,
    Option<Instant>,
) {
    let Some(client) = &source.client else {
        return (
            Ok(api::demo_snapshot(currency, demo_salt(&source.id))),
            cache,
            charts_at,
        );
    };
    let overview = match client.fetch_overview().await {
        Ok(overview) => overview,
        Err(failure) => return (Err(failure), cache, charts_at),
    };
    cache.retain(|id, _| {
        id == api::PURCHASES || overview.metrics.iter().any(|metric| metric.id == *id)
    });
    let missing_chart = overview.metrics.iter().any(|metric| {
        api::chart_for_metric(&metric.id, &metric.name).is_some() && !cache.contains_key(&metric.id)
    });
    let due =
        force_charts || missing_chart || charts_at.is_none_or(|then| then.elapsed() >= chart_ttl);
    let mut charts_at = charts_at;
    if due {
        let fresh = client.fetch_charts(&overview.metrics).await;
        let complete = overview.metrics.iter().all(|metric| {
            api::chart_for_metric(&metric.id, &metric.name).is_none()
                || fresh.contains_key(&metric.id)
        });
        for (id, values) in fresh {
            cache.insert(id, values);
        }
        if complete {
            charts_at = Some(Instant::now());
        }
    }
    let mut metrics = overview.metrics;
    let mut series: HashMap<String, Vec<f64>> = metrics
        .iter()
        .filter_map(|metric| {
            cache
                .get(&metric.id)
                .cloned()
                .map(|values| (metric.id.clone(), values))
        })
        .collect();
    if let Some(purchases) = cache.get(api::PURCHASES) {
        metrics.push(api::purchases_metric(purchases));
        series.insert(api::PURCHASES.to_string(), purchases.clone());
    }
    (
        Ok(Snapshot {
            metrics,
            series,
            currency: overview.currency,
        }),
        cache,
        charts_at,
    )
}

fn demo_salt(id: &str) -> u64 {
    id.bytes().fold(0u64, |hash, byte| {
        hash.wrapping_mul(1099511628211).wrapping_add(byte as u64)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Measure {
    Flow,
    Daily,
    Stock,
}

fn measure_of(id: &str) -> Measure {
    match id {
        "revenue" | "new_customers" | api::PURCHASES => Measure::Flow,
        "active_users" | "active_customers" => Measure::Daily,
        _ => Measure::Stock,
    }
}

/// True for metrics whose daily points add up to the card value, so today's partial day is left off charts.
pub fn is_flow(id: &str) -> bool {
    measure_of(id) != Measure::Stock
}

/// Compares the last complete stretch of days with the same stretch before it, up to 28 against 28.
fn trend_for(id: &str, series: &[f64]) -> Option<Trend> {
    let complete = &series[..series.len().checked_sub(1)?];
    let days = (complete.len() / 2).min(api::COMPARE_DAYS);
    if days < 7 {
        return None;
    }
    let split = complete.len() - days;
    let recent_days = &complete[split..];
    let previous_days = &complete[split - days..split];
    let (recent, previous) = match measure_of(id) {
        Measure::Flow => (recent_days.iter().sum(), previous_days.iter().sum()),
        Measure::Daily => (
            recent_days.iter().sum::<f64>() / days as f64,
            previous_days.iter().sum::<f64>() / days as f64,
        ),
        Measure::Stock => (recent_days[days - 1], previous_days[days - 1]),
    };
    if recent.abs() < 1e-9 && previous.abs() < 1e-9 {
        return None;
    }
    let ratio = if previous.abs() < 1e-9 {
        f64::INFINITY
    } else {
        (recent - previous) / previous.abs()
    };
    Some(Trend {
        ratio,
        change: recent - previous,
        previous,
        days,
    })
}

/// The home screen order: people first, then the money they paid, then the subscription details.
pub fn card_rank(id: &str) -> usize {
    match id {
        "active_users" | "active_customers" => 0,
        "revenue" => 1,
        "new_customers" => 2,
        api::PURCHASES => 3,
        "active_subscriptions" => 4,
        "mrr" => 5,
        "active_trials" => 6,
        _ => 7,
    }
}

pub struct AppOptions {
    pub sources: Vec<Source>,
    pub currency: String,
    pub interval: Duration,
    pub chart_ttl: Duration,
    pub motion: Motion,
    pub wall: bool,
    pub pins: Vec<String>,
    pub ui_scale: u8,
    pub ui_page: usize,
    pub feed_path: Option<PathBuf>,
    pub feed_seen_at: Option<i64>,
    pub enough: Option<f64>,
}

pub struct App {
    pub currency: String,
    pub demo: bool,
    pub views: Vec<ProjectView>,
    pub scope: usize,
    pub cards: Vec<Card>,
    pub status: Status,
    pub interval: Duration,
    pub next_refresh: Instant,
    pub last_updated: Option<Instant>,
    pub last_refresh: Option<Instant>,
    pub rects: HashMap<String, Rect>,
    pub fx: EffectManager<String>,
    pub picker: Option<Picker>,
    pub detail: Option<Detail>,
    pub health: Option<usize>,
    pub help: bool,
    pub wall: bool,
    pub focus: usize,
    pub last_input: Instant,
    pub notice: Option<(String, Instant)>,
    pub pins: Vec<String>,
    pub motion: Motion,
    pub ui_scale: u8,
    pub page: usize,
    pub activity: Activity,
    pub moments: Shelf,
    pub enough: Option<f64>,
    worker: Worker,
    last_frame: Instant,
    frame_delta: FxDuration,
    intro_done: bool,
    consecutive_failures: u32,
    fetch_success: bool,
    fetch_failure: Option<Failure>,
    fetch_failures: usize,
    confirm_remove: Option<usize>,
    focus_until: Instant,
    animating_until: Instant,
}

#[derive(Debug, Clone)]
pub struct Picker {
    pub query: String,
    pub selected: usize,
}

impl App {
    pub fn new(options: AppOptions) -> Self {
        let now = Instant::now();
        let demo = options.sources.iter().all(|source| source.client.is_none());
        let views = options
            .sources
            .iter()
            .map(|source| ProjectView {
                label: source.label.clone(),
                id: source.id.clone(),
                snapshot: None,
                failure: None,
                last_success: None,
            })
            .collect();
        let activity = Activity::new(options.feed_path, demo, options.feed_seen_at, now);
        let worker = Worker::spawn(options.sources, options.currency.clone(), options.chart_ttl);
        Self {
            currency: options.currency,
            demo,
            views,
            scope: 0,
            cards: Vec::new(),
            status: Status::Idle,
            interval: options.interval,
            next_refresh: now,
            last_updated: None,
            last_refresh: None,
            rects: HashMap::new(),
            fx: EffectManager::default(),
            picker: None,
            detail: None,
            health: None,
            help: false,
            wall: options.wall,
            focus: 0,
            last_input: now,
            notice: None,
            pins: options.pins,
            motion: options.motion,
            ui_scale: options.ui_scale.min(3),
            page: options.ui_page,
            activity,
            moments: Shelf::new(moments::log_path(), demo, now),
            enough: options.enough.filter(|amount| *amount > 0.0),
            worker,
            last_frame: now,
            frame_delta: FxDuration::from_millis(16),
            intro_done: false,
            consecutive_failures: 0,
            fetch_success: false,
            fetch_failure: None,
            fetch_failures: 0,
            confirm_remove: None,
            focus_until: now,
            animating_until: now,
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame);
        self.last_frame = now;
        self.frame_delta = FxDuration::from_millis(delta.as_millis().min(250) as u32);
        if self.activity.refresh(now) {
            self.animating_until = now
                + if self.activity.is_celebrating(now) {
                    Duration::from_secs(3)
                } else {
                    Duration::from_secs(1)
                };
            if self.activity.open
                && self
                    .activity
                    .current_celebration(now)
                    .is_some_and(Celebration::major)
                && let Some(entry) = self.activity.entries.first()
            {
                self.activity.selected_id = Some(entry.id.clone());
            }
        }

        if let Some(moment) = self.moments.refresh(now).last() {
            self.activity.celebration = Some(Celebration::for_moment(moment));
            self.animating_until = now + Duration::from_secs(5);
        }

        let mut apply_cards = false;
        for response in self.worker.drain() {
            match response {
                Response::Project(outcome) => {
                    let order = outcome.order;
                    let updated = self.ingest_project(outcome, now);
                    if updated && (self.scope == 0 || self.scope == order + 1) {
                        apply_cards = true;
                    }
                }
                Response::Done { full } => self.finish_fetch(full, now),
                Response::Range {
                    metric_id,
                    days,
                    result,
                } => self.ingest_range(&metric_id, days, result),
            }
        }
        if apply_cards {
            self.apply_active(true);
            self.normalize_page();
        }

        if now >= self.next_refresh && !matches!(self.status, Status::Fetching) {
            self.dispatch(None, false);
        }
    }

    pub fn poll_timeout(&self) -> Duration {
        let busy = matches!(self.status, Status::Fetching)
            || Instant::now() < self.animating_until
            || self.activity.is_celebrating(Instant::now())
            || self.last_input.elapsed() < Duration::from_secs(1);
        if busy {
            Duration::from_millis(33)
        } else {
            Duration::from_secs(1)
        }
    }

    pub fn refresh_now(&mut self) {
        if !matches!(self.status, Status::Fetching) {
            self.dispatch(None, true);
            self.last_input = Instant::now();
        }
    }

    pub fn retry_project(&mut self, index: usize) {
        if matches!(self.status, Status::Fetching) {
            return;
        }
        self.dispatch(Some(index), false);
    }

    fn dispatch(&mut self, only: Option<usize>, force_charts: bool) {
        self.fetch_success = false;
        self.fetch_failure = None;
        self.fetch_failures = 0;
        self.worker.send(Command::Fetch { only, force_charts });
        self.status = Status::Fetching;
        self.animating_until = Instant::now() + Duration::from_secs(1);
        if only.is_none() {
            self.next_refresh = Instant::now() + self.interval;
        }
    }

    pub fn project_count(&self) -> usize {
        self.views.len()
    }

    pub fn per_page(&self) -> usize {
        match self.ui_scale {
            1 => 3,
            2 => 2,
            3 => 1,
            _ => 0,
        }
    }

    pub fn page_count(&self) -> usize {
        let per_page = self.per_page();
        if per_page == 0 || self.cards.is_empty() {
            1
        } else {
            self.cards.len().div_ceil(per_page)
        }
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        let per_page = self.per_page();
        if per_page == 0 {
            return (0..self.cards.len()).collect();
        }
        if self.cards.is_empty() {
            return Vec::new();
        }
        let page = self.page.min(self.page_count().saturating_sub(1));
        let start = page * per_page;
        (start..(start + per_page).min(self.cards.len())).collect()
    }

    fn normalize_page(&mut self) {
        let page_count = self.page_count().max(1);
        self.page = self.page.min(page_count.saturating_sub(1));
        let visible = self.visible_indices();
        if let Some(first) = visible.first() {
            if !visible.contains(&self.focus) {
                self.focus = *first;
            }
        } else {
            self.focus = 0;
        }
    }

    fn set_ui_scale(&mut self, scale: u8) {
        let scale = scale.min(3);
        if scale == self.ui_scale {
            return;
        }
        self.ui_scale = scale;
        self.page = 0;
        self.normalize_page();
        let saved = self.save_ui_prefs().is_ok();
        let label = if scale == 0 {
            "auto".to_string()
        } else {
            format!("{} per page", self.per_page())
        };
        let suffix = if self.demo {
            String::new()
        } else if saved {
            " (saved)".to_string()
        } else {
            " (not saved)".to_string()
        };
        self.notice = Some((format!("ui scale {label}{suffix}"), Instant::now()));
    }

    fn change_page(&mut self, delta: isize) {
        let pages = self.page_count();
        if pages <= 1 {
            return;
        }
        self.page = (self.page as isize + delta).rem_euclid(pages as isize) as usize;
        self.normalize_page();
        let saved = self.save_ui_prefs().is_ok();
        let suffix = if self.demo {
            String::new()
        } else if saved {
            " (saved)".to_string()
        } else {
            " (not saved)".to_string()
        };
        self.notice = Some((
            format!("page {}/{}{suffix}", self.page + 1, pages),
            Instant::now(),
        ));
    }

    fn save_ui_prefs(&self) -> Result<(), String> {
        if self.demo {
            return Ok(());
        }
        let mut config = config::Config::load();
        config.ui_scale = Some(self.ui_scale);
        config.ui_page = Some(self.page);
        config.save().map(|_| ()).map_err(|err| err.to_string())
    }

    pub fn live_count(&self) -> usize {
        self.views.iter().filter(|view| view.live()).count()
    }

    pub fn failed_count(&self) -> usize {
        self.views
            .iter()
            .filter(|view| view.failure.is_some())
            .count()
    }

    pub fn partial(&self) -> bool {
        self.live_count() < self.project_count() && self.project_count() > 0
    }

    pub fn has_data(&self) -> bool {
        self.views.iter().any(|view| view.snapshot.is_some())
    }

    pub fn stale(&self, now: Instant) -> bool {
        self.last_refresh
            .is_some_and(|then| now.saturating_duration_since(then) > self.interval * 2)
    }

    pub fn stale_after(&self) -> Duration {
        self.interval * 2
    }

    pub fn failed_views(&self) -> Vec<(usize, &ProjectView)> {
        self.views
            .iter()
            .enumerate()
            .filter(|(_, view)| view.failure.is_some())
            .collect()
    }

    pub fn scope_label(&self) -> String {
        match self
            .scope
            .checked_sub(1)
            .and_then(|index| self.views.get(index))
        {
            Some(view) if self.scope > 0 => view.label.clone(),
            _ => {
                if self.views.len() == 1 {
                    self.views[0].label.clone()
                } else {
                    "All projects".to_string()
                }
            }
        }
    }

    pub fn scope_id(&self) -> Option<&str> {
        self.scope
            .checked_sub(1)
            .and_then(|index| self.views.get(index))
            .map(|view| view.id.as_str())
    }

    pub fn select_scope(&mut self, scope: usize) {
        if scope > self.views.len() {
            return;
        }
        self.scope = scope;
        self.picker = None;
        self.health = None;
        self.detail = None;
        self.apply_active(false);
    }

    pub fn set_scope_by_id(&mut self, id: &str) {
        if let Some(index) = self.views.iter().position(|view| view.id == id) {
            self.scope = index + 1;
        }
    }

    pub fn open_activity(&mut self) {
        if self.wall {
            return;
        }
        self.activity.open = true;
        self.activity.mark_seen();
        self.save_feed_seen();
        let ids = self.activity_ids(self.activity.range);
        self.activity.selected_id = ids.first().cloned();
        self.last_input = Instant::now();
    }

    fn close_activity(&mut self) {
        self.activity.open = false;
    }

    /// Opens the moments list, on one moment when an id is given (as "Open rc-tui" from a notification does).
    pub fn open_moments(&mut self, id: Option<&str>) {
        if self.wall {
            return;
        }
        self.moments.open = true;
        self.moments.selected = 0;
        if let Some(id) = id {
            self.moments.select_id(id);
        }
        self.last_input = Instant::now();
    }

    fn moments_key(&mut self, key: KeyEvent) -> Intent {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('m') => self.moments.open = false,
            KeyCode::Char('j') | KeyCode::Down => self.moments.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.moments.move_selection(-1),
            KeyCode::Char('g') | KeyCode::Home => self.moments.selected = 0,
            KeyCode::Char('G') | KeyCode::End => self.moments.move_selection(isize::MAX / 2),
            KeyCode::PageDown => self.moments.move_selection(5),
            KeyCode::PageUp => self.moments.move_selection(-5),
            KeyCode::Enter | KeyCode::Char('o') => self.open_card(),
            KeyCode::Char('?') => self.help = true,
            _ => {}
        }
        Intent::None
    }

    /// Opens the selected moment's card in the default image viewer.
    fn open_card(&mut self) {
        let card = self
            .moments
            .selected_moment()
            .and_then(|moment| moment.card.clone())
            .filter(|card| std::path::Path::new(card).exists());
        let Some(card) = card else {
            self.notice = Some(("this moment has no card".to_string(), Instant::now()));
            return;
        };
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let opened = std::process::Command::new(opener)
            .arg(&card)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok();
        let message = if opened {
            "card opened".to_string()
        } else {
            format!("could not open {card}")
        };
        self.notice = Some((message, Instant::now()));
    }

    fn save_feed_seen(&self) {
        if self.demo {
            return;
        }
        let mut config = config::Config::load();
        config.feed_seen_at = Some(self.activity.seen_at);
        if let Err(err) = config.save() {
            eprintln!("rc-tui: could not save activity cursor: {err}");
        }
    }

    pub fn activity_entries(&self, range: Range) -> Vec<&Entry> {
        let scope_id = self.scope_id().map(str::to_string);
        let label = self.scope_label();
        let now = chrono::Utc::now().timestamp_millis();
        self.activity.entries_for(
            scope_id.as_deref(),
            &label,
            self.activity.filter,
            range,
            now,
        )
    }

    pub fn activity_entries_all(&self, range: Range) -> Vec<&Entry> {
        let scope_id = self.scope_id().map(str::to_string);
        let label = self.scope_label();
        let now = chrono::Utc::now().timestamp_millis();
        self.activity
            .entries_for(scope_id.as_deref(), &label, Filter::All, range, now)
    }

    pub fn activity_unread(&self) -> usize {
        let scope_id = self.scope_id().map(str::to_string);
        let label = self.scope_label();
        self.activity.unread_count(
            scope_id.as_deref(),
            &label,
            chrono::Utc::now().timestamp_millis(),
        )
    }

    pub fn activity_ratings(&self) -> Vec<&Entry> {
        let scope_id = self.scope_id().map(str::to_string);
        let label = self.scope_label();
        let mut ratings: Vec<&Entry> = self
            .activity
            .entries
            .iter()
            .filter(|entry| entry.is_history())
            .filter(|entry| entry.scope_matches(scope_id.as_deref(), &label))
            .collect();
        ratings.sort_by(|left, right| {
            right
                .rating
                .as_ref()
                .map_or(0, |rating| rating.count)
                .cmp(&left.rating.as_ref().map_or(0, |rating| rating.count))
        });
        ratings
    }

    pub fn activity_rating_total(&self) -> i64 {
        let summaries = self.activity_ratings();
        let Some(seeded) = summaries.iter().map(|entry| entry.received_at).max() else {
            return 0;
        };
        let base: i64 = summaries
            .iter()
            .map(|entry| entry.rating.as_ref().map_or(0, |rating| rating.count))
            .sum();
        let scope_id = self.scope_id().map(str::to_string);
        let label = self.scope_label();
        let fresh: i64 = self
            .activity
            .entries
            .iter()
            .filter(|entry| entry.kind() == EntryKind::Rating && !entry.is_history())
            .filter(|entry| entry.timestamp() > seeded)
            .filter(|entry| entry.scope_matches(scope_id.as_deref(), &label))
            .map(|entry| entry.rating.as_ref().map_or(0, |rating| rating.count))
            .sum();
        base + fresh
    }

    pub fn activity_attention(&self) -> Option<&Entry> {
        let scope_id = self.scope_id().map(str::to_string);
        let label = self.scope_label();
        self.activity.attention(
            scope_id.as_deref(),
            &label,
            chrono::Utc::now().timestamp_millis(),
        )
    }

    fn activity_ids(&self, range: Range) -> Vec<String> {
        self.activity_entries(range)
            .into_iter()
            .map(|entry| entry.id.clone())
            .collect()
    }

    fn activity_key(&mut self, key: KeyEvent) -> Intent {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('f') => {
                self.close_activity();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let ids = self.activity_ids(self.activity.range);
                self.activity.move_selection_ids(&ids, 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let ids = self.activity_ids(self.activity.range);
                self.activity.move_selection_ids(&ids, -1);
            }
            KeyCode::Char('g') | KeyCode::Home => {
                let ids = self.activity_ids(self.activity.range);
                self.activity.selected_id = ids.first().cloned();
            }
            KeyCode::Char('G') | KeyCode::End => {
                let ids = self.activity_ids(self.activity.range);
                self.activity.selected_id = ids.last().cloned();
            }
            KeyCode::PageUp => {
                let ids = self.activity_ids(self.activity.range);
                self.activity.move_selection_ids(&ids, -10);
            }
            KeyCode::PageDown => {
                let ids = self.activity_ids(self.activity.range);
                self.activity.move_selection_ids(&ids, 10);
            }
            KeyCode::Tab => {
                self.activity.filter = self.activity.filter.next();
                let ids = self.activity_ids(self.activity.range);
                self.activity.selected_id = ids.first().cloned();
            }
            KeyCode::Char('[') => {
                self.activity.range = self.activity.range.previous();
                let ids = self.activity_ids(self.activity.range);
                self.activity.selected_id = ids.first().cloned();
            }
            KeyCode::Char(']') => {
                self.activity.range = self.activity.range.next();
                let ids = self.activity_ids(self.activity.range);
                self.activity.selected_id = ids.first().cloned();
            }
            KeyCode::Char('?') => self.help = true,
            _ => {}
        }
        Intent::None
    }

    pub fn open_picker(&mut self) {
        self.picker = Some(Picker {
            query: String::new(),
            selected: 0,
        });
        self.last_input = Instant::now();
    }

    pub fn picker_entries(&self) -> Vec<PickerEntry> {
        let query = self
            .picker
            .as_ref()
            .map(|picker| picker.query.trim().to_lowercase())
            .unwrap_or_default();
        let now = Instant::now();
        let mut entries = Vec::new();
        if query.is_empty() {
            entries.push(PickerEntry {
                scope: 0,
                view: None,
                label: "All projects".to_string(),
                detail: format!("{} projects", self.views.len()),
                state: RowState::Live,
            });
        }
        let mut order: Vec<usize> = (0..self.views.len()).collect();
        order.sort_by_key(|index| {
            let view = &self.views[*index];
            let pinned = self.pins.iter().position(|pinned| pinned == &view.id);
            (pinned.is_none(), pinned.unwrap_or(usize::MAX), *index)
        });
        for index in order {
            let view = &self.views[index];
            if !query.is_empty()
                && !view.label.to_lowercase().contains(&query)
                && !view.id.to_lowercase().contains(&query)
            {
                continue;
            }
            entries.push(PickerEntry {
                scope: index + 1,
                view: Some(index),
                label: view.label.clone(),
                detail: view.id.clone(),
                state: view.state(now, self.stale_after()),
            });
        }
        entries
    }

    pub fn picker_selected(&self) -> usize {
        self.picker.as_ref().map_or(0, |picker| picker.selected)
    }

    pub fn card(&self, ids: &[&str]) -> Option<&Card> {
        self.cards
            .iter()
            .find(|card| ids.contains(&card.id.as_str()))
    }

    /// Every project with the people it served and the money it earned, most people first.
    pub fn garden_rows(&self) -> Vec<GardenRow> {
        let now = Instant::now();
        let mut rows: Vec<GardenRow> = self
            .views
            .iter()
            .filter_map(|view| {
                let snapshot = view.snapshot.as_ref()?;
                let value = |ids: &[&str]| {
                    snapshot
                        .metrics
                        .iter()
                        .find(|metric| ids.contains(&metric.id.as_str()))
                        .map(|metric| metric.value)
                };
                Some(GardenRow {
                    label: view.label.clone(),
                    people: value(&PEOPLE),
                    revenue: value(&["revenue"]),
                    state: view.state(now, self.stale_after()),
                })
            })
            .collect();
        rows.sort_by(|left, right| {
            let people = right
                .people
                .unwrap_or(0.0)
                .total_cmp(&left.people.unwrap_or(0.0));
            people
                .then(
                    right
                        .revenue
                        .unwrap_or(0.0)
                        .total_cmp(&left.revenue.unwrap_or(0.0)),
                )
                .then_with(|| left.label.cmp(&right.label))
        });
        rows
    }

    pub fn health_rows(&self) -> Vec<HealthRow> {
        let now = Instant::now();
        self.views
            .iter()
            .map(|view| HealthRow {
                label: view.label.clone(),
                detail: view.id.clone(),
                state: view.state(now, self.stale_after()),
                failure: view.failure.clone(),
                last_success: view.last_success,
            })
            .collect()
    }

    pub fn open_health(&mut self) {
        let first = self
            .views
            .iter()
            .position(|view| view.failure.is_some())
            .unwrap_or(0);
        self.health = Some(first);
        self.last_input = Instant::now();
    }

    pub fn open_detail(&mut self) {
        if self.cards.is_empty() {
            return;
        }
        self.focus = self.focus.min(self.cards.len() - 1);
        let metric_id = self.cards[self.focus].id.clone();
        self.detail = Some(Detail {
            metric_id,
            window: Window::D28,
            range: None,
            loading: false,
            error: None,
            selected: 0,
        });
        self.last_input = Instant::now();
    }

    pub fn detail_values(&self) -> Vec<f64> {
        let Some(detail) = &self.detail else {
            return Vec::new();
        };
        if detail.window == Window::D90 {
            return detail.range.clone().unwrap_or_default();
        }
        let series = self.series_for(&detail.metric_id);
        let days = detail.window.days() as usize;
        series.iter().rev().take(days).rev().copied().collect()
    }

    pub fn detail_rows(&self) -> Vec<DetailRow> {
        let Some(detail) = &self.detail else {
            return Vec::new();
        };
        let now = Instant::now();
        let total: f64 = self
            .views
            .iter()
            .filter_map(|view| view.snapshot.as_ref())
            .filter_map(|snapshot| {
                snapshot
                    .metrics
                    .iter()
                    .find(|metric| metric.id == detail.metric_id)
                    .map(|metric| metric.value)
            })
            .sum();
        let mut rows: Vec<DetailRow> = self
            .views
            .iter()
            .enumerate()
            .filter_map(|(index, view)| {
                let snapshot = view.snapshot.as_ref()?;
                let metric = snapshot
                    .metrics
                    .iter()
                    .find(|metric| metric.id == detail.metric_id)?;
                let series = snapshot.series.get(&detail.metric_id);
                let trend = series.and_then(|series| trend_for(&detail.metric_id, series));
                Some(DetailRow {
                    view: index,
                    label: view.label.clone(),
                    value: metric.value,
                    share: if total.abs() > 1e-9 {
                        metric.value / total
                    } else {
                        0.0
                    },
                    trend,
                    state: view.state(now, self.stale_after()),
                })
            })
            .collect();
        rows.sort_by(|left, right| {
            right
                .value
                .partial_cmp(&left.value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        rows
    }

    fn series_for(&self, metric_id: &str) -> Vec<f64> {
        match self
            .scope
            .checked_sub(1)
            .and_then(|index| self.views.get(index))
        {
            Some(view) if self.scope > 0 => view
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.series.get(metric_id).cloned())
                .unwrap_or_default(),
            _ => self
                .active_snapshot()
                .and_then(|snapshot| snapshot.series.get(metric_id).cloned())
                .unwrap_or_default(),
        }
    }

    pub fn active_snapshot(&self) -> Option<Snapshot> {
        if let Some(view) = self
            .scope
            .checked_sub(1)
            .and_then(|index| self.views.get(index))
        {
            return view.snapshot.clone();
        }
        let snapshots: Vec<Snapshot> = self
            .views
            .iter()
            .filter_map(|view| view.snapshot.clone())
            .collect();
        if snapshots.is_empty() {
            None
        } else {
            Some(api::aggregate(&snapshots))
        }
    }

    fn apply_active(&mut self, animate: bool) {
        if let Some(snapshot) = self.active_snapshot() {
            self.apply(snapshot, Instant::now(), animate);
        }
    }

    fn ingest_project(&mut self, outcome: ProjectOutcome, now: Instant) -> bool {
        if outcome.order >= self.views.len() {
            self.fetch_failures += 1;
            if let Err(failure) = outcome.result {
                self.fetch_failure.get_or_insert(failure);
            }
            return false;
        }
        match outcome.result {
            Ok(snapshot) => {
                let view = &mut self.views[outcome.order];
                view.snapshot = Some(snapshot);
                view.failure = None;
                view.last_success = Some(now);
                self.fetch_success = true;
                self.last_updated = Some(now);
                self.animating_until = now + Duration::from_secs(2);
                true
            }
            Err(failure) => {
                self.views[outcome.order].failure = Some(failure.clone());
                self.fetch_failures += 1;
                self.fetch_failure.get_or_insert(failure);
                false
            }
        }
    }

    fn finish_fetch(&mut self, full: bool, now: Instant) {
        let failure = self.fetch_failure.clone();
        if self.fetch_success {
            self.status = Status::Idle;
            self.consecutive_failures = 0;
            if full {
                self.last_refresh = Some(now);
                self.next_refresh = now + self.interval;
            }
        } else if self.fetch_failures > 0 {
            self.status = if self.has_data() {
                Status::Idle
            } else {
                Status::Error(
                    failure
                        .clone()
                        .unwrap_or_else(|| Failure::local("refresh failed")),
                )
            };
            if full {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                let backoff = Duration::from_secs(60)
                    .saturating_mul(1u32 << (self.consecutive_failures - 1).min(4));
                let wait = failure
                    .and_then(|failure| failure.retry_after)
                    .unwrap_or(backoff)
                    .min(self.interval);
                self.next_refresh = now + wait;
            }
        } else {
            self.status = Status::Idle;
            if full {
                self.next_refresh = now + self.interval;
            }
        }

        self.animating_until = now + Duration::from_secs(2);
        self.fetch_success = false;
        self.fetch_failure = None;
        self.fetch_failures = 0;
        self.normalize_page();
    }

    #[cfg(test)]
    fn ingest(&mut self, outcomes: Vec<ProjectOutcome>, full: bool, now: Instant) {
        self.fetch_success = false;
        self.fetch_failure = None;
        self.fetch_failures = 0;
        let mut apply_cards = false;
        for outcome in outcomes {
            let order = outcome.order;
            if self.ingest_project(outcome, now) && (self.scope == 0 || self.scope == order + 1) {
                apply_cards = true;
            }
        }
        if apply_cards {
            self.apply_active(true);
        }
        self.finish_fetch(full, now);
    }

    fn ingest_range(&mut self, metric_id: &str, days: u64, result: Result<Vec<f64>, Failure>) {
        let Some(detail) = &mut self.detail else {
            return;
        };
        if detail.metric_id != metric_id || detail.window.days() != days {
            return;
        }
        detail.loading = false;
        match result {
            Ok(series) => {
                detail.range = Some(series);
                detail.error = None;
            }
            Err(failure) => detail.error = Some(failure),
        }
    }

    pub fn frame_delta(&self) -> FxDuration {
        self.frame_delta
    }

    pub fn focus_visible(&self, now: Instant) -> bool {
        now < self.focus_until
    }

    pub fn notice_active(&self, now: Instant) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|(_, at)| now.saturating_duration_since(*at) < NOTICE_TTL)
            .map(|(text, _)| text.as_str())
    }

    fn apply(&mut self, snapshot: Snapshot, now: Instant, animate: bool) {
        if !snapshot.currency.is_empty() {
            self.currency = snapshot.currency.clone();
        }
        let durations = if animate && self.motion != Motion::Off {
            (VALUE_TWEEN, SERIES_TWEEN)
        } else {
            (Duration::ZERO, Duration::ZERO)
        };
        let focused = self.cards.get(self.focus).map(|card| card.id.clone());
        for metric in &snapshot.metrics {
            let series = snapshot.series.get(&metric.id).cloned().unwrap_or_default();
            let mut changed = false;
            match self.cards.iter_mut().find(|card| card.id == metric.id) {
                Some(card) => {
                    let previous = card.value.target();
                    let threshold = previous.abs().max(1.0) * 0.005;
                    changed = (metric.value - previous).abs() > threshold;
                    card.trend = trend_for(&metric.id, &series);
                    card.name = metric.name.clone();
                    card.unit = metric.unit.clone();
                    card.period = metric.period.clone();
                    card.value.set(metric.value, now, durations.0);
                    if !series.is_empty() {
                        card.series.set(series, now, durations.1);
                    }
                    card.updated = Some(now);
                }
                None => {
                    let rank = card_rank(&metric.id);
                    let at = self
                        .cards
                        .iter()
                        .position(|card| card_rank(&card.id) > rank)
                        .unwrap_or(self.cards.len());
                    self.cards.insert(at, Card::new(metric, series, now));
                }
            }

            if animate
                && changed
                && self.motion == Motion::Full
                && let Some(rect) = self.rects.get(&metric.id)
            {
                let flash = fx::fade_from_fg(
                    theme::accent(&metric.id),
                    (FLASH_MS, Interpolation::CubicOut),
                )
                .with_filter(CellFilter::Text)
                .with_area(*rect);
                self.fx.add_unique_effect(metric.id.clone(), flash);
            }
        }

        self.focus = focused
            .and_then(|id| self.cards.iter().position(|card| card.id == id))
            .unwrap_or(0);

        if !self.intro_done {
            self.intro_done = true;
            if self.motion == Motion::Full {
                self.fx.add_effect(fx::sweep_in(
                    FxMotion::LeftToRight,
                    24,
                    0,
                    theme::bg(),
                    (600u32, Interpolation::QuadOut),
                ));
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Intent {
        self.last_input = Instant::now();
        self.focus_until = self.last_input + FOCUS_VISIBLE;
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Intent::Quit;
        }
        if self.help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                self.help = false;
            }
            return Intent::None;
        }
        if self.activity.open {
            return self.activity_key(key);
        }
        if self.moments.open {
            return self.moments_key(key);
        }
        if self.detail.is_some() {
            return self.detail_key(key);
        }
        if self.health.is_some() {
            return self.health_key(key);
        }
        if self.picker.is_some() {
            return self.picker_key(key);
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Intent::Quit,
            KeyCode::Char('r') => {
                self.refresh_now();
                Intent::None
            }
            KeyCode::Char('p') => {
                self.open_picker();
                Intent::None
            }
            KeyCode::Char('f') => {
                self.open_activity();
                Intent::None
            }
            KeyCode::Char('m') => {
                self.open_moments(None);
                Intent::None
            }
            KeyCode::Char('?') => {
                self.help = true;
                Intent::None
            }
            KeyCode::Char('!') => {
                self.open_health();
                Intent::None
            }
            KeyCode::Char('w') => {
                self.toggle_wall();
                Intent::None
            }
            KeyCode::Char('T') => {
                self.toggle_theme();
                Intent::None
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.set_ui_scale(self.ui_scale.saturating_add(1));
                Intent::None
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.set_ui_scale(self.ui_scale.saturating_sub(1));
                Intent::None
            }
            KeyCode::Char('[') => {
                self.change_page(-1);
                Intent::None
            }
            KeyCode::Char(']') => {
                self.change_page(1);
                Intent::None
            }
            KeyCode::Char('x') => {
                let _ = self.export_csv();
                Intent::None
            }
            KeyCode::Left | KeyCode::Up | KeyCode::BackTab => {
                self.move_focus(-1);
                Intent::None
            }
            KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
                self.move_focus(1);
                Intent::None
            }
            KeyCode::Enter => {
                self.open_detail();
                Intent::None
            }
            KeyCode::Char('a') if !self.demo => Intent::AddProject,
            KeyCode::Char('i') if !self.demo => Intent::Import,
            KeyCode::Char('s') if !self.demo => Intent::Setup,
            _ => Intent::None,
        }
    }

    fn move_focus(&mut self, delta: isize) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            return;
        }
        let position = visible
            .iter()
            .position(|index| *index == self.focus)
            .unwrap_or(0);
        let next = (position as isize + delta).rem_euclid(visible.len() as isize) as usize;
        self.focus = visible[next];
    }

    fn picker_key(&mut self, key: KeyEvent) -> Intent {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let selected = self.picker_selected();
        let entries = self.picker_entries();
        let entry = entries.get(selected).cloned();
        match key.code {
            KeyCode::Esc => {
                self.picker = None;
                self.confirm_remove = None;
                return Intent::None;
            }
            KeyCode::Enter => {
                if let Some(entry) = entry {
                    self.select_scope(entry.scope);
                }
                return Intent::None;
            }
            KeyCode::Char('e') if control => {
                return entry
                    .and_then(|entry| entry.view)
                    .map_or(Intent::None, Intent::EditProject);
            }
            KeyCode::Char('d') if control => {
                let Some(view) = entry.and_then(|entry| entry.view) else {
                    return Intent::None;
                };
                if self.confirm_remove == Some(view) {
                    self.picker = None;
                    self.confirm_remove = None;
                    return Intent::RemoveProject(view);
                }
                self.confirm_remove = Some(view);
                return Intent::None;
            }
            KeyCode::Char('f') if control => {
                if let Some(view) = entry.and_then(|entry| entry.view) {
                    self.toggle_pin(view);
                }
                return Intent::None;
            }
            KeyCode::Backspace => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.query.pop();
                    picker.selected = 0;
                }
                self.confirm_remove = None;
            }
            KeyCode::Up => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.selected = picker.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.selected += 1;
                }
            }
            KeyCode::Char(character) if !control && !character.is_control() => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.query.push(character);
                    picker.selected = 0;
                }
                self.confirm_remove = None;
            }
            _ => {}
        }
        let count = self.picker_entries().len();
        if let Some(picker) = self.picker.as_mut() {
            picker.selected = picker.selected.min(count.saturating_sub(1));
        }
        Intent::None
    }

    fn toggle_theme(&mut self) {
        let mode = theme::toggle();
        let suffix = if self.demo {
            String::new()
        } else if self.save_theme_pref(mode).is_ok() {
            " (saved)".to_string()
        } else {
            " (not saved)".to_string()
        };
        self.notice = Some((format!("{} theme{suffix}", mode.label()), Instant::now()));
    }

    fn save_theme_pref(&self, mode: theme::Mode) -> Result<(), String> {
        let mut config = config::Config::load();
        config.theme = Some(mode.label().to_string());
        config.save().map(|_| ()).map_err(|err| err.to_string())
    }

    fn toggle_wall(&mut self) {
        self.wall = !self.wall;
        if self.demo {
            return;
        }
        let mut config = config::Config::load();
        config.wall = Some(self.wall);
        match config.save() {
            Ok(_) => {
                self.notice = Some((
                    format!("wall mode {}", if self.wall { "on" } else { "off" }),
                    Instant::now(),
                ));
            }
            Err(err) => {
                self.notice = Some((format!("could not save wall mode: {err}"), Instant::now()));
            }
        }
    }

    fn toggle_pin(&mut self, view: usize) {
        let Some(project) = self.views.get(view) else {
            return;
        };
        let id = project.id.clone();
        let label = project.label.clone();
        let mut config = config::Config::load();
        config.toggle_pin(&id);
        match config.save() {
            Ok(_) => {
                self.pins = config.pins;
                let pinned = self.pins.contains(&id);
                self.notice = Some((
                    format!(
                        "{} {}{}",
                        if pinned { "pinned" } else { "unpinned" },
                        label,
                        if pinned { " to the top" } else { "" }
                    ),
                    Instant::now(),
                ));
            }
            Err(err) => {
                self.notice = Some((format!("could not save pins: {err}"), Instant::now()));
            }
        }
    }

    fn health_key(&mut self, key: KeyEvent) -> Intent {
        let selected = self.health.unwrap_or(0);
        match key.code {
            KeyCode::Esc => {
                self.health = None;
            }
            KeyCode::Up => {
                self.health = Some(selected.saturating_sub(1));
            }
            KeyCode::Down => {
                self.health = Some((selected + 1).min(self.views.len().saturating_sub(1)));
            }
            KeyCode::Char('r') => {
                self.retry_project(selected);
            }
            KeyCode::Char('R') => {
                self.refresh_now();
            }
            KeyCode::Enter => {
                if selected < self.views.len() {
                    self.select_scope(selected + 1);
                }
            }
            KeyCode::Char('q') => return Intent::Quit,
            _ => {}
        }
        Intent::None
    }

    fn detail_key(&mut self, key: KeyEvent) -> Intent {
        let selected = self.detail.as_ref().map_or(0, |detail| detail.selected);
        let rows = self.detail_rows().len();
        match key.code {
            KeyCode::Esc => {
                self.detail = None;
                self.normalize_page();
            }
            KeyCode::Char('q') => return Intent::Quit,
            KeyCode::Char('r') => self.refresh_now(),
            KeyCode::Up => self.switch_metric(-1),
            KeyCode::Down => self.switch_metric(1),
            KeyCode::Char('k') => {
                if let Some(detail) = self.detail.as_mut() {
                    detail.selected = selected.saturating_sub(1);
                }
            }
            KeyCode::Char('j') => {
                if let Some(detail) = self.detail.as_mut() {
                    detail.selected = (selected + 1).min(rows.saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                if let Some(row) = self.detail_rows().get(selected) {
                    let view = row.view;
                    self.select_scope(view + 1);
                }
            }
            KeyCode::Char('t') => {
                let next = self
                    .detail
                    .as_ref()
                    .map_or(Window::D28, |detail| detail.window.next());
                if let Some(detail) = self.detail.as_mut() {
                    detail.window = next;
                    detail.range = None;
                    detail.error = None;
                }
                if next == Window::D90 {
                    self.request_range();
                }
            }
            _ => {}
        }
        Intent::None
    }

    fn switch_metric(&mut self, delta: isize) {
        if self.cards.is_empty() {
            return;
        }
        self.focus = (self.focus as isize + delta).rem_euclid(self.cards.len() as isize) as usize;
        let metric_id = self.cards[self.focus].id.clone();
        let window = self
            .detail
            .as_ref()
            .map_or(Window::D28, |detail| detail.window);
        if let Some(detail) = self.detail.as_mut() {
            detail.metric_id = metric_id;
            detail.selected = 0;
            detail.range = None;
            detail.error = None;
        }
        if window == Window::D90 {
            self.request_range();
        }
    }

    fn request_range(&mut self) {
        let Some(detail) = &self.detail else {
            return;
        };
        let Some(card) = self.cards.iter().find(|card| card.id == detail.metric_id) else {
            return;
        };
        let Some(chart) = api::chart_for_metric(&card.id, &card.name) else {
            return;
        };
        let monetary = card.unit == "$" || card.unit.eq_ignore_ascii_case(&self.currency);
        let only = self.scope.checked_sub(1);
        self.worker.send(Command::Range {
            only,
            chart: chart.to_string(),
            metric_id: card.id.clone(),
            days: detail.window.days(),
            monetary,
            measure: api::measure_for_metric(&card.id),
        });
        if let Some(detail) = self.detail.as_mut() {
            detail.loading = true;
            detail.error = None;
        }
    }

    pub fn export_csv(&mut self) -> std::io::Result<std::path::PathBuf> {
        let directory = config::data_dir().join("rc-tui").join("exports");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!(
            "rc-tui-{}.csv",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        std::fs::write(&path, self.csv_body())?;
        self.notice = Some((format!("exported to {}", path.display()), Instant::now()));
        Ok(path)
    }

    fn csv_body(&self) -> String {
        let end = chrono::Utc::now().date_naive();
        let scope = self.scope_label();
        let now = Instant::now();
        let snapshot = self.active_snapshot();
        let mut body = String::from("scope,metric,metric_name,date,value,unit,period\n");
        for card in &self.cards {
            let values = snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.series.get(&card.id))
                .cloned()
                .unwrap_or_default();
            if values.is_empty() {
                let _ = writeln!(
                    body,
                    "{},{},{},{},{:.4},{},{}",
                    quote(&scope),
                    quote(&card.id),
                    quote(&card.name),
                    end,
                    card.value.value(now),
                    quote(&card.unit),
                    quote(&card.period)
                );
                continue;
            }
            for (index, value) in values.iter().enumerate() {
                let date = end
                    .checked_sub_days(Days::new((values.len() - 1 - index) as u64))
                    .unwrap_or(end);
                let _ = writeln!(
                    body,
                    "{},{},{},{},{:.4},{},{}",
                    quote(&scope),
                    quote(&card.id),
                    quote(&card.name),
                    date,
                    value,
                    quote(&card.unit),
                    quote(&card.period)
                );
            }
        }
        body
    }

    pub fn confirm_remove(&self) -> Option<usize> {
        self.confirm_remove
    }
}

fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(sources: Vec<Source>) -> AppOptions {
        AppOptions {
            sources,
            currency: "EUR".to_string(),
            interval: Duration::from_secs(300),
            chart_ttl: Duration::from_secs(900),
            motion: Motion::Full,
            wall: false,
            pins: Vec::new(),
            ui_scale: 0,
            ui_page: 0,
            feed_path: None,
            feed_seen_at: None,
            enough: None,
        }
    }

    fn demo_app() -> App {
        App::new(options(vec![
            Source::demo("One", "proj1"),
            Source::demo("Two", "proj2"),
            Source::demo("Three", "proj3"),
        ]))
    }

    fn wait_for_data(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !app.has_data() && Instant::now() < deadline {
            app.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(app.has_data(), "no data arrived");
    }

    #[test]
    fn rating_total_adds_only_events_observed_after_the_all_time_seed() {
        let mut app = demo_app();
        let seeded = 1_760_000_000_000;
        let summary = Entry {
            id: "rating-history:app".to_string(),
            kind: "rating".to_string(),
            received_at: seeded,
            app: "One".to_string(),
            project_id: "proj1".to_string(),
            backfilled: true,
            rating: Some(crate::activity::Rating {
                count: 24,
                average: 4.7,
                summary: true,
                ..crate::activity::Rating::default()
            }),
            ..Entry::default()
        };
        let live = Entry {
            id: "rating:app:us:25".to_string(),
            kind: "rating".to_string(),
            at: seeded + 1_000,
            received_at: seeded + 1_000,
            app: "One".to_string(),
            project_id: "proj1".to_string(),
            rating: Some(crate::activity::Rating {
                count: 1,
                average: 4.8,
                ..crate::activity::Rating::default()
            }),
            ..Entry::default()
        };
        let already_counted = Entry {
            id: "rating:app:us:24".to_string(),
            kind: "rating".to_string(),
            at: seeded - 1_000,
            received_at: seeded + 2_000,
            app: "One".to_string(),
            project_id: "proj1".to_string(),
            rating: Some(crate::activity::Rating {
                count: 1,
                average: 4.7,
                ..crate::activity::Rating::default()
            }),
            ..Entry::default()
        };
        app.activity.entries = vec![summary, live, already_counted];
        assert_eq!(app.activity_rating_total(), 25);
    }

    #[test]
    fn demo_app_aggregates_several_projects() {
        let mut app = demo_app();
        wait_for_data(&mut app);

        assert_eq!(app.cards.len(), 7);
        assert_eq!(app.views.len(), 3);
        assert_eq!(app.scope_label(), "All projects");
        assert!(matches!(app.status, Status::Idle));
        let order: Vec<&str> = app.cards.iter().map(|card| card.id.as_str()).collect();
        assert_eq!(
            order,
            [
                "active_users",
                "revenue",
                "new_customers",
                "purchases",
                "active_subscriptions",
                "mrr",
                "active_trials"
            ]
        );
        assert_eq!(app.focus, 0);

        let aggregate_mrr = app
            .cards
            .iter()
            .find(|card| card.id == "mrr")
            .map(|card| card.value.value(Instant::now() + Duration::from_secs(2)))
            .unwrap();
        let summed: f64 = app
            .views
            .iter()
            .filter_map(|view| view.snapshot.as_ref())
            .map(|snapshot| {
                snapshot
                    .metrics
                    .iter()
                    .find(|metric| metric.id == "mrr")
                    .map_or(0.0, |metric| metric.value)
            })
            .sum();
        assert!((aggregate_mrr - summed).abs() < 0.01);

        app.select_scope(1);
        assert_eq!(app.scope_label(), "One");
        assert_eq!(app.cards.len(), 7);
        let scoped_mrr = app
            .cards
            .iter()
            .find(|card| card.id == "mrr")
            .map(|card| card.value.value(Instant::now() + Duration::from_secs(2)))
            .unwrap();
        let own_mrr = app.views[0]
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.metrics.iter().find(|metric| metric.id == "mrr"))
            .map_or(0.0, |metric| metric.value);
        assert!(
            (scoped_mrr - own_mrr).abs() < 0.01,
            "scoped card {scoped_mrr} != project {own_mrr}"
        );
        app.select_scope(0);
        assert_eq!(app.scope_label(), "All projects");
    }

    #[test]
    fn the_picker_filters_by_name_and_id_and_lists_all_projects() {
        let mut app = demo_app();
        wait_for_data(&mut app);

        app.open_picker();
        assert_eq!(app.picker_entries().len(), 4);
        for character in "two".chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(character)));
        }
        let entries = app.picker_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, "Two");

        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(app.picker.is_none());
        assert_eq!(app.scope_label(), "Two");

        app.open_picker();
        for character in "proj1".chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(character)));
        }
        assert_eq!(app.picker_entries().len(), 1);
        app.handle_key(KeyEvent::from(KeyCode::Esc));
        assert!(app.picker.is_none());
    }

    #[test]
    fn failed_projects_keep_their_last_snapshot_and_are_flagged() {
        let mut app = demo_app();
        wait_for_data(&mut app);
        let before = app
            .views
            .iter()
            .filter_map(|view| view.snapshot.as_ref())
            .count();
        assert_eq!(before, 3);

        app.ingest(
            vec![ProjectOutcome {
                order: 0,
                result: Err(Failure::local("boom")),
            }],
            true,
            Instant::now(),
        );
        assert!(app.views[0].snapshot.is_some());
        assert_eq!(app.failed_count(), 1);
        assert!(app.partial());
        assert_eq!(app.live_count(), 2);
    }

    #[test]
    fn detail_rows_sort_by_value_and_share_sums_to_one() {
        let mut app = demo_app();
        wait_for_data(&mut app);
        app.open_detail();
        let rows = app.detail_rows();
        assert_eq!(rows.len(), 3);
        let share: f64 = rows.iter().map(|row| row.share).sum();
        assert!((share - 1.0).abs() < 0.001);
        for pair in rows.windows(2) {
            assert!(pair[0].value >= pair[1].value);
        }
    }

    #[test]
    fn ui_scale_pages_cards_without_losing_focus() {
        let mut app = demo_app();
        wait_for_data(&mut app);

        app.set_ui_scale(1);
        assert_eq!(app.per_page(), 3);
        assert_eq!(app.page_count(), 3);
        assert_eq!(app.visible_indices(), vec![0, 1, 2]);
        assert_eq!(app.focus, 0);

        app.move_focus(1);
        assert_eq!(app.focus, 1);
        app.change_page(1);
        assert_eq!(app.visible_indices(), vec![3, 4, 5]);
        assert_eq!(app.focus, 3);

        app.set_ui_scale(2);
        assert_eq!(app.page_count(), 4);
        assert_eq!(app.visible_indices(), vec![0, 1]);
        app.set_ui_scale(3);
        assert_eq!(app.page_count(), 7);
        assert_eq!(app.visible_indices(), vec![0]);
        app.set_ui_scale(0);
        assert_eq!(app.visible_indices().len(), 7);
    }

    #[test]
    fn failed_projects_stay_in_the_picker_and_health_view() {
        let mut app = demo_app();
        wait_for_data(&mut app);
        app.ingest(
            vec![ProjectOutcome {
                order: 1,
                result: Err(Failure::local("boom")),
            }],
            true,
            Instant::now(),
        );

        app.open_picker();
        let entries = app.picker_entries();
        let failed = entries
            .iter()
            .find(|entry| entry.label == "Two")
            .expect("failed project should stay in the picker");
        assert_eq!(failed.state, RowState::Failed);

        let health = app.health_rows();
        assert_eq!(health.len(), 3);
        assert_eq!(health[1].state, RowState::Failed);
        assert!(health[1].failure.is_some());
    }

    #[test]
    fn stale_after_two_intervals_and_idle_polls_slowly() {
        let mut app = demo_app();
        wait_for_data(&mut app);
        let now = Instant::now();
        app.last_refresh = Some(now - Duration::from_secs(601));
        assert!(app.stale(now));

        app.last_refresh = Some(now);
        app.last_input = now - Duration::from_secs(30);
        app.animating_until = now - Duration::from_secs(1);
        app.status = Status::Idle;
        assert_eq!(app.poll_timeout(), Duration::from_secs(1));
    }

    #[test]
    fn the_ninety_day_window_loads_from_the_worker() {
        let mut app = demo_app();
        wait_for_data(&mut app);
        app.open_detail();
        if let Some(detail) = app.detail.as_mut() {
            detail.window = Window::D90;
        }
        app.request_range();

        let deadline = Instant::now() + Duration::from_secs(5);
        while app.detail.as_ref().is_some_and(|detail| detail.loading) && Instant::now() < deadline
        {
            app.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        let values = app.detail_values();
        assert_eq!(values.len(), 90);
    }

    #[test]
    fn csv_export_covers_every_metric_and_day() {
        let mut app = demo_app();
        wait_for_data(&mut app);
        let body = app.csv_body();
        assert_eq!(body.lines().count(), 1 + 7 * 57);
        assert!(body.contains("\"All projects\",\"mrr\",\"MRR\""));
        assert!(body.contains("\"All projects\",\"purchases\",\"Purchases\""));
        assert!(body.starts_with("scope,metric,metric_name,date,value,unit,period\n"));
    }

    #[test]
    fn detail_window_slices_the_cached_series() {
        let mut app = demo_app();
        wait_for_data(&mut app);
        app.open_detail();
        let twenty_eight = app.detail_values().len();
        assert_eq!(twenty_eight, 28);
        if let Some(detail) = app.detail.as_mut() {
            detail.window = Window::D7;
        }
        assert_eq!(app.detail_values().len(), 7);
    }

    #[test]
    fn trend_compares_28_complete_days_with_the_28_before() {
        let mut series = vec![10.0; 28];
        series.extend([20.0; 28]);
        series.push(1.0);

        let revenue = trend_for("revenue", &series).expect("trend");
        assert_eq!(revenue.days, 28);
        assert!((revenue.ratio - 1.0).abs() < 1e-9, "{revenue:?}");
        assert!((revenue.change - 280.0).abs() < 1e-9, "{revenue:?}");
        assert!((revenue.previous - 280.0).abs() < 1e-9, "{revenue:?}");

        let people = trend_for("active_users", &series).expect("trend");
        assert!((people.ratio - 1.0).abs() < 1e-9, "{people:?}");
        assert!((people.change - 10.0).abs() < 1e-9, "{people:?}");

        let mut stock: Vec<f64> = (0..56).map(f64::from).collect();
        stock.push(0.0);
        let mrr = trend_for("mrr", &stock).expect("trend");
        assert!((mrr.change - 28.0).abs() < 1e-9, "{mrr:?}");
    }

    #[test]
    fn trend_leaves_out_the_partial_current_day() {
        let mut series = vec![50.0; 28];
        series.extend([25.0; 28]);
        series.push(10_000.0);

        let trend = trend_for("active_users", &series).expect("trend");
        assert!(
            (trend.ratio - -0.5).abs() < 1e-9,
            "ratio was {}",
            trend.ratio
        );
    }

    #[test]
    fn neighbouring_demo_projects_get_their_own_numbers() {
        assert_ne!(demo_salt("proj_demo_6"), demo_salt("proj_demo_7"));
        let first = api::demo_snapshot("EUR", demo_salt("proj_demo_6"));
        let second = api::demo_snapshot("EUR", demo_salt("proj_demo_7"));
        assert_ne!(first.series["revenue"], second.series["revenue"]);
    }

    #[test]
    fn trend_shrinks_to_the_history_it_has() {
        let trend = trend_for("revenue", &[5.0; 29]).expect("trend");
        assert_eq!(trend.days, 14);
        assert!(trend.ratio.abs() < 1e-9);
    }

    #[test]
    fn trend_needs_two_complete_weeks() {
        assert!(trend_for("active_users", &[10.0; 14]).is_none());
        assert!(trend_for("active_users", &[10.0; 15]).is_some());
    }

    #[test]
    fn demo_app_refreshes() {
        let mut app = demo_app();
        wait_for_data(&mut app);

        app.refresh_now();
        assert!(matches!(app.status, Status::Fetching));

        let deadline = Instant::now() + Duration::from_secs(5);
        while matches!(app.status, Status::Fetching) && Instant::now() < deadline {
            app.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            matches!(app.status, Status::Idle),
            "refresh did not finish: {:?}",
            app.status
        );
    }
}
