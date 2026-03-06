use anyhow::{anyhow, Result};
use tracing::info;

use crate::blueprint::nodes::DeterministicAction;
use crate::config::AppConfig;
use crate::playwright::PlaywrightClient;
use super::agentic::{
    find_input_ref_ctx, find_nth_textbox, find_button_ref_ctx, find_button_ref_prioritized,
    find_link_ref, truncate,
};

/// Executes deterministic actions using real Playwright browser automation.
/// No LLM involved — actions call the Playwright MCP server directly.
pub struct DeterministicExecutor {
    app_config: AppConfig,
    playwright: Option<PlaywrightClient>,
    report_dir: std::path::PathBuf,
    step_counter: u32,
    headed: bool,
}

impl DeterministicExecutor {
    pub fn new(app_config: AppConfig, headed: bool) -> Self {
        Self {
            app_config,
            playwright: None,
            report_dir: std::path::PathBuf::from("reports"),
            step_counter: 0,
            headed,
        }
    }

    pub fn set_report_dir(&mut self, dir: std::path::PathBuf) {
        self.report_dir = dir;
    }

    pub fn playwright(&self) -> Option<&PlaywrightClient> {
        self.playwright.as_ref()
    }

    /// Execute a deterministic action
    pub async fn execute(&mut self, action: &DeterministicAction) -> Result<String> {
        match action {
            DeterministicAction::LaunchBrowser => self.launch_browser().await,
            DeterministicAction::Navigate { url_template } => {
                self.navigate(url_template).await
            }
            DeterministicAction::Login => self.login().await,
            DeterministicAction::FillForm { fields } => self.fill_form(fields).await,
            DeterministicAction::Click { selector } => self.click(selector).await,
            DeterministicAction::WaitFor {
                selector,
                timeout_ms,
            } => self.wait_for(selector, *timeout_ms).await,
            DeterministicAction::Screenshot { name } => self.screenshot(name).await,
            DeterministicAction::GenerateReport => {
                Ok("Report generation delegated to engine".to_string())
            }
        }
    }

    async fn pw(&self) -> Result<&PlaywrightClient> {
        self.playwright
            .as_ref()
            .ok_or_else(|| anyhow!("Browser not launched — call LaunchBrowser first"))
    }

    async fn launch_browser(&mut self) -> Result<String> {
        info!(
            "Launching Playwright browser for: {}",
            self.app_config.target.name
        );

        // Create reports directory
        std::fs::create_dir_all(&self.report_dir)?;

        let headless = !self.headed;
        let client = PlaywrightClient::start(headless).await?;

        // Install browser if needed
        match client.install_browser().await {
            Ok(_) => info!("Browser installed"),
            Err(e) => info!("Browser install note: {} (may already be installed)", e),
        }

        self.playwright = Some(client);

        Ok("Browser launched via Playwright MCP".to_string())
    }

    async fn navigate(&self, url_template: &str) -> Result<String> {
        let pw = self.pw().await?;

        // Resolve template variables
        let url = url_template
            .replace("{{base_url}}", &self.app_config.target.base_url)
            .replace(
                "{{login_path}}",
                self.app_config
                    .auth
                    .login_url
                    .as_deref()
                    .unwrap_or(""),
            );

        let result = pw.navigate(&url).await?;
        Ok(format!("Navigated to: {} — {}", url, truncate(&result, 200)))
    }

