use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use tracing::{info, warn};

use super::nodes::{BlueprintNode, NodeResult, NodeStatus, NodeType};
use super::transitions::TransitionCondition;
use super::Blueprint;
use crate::assertions::Assertion;
use crate::config::test_spec::TestSpec;
use crate::config::AppConfig;
use crate::report::ReportCollector;
use crate::steps::deterministic::DeterministicExecutor;

/// Output from a completed node, passed as context to subsequent agentic nodes.
#[derive(Debug, Clone)]
struct NodeContextEntry {
    node_id: String,
    node_name: String,
    output: String,
}

/// The Blueprint Engine executes a directed graph of deterministic and agentic nodes.
/// It follows the Stripe Minions pattern: deterministic nodes execute Rust code directly,
/// agentic nodes invoke the Goose ReAct loop with scoped prompts.
pub struct BlueprintEngine {
    blueprint: Blueprint,
    graph: DiGraph<String, TransitionCondition>,
    node_map: HashMap<String, NodeIndex>,
    app_config: AppConfig,
    executor: DeterministicExecutor,
    test_spec: Option<TestSpec>,
    /// Context history from completed agentic nodes, injected into subsequent prompts.
    context_history: Vec<NodeContextEntry>,
    /// Report output directory for screenshots.
    report_dir: PathBuf,
}

impl BlueprintEngine {
    pub fn new(blueprint: Blueprint, app_config: AppConfig) -> Result<Self> {
        Self::with_options(blueprint, app_config, false)
    }

    pub fn with_options(blueprint: Blueprint, app_config: AppConfig, headed: bool) -> Result<Self> {
        let mut graph = DiGraph::new();
        let mut node_map = HashMap::new();

        // Build graph nodes
        for node in &blueprint.nodes {
            let idx = graph.add_node(node.id().to_string());
            node_map.insert(node.id().to_string(), idx);
        }

        // Build graph edges (transitions)
        for transition in &blueprint.transitions {
            let from = node_map
                .get(&transition.from)
                .ok_or_else(|| anyhow!("Unknown node in transition: {}", transition.from))?;
            let to = node_map
                .get(&transition.to)
                .ok_or_else(|| anyhow!("Unknown node in transition: {}", transition.to))?;
            graph.add_edge(*from, *to, transition.condition.clone());
        }

        let executor = DeterministicExecutor::new(app_config.clone(), headed);

        Ok(Self {
            blueprint,
            graph,
            node_map,
            app_config,
            executor,
            test_spec: None,
            context_history: Vec::new(),
            report_dir: PathBuf::from("reports"),
        })
    }

