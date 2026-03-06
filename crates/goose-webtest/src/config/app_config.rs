use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Configuration for a target web application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub target: TargetConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub navigation: NavigationConfig,
    #[serde(default)]
    pub timeouts: TimeoutConfig,
    #[serde(default)]
    pub screenshots: ScreenshotConfig,
    #[serde(default)]
    pub expected_state: toml::Table,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_language")]
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_auth_type")]
    pub r#type: String,
    #[serde(default)]
    pub login_url: Option<String>,
    #[serde(default)]
    pub username_field: Option<String>,
    #[serde(default)]
    pub password_field: Option<String>,
    #[serde(default)]
    pub submit_button: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub dashboard_selector: Option<String>,
    #[serde(default)]
    pub success_indicator: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            r#type: default_auth_type(),
            login_url: None,
            username_field: None,
            password_field: None,
            submit_button: None,
            username: None,
            password: None,
            dashboard_selector: None,
            success_indicator: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationPage {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NavigationConfig {
    #[serde(default)]
    pub pages: Vec<NavigationPage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    #[serde(default = "default_page_load")]
    pub page_load_ms: u64,
    #[serde(default = "default_element_wait")]
    pub element_wait_ms: u64,
    #[serde(default = "default_login_wait")]
    pub login_wait_ms: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            page_load_ms: default_page_load(),
            element_wait_ms: default_element_wait(),
            login_wait_ms: default_login_wait(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotConfig {
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub full_page: bool,
    #[serde(default = "default_viewport_width")]
    pub viewport_width: u32,
    #[serde(default = "default_viewport_height")]
    pub viewport_height: u32,
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            format: "png".to_string(),
            full_page: false,
            viewport_width: 1280,
            viewport_height: 720,
        }
    }
}

fn default_language() -> String {
    "en".to_string()
}

fn default_auth_type() -> String {
    "none".to_string()
}

fn default_page_load() -> u64 {
    15000
}

fn default_element_wait() -> u64 {
    5000
}

fn default_login_wait() -> u64 {
    10000
}

fn default_format() -> String {
    "png".to_string()
}

fn default_viewport_width() -> u32 {
    1280
}

fn default_viewport_height() -> u32 {
    720
}

impl AppConfig {
    /// Load config from a TOML file
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Create a minimal config from just a URL (no auth)
    pub fn from_url(url: &str) -> Self {
        Self {
            target: TargetConfig {
                name: url.to_string(),
                base_url: url.to_string(),
                description: String::new(),
                language: "en".to_string(),
            },
            auth: AuthConfig::default(),
            navigation: NavigationConfig::default(),
            timeouts: TimeoutConfig::default(),
            screenshots: ScreenshotConfig::default(),
            expected_state: toml::Table::new(),
        }
    }

    /// Get the login URL (falls back to base_url if not specified)
    pub fn login_url(&self) -> &str {
        self.auth
            .login_url
            .as_deref()
            .unwrap_or(&self.target.base_url)
    }

    pub fn requires_auth(&self) -> bool {
        self.auth.r#type != "none"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_config() {
        let toml = r#"
[target]
name = "Test App"
base_url = "http://localhost:3000"
"#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.target.name, "Test App");
        assert_eq!(config.target.base_url, "http://localhost:3000");
        assert!(!config.requires_auth());
    }

    #[test]
    fn test_parse_full_config() {
        let toml = r#"
[target]
name = "Sparkasse Demo"
base_url = "https://www.sparkasse-hannover.de"
language = "de"

[auth]
type = "form"
login_url = "https://www.sparkasse-hannover.de/login"
username_field = "input[name='user']"
password_field = "input[name='pass']"
submit_button = "button[type='submit']"
username = "demo"
password = "12345"

[timeouts]
page_load_ms = 20000

[screenshots]
viewport_width = 1920
viewport_height = 1080
"#;
        let config: AppConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.target.language, "de");
        assert!(config.requires_auth());
        assert_eq!(config.auth.username.as_deref(), Some("demo"));
        assert_eq!(config.timeouts.page_load_ms, 20000);
        assert_eq!(config.screenshots.viewport_width, 1920);
    }
}
