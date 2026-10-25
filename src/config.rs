use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const HEADER: &str = "# rc-tui configuration\n\
                      # Each [[projects]] entry needs a v2 secret API key with Charts & Metrics: Read.\n\
                      # https://www.revenuecat.com/docs/projects/authentication\n\n";

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_minutes: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall: Option<bool>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pins: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_scale: Option<u8>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_page: Option<usize>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_project: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_path: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_seen_at: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enough: Option<u64>,

    #[serde(default)]
    pub projects: Vec<ProjectConfig>,

    #[serde(
        default,
        rename = "project_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub legacy_project_id: Option<String>,

    #[serde(default, rename = "api_key", skip_serializing_if = "Option::is_none")]
    pub legacy_api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub id: String,
    pub api_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ProjectConfig {
    pub fn new(id: impl Into<String>, api_key: impl Into<String>, name: Option<String>) -> Self {
        Self {
            id: id.into(),
            api_key: api_key.into(),
            name,
        }
    }

    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

impl Config {
    pub fn load() -> Self {
        let Ok(text) = fs::read_to_string(Self::path()) else {
            return Self::default();
        };
        match Self::parse(&text) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("rc-tui: ignoring {} — {err}", Self::path().display());
                Self::default()
            }
        }
    }

    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        let mut config: Self = toml::from_str(text)?;
        config.normalize();
        Ok(config)
    }

    fn normalize(&mut self) {
        if self.projects.is_empty()
            && let (Some(id), Some(api_key)) =
                (self.legacy_project_id.clone(), self.legacy_api_key.clone())
        {
            self.projects.push(ProjectConfig::new(id, api_key, None));
        }
        self.legacy_project_id = None;
        self.legacy_api_key = None;
    }

    pub fn remove_project(&mut self, id: &str) {
        self.projects.retain(|project| project.id != id);
        self.pins.retain(|pinned| pinned != id);
    }

    pub fn toggle_pin(&mut self, id: &str) {
        if let Some(index) = self.pins.iter().position(|pinned| pinned == id) {
            self.pins.remove(index);
        } else {
            self.pins.push(id.to_string());
        }
    }

    pub fn upsert(&mut self, project: ProjectConfig) {
        match self
            .projects
            .iter_mut()
            .find(|existing| existing.id == project.id)
        {
            Some(existing) => *existing = project,
            None => self.projects.push(project),
        }
    }

    pub fn render(&self) -> String {
        let mut text = String::from(HEADER);
        let mut copy = self.clone();
        copy.legacy_project_id = None;
        copy.legacy_api_key = None;
        copy.projects
            .retain(|project| !project.id.trim().is_empty());
        text.push_str(&toml::to_string_pretty(&copy).unwrap_or_default());
        text
    }

    pub fn save(&self) -> io::Result<PathBuf> {
        let path = Self::path();
        self.save_to(&path)?;
        Ok(path)
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.render())?;
        restrict_permissions(path)
    }

    pub fn path() -> PathBuf {
        config_dir().join("rc-tui").join("config.toml")
    }
}

fn restrict_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            return dir;
        }
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".local").join("share"),
        None => PathBuf::from(".local").join("share"),
    }
}

fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            return dir;
        }
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".config"),
        None => PathBuf::from(".config"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_projects_and_currency() {
        let config = Config::parse(
            "currency = \"USD\"\n\n\
             [[projects]]\nid = \"proj1\"\napi_key = \"sk_one\"\nname = \"MagicWeather\"\n\n\
             [[projects]]\nid = \"proj2\"\napi_key = \"sk_two\"\n",
        )
        .unwrap();
        assert_eq!(config.currency.as_deref(), Some("USD"));
        assert_eq!(config.projects.len(), 2);
        assert_eq!(config.projects[0].label(), "MagicWeather");
        assert_eq!(config.projects[1].label(), "proj2");
    }

    #[test]
    fn migrates_a_single_project_file_from_the_first_release() {
        let config =
            Config::parse("project_id = \"proj123\"\napi_key = \"sk_abc\"\ncurrency = \"EUR\"\n")
                .unwrap();
        assert_eq!(config.projects.len(), 1);
        assert_eq!(config.projects[0].id, "proj123");
        assert_eq!(config.projects[0].api_key, "sk_abc");
        assert_eq!(config.currency.as_deref(), Some("EUR"));
        assert!(config.legacy_project_id.is_none());
        assert!(config.legacy_api_key.is_none());
    }

    #[test]
    fn parses_preferences_and_pins() {
        let config = Config::parse(
            "currency = \"EUR\"\nchart_minutes = 20\nmotion = \"minimal\"\ntheme = \"light\"\nwall = true\npins = [\"proj2\"]\n",
        )
        .unwrap();
        assert_eq!(config.chart_minutes, Some(20));
        assert_eq!(config.motion.as_deref(), Some("minimal"));
        assert_eq!(config.theme.as_deref(), Some("light"));
        assert_eq!(config.wall, Some(true));
        assert_eq!(config.pins, vec!["proj2".to_string()]);
    }

    #[test]
    fn parses_persisted_ui_state() {
        let config = Config::parse(
            "ui_scale = 2\nui_page = 1\nlast_project = \"proj2\"\nfeed_seen_at = 1760900000000\nfeed_path = \"/tmp/feed.json\"\n",
        )
        .unwrap();
        assert_eq!(config.ui_scale, Some(2));
        assert_eq!(config.ui_page, Some(1));
        assert_eq!(config.last_project.as_deref(), Some("proj2"));
        assert_eq!(config.feed_seen_at, Some(1760900000000));
        assert_eq!(config.feed_path.as_deref(), Some("/tmp/feed.json"));
    }

    #[test]
    fn toggles_pins_and_removes_projects() {
        let mut config = Config {
            projects: vec![
                ProjectConfig::new("proj1", "sk_one", None),
                ProjectConfig::new("proj2", "sk_two", None),
            ],
            ..Config::default()
        };
        config.toggle_pin("proj2");
        assert_eq!(config.pins, vec!["proj2".to_string()]);
        config.toggle_pin("proj2");
        assert!(config.pins.is_empty());

        config.toggle_pin("proj1");
        config.remove_project("proj1");
        assert_eq!(config.projects.len(), 1);
        assert!(config.pins.is_empty());
    }

    #[test]
    fn renders_a_file_that_parses_back() {
        let mut config = Config {
            currency: Some("EUR".to_string()),
            ..Config::default()
        };
        config.upsert(ProjectConfig::new("proj1", "sk_one", Some("One".into())));
        config.upsert(ProjectConfig::new("proj2", "sk_two", None));
        config.upsert(ProjectConfig::new(
            "proj1",
            "sk_updated",
            Some("One".into()),
        ));

        let rendered = config.render();
        assert!(rendered.starts_with("# rc-tui configuration"));
        assert!(rendered.contains("[[projects]]"));

        let parsed = Config::parse(&rendered).unwrap();
        assert_eq!(parsed.projects.len(), 2);
        assert_eq!(parsed.projects[0].api_key, "sk_updated");
        assert_eq!(parsed.currency.as_deref(), Some("EUR"));
    }

    #[test]
    fn saves_the_file_read_only_for_the_owner() {
        let dir = std::env::temp_dir().join(format!("rc-tui-config-test-{}", std::process::id()));
        let path = dir.join("nested").join("config.toml");
        let config = Config {
            currency: Some("EUR".to_string()),
            ..Config::default()
        };

        config.save_to(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            Config::parse(&text).unwrap().currency.as_deref(),
            Some("EUR")
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        fs::remove_dir_all(&dir).unwrap();
    }
}
