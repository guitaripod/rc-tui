use std::collections::HashMap;
use std::time::Duration;

use chrono::{Days, Utc};
use serde::Deserialize;

const CHART_WINDOW_DAYS: u64 = 28;

pub const DOCS_API_KEYS: &str = "https://www.revenuecat.com/docs/projects/authentication";

pub const SCOPE_OVERVIEW: &str = "charts_metrics:overview:read";
pub const SCOPE_CHARTS: &str = "charts_metrics:charts:read";
pub const SCOPE_PROJECTS: &str = "project_configuration:projects:read";

#[derive(Debug, Clone)]
pub struct Settings {
    pub base_url: String,
    pub api_key: String,
    pub project_id: String,
    pub currency: String,
}

impl Settings {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        project_id: impl Into<String>,
        currency: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            project_id: project_id.into(),
            currency: currency.into(),
        }
    }

    pub fn redacted_key(&self) -> String {
        redact(&self.api_key)
    }
}

pub fn redact(secret: &str) -> String {
    let secret = secret.trim();
    let count = secret.chars().count();
    if count <= 4 {
        return "•".repeat(count.max(1));
    }
    let tail: String = secret.chars().skip(count - 4).collect();
    let bullets = (count - 4).min(20);
    format!("{}{tail}", "•".repeat(bullets))
}

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureKind {
    Unauthorized,
    Forbidden,
    NotFound,
    RateLimited,
    Server,
    Network,
    Malformed,
    Other,
}

#[derive(Debug, Clone)]
pub struct Failure {
    pub kind: FailureKind,
    pub status: Option<u16>,
    pub message: String,
    pub doc_url: Option<String>,
    pub context: Option<String>,
    pub retry_after: Option<Duration>,
}

impl Failure {
    pub fn local(message: impl Into<String>) -> Self {
        Self {
            kind: FailureKind::Other,
            status: None,
            message: message.into(),
            doc_url: None,
            context: None,
            retry_after: None,
        }
    }

    pub fn headline(&self) -> String {
        match self.kind {
            FailureKind::Unauthorized => "The API key was rejected (401)".to_string(),
            FailureKind::Forbidden => "The key is missing permissions (403)".to_string(),
            FailureKind::NotFound => "Not found (404)".to_string(),
            FailureKind::RateLimited => "Rate limited (429)".to_string(),
            FailureKind::Server => "RevenueCat server error".to_string(),
            FailureKind::Network => "Could not reach RevenueCat".to_string(),
            FailureKind::Malformed => "Unexpected response from RevenueCat".to_string(),
            FailureKind::Other => match self.status {
                Some(status) => format!("HTTP {status}"),
                None => self.message.clone(),
            },
        }
    }

    pub fn detail(&self) -> Option<String> {
        if self.message.is_empty() {
            return None;
        }
        let mut detail = self.message.clone();
        if let Some(context) = &self.context {
            detail = format!("{detail}  ({context})");
        }
        Some(detail)
    }

