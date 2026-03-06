//! Virtual tools injected alongside Playwright tools.
//! These are handled locally by the agentic executor instead of being
//! forwarded to the Playwright MCP server.

use rmcp::model::Tool;
use serde_json::json;
use std::sync::Arc;

/// Names of all virtual tools — used to intercept calls before forwarding to Playwright.
pub const VIRTUAL_TOOL_NAMES: &[&str] = &[
    "assert_visible",
    "assert_text_contains",
    "assert_url_matches",
    "assert_element_exists",
    "log_step",
];

/// Check if a tool name is a virtual tool.
pub fn is_virtual_tool(name: &str) -> bool {
    VIRTUAL_TOOL_NAMES.contains(&name)
}

/// Helper to build a Tool with the rmcp 0.16 schema.
fn make_tool(name: &'static str, description: &'static str, schema: serde_json::Value) -> Tool {
    Tool {
        name: name.into(),
        title: None,
        description: Some(description.into()),
        input_schema: Arc::new(serde_json::from_value(schema).unwrap()),
        output_schema: None,
        annotations: None,
        execution: None,
        icons: None,
        meta: None,
    }
}

/// Build Tool definitions for all virtual tools to inject into the LLM's tool list.
pub fn virtual_tool_definitions() -> Vec<Tool> {
    vec![
        make_tool(
            "assert_visible",
            "Assert that specific text is visible on the current page. Takes a snapshot and checks if the text appears in the accessibility tree. Returns PASS or FAIL.",
            json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The text that should be visible on the page"
                    }
                },
                "required": ["text"]
            }),
        ),
        make_tool(
            "assert_text_contains",
            "Assert that a specific element contains expected text. Uses the accessibility snapshot to find the element.",
            json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "Description or ref of the element to check"
                    },
                    "expected": {
                        "type": "string",
                        "description": "The text that should be contained in the element"
                    }
                },
                "required": ["selector", "expected"]
            }),
        ),
        make_tool(
            "assert_url_matches",
            "Assert that the current page URL contains the expected pattern.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "URL substring or pattern that should be present in the current URL"
                    }
                },
                "required": ["pattern"]
            }),
        ),
        make_tool(
            "assert_element_exists",
            "Assert that an element with specific characteristics exists in the accessibility snapshot.",
            json!({
                "type": "object",
                "properties": {
                    "description": {
                        "type": "string",
                        "description": "Description of the element to look for (e.g. 'button', 'link', 'textbox')"
                    },
                    "text": {
                        "type": "string",
                        "description": "Text associated with the element"
                    }
                },
                "required": ["description"]
            }),
        ),
        make_tool(
            "log_step",
            "Log a test step or observation. Use this to record what you're doing and what you observe during testing.",
            json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Description of the test step or observation"
                    },
                    "status": {
                        "type": "string",
                        "enum": ["info", "pass", "fail", "warning"],
                        "description": "Status level (default: info)"
                    }
                },
                "required": ["message"]
            }),
        ),
    ]
}
