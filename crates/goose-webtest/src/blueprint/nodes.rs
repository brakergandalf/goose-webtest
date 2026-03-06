use serde::{Deserialize, Serialize};

/// A node in the blueprint graph — either deterministic (Rust code)
/// or agentic (LLM-driven via Goose ReAct loop).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BlueprintNode {
    #[serde(rename = "deterministic")]
    Deterministic {
        id: String,
        name: String,
        action: DeterministicAction,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    #[serde(rename = "agentic")]
    Agentic {
        id: String,
        name: String,
        prompt: String,
        #[serde(default)]
        max_turns: Option<u32>,
        /// Restrict which MCP tools this node can use
        #[serde(default)]
        tools: Option<Vec<String>>,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
}

impl BlueprintNode {
    pub fn id(&self) -> &str {
        match self {
            BlueprintNode::Deterministic { id, .. } => id,
            BlueprintNode::Agentic { id, .. } => id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            BlueprintNode::Deterministic { name, .. } => name,
            BlueprintNode::Agentic { name, .. } => name,
        }
    }

    pub fn is_deterministic(&self) -> bool {
        matches!(self, BlueprintNode::Deterministic { .. })
    }

    pub fn is_agentic(&self) -> bool {
        matches!(self, BlueprintNode::Agentic { .. })
    }
}

/// Actions that execute deterministically (no LLM involved)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DeterministicAction {
    /// Start Playwright browser and create a new page
    LaunchBrowser,
    /// Navigate to a URL (supports template variables from app config)
    Navigate {
        url_template: String,
    },
    /// Perform login using app config credentials
    Login,
    /// Fill a form with specified field values
    FillForm {
        fields: Vec<FormField>,
    },
    /// Click an element by CSS selector
    Click {
        selector: String,
    },
    /// Wait for an element to appear
    WaitFor {
        selector: String,
        #[serde(default = "default_wait_timeout")]
        timeout_ms: u64,
    },
    /// Take a named screenshot
    Screenshot {
        name: String,
    },
    /// Generate the test report from collected results
    GenerateReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormField {
    pub selector: String,
    pub value: String,
}

fn default_wait_timeout() -> u64 {
    5000
}

/// Result of executing a single node
#[derive(Debug, Clone, Serialize)]
pub struct NodeResult {
    pub node_id: String,
    pub node_name: String,
    pub node_type: NodeType,
    pub status: NodeStatus,
    pub duration_ms: u64,
    pub detail: Option<String>,
    pub screenshot_path: Option<String>,
    pub error: Option<String>,
    /// All screenshots captured during this node's execution
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshots: Vec<String>,
    /// Structured assertion results from virtual tools
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<crate::assertions::Assertion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeType {
    Deterministic,
    Agentic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeStatus {
    Pass,
    Fail,
    Skip,
    Error,
}
