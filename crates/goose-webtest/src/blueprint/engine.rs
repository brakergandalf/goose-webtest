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

                // Build prompt: context from prior nodes + test spec + node prompt
                let context_prefix = self.build_context_prefix();
                let full_prompt = if let Some(ref spec) = self.test_spec {
                    format!(
                        "{}{}\n\n--- Test Specification ---\n{}",
                        context_prefix, prompt, spec.to_prompt()
                    )
                } else {
                    format!("{}{}", context_prefix, prompt)
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