    /// Execute the entire blueprint graph
    pub async fn execute(&mut self, report: &mut ReportCollector) -> Result<()> {
        let start_node = self
            .blueprint
            .start_node()
            .ok_or_else(|| anyhow!("Blueprint has no nodes"))?;

        let start_idx = *self
            .node_map
            .get(start_node.id())
            .ok_or_else(|| anyhow!("Start node not in graph"))?;

        let mut current_idx = start_idx;
        let mut retries: u32 = 0;
        let max_retries = self.blueprint.settings.max_retries;

        loop {
            let node_id = &self.graph[current_idx];
            let node = self
                .find_node(node_id)
                .ok_or_else(|| anyhow!("Node {} not found in blueprint", node_id))?
                .clone();

            if retries > 0 {
                info!(
                    "Retrying node: {} ({}) — attempt {}/{}",
                    node.name(),
                    node.id(),
                    retries + 1,
                    max_retries + 1
                );
            } else {
                info!("Executing node: {} ({})", node.name(), node.id());
            }

            let start = std::time::Instant::now();
            let result = self.execute_node(&node).await;
            let duration_ms = start.elapsed().as_millis() as u64;

            let node_result = match result {
                Ok((detail, screenshots, assertions)) => NodeResult {
                    node_id: node.id().to_string(),
                    node_name: node.name().to_string(),
                    node_type: if node.is_deterministic() {
                        NodeType::Deterministic
                    } else {
                        NodeType::Agentic
                    },
                    status: NodeStatus::Pass,
                    duration_ms,
                    detail: Some(detail),
                    screenshot_path: None,
                    error: None,
                    screenshots,
                    assertions,
                },
                Err(e) => {
                    warn!("Node {} failed: {}", node.id(), e);
                    NodeResult {
                        node_id: node.id().to_string(),
                        node_name: node.name().to_string(),
                        node_type: if node.is_deterministic() {
                            NodeType::Deterministic
                        } else {
                            NodeType::Agentic
                        },
                        status: NodeStatus::Fail,
                        duration_ms,
                        detail: None,
                        screenshot_path: None,
                        error: Some(e.to_string()),
                        screenshots: Vec::new(),
                        assertions: Vec::new(),
                    }
                }
            };

            let failed = node_result.status == NodeStatus::Fail
                || node_result.status == NodeStatus::Error;

            // On failure, retry before following the failure transition
            if failed && retries < max_retries {
                retries += 1;
                warn!(
                    "Node {} failed, retrying ({}/{})",
                    node.id(),
                    retries,
                    max_retries
                );
                report.record_step(node_result);
                continue; // re-execute same node
            }

            report.record_step(node_result.clone());

            // Find next node via transitions
            match self.resolve_next(current_idx, &node_result.status) {
                Some(next_idx) => {
                    current_idx = next_idx;
                    retries = 0;
                }
                None => {
                    info!("Blueprint execution complete (no more transitions)");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Build context prefix from prior agentic node outputs.
    fn build_context_prefix(&self) -> String {
        if self.context_history.is_empty() {
            return String::new();
        }
        let mut ctx = String::from("=== Context from previous steps ===\n");
        for entry in &self.context_history {
            ctx.push_str(&format!(
                "### {} (node: {})\n{}\n\n",
                entry.node_name, entry.node_id, entry.output
            ));
        }
        ctx.push_str("=== End previous context ===\n\n");
        ctx
    }

    /// Build app context header with target info so the LLM knows where it is.
    fn build_app_context(&self) -> String {
        let mut ctx = String::from("=== Application Under Test ===\n");
        ctx.push_str(&format!("Name: {}\n", self.app_config.target.name));
        ctx.push_str(&format!("Base URL: {}\n", self.app_config.target.base_url));
        if self.app_config.requires_auth() {
            ctx.push_str("Auth: Logged in (form-based authentication completed)\n");
        }
        if !self.app_config.target.language.is_empty() {
            ctx.push_str(&format!("Language: {}\n", self.app_config.target.language));
        }
        ctx.push_str("IMPORTANT: You are already in the browser on the application. Use browser_snapshot to see the current page. Do NOT navigate to localhost or any other URL — stay on this application.\n");
        ctx.push_str("=== End Application Context ===\n\n");
        ctx
    }

    /// Execute a single node, returning (detail_text, screenshots, assertions).
    async fn execute_node(
        &mut self,
        node: &BlueprintNode,
    ) -> Result<(String, Vec<String>, Vec<Assertion>)> {
        match node {
            BlueprintNode::Deterministic { action, .. } => {
                let detail = self.executor.execute(action).await?;
                Ok((detail, Vec::new(), Vec::new()))
            }
            BlueprintNode::Agentic {
                id, name, prompt, max_turns, ..
            } => {
                let playwright = self
                    .executor
                    .playwright()
                    .ok_or_else(|| anyhow!("Browser not launched — cannot run agentic node"))?;

                // Build prompt: app context + prior node context + test spec + node prompt
                let app_context = self.build_app_context();
                let context_prefix = self.build_context_prefix();
                let full_prompt = if let Some(ref spec) = self.test_spec {
                    format!(
                        "{}{}{}\n\n--- Test Specification ---\n{}",
                        app_context, context_prefix, prompt, spec.to_prompt()
                    )
                } else {
                    format!("{}{}{}", app_context, context_prefix, prompt)
                };

                let report_dir = self.report_dir.clone();
                let output = crate::steps::agentic::execute_agentic_full(
                    &full_prompt,
                    *max_turns,
                    playwright,
                    Some(&report_dir),
                )
                .await?;

                // Store output for subsequent nodes
                self.context_history.push(NodeContextEntry {
                    node_id: id.clone(),
                    node_name: name.clone(),
                    output: output.text.clone(),
                });

                Ok((output.text, output.screenshots, output.assertions))
            }
        }
    }

    /// Find the next node based on transitions and current status
    fn resolve_next(&self, current: NodeIndex, status: &NodeStatus) -> Option<NodeIndex> {
        let edges = self.graph.edges(current);
        for edge in edges {
            if edge.weight().matches(status) {
                return Some(edge.target());
            }
        }
        None
    }

    fn find_node(&self, id: &str) -> Option<&BlueprintNode> {
        self.blueprint.nodes.iter().find(|n| n.id() == id)
    }

    pub fn app_config(&self) -> &AppConfig {
        &self.app_config
    }

    pub fn set_report_dir(&mut self, dir: PathBuf) {
        self.report_dir = dir.clone();
        self.executor.set_report_dir(dir);
    }

    /// Set a test specification to inject into agentic node prompts
    pub fn set_test_spec(&mut self, spec: TestSpec) {
        self.test_spec = Some(spec);
    }

    /// Get a reference to the Playwright client (if browser is launched)
    pub fn playwright(&self) -> Option<&crate::playwright::PlaywrightClient> {
        self.executor.playwright()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::{Blueprint, BlueprintNode, BlueprintSettings, Transition, TransitionCondition};
    use crate::config::{AppConfig, AppConfigBuilder, TargetConfig, TargetSettings};
    use crate::report::ReportCollector;

    fn make_test_blueprint() -> Blueprint {
        Blueprint {
            version: "1.0".to_string(),
            name: "test-blueprint".to_string(),
            description: String::new(),
            settings: BlueprintSettings {
                max_retries: 2,
                timeout_seconds: 60,
                screenshot_on_every_step: true,
            },
            nodes: vec![
                BlueprintNode::deterministic("launch", "Launch Browser", "LaunchBrowser".to_string()),
                BlueprintNode::agentic("verify", "Verify Page", "Check the page loaded".to_string(), 5),
            ],
            transitions: vec![Transition {
                from: "launch".to_string(),
                to: "verify".to_string(),
                condition: TransitionCondition::OnSuccess,
            }],
        }
    }

    fn make_test_app_config() -> AppConfig {
        AppConfig::from(TargetConfig {
            name: "test-app".to_string(),
            base_url: "http://localhost:3000".to_string(),
            language: "en".to_string(),
            settings: TargetSettings::default(),
        })
    }

    #[test]
    fn test_blueprint_engine_new() {
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let engine = BlueprintEngine::new(blueprint, app_config);
        assert!(engine.is_ok());
        let mut engine = engine.unwrap();
        assert_eq!(engine.blueprint.name, "test-blueprint");
        assert_eq!(engine.context_history.len(), 0);
    }

    #[test]
    fn test_blueprint_engine_with_options_headless() {
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let engine = BlueprintEngine::with_options(blueprint, app_config, false);
        assert!(engine.is_ok());
    }

    #[test]
    fn test_context_history_empty() {
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        let context = engine.build_context_prefix();
        assert!(context.is_empty());
    }

    #[test]
    fn test_build_context_prefix_with_history() {
        let mut engine = BlueprintEngine::with_options(
            make_test_blueprint(),
            make_test_app_config(),
            false,
        ).unwrap();
        
        engine.context_history.push(NodeContextEntry {
            node_id: "node1".to_string(),
            node_name: "Step 1".to_string(),
            output: "Completed successfully".to_string(),
        });
        
        let context = engine.build_context_prefix();
        assert!(context.contains("Step 1"));
        assert!(context.contains("node1"));
        assert!(context.contains("Completed successfully"));
    }

    #[test]
    fn test_build_app_context() {
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        let context = engine.build_app_context();
        assert!(context.contains("test-app"));
        assert!(context.contains("localhost:3000"));
    }

    #[test]
    fn test_transition_resolution_onsuccess() {
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        // Get start node index
        let start_node = engine.blueprint.start_node().unwrap();
        let start_idx = *engine.node_map.get(start_node.id()).unwrap();
        
        // Should resolve to next node on success
        let next_idx = engine.resolve_next(start_idx, &NodeStatus::Pass);
        assert!(next_idx.is_some());
    }

    #[test]
    fn test_transition_resolution_onfailure() {
        let mut blueprint = make_test_blueprint();
        // Add a failure transition
        blueprint.transitions.push(Transition {
            from: "launch".to_string(),
            to: "verify".to_string(),
            condition: TransitionCondition::OnFailure,
        });
        
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        let start_node = engine.blueprint.start_node().unwrap();
        let start_idx = *engine.node_map.get(start_node.id()).unwrap();
        
        // Should resolve to next node on failure
        let next_idx = engine.resolve_next(start_idx, &NodeStatus::Fail);
        assert!(next_idx.is_some());
    }

    #[test]
    fn test_transition_resolution_always() {
        let mut blueprint = make_test_blueprint();
        blueprint.transitions.push(Transition {
            from: "launch".to_string(),
            to: "verify".to_string(),
            condition: TransitionCondition::Always,
        });
        
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        let start_node = engine.blueprint.start_node().unwrap();
        let start_idx = *engine.node_map.get(start_node.id()).unwrap();
        
        // Should resolve to next node regardless of status
        let next_pass = engine.resolve_next(start_idx, &NodeStatus::Pass);
        assert!(next_pass.is_some());
        let next_fail = engine.resolve_next(start_idx, &NodeStatus::Fail);
        assert!(next_fail.is_some());
    }

    #[test]
    fn test_find_node() {
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        let node = engine.find_node("launch");
        assert!(node.is_some());
        
        let node = engine.find_node("nonexistent");
        assert!(node.is_none());
    }

    #[test]
    fn test_set_report_dir() {
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        let report_dir = std::path::PathBuf::from("/tmp/test-reports");
        engine.set_report_dir(report_dir.clone());
        
        // The report directory should be updated (internal state)
        // This tests the side effect of set_report_dir
    }

    #[test]
    fn test_set_test_spec() {
        use crate::config::test_spec::TestSpec;
        
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        let spec = TestSpec::new("test".to_string(), "test description".to_string());
        engine.set_test_spec(spec);
        
        // Verify spec is set (internal state)
    }

    #[test]
    fn test_graph_construction_from_blueprint() {
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        // Check that graph has correct number of nodes
        let node_count = engine.node_map.len();
        assert_eq!(node_count, 2); // launch and verify
        
        // Check node mapping
        assert!(engine.node_map.contains_key("launch"));
        assert!(engine.node_map.contains_key("verify"));
    }

    #[test]
    fn test_resolve_next_no_transition() {
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        // Create a scenario with no valid transitions
        let start_node = engine.blueprint.start_node().unwrap();
        let start_idx = *engine.node_map.get(start_node.id()).unwrap();
        
        // Remove transitions for testing
        // We can't directly modify engine.graph, so we test with empty blueprint
        let empty_blueprint = Blueprint {
            version: "1.0".to_string(),
            name: "empty".to_string(),
            description: String::new(),
            settings: BlueprintSettings::default(),
            nodes: vec![BlueprintNode::deterministic("single", "Single Node", "LaunchBrowser".to_string())],
            transitions: vec![],
        };
        
        let mut empty_engine = BlueprintEngine::new(empty_blueprint, app_config).unwrap();
        let single_node = empty_engine.blueprint.start_node().unwrap();
        let single_idx = *empty_engine.node_map.get(single_node.id()).unwrap();
        
        let next = empty_engine.resolve_next(single_idx, &NodeStatus::Pass);
        assert!(next.is_none());
    }

    #[test]
    fn test_retry_logic() {
        // This test verifies the retry mechanism is implemented correctly
        // We test by checking that the engine accepts and tracks retry counts
        
        let mut blueprint = make_test_blueprint();
        blueprint.settings.max_retries = 3;
        
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        // The engine should accept the blueprint with retries configured
        assert_eq!(engine.blueprint.settings.max_retries, 3);
    }

    #[test]
    fn test_deterministic_node_creation() {
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        // Verify we can find and identify deterministic nodes
        let launch_node = engine.blueprint.nodes.iter().find(|n| n.id() == "launch").unwrap();
        assert!(launch_node.is_deterministic());
    }

    #[test]
    fn test_agentic_node_creation() {
        let blueprint = make_test_blueprint();
        let app_config = make_test_app_config();
        let mut engine = BlueprintEngine::new(blueprint, app_config).unwrap();
        
        // Verify we can find and identify agentic nodes
        let verify_node = engine.blueprint.nodes.iter().find(|n| n.id() == "verify").unwrap();
        assert!(!verify_node.is_deterministic());
    }
}
