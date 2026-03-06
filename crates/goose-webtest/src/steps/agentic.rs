//! Lightweight ReAct loop for agentic blueprint nodes.
//! Uses the Goose Provider trait directly (no full Agent needed)
//! to drive LLM reasoning over Playwright browser tools.

use anyhow::{anyhow, Result};
use goose::conversation::message::{Message, MessageContent, MessageMetadata};
use rmcp::model::Role;
use goose::model::ModelConfig;
use goose::providers::base::Provider;
use goose::providers::create;
use rmcp::model::{CallToolResult, Content};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::assertions::{Assertion, AssertionType};
use crate::playwright::PlaywrightClient;
use crate::steps::virtual_tools;

const WEBTEST_SYSTEM_PROMPT: &str = r#"You are a web application testing agent. You have access to browser automation tools via the Playwright MCP server.

Your job is to follow the instructions given to you, interact with the web page, and report your findings clearly.

Guidelines:
- Use browser_snapshot to see the current state of the page (accessibility tree)
- Use browser_click, browser_type, browser_press_key to interact with elements
- Use browser_take_screenshot to capture visual evidence
- Use browser_navigate to go to URLs
- Reference elements by their [ref=XXX] from the snapshot
- Use assert_visible, assert_text_contains, assert_url_matches, assert_element_exists to validate expectations
- Use log_step to record observations and test progress
- Be concise but thorough

When you have completed the task, provide a clear summary of what you found."#;

/// Result of an agentic execution including collected evidence.
pub struct AgenticOutput {
    pub text: String,
    pub screenshots: Vec<String>,
    pub assertions: Vec<Assertion>,
}

/// Execute an agentic node using a lightweight ReAct loop.
/// Returns the final text plus collected screenshots and assertions.
pub async fn execute_agentic(
    prompt: &str,
    max_turns: Option<u32>,
    playwright: &PlaywrightClient,
) -> Result<String> {
    let output = execute_agentic_full(prompt, max_turns, playwright, None).await?;
    Ok(output.text)
}