    async fn login(&mut self) -> Result<String> {
        if !self.app_config.requires_auth() {
            return Ok("No auth required, skipping login".to_string());
        }

        let login_url = self.app_config.login_url().to_string();
        info!("Performing login at: {}", login_url);

        // Step 1: Navigate to login page
        {
            let pw = self.pw().await?;
            pw.navigate(&login_url).await?;
        }

        // Step 2: Wait for page to load and take snapshot
        let pw = self.pw().await?;
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let snapshot = pw.snapshot().await?;
        info!("Login page loaded, snapshot length: {} chars", snapshot.len());

        // Step 3: Dismiss cookie consent banner if present
        // Try multiple common cookie consent patterns (German + English)
        let cookie_dismiss_keywords = [
            "ablehnen", "reject", "decline", "nur erforderliche",
            "alle akzeptieren", "accept all", "akzeptieren", "zustimmen",
            "einverstanden", "alle cookies", "accept cookies",
        ];
        if let Some(cookie_ref) = find_button_ref_ctx(&snapshot, &cookie_dismiss_keywords) {
            info!("Dismissing cookie consent banner (ref={})", cookie_ref);
            pw.click(&cookie_ref).await?;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        } else if let Some(cookie_ref) = find_link_ref(&snapshot, &cookie_dismiss_keywords) {
            info!("Dismissing cookie consent via link (ref={})", cookie_ref);
            pw.click(&cookie_ref).await?;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        // Step 4: Re-snapshot after cookie dismissal
        let mut snapshot = pw.snapshot().await?;

        // Step 4b: If no textbox found yet, try clicking "Zum Hauptinhalt" to scroll to form
        if find_nth_textbox(&snapshot, 0).is_none() {
            info!("No textbox found in initial snapshot, trying to navigate to main content");
            if let Some(main_ref) = find_button_ref_ctx(&snapshot, &["hauptinhalt", "main content"]) {
                info!("Clicking 'Zum Hauptinhalt' (ref={})", main_ref);
                pw.click(&main_ref).await?;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                snapshot = pw.snapshot().await?;
            }
        }

        // Step 4c: If still no textbox, wait longer and retry (JS might be slow)
        if find_nth_textbox(&snapshot, 0).is_none() {
            info!("Still no textbox found, waiting 5 more seconds for JS to render");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            snapshot = pw.snapshot().await?;
        }

        // Step 5: Find and fill username field
        if let Some(value) = &self.app_config.auth.username {
            info!("Filling username: {}", value);
            // In the Playwright MCP accessibility tree, the label for an input
            // is often on a neighboring line, not the same line as "textbox".
            // We use context-aware matching: find textbox, check nearby lines for keywords.
            let username_keywords = ["anmeldename", "user", "login", "username", "benutzername"];
            if let Some(username_ref) = find_input_ref_ctx(&snapshot, &username_keywords) {
                pw.fill(&username_ref, value).await?;
            } else {
                // Fallback: use the first textbox found (common for simple login forms)
                if let Some(first_ref) = find_nth_textbox(&snapshot, 0) {
                    info!("Using first textbox as username field (ref={})", first_ref);
                    pw.fill(&first_ref, value).await?;
                } else {
                    return Err(anyhow!(
                        "Could not find username field. Snapshot excerpt: {}",
                        truncate(&snapshot, 500)
                    ));
                }
            }
        }

        // Step 6: Fill password field
        if let Some(value) = &self.app_config.auth.password {
            info!("Filling password");
            // Re-snapshot after filling username (DOM may have changed)
            let snapshot = pw.snapshot().await?;
            let password_keywords = ["pin", "password", "passwort", "kennwort", "online-banking-pin"];
            if let Some(password_ref) = find_input_ref_ctx(&snapshot, &password_keywords) {
                pw.fill(&password_ref, value).await?;
            } else {
                // Fallback: use the second textbox (password is usually the 2nd input)
                if let Some(second_ref) = find_nth_textbox(&snapshot, 1) {
                    info!("Using second textbox as password field (ref={})", second_ref);
                    pw.fill(&second_ref, value).await?;
                } else {
                    return Err(anyhow!(
                        "Could not find password field. Snapshot excerpt: {}",
                        truncate(&snapshot, 500)
                    ));
                }
            }
        }

        // Step 7: Click submit button
        // Search in priority order: specific labels first, generic "submit" last.
        // Hidden <input type="submit"> buttons often match "submit" but are aria-hidden.
        let snapshot = pw.snapshot().await?;
        let submit_ref = find_button_ref_prioritized(&snapshot, &[
            &["anmelden", "einloggen", "sign in", "log in"],  // high priority: visible labels
            &["login"],                                         // medium priority
        ]);
        if let Some(ref_val) = submit_ref {
            info!("Clicking login button (ref={})", ref_val);
            pw.click(&ref_val).await?;
        } else {
            info!("No named submit button found, pressing Enter");
            pw.press_key("Enter").await?;
        }

        // Step 8: Wait for dashboard to load
        info!("Waiting for page to load after login...");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        // End the pw borrow so we can mutate self.step_counter
        let _ = pw;

        // Step 9: Take a screenshot of the result
        self.step_counter += 1;
        let screenshot_path = self
            .report_dir
            .join(format!("{:02}_after_login.png", self.step_counter));
        {
            let pw = self.pw().await?;
            match pw.save_screenshot(&screenshot_path).await {
                Ok(_) => info!("Post-login screenshot saved: {}", screenshot_path.display()),
                Err(e) => info!("Screenshot failed (non-fatal): {}", e),
            }
        }

        // Step 10: Verify login success by checking snapshot
        let pw = self.pw().await?;
        let post_login_snapshot = pw.snapshot().await?;
        info!(
            "Post-login page snapshot: {} chars",
            post_login_snapshot.len()
        );

        // Check if login actually succeeded by verifying URL changed
        let login_url_lower = self.app_config.login_url().to_lowercase();
        let still_on_login = post_login_snapshot
            .lines()
            .any(|line| {
                let lower = line.to_lowercase();
                lower.contains("page url:") && lower.contains(&login_url_lower.split('?').next().unwrap_or(""))
            });

        if still_on_login {
            // Check for error indicators
            let lower_snap = post_login_snapshot.to_lowercase();
            if lower_snap.contains("fehler") || lower_snap.contains("error")
                || lower_snap.contains("hinweis")
            {
                return Err(anyhow!(
                    "Login failed — still on login page with errors. Snapshot: {}",
                    truncate(&post_login_snapshot, 500)
                ));
            }
        }

        Ok(format!(
            "Login completed. Post-login snapshot: {}",
            truncate(&post_login_snapshot, 300)
        ))
    }

    async fn fill_form(
        &self,
        fields: &[crate::blueprint::nodes::FormField],
    ) -> Result<String> {
        let pw = self.pw().await?;
        for field in fields {
            info!("Filling field {} = {}", field.selector, field.value);
            pw.fill(&field.selector, &field.value).await?;
        }
        Ok(format!("Filled {} fields", fields.len()))
    }

    async fn click(&self, selector: &str) -> Result<String> {
        let pw = self.pw().await?;
        let result = pw.click(selector).await?;
        Ok(format!("Clicked: {} — {}", selector, truncate(&result, 100)))
    }

    async fn wait_for(&self, text: &str, timeout_ms: u64) -> Result<String> {
        let pw = self.pw().await?;
        let found = pw.wait_for_text(text, timeout_ms).await?;
        if found {
            Ok(format!("Found '{}' on page", text))
        } else {
            Err(anyhow!(
                "Timeout ({}ms) waiting for '{}' on page",
                timeout_ms,
                text
            ))
        }
    }

    async fn screenshot(&mut self, name: &str) -> Result<String> {
        self.step_counter += 1;
        let path = self
            .report_dir
            .join(format!("{:02}_{}.png", self.step_counter, name));

        let pw = self.pw().await?;
        match pw.save_screenshot(&path).await {
            Ok(_) => Ok(format!("Screenshot saved: {}", path.display())),
            Err(e) => {
                // Non-fatal — log but don't fail the step
                info!("Screenshot failed (non-fatal): {}", e);
                Ok(format!("Screenshot skipped: {}", e))
            }
        }
    }
}

