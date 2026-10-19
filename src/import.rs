use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::task::JoinSet;

use crate::api::{Client, Failure, Settings};
use crate::config::ProjectConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Editing,
    Validating,
    Results,
}

pub enum Action {
    None,
    Validate(Vec<Entry>),
    Save,
    Cancel,
}

pub struct Import {
    pub text: String,
    pub stage: Stage,
    pub results: Vec<Outcome>,
    pub notice: Option<String>,
}

pub struct Entry {
    pub order: usize,
    pub project_id: Option<String>,
    pub api_key: String,
}

pub struct Outcome {
    pub order: usize,
    pub label: String,
    pub project: Option<ProjectConfig>,
    pub failure: Option<Failure>,
    pub charts: Option<bool>,
}

impl Import {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            stage: Stage::Editing,
            results: Vec::new(),
            notice: None,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Action::Cancel;
        }
        match self.stage {
            Stage::Editing => match key.code {
                KeyCode::Esc => Action::Cancel,
                KeyCode::Enter => {
                    self.text.push('\n');
                    self.notice = None;
                    Action::None
                }
                KeyCode::Backspace => {
                    self.text.pop();
                    Action::None
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.text.clear();
                    Action::None
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => self.start(),
                KeyCode::Char(character)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !character.is_control() =>
                {
                    self.text.push(character);
                    self.notice = None;
                    Action::None
                }
                _ => Action::None,
            },
            Stage::Results => match key.code {
                KeyCode::Enter => Action::Save,
                KeyCode::Esc => Action::Cancel,
                _ => Action::None,
            },
            Stage::Validating => Action::None,
        }
    }

    fn start(&mut self) -> Action {
        let (entries, mut errors) = self.entries();
        if entries.is_empty() && errors.is_empty() {
            self.notice = Some("Paste at least one sk_ key.".to_string());
            return Action::None;
        }
        errors.shrink_to_fit();
        self.results = errors;
        self.stage = Stage::Validating;
        self.notice = None;
        Action::Validate(entries)
    }

    pub fn finish(&mut self, outcomes: Vec<Outcome>) {
        self.results.extend(outcomes);
        self.results.sort_by_key(|outcome| outcome.order);
        self.stage = Stage::Results;
    }

    pub fn valid_projects(&self) -> Vec<ProjectConfig> {
        let mut projects: Vec<ProjectConfig> = Vec::new();
        for outcome in &self.results {
            let Some(project) = &outcome.project else {
                continue;
            };
            if projects.iter().any(|existing| existing.id == project.id) {
                continue;
            }
            projects.push(project.clone());
        }
        projects
    }

    pub fn failures(&self) -> usize {
        self.results
            .iter()
            .filter(|outcome| outcome.project.is_none())
            .count()
    }

    pub fn ready(&self) -> usize {
        self.valid_projects().len()
    }

    pub fn key_count(&self) -> usize {
        self.entries().0.len()
    }

    fn entries(&self) -> (Vec<Entry>, Vec<Outcome>) {
        let mut entries: Vec<Entry> = Vec::new();
        let mut errors = Vec::new();
        for (index, line) in self.text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let tokens: Vec<&str> = trimmed
                .split(|character: char| {
                    character.is_whitespace() || character == '=' || character == ','
                })
                .filter(|token| !token.is_empty())
                .collect();
            let api_key = tokens.iter().find(|token| token.starts_with("sk_"));
            let project_id = tokens.iter().find(|token| token.starts_with("proj"));
            match api_key {
                Some(api_key) => {
                    if entries.iter().any(|existing| existing.api_key == *api_key) {
                        continue;
                    }
                    entries.push(Entry {
                        order: index,
                        project_id: project_id.map(|id| id.to_string()),
                        api_key: api_key.to_string(),
                    });
                }
                None => errors.push(Outcome {
                    order: index,
                    label: format!("line {}", index + 1),
                    project: None,
                    failure: Some(Failure::local(
                        "No sk_ key on this line; expected sk_… or proj… sk_…",
                    )),
                    charts: None,
                }),
            }
        }
        (entries, errors)
    }
}

pub fn validate(
    entries: Vec<Entry>,
    base_url: &str,
    currency: &str,
    http: &reqwest::Client,
) -> Vec<Outcome> {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            return entries
                .into_iter()
                .map(|entry| Outcome {
                    order: entry.order,
                    label: redacted_tail(&entry.api_key),
                    project: None,
                    failure: Some(Failure::local(format!("runtime: {err}"))),
                    charts: None,
                })
                .collect();
        }
    };

    runtime.block_on(async move {
        let mut set = JoinSet::new();
        for entry in entries {
            let base_url = base_url.to_string();
            let currency = currency.to_string();
            let http = http.clone();
            set.spawn(async move { resolve(entry, &base_url, &currency, http).await });
        }
        let mut outcomes = Vec::new();
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(outcome) => outcomes.push(outcome),
                Err(err) => outcomes.push(Outcome {
                    order: usize::MAX,
                    label: "worker".to_string(),
                    project: None,
                    failure: Some(Failure::local(format!("task failed: {err}"))),
                    charts: None,
                }),
            }
        }
        outcomes.sort_by_key(|outcome| outcome.order);
        outcomes
    })
}