    pub fn hints(&self) -> Vec<String> {
        match self.kind {
            FailureKind::Unauthorized => vec![
                format!(
                    "Create a v2 secret key: RevenueCat dashboard → your project → API keys → + New secret API key"
                ),
                "Secret keys start with sk_. Public SDK keys and v1 keys do not work with the v2 API.".to_string(),
                "Paste the key again, or set RC_API_KEY.".to_string(),
            ],
            FailureKind::Forbidden => vec![
                format!("Overview cards need {SCOPE_OVERVIEW}."),
                format!("Sparklines need {SCOPE_CHARTS}."),
                format!("Picking a project automatically needs {SCOPE_PROJECTS}."),
                "Add the permissions to the key in the dashboard, or create a new key with them."
                    .to_string(),
            ],
            FailureKind::NotFound => vec![
                "Check the project id (it looks like proj1ab2c3d4).".to_string(),
                "Run rc-tui --setup and press Ctrl+L to pick from the projects your key can list."
                    .to_string(),
            ],
            FailureKind::RateLimited => vec![
                "Charts & Metrics allow 25 requests per minute and rc-tui sends 7 per refresh."
                    .to_string(),
                "It will retry at the next refresh.".to_string(),
            ],
            FailureKind::Server => {
                vec!["RevenueCat returned a server error; the next refresh will retry.".to_string()]
            }
            FailureKind::Network => vec![
                "Check this machine can reach api.revenuecat.com.".to_string(),
                "Behind a proxy, set HTTPS_PROXY.".to_string(),
            ],
            FailureKind::Malformed => {
                vec!["Run rc-tui --check --verbose to see the raw response.".to_string()]
            }
            FailureKind::Other => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OverviewMetric {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub unit: String,
    pub period: String,
    pub value: f64,
}

#[derive(Debug, Clone)]
pub struct Overview {
    pub metrics: Vec<OverviewMetric>,
    pub currency: String,
}

#[derive(Debug, Deserialize)]
struct ListResponse {
    #[serde(default)]
    items: Vec<ListedProject>,
}

#[derive(Debug, Deserialize)]
struct ListedProject {
    id: String,
    name: String,
}

#[derive(Debug, Clone)]
pub struct ScopeReport {
    pub overview: Result<(), Failure>,
    pub charts: Result<(), Failure>,
}

impl ScopeReport {
    pub fn failure(&self) -> Option<&Failure> {
        self.charts
            .as_ref()
            .err()
            .or_else(|| self.overview.as_ref().err())
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub metrics: Vec<OverviewMetric>,
    pub series: HashMap<String, Vec<f64>>,
    pub currency: String,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    settings: Settings,
}

impl Client {
    pub fn new(settings: Settings) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("rc-tui/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { http, settings }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub async fn fetch_overview(&self) -> Result<Overview, Failure> {
        let url = format!(
            "{}/projects/{}/metrics/overview",
            self.settings.base_url, self.settings.project_id
        );
        let body = self
            .get(&url, &[("currency", self.settings.currency.as_str())])
            .await?;
        #[derive(Deserialize)]
        struct Body {
            metrics: Vec<OverviewMetric>,
            #[serde(default)]
            currency: String,
        }
        let body: Body = serde_json::from_str(&body).map_err(|err| Failure {
            kind: FailureKind::Malformed,
            status: None,
            message: err.to_string(),
            doc_url: None,
            context: Some("GET /metrics/overview".to_string()),
            retry_after: None,
        })?;
        Ok(Overview {
            metrics: body.metrics,
            currency: body.currency,
        })
    }

    pub async fn fetch_charts(&self, metrics: &[OverviewMetric]) -> HashMap<String, Vec<f64>> {
        let mut series = HashMap::new();
        for metric in metrics {
            let Some(chart) = chart_for_metric(&metric.id, &metric.name) else {
                continue;
            };
            let monetary =
                metric.unit == "$" || metric.unit.eq_ignore_ascii_case(&self.settings.currency);
            if let Ok(values) = self.chart(chart, monetary, CHART_WINDOW_DAYS).await
                && !values.is_empty()
            {
                series.insert(metric.id.clone(), values);
            }
        }
        series
    }

    pub async fn fetch_range(
        &self,
        chart: &str,
        days: u64,
        monetary: bool,
    ) -> Result<Vec<f64>, Failure> {
        self.chart(chart, monetary, days).await
    }

    pub async fn check_scopes(&self) -> ScopeReport {
        let overview = self.fetch_overview().await.map(|_| ());
        let charts = self
            .chart("trials", false, 7)
            .await
            .map(|_| ())
            .map_err(|failure| Failure {
                context: Some(format!(
                    "{} (charts scope probe)",
                    failure.context.unwrap_or_default()
                )),
                ..failure
            });
        ScopeReport { overview, charts }
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>, Failure> {
        let url = format!("{}/projects", self.settings.base_url);
        let body = self.get(&url, &[("limit", "100")]).await?;
        let list: ListResponse = serde_json::from_str(&body).map_err(|err| Failure {
            kind: FailureKind::Malformed,
            status: None,
            message: err.to_string(),
            doc_url: None,
            context: Some("GET /projects".to_string()),
            retry_after: None,
        })?;
        Ok(list
            .items
            .into_iter()
            .map(|project| Project {
                id: project.id,
                name: project.name,
            })
            .collect())
    }

    async fn chart(&self, name: &str, monetary: bool, days: u64) -> Result<Vec<f64>, Failure> {
        let end = Utc::now().date_naive();
        let start = end
            .checked_sub_days(Days::new(days.saturating_sub(1)))
            .unwrap_or(end);
        let url = format!(
            "{}/projects/{}/charts/{}",
            self.settings.base_url, self.settings.project_id, name
        );
        let start = start.format("%Y-%m-%d").to_string();
        let end = end.format("%Y-%m-%d").to_string();
        let mut query = vec![
            ("realtime", "true"),
            ("resolution", "0"),
            ("start_date", start.as_str()),
            ("end_date", end.as_str()),
        ];
        if monetary {
            query.push(("currency", self.settings.currency.as_str()));
        }
        let body = self.get(&url, &query).await?;
        let chart: serde_json::Value = serde_json::from_str(&body).map_err(|err| Failure {
            kind: FailureKind::Malformed,
            status: None,
            message: err.to_string(),
            doc_url: None,
            context: Some(format!("GET /charts/{name}")),
            retry_after: None,
        })?;
        Ok(series_from_chart(&chart))
    }

    async fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<String, Failure> {
        let response = self
            .http
            .get(url)
            .bearer_auth(self.settings.api_key.trim())
            .query(query)
            .send()
            .await
            .map_err(|err| Failure {
                kind: FailureKind::Network,
                status: None,
                message: err.to_string(),
                doc_url: None,
                context: Some(path_of(url)),
                retry_after: None,
            })?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let body = response.text().await.map_err(|err| Failure {
            kind: FailureKind::Network,
            status: Some(status),
            message: err.to_string(),
            doc_url: None,
            context: Some(path_of(url)),
            retry_after,
        })?;
        if !(200..300).contains(&status) {
            return Err(failure_from_response(
                status,
                &body,
                &path_of(url),
                retry_after,
            ));
        }
        Ok(body)
    }
}

fn path_of(url: &str) -> String {
    match url.split_once("/v2") {
        Some((_, rest)) => format!("GET /v2{rest}"),
        None => url.to_string(),
    }
}

fn failure_from_response(
    status: u16,
    body: &str,
    context: &str,
    retry_after: Option<Duration>,
) -> Failure {
    let parsed: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let error_type = parsed
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let message = parsed
        .get("message")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| snippet(body));
    let doc_url = parsed
        .get("doc_url")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let kind = match (status, error_type.as_str()) {
        (401, _) | (_, "authentication_error") => FailureKind::Unauthorized,
        (403, _) | (_, "authorization_error") => FailureKind::Forbidden,
        (404, _) | (_, "resource_missing") => FailureKind::NotFound,
        (429, _) | (_, "rate_limit_error") => FailureKind::RateLimited,
        (500..=599, _) | (_, "server_error") => FailureKind::Server,
        _ => FailureKind::Other,
    };
    Failure {
        kind,
        status: Some(status),
        message,
        doc_url,
        context: Some(context.to_string()),
        retry_after,
    }
}

fn snippet(body: &str) -> String {
    let trimmed = body.trim();
    let cut = trimmed
        .char_indices()
        .nth(200)
        .map(|(index, _)| index)
        .unwrap_or(trimmed.len());
    trimmed[..cut].to_string()
}

pub fn chart_for_metric<'a>(id: &'a str, name: &str) -> Option<&'a str> {
    match id {
        "active_trials" => Some("trials"),
        "active_subscriptions" => Some("actives"),
        "mrr" => Some("mrr"),
        "revenue" => Some("revenue"),
        "new_customers" => Some("customers_new"),
        "active_customers" | "active_users" => Some("customers_active"),
        _ => {
            let name = name.to_ascii_lowercase();
            if name.contains("trial") {
                Some("trials")
            } else if name.contains("active subscription")
                || name.contains("mrr")
                || name.contains("revenue")
            {
                Some("revenue")
            } else if name.contains("active user") || name.contains("active customer") {
                Some("customers_active")
            } else if name.contains("customer") {
                Some("customers_new")
            } else {
                None
            }
        }
    }
}

fn series_from_chart(chart: &serde_json::Value) -> Vec<f64> {
    let mut points: Vec<(i64, f64)> = Vec::new();
    let Some(values) = chart.get("values").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    for (index, item) in values.iter().enumerate() {
        match item {
            serde_json::Value::Object(fields) => {
                let measure = fields
                    .get("measure")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0);
                if measure != 0 {
                    continue;
                }
                let value = fields
                    .get("value")
                    .and_then(|value| value.as_f64())
                    .unwrap_or(0.0);
                let cohort = fields
                    .get("cohort")
                    .and_then(|value| value.as_i64())
                    .unwrap_or(index as i64);
                points.push((cohort, value));
            }
            serde_json::Value::Number(number) => {
                points.push((index as i64, number.as_f64().unwrap_or(0.0)));
            }
            _ => {}
        }
    }
    points.sort_by_key(|(cohort, _)| *cohort);
    points.into_iter().map(|(_, value)| value).collect()
}

pub fn aggregate_series(series: &[Vec<f64>]) -> Vec<f64> {
    let mut total: Vec<f64> = Vec::new();
    for values in series {
        if values.is_empty() {
            continue;
        }
        if total.is_empty() {
            total = values.clone();
            continue;
        }
        let length = total.len().max(values.len());
        let mut sum = resample(&total, length);
        for (slot, value) in sum.iter_mut().zip(resample(values, length)) {
            *slot += value;
        }
        total = sum;
    }
    total
}

pub fn resample(values: &[f64], length: usize) -> Vec<f64> {
    if length == 0 || values.is_empty() {
        return Vec::new();
    }
    if values.len() == length {
        return values.to_vec();
    }
    (0..length)
        .map(|i| {
            let position = i as f64 * (values.len() - 1) as f64 / (length - 1).max(1) as f64;
            let low = position.floor() as usize;
            let high = (low + 1).min(values.len() - 1);
            let fraction = position - low as f64;
            values[low] * (1.0 - fraction) + values[high] * fraction
        })
        .collect()
}

/// Sums the same metric across projects, mirroring the dashboard's "All projects".
pub fn aggregate(snapshots: &[Snapshot]) -> Snapshot {
    let mut metrics: Vec<OverviewMetric> = Vec::new();
    let mut series: HashMap<String, Vec<f64>> = HashMap::new();
    let mut currency = String::new();
    for snapshot in snapshots {
        if currency.is_empty() && !snapshot.currency.is_empty() {
            currency = snapshot.currency.clone();
        }
        for metric in &snapshot.metrics {
            match metrics.iter_mut().find(|existing| existing.id == metric.id) {
                Some(existing) => existing.value += metric.value,
                None => metrics.push(metric.clone()),
            }
            let Some(values) = snapshot.series.get(&metric.id) else {
                continue;
            };
            match series.get_mut(&metric.id) {
                Some(accumulated) => {
                    let length = accumulated.len().max(values.len());
                    let mut sum = resample(accumulated, length);
                    for (total, value) in sum.iter_mut().zip(resample(values, length)) {
                        *total += value;
                    }
                    *accumulated = sum;
                }
                None => {
                    series.insert(metric.id.clone(), values.clone());
                }
            }
        }
    }
    Snapshot {
        metrics,
        series,
        currency,
    }
}

pub fn demo_snapshot(currency: &str, salt: u64) -> Snapshot {
    const CARDS: [(&str, &str, &str, &str, f64); 6] = [
        ("active_trials", "Active Trials", "#", "P0D", 3.0),
        (
            "active_subscriptions",
            "Active Subscriptions",
            "#",
            "P0D",
            2.0,
        ),
        ("mrr", "MRR", "$", "P0D", 6.0),
        ("revenue", "Revenue", "$", "P28D", 132.0),
        ("new_customers", "New Customers", "#", "P28D", 695.0),
        ("active_customers", "Active Customers", "#", "P28D", 882.0),
    ];

    let mut seed = 0x9e37_79b9_7f4a_7c15u64 ^ salt.wrapping_mul(2_654_435_761).rotate_left(16) | 1;
    let mut roll = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((seed >> 40) as f64) / ((1u64 << 24) as f64)
    };

