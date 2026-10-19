use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::api::{Failure, FailureKind, Project, Settings};
use crate::config::ProjectConfig;

const MAX_API_KEY: usize = 256;
const MAX_PROJECT_ID: usize = 255;
const MAX_CURRENCY: usize = 3;

const STAGES: [Stage; 3] = [Stage::ApiKey, Stage::ProjectId, Stage::Currency];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    ApiKey,
    ListingProjects,
    Projects,
    ProjectId,
    Currency,
    Connected,
}

pub enum Action {
    None,
    ListProjects { base_url: String, api_key: String },
    Start(Settings),
    Finish,
    Quit,
}

pub struct Setup {
    pub base_url: String,
    pub api_key: String,
    pub project_id: String,
    pub project_name: Option<String>,
    pub currency: String,
    pub stage: Stage,
    pub projects: Vec<Project>,
    pub selected: usize,
    pub busy: bool,
    pub error: Option<Failure>,
    pub notice: Option<String>,
}

impl Setup {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        project_id: impl Into<String>,
        currency: impl Into<String>,
    ) -> Self {
        let api_key = api_key.into();
        let project_id = project_id.into();
        let stage = if api_key.trim().is_empty() {
            Stage::ApiKey
        } else if project_id.trim().is_empty() {
            Stage::ProjectId
        } else {
            Stage::Currency
        };
        Self {
            base_url: base_url.into(),
            api_key,
            project_id,
            project_name: None,
            currency: currency.into(),
            stage,
            projects: Vec::new(),
            selected: 0,
            busy: false,
            error: None,
            notice: None,
        }
    }

    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.project_name = name;
        self
    }

    pub fn edit_key(mut self) -> Self {
        if !self.api_key.trim().is_empty() {
            self.stage = Stage::ApiKey;
        }
        self
    }

    pub fn currency_value(&self) -> String {
        let trimmed = self.currency.trim();
        if trimmed.is_empty() {
            "EUR".to_string()
        } else {
            trimmed.to_ascii_uppercase()
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            return self.list_projects();
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('u') {
            self.edit(|value| value.clear());
            return Action::None;
        }
        if self.busy {
            if key.code == KeyCode::Esc {
                self.busy = false;
                self.stage = Stage::ApiKey;
                self.notice = None;
            }
            return Action::None;
        }
        if self.stage == Stage::Connected {
            return match key.code {
                KeyCode::Char('y') => {
                    self.start_next();
                    Action::None
                }
                KeyCode::Enter | KeyCode::Char('n') | KeyCode::Char('q') | KeyCode::Esc => {
                    Action::Finish
                }
                _ => Action::None,
            };
        }
        match key.code {
            KeyCode::Esc => self.escape(),
            KeyCode::Enter => self.submit(),
            KeyCode::Tab => {
                self.cycle(1);
                Action::None
            }
            KeyCode::BackTab => {
                self.cycle(-1);
                Action::None
            }
            KeyCode::Up | KeyCode::Char('k') if self.stage == Stage::Projects => {
                self.selected = self.selected.saturating_sub(1);
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') if self.stage == Stage::Projects => {
                if self.selected + 1 < self.projects.len() {
                    self.selected += 1;
                }
                Action::None
            }
            KeyCode::Backspace => {
                self.edit(|value| {
                    value.pop();
                });
                Action::None
            }
            KeyCode::Char(character) => {
                let limit = self.field_limit();
                self.edit(|value| {
                    if value.chars().count() < limit && allowed(character) {
                        value.push(character);
                    }
                });
                Action::None
            }
            _ => Action::None,
        }
    }

    fn submit(&mut self) -> Action {
        match self.stage {
            Stage::ApiKey => {
                if self.api_key.trim().is_empty() {
                    self.error = Some(Failure::local(
                        "Paste your v2 secret API key first (it starts with sk_).",
                    ));
                    return Action::None;
                }
                self.list_projects()
            }
            Stage::ProjectId => {
                if self.project_id.trim().is_empty() {
                    self.error = Some(Failure::local(
                        "Enter a project id (proj1ab2c3d4), or press Ctrl+L to pick from the list.",
                    ));
                    return Action::None;
                }
                self.error = None;
                self.stage = Stage::Currency;
                Action::None
            }
            Stage::Projects => {
                if let Some(project) = self.projects.get(self.selected) {
                    self.project_id = project.id.clone();
                    self.project_name = Some(project.name.clone());
                    self.notice = Some(format!("Using {} ({})", project.name, project.id));
                }
                self.stage = Stage::Currency;
                self.error = None;
                Action::None
            }
            Stage::Currency => {
                if self.api_key.trim().is_empty() || self.project_id.trim().is_empty() {
                    self.error = Some(Failure::local(
                        "An API key and a project id are both required.",
                    ));
                    return Action::None;
                }
                let settings = Settings::new(
                    self.base_url.clone(),
                    self.api_key.trim().to_string(),
                    self.project_id.trim().to_string(),
                    self.currency_value(),
                );
                self.error = None;
                self.notice = Some("Checking the key against this project…".to_string());
                self.busy = true;
                Action::Start(settings)
            }
            Stage::ListingProjects | Stage::Connected => Action::None,
        }
    }

    fn list_projects(&mut self) -> Action {
        let api_key = self.api_key.trim().to_string();
        if api_key.is_empty() {
            self.error = Some(Failure::local("Paste your v2 secret API key first."));
            self.stage = Stage::ApiKey;
            return Action::None;
        }
        self.error = None;
        self.notice = Some("Listing your projects…".to_string());
        self.busy = true;
        self.stage = Stage::ListingProjects;
        Action::ListProjects {
            base_url: self.base_url.clone(),
            api_key,
        }
    }

    pub fn projects_result(&mut self, result: Result<Vec<Project>, Failure>) {
        self.busy = false;
        match result {
            Ok(projects) => {
                self.error = None;
                self.projects = projects;
                self.selected = 0;
                match self.projects.len() {
                    0 => {
                        self.stage = Stage::ProjectId;
                        self.notice = Some(
                            "The API returned no projects; enter the project id by hand."
                                .to_string(),
                        );
                    }
                    1 => {
                        let project = &self.projects[0];
                        self.project_id = project.id.clone();
                        self.project_name = Some(project.name.clone());
                        self.notice = Some(format!("Using {} ({})", project.name, project.id));
                        self.stage = Stage::Currency;
                    }
                    _ => {
                        self.notice = Some("Pick a project with ↑/↓ and Enter.".to_string());
                        self.stage = Stage::Projects;
                    }
                }
            }
            Err(failure) if failure.kind == FailureKind::Forbidden => {
                self.stage = Stage::ProjectId;
                self.notice = Some(format!(
                    "This key cannot list projects (needs {scope}). Enter the project id, or add the permission and press Ctrl+L.",
                    scope = crate::api::SCOPE_PROJECTS
                ));
            }
            Err(failure) => {
                self.stage = Stage::ApiKey;
                self.notice = None;
                self.error = Some(failure);
            }
        }
    }

    pub fn validation_failed(&mut self, failure: Failure) {
        let kind = failure.kind.clone();
        self.busy = false;
        self.notice = None;
        self.error = Some(failure);
        match kind {
            FailureKind::Unauthorized => self.stage = Stage::ApiKey,
            FailureKind::NotFound => self.stage = Stage::ProjectId,
            _ => {}
        }
    }

    pub fn connected(&mut self) {
        self.busy = false;
        self.error = None;
        self.notice = None;
        self.stage = Stage::Connected;
    }

    pub fn take_project(&self) -> ProjectConfig {
        ProjectConfig::new(
            self.project_id.trim(),
            self.api_key.trim(),
            self.project_name.clone(),
        )
    }

    fn start_next(&mut self) {
        self.api_key.clear();
        self.project_id.clear();
        self.project_name = None;
        self.projects.clear();
        self.selected = 0;
        self.error = None;
        self.notice = None;
        self.stage = Stage::ApiKey;
    }

    fn escape(&mut self) -> Action {
        match self.stage {
            Stage::ApiKey => Action::Quit,
            Stage::ListingProjects => {
                self.busy = false;
                self.stage = Stage::ApiKey;
                self.notice = None;
                Action::None
            }
            Stage::Projects | Stage::ProjectId => {
                self.stage = Stage::ApiKey;
                self.notice = None;
                Action::None
            }
            Stage::Currency => {
                self.stage = Stage::ProjectId;
                self.notice = None;
                Action::None
            }
            Stage::Connected => Action::Finish,
        }
    }

    fn cycle(&mut self, direction: i32) {
        if self.busy {
            return;
        }
        let current = match self.stage {
            Stage::ApiKey => 0,
            Stage::ProjectId | Stage::Projects => 1,
            Stage::Currency | Stage::ListingProjects | Stage::Connected => 2,
        };
        let next = (current + direction).rem_euclid(STAGES.len() as i32) as usize;
        self.stage = STAGES[next];
        self.notice = None;
    }

    fn edit(&mut self, edit: impl FnOnce(&mut String)) {
        match self.stage {
            Stage::ApiKey => edit(&mut self.api_key),
            Stage::ProjectId => {
                edit(&mut self.project_id);
                self.project_name = None;
            }
            Stage::Currency => edit(&mut self.currency),
            Stage::ListingProjects | Stage::Projects | Stage::Connected => {}
        }
    }

    fn field_limit(&self) -> usize {
        match self.stage {
            Stage::ApiKey => MAX_API_KEY,
            Stage::ProjectId => MAX_PROJECT_ID,
            _ => MAX_CURRENCY,
        }
    }
}

