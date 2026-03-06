pub mod html_report;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::blueprint::nodes::{NodeResult, NodeStatus};

/// Collects results during blueprint execution
pub struct ReportCollector {
    pub target_name: String,
    pub blueprint_name: String,
    pub test_spec: Option<String>,
    pub steps: Vec<NodeResult>,
    pub start_time: DateTime<Utc>,
}

impl ReportCollector {
    pub fn new(target_name: &str, blueprint_name: &str) -> Self {
        Self {
            target_name: target_name.to_string(),
            blueprint_name: blueprint_name.to_string(),
            test_spec: None,
            steps: Vec::new(),
            start_time: Utc::now(),
        }
    }

    pub fn record_step(&mut self, result: NodeResult) {
        self.steps.push(result);
    }

    /// Generate the final report
    pub fn finalize(&self) -> TestReport {
        let total = self.steps.len();
        let passed = self
            .steps
            .iter()
            .filter(|s| s.status == NodeStatus::Pass)
            .count();
        let failed = self
            .steps
            .iter()
            .filter(|s| s.status == NodeStatus::Fail)
            .count();
        let errors = self
            .steps
            .iter()
            .filter(|s| s.status == NodeStatus::Error)
            .count();
        let skipped = self
            .steps
            .iter()
            .filter(|s| s.status == NodeStatus::Skip)
            .count();

        let result = if failed == 0 && errors == 0 {
            OverallResult::Pass
        } else if passed > 0 {
            OverallResult::Partial
        } else {
            OverallResult::Fail
        };

        let duration_seconds = (Utc::now() - self.start_time).num_seconds() as f64;

        // Aggregate assertions across all steps
        let all_assertions: Vec<_> = self
            .steps
            .iter()
            .flat_map(|s| s.assertions.iter())
            .collect();
        let assertions_passed = all_assertions.iter().filter(|a| a.passed).count();
        let assertions_failed = all_assertions.iter().filter(|a| !a.passed).count();

        TestReport {
            metadata: ReportMetadata {
                report_id: Uuid::new_v4().to_string(),
                timestamp: Utc::now(),
                duration_seconds,
                target_name: self.target_name.clone(),
                blueprint_name: self.blueprint_name.clone(),
                test_spec: self.test_spec.clone(),
                agent_version: env!("CARGO_PKG_VERSION").to_string(),
            },
            summary: ReportSummary {
                total_steps: total,
                passed,
                failed,
                errors,
                skipped,
                result,
                assertions_total: all_assertions.len(),
                assertions_passed,
                assertions_failed,
            },
            steps: self.steps.clone(),
        }
    }
}

/// The complete test report
#[derive(Debug, Clone, Serialize)]
pub struct TestReport {
    pub metadata: ReportMetadata,
    pub summary: ReportSummary,
    pub steps: Vec<NodeResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportMetadata {
    pub report_id: String,
    pub timestamp: DateTime<Utc>,
    pub duration_seconds: f64,
    pub target_name: String,
    pub blueprint_name: String,
    pub test_spec: Option<String>,
    pub agent_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportSummary {
    pub total_steps: usize,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
    pub skipped: usize,
    pub result: OverallResult,
    pub assertions_total: usize,
    pub assertions_passed: usize,
    pub assertions_failed: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OverallResult {
    Pass,
    Fail,
    Partial,
    Error,
}

impl TestReport {
    /// Write JSON report to file
    pub fn write_json(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}