async fn resolve(entry: Entry, base_url: &str, currency: &str, http: reqwest::Client) -> Outcome {
    let fallback = redacted_tail(&entry.api_key);
    let mut project_id = entry.project_id.clone().unwrap_or_default();
    let mut name = None;

    if project_id.is_empty() {
        let client = Client::with_http(
            Settings::new(base_url, entry.api_key.clone(), String::new(), currency),
            http.clone(),
        );
        match client.list_projects().await {
            Ok(projects) => match projects.into_iter().next() {
                Some(project) => {
                    project_id = project.id;
                    name = Some(project.name);
                }
                None => {
                    return Outcome {
                        order: entry.order,
                        label: fallback,
                        project: None,
                        failure: Some(Failure::local(
                            "This key returned no projects. Put the project id before the key, or add project_configuration:projects:read.",
                        )),
                        charts: None,
                    };
                }
            },
            Err(failure) => {
                return Outcome {
                    order: entry.order,
                    label: fallback,
                    project: None,
                    failure: Some(failure),
                    charts: None,
                };
            }
        }
    }

    let client = Client::with_http(
        Settings::new(
            base_url,
            entry.api_key.clone(),
            project_id.clone(),
            currency,
        ),
        http,
    );
    let report = client.check_scopes().await;
    let label = name.clone().unwrap_or_else(|| project_id.clone());
    if let Err(failure) = report.overview {
        return Outcome {
            order: entry.order,
            label,
            project: None,
            failure: Some(failure),
            charts: None,
        };
    }
    match report.charts {
        Ok(()) => Outcome {
            order: entry.order,
            label,
            project: Some(ProjectConfig::new(project_id, entry.api_key, name)),
            failure: None,
            charts: Some(true),
        },
        Err(failure) => Outcome {
            order: entry.order,
            label,
            project: None,
            failure: Some(failure),
            charts: Some(false),
        },
    }
}

pub fn redacted_tail(secret: &str) -> String {
    let count = secret.chars().count();
    if count <= 4 {
        return "sk_…".to_string();
    }
    let tail: String = secret.chars().skip(count - 4).collect();
    format!("sk_…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(import: &mut Import, code: KeyCode) -> Action {
        import.handle_key(KeyEvent::from(code))
    }

    fn type_text(import: &mut Import, text: &str) {
        for character in text.chars() {
            match character {
                '\n' => {
                    press(import, KeyCode::Enter);
                }
                _ => {
                    press(import, KeyCode::Char(character));
                }
            }
        }
    }

    #[test]
    fn parses_keys_with_and_without_project_ids() {
        let mut import = Import::new();
        type_text(
            &mut import,
            "sk_one\nproj_abc sk_two\n# comment\n\nproj_def=sk_three\n",
        );
        let (entries, errors) = import.entries();
        assert!(errors.is_empty());
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].project_id, None);
        assert_eq!(entries[0].api_key, "sk_one");
        assert_eq!(entries[1].project_id.as_deref(), Some("proj_abc"));
        assert_eq!(entries[2].project_id.as_deref(), Some("proj_def"));
    }

    #[test]
    fn reports_lines_without_a_key_and_skips_duplicates() {
        let mut import = Import::new();
        type_text(&mut import, "sk_one\nnot-a-key\nsk_one\n");
        let (entries, errors) = import.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0]
                .failure
                .as_ref()
                .is_some_and(|failure| { failure.message.contains("No sk_ key") })
        );
    }

    #[test]
    fn ctrl_d_validates_and_ctrl_u_clears() {
        let mut import = Import::new();
        type_text(&mut import, "sk_one\nsk_two");

        match import.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)) {
            Action::Validate(entries) => assert_eq!(entries.len(), 2),
            _ => panic!("expected validation"),
        }
        assert_eq!(import.stage, Stage::Validating);

        import.stage = Stage::Editing;
        import.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(import.text.is_empty());
    }

    #[test]
    fn empty_input_is_reported_instead_of_validating() {
        let mut import = Import::new();
        assert!(matches!(
            import.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Action::None
        ));
        assert_eq!(import.stage, Stage::Editing);
        assert!(import.notice.is_some());
    }

    #[test]
    fn saves_only_successful_projects_once() {
        let mut import = Import::new();
        import.stage = Stage::Validating;
        import.finish(vec![
            Outcome {
                order: 0,
                label: "One".to_string(),
                project: Some(ProjectConfig::new("proj1", "sk_one", Some("One".into()))),
                failure: None,
                charts: Some(true),
            },
            Outcome {
                order: 1,
                label: "proj1".to_string(),
                project: Some(ProjectConfig::new("proj1", "sk_two", None)),
                failure: None,
                charts: Some(true),
            },
            Outcome {
                order: 2,
                label: "line 3".to_string(),
                project: None,
                failure: Some(Failure::local("bad")),
                charts: None,
            },
        ]);

        assert_eq!(import.stage, Stage::Results);
        assert_eq!(import.ready(), 1);
        assert_eq!(import.failures(), 1);
        assert_eq!(import.valid_projects()[0].api_key, "sk_one");
    }

    #[test]
    fn enter_saves_and_escape_cancels_from_results() {
        let mut import = Import::new();
        import.stage = Stage::Results;
        assert!(matches!(press(&mut import, KeyCode::Enter), Action::Save));
        assert!(matches!(press(&mut import, KeyCode::Esc), Action::Cancel));
    }
}
