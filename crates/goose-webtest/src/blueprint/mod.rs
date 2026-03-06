pub mod engine;
pub mod nodes;
pub mod transitions;

use serde::{Deserialize, Serialize};

use self::nodes::BlueprintNode;
use self::transitions::Transition;

/// A Blueprint defines a directed graph of deterministic and agentic nodes
/// for testing a web application. Inspired by Stripe's Minions architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blueprint {
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub settings: BlueprintSettings,
    pub nodes: Vec<BlueprintNode>,
    pub transitions: Vec<Transition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueprintSettings {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_true")]
    pub screenshot_on_every_step: bool,
}

impl Default for BlueprintSettings {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            timeout_seconds: default_timeout(),
            screenshot_on_every_step: true,
        }
    }
}

fn default_max_retries() -> u32 {
    2
}

fn default_timeout() -> u64 {
    120
}

fn default_true() -> bool {
    true
}

impl Blueprint {
    /// Load a blueprint from a YAML file
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let blueprint: Blueprint = serde_yaml::from_str(&content)?;
        Ok(blueprint)
    }

    /// Load a blueprint from a YAML string
    pub fn from_yaml(yaml: &str) -> anyhow::Result<Self> {
        let blueprint: Blueprint = serde_yaml::from_str(yaml)?;
        Ok(blueprint)
    }

    /// Find the start node (first node in the list)
    pub fn start_node(&self) -> Option<&BlueprintNode> {
        self.nodes.first()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_blueprint_yaml() {
        let yaml = r#"
version: "1.0"
name: "test-blueprint"
description: "A test blueprint"
settings:
  max_retries: 3
  timeout_seconds: 60
nodes:
  - id: launch
    type: deterministic
    name: "Launch Browser"
    action:
      type: LaunchBrowser
  - id: verify
    type: agentic
    name: "Verify Page"
    prompt: "Check the page loaded correctly"
    max_turns: 5
transitions:
  - from: launch
    to: verify
    condition: OnSuccess
"#;
        let bp = Blueprint::from_yaml(yaml).unwrap();
        assert_eq!(bp.name, "test-blueprint");
        assert_eq!(bp.nodes.len(), 2);
        assert_eq!(bp.transitions.len(), 1);
        assert_eq!(bp.settings.max_retries, 3);
    }
}