fn allowed(character: char) -> bool {
    !character.is_whitespace() && !character.is_control()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(setup: &mut Setup, code: KeyCode) -> Action {
        setup.handle_key(KeyEvent::from(code))
    }

    fn type_text(setup: &mut Setup, text: &str) {
        for character in text.chars() {
            press(setup, KeyCode::Char(character));
        }
    }

    #[test]
    fn api_key_submit_asks_for_the_project_list() {
        let mut setup = Setup::new("https://api.revenuecat.com/v2", "", "", "EUR");
        assert_eq!(setup.stage, Stage::ApiKey);

        type_text(&mut setup, "sk_abc123");
        match press(&mut setup, KeyCode::Enter) {
            Action::ListProjects { api_key, .. } => assert_eq!(api_key, "sk_abc123"),
            _ => panic!("expected a ListProjects action"),
        }
        assert_eq!(setup.stage, Stage::ListingProjects);
        assert!(setup.busy);
    }

    #[test]
    fn empty_api_key_is_reported_inline() {
        let mut setup = Setup::new("https://api.revenuecat.com/v2", "", "", "EUR");
        assert!(matches!(press(&mut setup, KeyCode::Enter), Action::None));
        assert!(setup.error.is_some());
        assert_eq!(setup.stage, Stage::ApiKey);
    }

    #[test]
    fn a_single_project_is_selected_automatically() {
        let mut setup = Setup::new("https://api.revenuecat.com/v2", "sk_abc", "", "EUR");
        setup.projects_result(Ok(vec![Project {
            id: "proj1".to_string(),
            name: "MagicWeather".to_string(),
        }]));
        assert_eq!(setup.project_id, "proj1");
        assert_eq!(setup.stage, Stage::Currency);

        match press(&mut setup, KeyCode::Enter) {
            Action::Start(settings) => {
                assert_eq!(settings.project_id, "proj1");
                assert_eq!(settings.currency, "EUR");
            }
            _ => panic!("expected a Start action"),
        }
    }

    #[test]
    fn several_projects_are_picked_from_a_list() {
        let mut setup = Setup::new("https://api.revenuecat.com/v2", "sk_abc", "", "EUR");
        setup.projects_result(Ok(vec![
            Project {
                id: "proj1".to_string(),
                name: "One".to_string(),
            },
            Project {
                id: "proj2".to_string(),
                name: "Two".to_string(),
            },
        ]));
        assert_eq!(setup.stage, Stage::Projects);

        press(&mut setup, KeyCode::Down);
        press(&mut setup, KeyCode::Enter);
        assert_eq!(setup.project_id, "proj2");
        assert_eq!(setup.stage, Stage::Currency);
    }

    #[test]
    fn a_key_without_project_permissions_falls_back_to_manual_entry() {
        let mut setup = Setup::new("https://api.revenuecat.com/v2", "sk_abc", "", "EUR");
        setup.projects_result(Err(Failure {
            kind: FailureKind::Forbidden,
            status: Some(403),
            message: "Missing permission".to_string(),
            doc_url: None,
            context: None,
            retry_after: None,
        }));
        assert_eq!(setup.stage, Stage::ProjectId);
        assert!(setup.error.is_none());
        assert!(
            setup
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("project_configuration:projects:read"))
        );
    }

    #[test]
    fn a_rejected_key_is_shown_on_the_key_field() {
        let mut setup = Setup::new("https://api.revenuecat.com/v2", "sk_abc", "", "EUR");
        setup.projects_result(Err(Failure {
            kind: FailureKind::Unauthorized,
            status: Some(401),
            message: "Invalid API key.".to_string(),
            doc_url: None,
            context: None,
            retry_after: None,
        }));
        assert_eq!(setup.stage, Stage::ApiKey);
        assert_eq!(
            setup.error.as_ref().map(|error| error.headline()),
            Some("The API key was rejected (401)".to_string())
        );
    }

    #[test]
    fn escape_walks_back_a_stage_and_quits_from_the_key() {
        let mut setup = Setup::new("https://api.revenuecat.com/v2", "sk_abc", "", "EUR");
        setup.stage = Stage::Currency;
        assert!(matches!(press(&mut setup, KeyCode::Esc), Action::None));
        assert_eq!(setup.stage, Stage::ProjectId);
        press(&mut setup, KeyCode::Esc);
        assert_eq!(setup.stage, Stage::ApiKey);
        assert!(matches!(press(&mut setup, KeyCode::Esc), Action::Quit));
    }

    #[test]
    fn tab_cycles_the_editable_fields() {
        let mut setup = Setup::new("https://api.revenuecat.com/v2", "", "", "EUR");
        press(&mut setup, KeyCode::Tab);
        assert_eq!(setup.stage, Stage::ProjectId);
        press(&mut setup, KeyCode::Tab);
        assert_eq!(setup.stage, Stage::Currency);
        press(&mut setup, KeyCode::Tab);
        assert_eq!(setup.stage, Stage::ApiKey);
        press(&mut setup, KeyCode::BackTab);
        assert_eq!(setup.stage, Stage::Currency);
    }

    #[test]
    fn editing_starts_on_the_key_even_when_credentials_are_known() {
        let setup =
            Setup::new("https://api.revenuecat.com/v2", "sk_abc", "proj1", "EUR").edit_key();
        assert_eq!(setup.stage, Stage::ApiKey);

        let empty = Setup::new("https://api.revenuecat.com/v2", "", "", "EUR").edit_key();
        assert_eq!(empty.stage, Stage::ApiKey);
    }

    #[test]
    fn connected_offers_another_project_or_finish() {
        let mut setup = Setup::new("https://api.revenuecat.com/v2", "sk_abc", "proj1", "EUR")
            .with_name(Some("One".to_string()));
        setup.connected();
        assert_eq!(setup.stage, Stage::Connected);

        let project = setup.take_project();
        assert_eq!(project.id, "proj1");
        assert_eq!(project.api_key, "sk_abc");
        assert_eq!(project.name.as_deref(), Some("One"));

        assert!(matches!(
            press(&mut setup, KeyCode::Char('y')),
            Action::None
        ));
        assert_eq!(setup.stage, Stage::ApiKey);
        assert!(setup.api_key.is_empty());
        assert!(setup.project_id.is_empty());
        assert!(setup.project_name.is_none());

        setup.api_key = "sk_two".to_string();
        setup.project_id = "proj2".to_string();
        setup.connected();
        assert!(matches!(press(&mut setup, KeyCode::Enter), Action::Finish));
    }

    #[test]
    fn starts_on_the_project_field_when_the_key_is_known() {
        let setup = Setup::new("https://api.revenuecat.com/v2", "sk_abc", "", "EUR");
        assert_eq!(setup.stage, Stage::ProjectId);
        let setup = Setup::new("https://api.revenuecat.com/v2", "sk_abc", "proj1", "EUR");
        assert_eq!(setup.stage, Stage::Currency);
    }

    #[test]
    fn validation_failures_return_to_the_causal_field() {
        let mut setup = Setup::new("https://api.revenuecat.com/v2", "sk_abc", "proj1", "EUR");
        setup.validation_failed(Failure {
            kind: FailureKind::Unauthorized,
            status: Some(401),
            message: "Invalid API key.".to_string(),
            doc_url: None,
            context: None,
            retry_after: None,
        });
        assert_eq!(setup.stage, Stage::ApiKey);

        let mut setup = Setup::new("https://api.revenuecat.com/v2", "sk_abc", "proj1", "EUR");
        setup.validation_failed(Failure {
            kind: FailureKind::NotFound,
            status: Some(404),
            message: "Project not found".to_string(),
            doc_url: None,
            context: None,
            retry_after: None,
        });
        assert_eq!(setup.stage, Stage::ProjectId);
    }
}
