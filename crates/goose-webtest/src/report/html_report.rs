//! HTML report generator — produces a self-contained HTML file
//! with embedded screenshots, step timeline, assertion tables, and color-coded results.

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::Path;

use super::{NodeResult, OverallResult, TestReport};
use crate::blueprint::nodes::NodeStatus;

impl TestReport {
    /// Write a self-contained HTML report to a file.
    /// Screenshots referenced in step results are embedded as base64.
    pub fn write_html(&self, path: &Path, report_dir: &Path) -> Result<()> {
        let html = self.render_html(report_dir)?;
        std::fs::write(path, html)?;
        Ok(())
    }

    fn render_html(&self, report_dir: &Path) -> Result<String> {
        let mut steps_html = String::new();
        for (i, step) in self.steps.iter().enumerate() {
            steps_html.push_str(&self.render_step(i + 1, step, report_dir));
        }

        let result_class = match self.summary.result {
            OverallResult::Pass => "pass",
            OverallResult::Fail => "fail",
            OverallResult::Partial => "partial",
            OverallResult::Error => "error",
        };

        let result_label = match self.summary.result {
            OverallResult::Pass => "PASS",
            OverallResult::Fail => "FAIL",
            OverallResult::Partial => "PARTIAL",
            OverallResult::Error => "ERROR",
        };

        let pass_pct = if self.summary.total_steps > 0 {
            (self.summary.passed as f64 / self.summary.total_steps as f64 * 100.0) as u32
        } else {
            0
        };

        // Assertion summary card (only if assertions exist)
        let assertion_card = if self.summary.assertions_total > 0 {
            format!(
                r#"<div class="stat-card pass">
    <div class="value">{}</div>
    <div class="label">Assertions Passed</div>
  </div>
  <div class="stat-card fail">
    <div class="value">{}</div>
    <div class="label">Assertions Failed</div>
  </div>"#,
                self.summary.assertions_passed,
                self.summary.assertions_failed,
            )
        } else {
            String::new()
        };

        let html = format!(
            r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Test Report — {target_name}</title>
<style>
:root {{
  --pass: #22c55e;
  --fail: #ef4444;
  --partial: #f59e0b;
  --error: #dc2626;
  --bg: #0f172a;
  --surface: #1e293b;
  --surface2: #334155;
  --text: #f1f5f9;
  --text-dim: #94a3b8;
  --border: #475569;
}}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  background: var(--bg);
  color: var(--text);
  line-height: 1.6;
  padding: 2rem;
  max-width: 1200px;
  margin: 0 auto;
}}
.header {{
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  margin-bottom: 2rem;
  padding-bottom: 1.5rem;
  border-bottom: 1px solid var(--border);
}}
.header h1 {{
  font-size: 1.5rem;
  font-weight: 600;
}}
.header .meta {{
  font-size: 0.85rem;
  color: var(--text-dim);
  margin-top: 0.25rem;
}}
.badge {{
  display: inline-block;
  padding: 0.35rem 1rem;
  border-radius: 6px;
  font-weight: 700;
  font-size: 1rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}}
.badge.pass {{ background: var(--pass); color: #000; }}
.badge.fail {{ background: var(--fail); color: #fff; }}
.badge.partial {{ background: var(--partial); color: #000; }}
.badge.error {{ background: var(--error); color: #fff; }}
.stats {{
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 1rem;
  margin-bottom: 2rem;
}}
.stat-card {{
  background: var(--surface);
  border-radius: 8px;
  padding: 1rem 1.25rem;
  border: 1px solid var(--border);
}}
.stat-card .value {{
  font-size: 1.75rem;
  font-weight: 700;
}}
.stat-card .label {{
  font-size: 0.8rem;
  color: var(--text-dim);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}}
.stat-card.pass .value {{ color: var(--pass); }}
.stat-card.fail .value {{ color: var(--fail); }}
.progress-bar {{
  background: var(--surface2);
  border-radius: 4px;
  height: 8px;
  margin-bottom: 2rem;
  overflow: hidden;
}}
.progress-bar .fill {{
  height: 100%;
  border-radius: 4px;
  transition: width 0.3s;
}}
.progress-bar .fill.pass {{ background: var(--pass); }}
.progress-bar .fill.partial {{ background: var(--partial); }}
.progress-bar .fill.fail {{ background: var(--fail); }}
h2 {{
  font-size: 1.1rem;
  font-weight: 600;
  margin-bottom: 1rem;
  color: var(--text-dim);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}}
.step {{
  background: var(--surface);
  border-radius: 8px;
  border: 1px solid var(--border);
  margin-bottom: 0.75rem;
  overflow: hidden;
}}
.step-header {{
  display: flex;
  align-items: center;
  padding: 0.75rem 1rem;
  cursor: pointer;
  gap: 0.75rem;
}}
.step-header:hover {{
  background: var(--surface2);
}}
.step-num {{
  font-size: 0.75rem;
  font-weight: 700;
  color: var(--text-dim);
  width: 1.5rem;
  text-align: center;
}}
.step-status {{
  width: 10px;
  height: 10px;
  border-radius: 50%;
  flex-shrink: 0;
}}
.step-status.pass {{ background: var(--pass); }}
.step-status.fail {{ background: var(--fail); }}
.step-status.skip {{ background: var(--text-dim); }}
.step-status.error {{ background: var(--error); }}
.step-name {{
  flex: 1;
  font-weight: 500;
}}
.step-type {{
  font-size: 0.7rem;
  padding: 0.15rem 0.5rem;
  border-radius: 4px;
  background: var(--surface2);
  color: var(--text-dim);
  text-transform: uppercase;
}}
.step-duration {{
  font-size: 0.8rem;
  color: var(--text-dim);
  min-width: 4rem;
  text-align: right;
}}
.step-detail {{
  display: none;
  padding: 0 1rem 1rem;
  border-top: 1px solid var(--border);
}}
.step.open .step-detail {{
  display: block;
}}
.step-detail pre {{
  background: var(--bg);
  padding: 0.75rem;
  border-radius: 6px;
  font-size: 0.8rem;
  overflow-x: auto;
  white-space: pre-wrap;
  word-break: break-word;
  margin-top: 0.5rem;
}}
.step-detail .error-msg {{
  color: var(--fail);
  margin-top: 0.5rem;
}}
.screenshot {{
  margin-top: 0.75rem;
  border-radius: 6px;
  overflow: hidden;
  border: 1px solid var(--border);
}}
.screenshot img {{
  width: 100%;
  display: block;
}}
.screenshot .caption {{
  padding: 0.4rem 0.75rem;
  font-size: 0.75rem;
  color: var(--text-dim);
  background: var(--bg);
}}
.assertions-table {{
  margin-top: 0.75rem;
  width: 100%;
  border-collapse: collapse;
  font-size: 0.8rem;
}}
.assertions-table th {{
  text-align: left;
  padding: 0.4rem 0.6rem;
  background: var(--bg);
  color: var(--text-dim);
  font-weight: 600;
  text-transform: uppercase;
  font-size: 0.7rem;
  letter-spacing: 0.03em;
}}
.assertions-table td {{
  padding: 0.4rem 0.6rem;
  border-top: 1px solid var(--border);
}}
.assertions-table tr.pass td {{ border-left: 3px solid var(--pass); }}
.assertions-table tr.fail td {{ border-left: 3px solid var(--fail); }}
.assert-badge {{
  display: inline-block;
  padding: 0.1rem 0.4rem;
  border-radius: 3px;
  font-weight: 700;
  font-size: 0.65rem;
  text-transform: uppercase;
}}
.assert-badge.pass {{ background: var(--pass); color: #000; }}
.assert-badge.fail {{ background: var(--fail); color: #fff; }}
.footer {{
  margin-top: 2rem;
  padding-top: 1rem;
  border-top: 1px solid var(--border);
  font-size: 0.75rem;
  color: var(--text-dim);
  text-align: center;
}}
</style>
</head>
<body>

<div class="header">
  <div>
    <h1>{target_name}</h1>
    <div class="meta">
      Blueprint: {blueprint_name}
      {test_spec_line}
      <br>Report ID: {report_id}
      <br>{timestamp} &middot; {duration:.1}s
    </div>
  </div>
  <span class="badge {result_class}">{result_label}</span>
</div>

<div class="stats">
  <div class="stat-card">
    <div class="value">{total}</div>
    <div class="label">Total Steps</div>
  </div>
  <div class="stat-card pass">
    <div class="value">{passed}</div>
    <div class="label">Passed</div>
  </div>
  <div class="stat-card fail">
    <div class="value">{failed}</div>
    <div class="label">Failed</div>
  </div>
  <div class="stat-card">
    <div class="value">{duration:.1}s</div>
    <div class="label">Duration</div>
  </div>
  {assertion_card}
</div>

<div class="progress-bar">
  <div class="fill {result_class}" style="width: {pass_pct}%"></div>
</div>

<h2>Step Timeline</h2>
{steps_html}

<div class="footer">
  Generated by goose-webtest v{version} &middot; {timestamp}
</div>

<script>
document.querySelectorAll('.step-header').forEach(h => {{
  h.addEventListener('click', () => {{
    h.closest('.step').classList.toggle('open');
  }});
}});
// Auto-open failed steps
document.querySelectorAll('.step').forEach(s => {{
  if (s.querySelector('.step-status.fail') || s.querySelector('.step-status.error')) {{
    s.classList.add('open');
  }}
}});
</script>
</body>
</html>"##,
            target_name = html_escape(&self.metadata.target_name),
            blueprint_name = html_escape(&self.metadata.blueprint_name),
            test_spec_line = self.metadata.test_spec.as_ref()
                .map(|s| format!("<br>Test Spec: {}", html_escape(s)))
                .unwrap_or_default(),
            report_id = &self.metadata.report_id[..8],
            timestamp = self.metadata.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
            duration = self.metadata.duration_seconds,
            result_class = result_class,
            result_label = result_label,
            total = self.summary.total_steps,
            passed = self.summary.passed,
            failed = self.summary.failed,
            pass_pct = pass_pct,
            assertion_card = assertion_card,
            steps_html = steps_html,
            version = self.metadata.agent_version,
        );

        Ok(html)
    }

    fn render_step(&self, num: usize, step: &NodeResult, report_dir: &Path) -> String {
        let status_class = match step.status {
            NodeStatus::Pass => "pass",
            NodeStatus::Fail => "fail",
            NodeStatus::Skip => "skip",
            NodeStatus::Error => "error",
        };

        let duration_str = if step.duration_ms >= 1000 {
            format!("{:.1}s", step.duration_ms as f64 / 1000.0)
        } else {
            format!("{}ms", step.duration_ms)
        };

        let node_type = format!("{:?}", step.node_type);

        let mut detail_html = String::new();

        if let Some(ref detail) = step.detail {
            detail_html.push_str(&format!(
                "<pre>{}</pre>",
                html_escape(&truncate_str(detail, 2000))
            ));
        }

        if let Some(ref error) = step.error {
            detail_html.push_str(&format!(
                "<div class=\"error-msg\"><strong>Error:</strong><pre>{}</pre></div>",
                html_escape(&truncate_str(error, 2000))
            ));
        }

        // Render assertion table if any
        if !step.assertions.is_empty() {
            let mut table = String::from(
                r#"<table class="assertions-table">
<tr><th>Status</th><th>Type</th><th>Target</th><th>Expected</th><th>Actual</th></tr>
"#,
            );
            for assertion in &step.assertions {
                let row_class = if assertion.passed { "pass" } else { "fail" };
                let badge_class = if assertion.passed { "pass" } else { "fail" };
                let badge_label = if assertion.passed { "PASS" } else { "FAIL" };
                table.push_str(&format!(
                    r#"<tr class="{row_class}"><td><span class="assert-badge {badge_class}">{badge_label}</span></td><td>{atype:?}</td><td>{target}</td><td>{expected}</td><td>{actual}</td></tr>
"#,
                    row_class = row_class,
                    badge_class = badge_class,
                    badge_label = badge_label,
                    atype = assertion.assertion_type,
                    target = html_escape(&assertion.target),
                    expected = html_escape(&assertion.expected),
                    actual = html_escape(&assertion.actual),
                ));
            }
            table.push_str("</table>");
            detail_html.push_str(&table);
        }

        // Embed screenshot from screenshot_path field
        if let Some(ref screenshot_path) = step.screenshot_path {
            if let Some(img_html) = embed_screenshot(Path::new(screenshot_path)) {
                detail_html.push_str(&img_html);
            }
        }

        // Embed all collected screenshots from the screenshots array
        for screenshot_path in &step.screenshots {
            if let Some(img_html) = embed_screenshot(Path::new(screenshot_path)) {
                detail_html.push_str(&img_html);
            }
        }

        // Also check for screenshots in the report directory matching this step
        let patterns = [
            format!("{:02}_", num),
            format!("{}_", step.node_id),
        ];
        if let Ok(entries) = std::fs::read_dir(report_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".png") && patterns.iter().any(|p| name.starts_with(p)) {
                    // Skip if already embedded from screenshots array
                    let path_str = entry.path().to_string_lossy().to_string();
                    if !step.screenshots.contains(&path_str) {
                        if let Some(img_html) = embed_screenshot(&entry.path()) {
                            detail_html.push_str(&img_html);
                        }
                    }
                }
            }
        }

        format!(
            r#"<div class="step">
  <div class="step-header">
    <span class="step-num">{num}</span>
    <span class="step-status {status_class}"></span>
    <span class="step-name">{name}</span>
    <span class="step-type">{node_type}</span>
    <span class="step-duration">{duration}</span>
  </div>
  <div class="step-detail">
    {detail}
  </div>
</div>
"#,
            num = num,
            status_class = status_class,
            name = html_escape(&step.node_name),
            node_type = node_type,
            duration = duration_str,
            detail = detail_html,
        )
    }
}

/// Embed a screenshot as base64 in an HTML img tag
fn embed_screenshot(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let b64 = STANDARD.encode(&data);
    let filename = path.file_name()?.to_string_lossy();
    Some(format!(
        r#"<div class="screenshot">
  <img src="data:image/png;base64,{b64}" alt="{filename}">
  <div class="caption">{filename}</div>
</div>"#,
        b64 = b64,
        filename = html_escape(&filename),
    ))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}... [truncated]", &s[..max])
    }
}
