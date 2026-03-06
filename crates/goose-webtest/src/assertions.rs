//! Structured assertion framework for web test results.
//! Virtual tools (assert_visible, assert_text_contains, etc.) produce these
//! records which are included in the test report.

use serde::Serialize;

/// A single assertion result from a virtual tool invocation.
#[derive(Debug, Clone, Serialize)]
pub struct Assertion {
    pub assertion_type: AssertionType,
    /// What was being checked (CSS selector, URL pattern, etc.)
    pub target: String,
    /// What was expected
    pub expected: String,
    /// What was actually found
    pub actual: String,
    pub passed: bool,
    /// Screenshot taken at the moment of assertion (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionType {
    Visible,
    TextContains,
    ElementExists,
    UrlMatches,
    Custom,
}

impl Assertion {
    pub fn status_label(&self) -> &'static str {
        if self.passed { "PASS" } else { "FAIL" }
    }
}