    let mut metrics = Vec::new();
    let mut series = HashMap::new();
    for (id, name, unit, period, base) in CARDS {
        let drift = 0.9 + 0.2 * roll();
        let value = (base * drift).round().max(0.0);

        let length = CHART_WINDOW_DAYS as usize;
        let points: Vec<f64> = if matches!(id, "revenue" | "new_customers") {
            let mut raw: Vec<f64> = (0..length)
                .map(|index| {
                    let wave = 0.45 + 0.9 * roll();
                    let spike = if roll() > 0.86 {
                        2.0 + 2.0 * roll()
                    } else {
                        1.0
                    };
                    wave * spike * (0.7 + 0.6 * index as f64 / length as f64)
                })
                .collect();
            let sum: f64 = raw.iter().sum();
            let scale = value / sum.max(1e-9);
            raw.iter_mut().for_each(|item| *item *= scale);
            raw.iter()
                .map(|item| {
                    if unit == "$" {
                        (item * 100.0).round() / 100.0
                    } else {
                        item.round()
                    }
                })
                .collect()
        } else if id == "active_users" {
            let target = value / 18.0;
            let mut level = target * (0.7 + 0.6 * roll());
            (0..length)
                .map(|_| {
                    level = (level * (0.9 + 0.2 * roll())).max(0.0);
                    level.round()
                })
                .collect()
        } else {
            let start = value * (0.72 + 0.2 * roll());
            let mut points: Vec<f64> = (0..length)
                .map(|index| {
                    let progress = index as f64 / (length - 1) as f64;
                    let level = start + (value - start) * progress;
                    let noise = (roll() - 0.5) * value * 0.06;
                    if unit == "$" {
                        ((level + noise) * 100.0).round() / 100.0
                    } else {
                        (level + noise).round()
                    }
                })
                .collect();
            if let Some(last) = points.last_mut() {
                *last = value;
            }
            points
        };

        series.insert(id.to_string(), points);

        metrics.push(OverviewMetric {
            id: id.to_string(),
            name: name.to_string(),
            unit: unit.to_string(),
            period: period.to_string(),
            value,
        });
    }

