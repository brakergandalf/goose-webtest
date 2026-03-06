//! Playwright MCP client — spawns a Playwright MCP process and provides
//! high-level methods for browser automation.

use anyhow::{anyhow, Result};
use goose::agents::mcp_client::{GooseMcpClientCapabilities, McpClient, McpClientTrait};
use rmcp::model::{CallToolResult, JsonObject};
use rmcp::transport::TokioChildProcess;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

const SESSION_ID: &str = "webtest-session";

/// High-level Playwright browser automation client.
/// Wraps the Playwright MCP server process and provides typed methods.
pub struct PlaywrightClient {
    client: McpClient,
    headless: bool,
}

impl PlaywrightClient {
    /// Spawn a new Playwright MCP server and connect to it.
    pub async fn start(headless: bool) -> Result<Self> {
        info!(
            "Starting Playwright MCP server (headless: {})",
            headless
        );

        let mut args = vec![
            "@playwright/mcp@latest".to_string(),
            "--browser".to_string(),
            "chromium".to_string(),
        ];
        if headless {
            args.push("--headless".to_string());
        }

        let mut command = tokio::process::Command::new("npx");
        command
            .args(&args)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .stderr(Stdio::piped());

        let (transport, _stderr) = TokioChildProcess::builder(command)
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn Playwright MCP: {}", e))?;

        // No LLM provider needed for deterministic calls
        let provider = Arc::new(Mutex::new(None));

        let client = McpClient::connect(
            transport,
            Duration::from_secs(60),
            provider,
            "goose-webtest".to_string(),
            GooseMcpClientCapabilities { mcpui: false },
        )
        .await
        .map_err(|e| anyhow!("Failed to connect to Playwright MCP: {}", e))?;

        info!("Playwright MCP server connected");

        // Log available tools
        let tools = client
            .list_tools(SESSION_ID, None, CancellationToken::new())
            .await
            .map_err(|e| anyhow!("Failed to list tools: {}", e))?;

        let tool_names: Vec<_> = tools.tools.iter().map(|t| t.name.to_string()).collect::<Vec<String>>();
        let tool_list = tool_names.join(", ");
        eprintln!(
            "   Playwright tools ({}): {}",
            tool_names.len(),
            tool_list
        );

        Ok(Self { client, headless })
    }

    /// Call a Playwright MCP tool and return the result
    async fn call_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<CallToolResult> {
        let arguments: Option<JsonObject> = args
            .as_object()
            .map(|o| o.clone().into_iter().collect());

        debug!("Calling tool: {} with args: {}", tool_name, args);

        let result = self
            .client
            .call_tool(
                SESSION_ID,
                tool_name,
                arguments,
                None,
                CancellationToken::new(),
            )
            .await
            .map_err(|e| anyhow!("Tool call '{}' failed: {}", tool_name, e))?;

        // Check for tool-level errors
        if let Some(true) = result.is_error {
            let error_msg = result
                .content
                .iter()
                .find_map(|c| c.as_text().map(|t| t.text.clone()))
                .unwrap_or_else(|| "Unknown tool error".to_string());
            return Err(anyhow!("Tool '{}' returned error: {}", tool_name, error_msg));
        }

        Ok(result)
    }

    /// List available Playwright MCP tools (for agentic nodes)
    pub async fn list_tools(&self) -> Result<Vec<rmcp::model::Tool>> {
        let result = self
            .client
            .list_tools(SESSION_ID, None, CancellationToken::new())
            .await
            .map_err(|e| anyhow!("Failed to list tools: {}", e))?;
        Ok(result.tools)
    }

    /// Call a tool by name with JSON arguments (public, for agentic nodes)
    pub async fn call_tool_raw(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<CallToolResult> {
        self.call_tool(tool_name, args).await
    }

    /// Extract text content from a tool result
    pub fn extract_text(result: &CallToolResult) -> String {
        result
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    // --- High-level browser methods ---

    /// Install the browser (must be called before first navigation)
    pub async fn install_browser(&self) -> Result<()> {
        info!("Installing browser via Playwright MCP...");
        self.call_tool("browser_install", serde_json::json!({}))
            .await?;
        info!("Browser installed successfully");
        Ok(())
    }

    /// Navigate to a URL
    pub async fn navigate(&self, url: &str) -> Result<String> {
        info!("Navigating to: {}", url);
        let result = self
            .call_tool(
                "browser_navigate",
                serde_json::json!({ "url": url }),
            )
            .await?;
        Ok(Self::extract_text(&result))
    }

    /// Take a snapshot of the current page (accessibility tree)
    pub async fn snapshot(&self) -> Result<String> {
        debug!("Taking page snapshot");
        let result = self
            .call_tool("browser_snapshot", serde_json::json!({}))
            .await?;
        Ok(Self::extract_text(&result))
    }

    /// Click an element by ref (accessibility tree reference)
    pub async fn click(&self, element_ref: &str) -> Result<String> {
        info!("Clicking element: {}", element_ref);
        let result = self
            .call_tool(
                "browser_click",
                serde_json::json!({ "element": element_ref, "ref": element_ref }),
            )
            .await?;
        Ok(Self::extract_text(&result))
    }

    /// Fill a text field (by accessibility reference)
    pub async fn fill(&self, element_ref: &str, value: &str) -> Result<String> {
        info!("Filling element {} with value", element_ref);
        let result = self
            .call_tool(
                "browser_type",
                serde_json::json!({
                    "element": element_ref,
                    "ref": element_ref,
                    "text": value
                }),
            )
            .await?;
        Ok(Self::extract_text(&result))
    }

    /// Take a screenshot and return base64-encoded image data
    pub async fn screenshot(&self) -> Result<Vec<u8>> {
        info!("Taking screenshot");
        let result = self
            .call_tool("browser_take_screenshot", serde_json::json!({}))
            .await?;

        // Screenshot result contains base64-encoded image in content
        for content in &result.content {
            if let Some(image) = content.as_image() {
                let decoded = base64::Engine::decode(
                    &base64::engine::general_purpose::STANDARD,
                    &image.data,
                )
                .map_err(|e| anyhow!("Failed to decode screenshot: {}", e))?;
                return Ok(decoded);
            }
        }

        // If no image content, try to extract raw data
        Err(anyhow!("Screenshot returned no image content"))
    }

    /// Save a screenshot to a file
    pub async fn save_screenshot(&self, path: &std::path::Path) -> Result<()> {
        let data = self.screenshot().await?;
        std::fs::write(path, &data)?;
        info!("Screenshot saved to: {}", path.display());
        Ok(())
    }

    /// Get visible text on the page
    pub async fn get_visible_text(&self) -> Result<String> {
        let result = self
            .call_tool("browser_snapshot", serde_json::json!({}))
            .await?;
        Ok(Self::extract_text(&result))
    }

    /// Press a keyboard key
    pub async fn press_key(&self, key: &str) -> Result<String> {
        let result = self
            .call_tool(
                "browser_press_key",
                serde_json::json!({ "key": key }),
            )
            .await?;
        Ok(Self::extract_text(&result))
    }

    /// Wait by taking repeated snapshots until an element appears
    /// (Playwright MCP doesn't have an explicit wait, we poll)
    pub async fn wait_for_text(
        &self,
        text: &str,
        timeout_ms: u64,
    ) -> Result<bool> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_millis(timeout_ms);

        loop {
            let snapshot = self.snapshot().await?;
            if snapshot.contains(text) {
                return Ok(true);
            }

            if start.elapsed() > timeout {
                return Ok(false);
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    pub fn is_headless(&self) -> bool {
        self.headless
    }
}