/// Execute an agentic node with full evidence collection.
/// When `report_dir` is provided, screenshots from `browser_take_screenshot` are saved
/// and their paths tracked.
pub async fn execute_agentic_full(
    prompt: &str,
    max_turns: Option<u32>,
    playwright: &PlaywrightClient,
    report_dir: Option<&PathBuf>,
) -> Result<AgenticOutput> {
    let max_turns = max_turns.unwrap_or(5) as usize;

    // Resolve provider from environment / goose config
    let provider = create_provider().await?;

    // Get available Playwright tools + virtual tools
    let mut tools = playwright.list_tools().await?;
    tools.extend(virtual_tools::virtual_tool_definitions());
    info!(
        "Agentic node starting with {} tools ({} virtual), max {} turns",
        tools.len(),
        virtual_tools::VIRTUAL_TOOL_NAMES.len(),
        max_turns
    );

    // Evidence collectors
    let mut screenshots: Vec<String> = Vec::new();
    let mut assertions: Vec<Assertion> = Vec::new();
    let mut screenshot_counter: u32 = 0;

    // Build initial messages
    let model_config = provider.get_model_config();
    let session_id = "webtest-agentic";
    let system = WEBTEST_SYSTEM_PROMPT;

    let user_message = Message::user().with_text(prompt);
    let mut messages = vec![user_message];

    // ReAct loop
    for turn in 0..max_turns {
        info!("Agentic turn {}/{}", turn + 1, max_turns);

        let (response, _usage) = provider
            .complete(&model_config, session_id, system, &messages, &tools)
            .await
            .map_err(|e| anyhow!("LLM completion failed: {}", e))?;

        // Check if response contains tool calls
        let tool_calls: Vec<_> = response
            .content
            .iter()
            .filter_map(|c| {
                if let MessageContent::ToolRequest(req) = c {
                    Some(req.clone())
                } else {
                    None
                }
            })
            .collect();

        if tool_calls.is_empty() {
            // No tool calls — LLM is done, extract final text
            let final_text = response
                .content
                .iter()
                .filter_map(|c| {
                    if let MessageContent::Text(t) = c {
                        Some(t.text.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            info!("Agentic node completed after {} turns", turn + 1);
            return Ok(AgenticOutput {
                text: final_text,
                screenshots,
                assertions,
            });
        }

        // Add assistant response to conversation
        messages.push(response);

        // Execute each tool call
        let mut tool_results = Vec::new();
        for req in &tool_calls {
            let tool_call = match &req.tool_call {
                Ok(call) => call,
                Err(e) => {
                    warn!("Invalid tool call: {}", e);
                    tool_results.push(MessageContent::tool_response(
                        req.id.clone(),
                        Err(e.clone()),
                    ));
                    continue;
                }
            };

            let tool_name = tool_call.name.to_string();
            debug!(
                "Executing tool: {} with args: {:?}",
                tool_name, tool_call.arguments
            );

            let args_value = tool_call
                .arguments
                .as_ref()
                .map(|a| serde_json::Value::Object(a.clone()))
                .unwrap_or(serde_json::json!({}));

            // Check if this is a virtual tool
            if virtual_tools::is_virtual_tool(&tool_name) {
                let result = handle_virtual_tool(
                    &tool_name,
                    &args_value,
                    playwright,
                    &mut assertions,
                    &mut screenshots,
                    &mut screenshot_counter,
                    report_dir,
                )
                .await;
                tool_results.push(MessageContent::tool_response(
                    req.id.clone(),
                    Ok(result),
                ));
                continue;
            }

            // Track screenshots from browser_take_screenshot
            if tool_name == "browser_take_screenshot" {
                if let Some(dir) = report_dir {
                    screenshot_counter += 1;
                    let path = dir.join(format!("agentic_{:03}.png", screenshot_counter));
                    match playwright.save_screenshot(&path).await {
                        Ok(_) => {
                            let path_str = path.to_string_lossy().to_string();
                            screenshots.push(path_str.clone());
                            info!("Agentic screenshot saved: {}", path_str);
                        }
                        Err(e) => info!("Screenshot save failed (non-fatal): {}", e),
                    }
                }
            }

            // Forward to Playwright
            let result = playwright
                .call_tool_raw(&tool_call.name, args_value)
                .await;

            match result {
                Ok(call_result) => {
                    let text = PlaywrightClient::extract_text(&call_result);
                    debug!("Tool result: {}", &text[..text.len().min(200)]);
                    tool_results.push(MessageContent::tool_response(
                        req.id.clone(),
                        Ok(call_result),
                    ));
                }
                Err(e) => {
                    warn!("Tool {} failed: {}", tool_name, e);
                    let error_result = CallToolResult {
                        content: vec![Content::text(format!("Error: {}", e))],
                        is_error: Some(true),
                        meta: None,
                        structured_content: None,
                    };
                    tool_results.push(MessageContent::tool_response(
                        req.id.clone(),
                        Ok(error_result),
                    ));
                }
            }
        }

        // Add tool results as user message
        let tool_message = Message {
            role: Role::User,
            created: chrono::Utc::now().timestamp(),
            content: tool_results,
            id: None,
            metadata: MessageMetadata::default(),
        };
        messages.push(tool_message);
    }

    Err(anyhow!(
        "Agentic node exceeded max turns ({}) without completing",
        max_turns
    ))
}

/// Handle a virtual tool call locally (not forwarded to Playwright).
async fn handle_virtual_tool(
    name: &str,
    args: &serde_json::Value,
    playwright: &PlaywrightClient,
    assertions: &mut Vec<Assertion>,
    screenshots: &mut Vec<String>,
    screenshot_counter: &mut u32,
    report_dir: Option<&PathBuf>,
) -> CallToolResult {
    match name {
        "assert_visible" => {
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let snapshot = match playwright.snapshot().await {
                Ok(s) => s,
                Err(e) => {
                    return make_error_result(&format!("Failed to take snapshot: {}", e));
                }
            };
            let found = snapshot.to_lowercase().contains(&text.to_lowercase());

            // Take evidence screenshot
            let screenshot_path = take_evidence_screenshot(
                playwright, screenshots, screenshot_counter, report_dir,
                &format!("assert_visible_{}", sanitize(text)),
            ).await;

            assertions.push(Assertion {
                assertion_type: AssertionType::Visible,
                target: text.to_string(),
                expected: format!("Text '{}' visible on page", text),
                actual: if found { "Found".to_string() } else { "Not found".to_string() },
                passed: found,
                screenshot_path,
            });

            let status = if found { "PASS" } else { "FAIL" };
            info!("assert_visible('{}') → {}", text, status);
            make_text_result(&format!("{}: Text '{}' is {}", status, text,
                if found { "visible on the page" } else { "NOT visible on the page" }))
        }
        "assert_text_contains" => {
            let selector = args.get("selector").and_then(|v| v.as_str()).unwrap_or("");
            let expected = args.get("expected").and_then(|v| v.as_str()).unwrap_or("");
            let snapshot = match playwright.snapshot().await {
                Ok(s) => s,
                Err(e) => {
                    return make_error_result(&format!("Failed to take snapshot: {}", e));
                }
            };
            let found = snapshot.to_lowercase().contains(&expected.to_lowercase());

            let screenshot_path = take_evidence_screenshot(
                playwright, screenshots, screenshot_counter, report_dir,
                &format!("assert_text_{}", sanitize(expected)),
            ).await;

            assertions.push(Assertion {
                assertion_type: AssertionType::TextContains,
                target: selector.to_string(),
                expected: expected.to_string(),
                actual: if found { format!("Contains '{}'", expected) } else { "Not found".to_string() },
                passed: found,
                screenshot_path,
            });

            let status = if found { "PASS" } else { "FAIL" };
            info!("assert_text_contains('{}', '{}') → {}", selector, expected, status);
            make_text_result(&format!("{}: Element '{}' {} '{}'", status, selector,
                if found { "contains" } else { "does NOT contain" }, expected))
        }
        "assert_url_matches" => {
            let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let snapshot = match playwright.snapshot().await {
                Ok(s) => s,
                Err(e) => {
                    return make_error_result(&format!("Failed to take snapshot: {}", e));
                }
            };
            // Extract URL from snapshot (first line often contains "page url: ...")
            let current_url = snapshot.lines()
                .find(|l| l.to_lowercase().contains("page url:"))
                .map(|l| l.split("page url:").last().unwrap_or("").trim().to_string())
                .unwrap_or_default();
            let found = current_url.to_lowercase().contains(&pattern.to_lowercase());

            assertions.push(Assertion {
                assertion_type: AssertionType::UrlMatches,
                target: "current URL".to_string(),
                expected: pattern.to_string(),
                actual: current_url.clone(),
                passed: found,
                screenshot_path: None,
            });

            let status = if found { "PASS" } else { "FAIL" };
            info!("assert_url_matches('{}') → {} (url: {})", pattern, status, current_url);
            make_text_result(&format!("{}: URL '{}' {} pattern '{}'", status, current_url,
                if found { "matches" } else { "does NOT match" }, pattern))
        }
        "assert_element_exists" => {
            let description = args.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let snapshot = match playwright.snapshot().await {
                Ok(s) => s,
                Err(e) => {
                    return make_error_result(&format!("Failed to take snapshot: {}", e));
                }
            };
            let lower_snap = snapshot.to_lowercase();
            let found = lower_snap.contains(&description.to_lowercase())
                && (text.is_empty() || lower_snap.contains(&text.to_lowercase()));

            assertions.push(Assertion {
                assertion_type: AssertionType::ElementExists,
                target: description.to_string(),
                expected: if text.is_empty() {
                    format!("Element '{}' exists", description)
                } else {
                    format!("Element '{}' with text '{}'", description, text)
                },
                actual: if found { "Found".to_string() } else { "Not found".to_string() },
                passed: found,
                screenshot_path: None,
            });

            let status = if found { "PASS" } else { "FAIL" };
            info!("assert_element_exists('{}', '{}') → {}", description, text, status);
            make_text_result(&format!("{}: Element '{}' {}", status, description,
                if found { "exists" } else { "does NOT exist" }))
        }
        "log_step" => {
            let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("");
            let status = args.get("status").and_then(|v| v.as_str()).unwrap_or("info");
            info!("[log_step/{}] {}", status, message);
            make_text_result(&format!("[{}] {}", status.to_uppercase(), message))
        }
        _ => make_error_result(&format!("Unknown virtual tool: {}", name)),
    }
}

/// Take an evidence screenshot and track it.
async fn take_evidence_screenshot(
    playwright: &PlaywrightClient,
    screenshots: &mut Vec<String>,
    counter: &mut u32,
    report_dir: Option<&PathBuf>,
    label: &str,
) -> Option<String> {
    let dir = report_dir?;
    *counter += 1;
    let path = dir.join(format!("evidence_{:03}_{}.png", *counter, label));
    match playwright.save_screenshot(&path).await {
        Ok(_) => {
            let path_str = path.to_string_lossy().to_string();
            screenshots.push(path_str.clone());
            Some(path_str)
        }
        Err(e) => {
            info!("Evidence screenshot failed (non-fatal): {}", e);
            None
        }
    }
}

/// Sanitize a string for use in filenames.
fn sanitize(s: &str) -> String {
    s.chars()
        .take(30)
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn make_text_result(text: &str) -> CallToolResult {
    CallToolResult {
        content: vec![Content::text(text.to_string())],
        is_error: Some(false),
        meta: None,
        structured_content: None,
    }
}

fn make_error_result(text: &str) -> CallToolResult {
    CallToolResult {
        content: vec![Content::text(text.to_string())],
        is_error: Some(true),
        meta: None,
        structured_content: None,
    }
}

/// Create a provider from environment or Goose configuration.
///
/// Priority order (highest first):
///   1. WEBTEST_PROVIDER / WEBTEST_MODEL env vars (webtest-specific)
///   2. GOOSE_PROVIDER / GOOSE_MODEL env vars
///   3. Goose config file (~/.config/goose/config.yaml)
///   4. Defaults: anthropic / claude-sonnet-4-6
async fn create_provider() -> Result<Arc<dyn Provider>> {
    let config = goose::config::Config::global();

    let provider_name = std::env::var("WEBTEST_PROVIDER")
        .or_else(|_| std::env::var("GOOSE_PROVIDER"))
        .unwrap_or_else(|_| {
            config
                .get_param::<String>("GOOSE_PROVIDER")
                .unwrap_or_else(|_| "anthropic".to_string())
        });

    let model_name = std::env::var("WEBTEST_MODEL")
        .or_else(|_| std::env::var("GOOSE_MODEL"))
        .unwrap_or_else(|_| {
            config
                .get_param::<String>("GOOSE_MODEL")
                .unwrap_or_else(|_| "anthropic/claude-sonnet-4.6".to_string())
        });

    info!(
        "Creating provider: {} with model: {}",
        provider_name, model_name
    );

    // Max tokens: WEBTEST_MAX_TOKENS env, GOOSE_MAX_TOKENS env, or 16384 default
    let max_tokens: i32 = std::env::var("WEBTEST_MAX_TOKENS")
        .or_else(|_| std::env::var("GOOSE_MAX_TOKENS"))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16_384);

    // Context limit: WEBTEST_CONTEXT_LIMIT env or 262144 (256k) default
    let context_limit: usize = std::env::var("WEBTEST_CONTEXT_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(262_144);

    info!(
        "Model config: max_tokens={}, context_limit={}",
        max_tokens, context_limit
    );

    let model_config = ModelConfig::new(&model_name)
        .map_err(|e| anyhow!("Invalid model config: {}", e))?
        .with_max_tokens(Some(max_tokens))
        .with_context_limit(Some(context_limit));

    let provider = create(&provider_name, model_config, vec![])
        .await
        .map_err(|e| anyhow!(
            "Failed to create provider '{}' with model '{}': {}\n\
             Available providers: anthropic, openai, vllm_local, openrouter\n\
             Set via: --provider <name> or WEBTEST_PROVIDER env var",
            provider_name, model_name, e
        ))?;

    Ok(provider)
}

/// Find a textbox whose label matches a keyword.
/// Two-pass strategy:
///   Pass 1 (inline): match keywords directly on the textbox line
///          e.g. `textbox "Online-Banking-PIN" [ref=e900]` matches keyword "pin"
///   Pass 2 (context): check nearby lines (±2) for keyword matches
///          e.g. `generic [ref=e47]: Anmeldename` near a textbox
pub(crate) fn find_input_ref_ctx(snapshot: &str, keywords: &[&str]) -> Option<String> {
    let lines: Vec<&str> = snapshot.lines().collect();

    // Pass 1: inline label match (most reliable)
    for line in &lines {
        let lower = line.to_lowercase();
        if !lower.contains("textbox") {
            continue;
        }
        for kw in keywords {
            if lower.contains(&kw.to_lowercase()) {
                if let Some(ref_val) = extract_ref(line) {
                    return Some(ref_val);
                }
            }
        }
    }

    // Pass 2: context match (check ±2 lines around each textbox)
    let context_window = 2;
    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        if !lower.contains("textbox") {
            continue;
        }

        let start = i.saturating_sub(context_window);
        let end = (i + context_window + 1).min(lines.len());

        for kw in keywords {
            let kw_lower = kw.to_lowercase();
            for ctx_line in &lines[start..end] {
                if ctx_line.to_lowercase().contains(&kw_lower) {
                    if let Some(ref_val) = extract_ref(line) {
                        return Some(ref_val);
                    }
                }
            }
        }
    }

    None
}

/// Find the Nth textbox in the snapshot (0-indexed).
/// Useful as a fallback when keyword matching fails.
pub(crate) fn find_nth_textbox(snapshot: &str, n: usize) -> Option<String> {
    let mut count = 0;
    for line in snapshot.lines() {
        if line.to_lowercase().contains("textbox") {
            if count == n {
                return extract_ref(line);
            }
            count += 1;
        }
    }
    None
}

/// Search for a button matching keywords in priority groups.
/// Tries each group in order; returns the first match from the highest-priority group.
pub(crate) fn find_button_ref_prioritized(snapshot: &str, priority_groups: &[&[&str]]) -> Option<String> {
    for group in priority_groups {
        if let Some(ref_val) = find_button_ref_ctx(snapshot, group) {
            return Some(ref_val);
        }
    }
    None
}

/// Search the snapshot for a button matching any of the keywords.
pub(crate) fn find_button_ref_ctx(snapshot: &str, keywords: &[&str]) -> Option<String> {
    for line in snapshot.lines() {
        let lower = line.to_lowercase();
        let is_button = lower.contains("button");

        if !is_button {
            continue;
        }

        for kw in keywords {
            if lower.contains(&kw.to_lowercase()) {
                if let Some(ref_val) = extract_ref(line) {
                    return Some(ref_val);
                }
            }
        }
    }

    None
}

/// Search the snapshot for a link matching any of the keywords.
pub(crate) fn find_link_ref(snapshot: &str, keywords: &[&str]) -> Option<String> {
    for line in snapshot.lines() {
        let lower = line.to_lowercase();
        if !lower.contains("link") {
            continue;
        }

        for kw in keywords {
            if lower.contains(&kw.to_lowercase()) {
                if let Some(ref_val) = extract_ref(line) {
                    return Some(ref_val);
                }
            }
        }
    }

    None
}

/// Extract the `[ref=XX]` value from a snapshot line.
/// Handles format: `- textbox [ref=e46]: chipDEMO`
pub(crate) fn extract_ref(line: &str) -> Option<String> {
    // Primary: look for [ref=XX] pattern (bracket-enclosed)
    if let Some(start) = line.find("[ref=") {
        let after = &line[start + 5..];
        if let Some(end) = after.find(']') {
            let ref_val = &after[..end];
            if !ref_val.is_empty() {
                return Some(ref_val.to_string());
            }
        }
    }

    // Fallback: look for ref=XX without brackets
    if let Some(idx) = line.find("ref=") {
        let after = &line[idx + 4..];
        let ref_val: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !ref_val.is_empty() {
            return Some(ref_val);
        }
    }

    None
}

pub(crate) fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}