    Snapshot {
        metrics,
        series,
        currency: currency.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_all_but_the_last_four_characters() {
        assert_eq!(redact("sk_1234567890abcd"), "•••••••••••••abcd");
        assert_eq!(redact("sk_ab"), "•k_ab");
        assert_eq!(redact("ab"), "••");
    }

    #[test]
    fn maps_status_codes_to_actionable_failures() {
        let unauthorized = failure_from_response(
            401,
            r#"{"type":"authentication_error","message":"Invalid API key.","doc_url":"https://errors.rev.cat/authentication-error"}"#,
            "GET /v2/projects",
            None,
        );
        assert_eq!(unauthorized.kind, FailureKind::Unauthorized);
        assert_eq!(unauthorized.status, Some(401));
        assert!(unauthorized.hints().iter().any(|hint| hint.contains("sk_")));

        let forbidden = failure_from_response(
            403,
            r#"{"type":"authorization_error","message":"Missing permission"}"#,
            "GET /v2/projects/x/metrics/overview",
            None,
        );
        assert_eq!(forbidden.kind, FailureKind::Forbidden);
        assert!(
            forbidden
                .hints()
                .iter()
                .any(|hint| hint.contains("charts_metrics:overview:read"))
        );

        let missing = failure_from_response(
            404,
            r#"{"type":"resource_missing","message":"Project not found"}"#,
            "GET /v2/projects/x/metrics/overview",
            None,
        );
        assert_eq!(missing.kind, FailureKind::NotFound);
    }

    #[test]
    fn maps_the_active_users_metric_to_its_chart() {
        assert_eq!(
            chart_for_metric("active_users", "Active Users"),
            Some("customers_active")
        );
        assert_eq!(
            chart_for_metric("active_customers", "Active Customers"),
            Some("customers_active")
        );
        assert_eq!(
            chart_for_metric("something_new", "Active Users"),
            Some("customers_active")
        );
        assert_eq!(
            chart_for_metric("active_trials", "Active Trials"),
            Some("trials")
        );
    }

    #[test]
    fn carries_retry_after_on_rate_limits() {
        let failure = failure_from_response(
            429,
            r#"{"type":"rate_limit_error","message":"slow down"}"#,
            "GET /v2/projects/x/metrics/overview",
            Some(Duration::from_secs(42)),
        );
        assert_eq!(failure.kind, FailureKind::RateLimited);
        assert_eq!(failure.retry_after, Some(Duration::from_secs(42)));
    }

    #[test]
    fn aggregates_values_and_series_across_projects() {
        let metric = |id: &str, value: f64| OverviewMetric {
            id: id.to_string(),
            name: id.to_string(),
            unit: "#".to_string(),
            period: "P0D".to_string(),
            value,
        };
        let first = Snapshot {
            metrics: vec![metric("mrr", 5.0)],
            series: HashMap::from([("mrr".to_string(), vec![1.0, 2.0])]),
            currency: "EUR".to_string(),
        };
        let second = Snapshot {
            metrics: vec![metric("mrr", 7.0)],
            series: HashMap::from([("mrr".to_string(), vec![3.0, 4.0, 5.0])]),
            currency: String::new(),
        };

        let total = aggregate(&[first, second]);
        assert_eq!(total.metrics.len(), 1);
        assert!((total.metrics[0].value - 12.0).abs() < f64::EPSILON);
        assert_eq!(total.currency, "EUR");

        let series = &total.series["mrr"];
        assert_eq!(series.len(), 3);
        let expected = [4.0, 5.5, 7.0];
        for (value, expected) in series.iter().zip(expected) {
            assert!((value - expected).abs() < 1e-9, "{series:?}");
        }
    }

    #[test]
    fn falls_back_to_the_raw_body_for_unparseable_errors() {
        let failure = failure_from_response(500, "<html>oops</html>", "GET /v2/projects", None);
        assert_eq!(failure.kind, FailureKind::Server);
        assert_eq!(failure.message, "<html>oops</html>");
        assert_eq!(
            failure.detail().as_deref(),
            Some("<html>oops</html>  (GET /v2/projects)")
        );
    }
}
