use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

use goose_webtest::blueprint::Blueprint;
use goose_webtest::config::app_config::AppConfig;
use goose_webtest::config::test_spec::TestSpec;
use goose_webtest::report::ReportCollector;
use goose_webtest::BlueprintEngine;

/// Output format for test reports
#[derive(Debug, Clone)]
pub enum ReportFormat {
    Json,
    Html,
    Both,
}

impl std::str::FromStr for ReportFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "json" => Ok(ReportFormat::Json),
            "html" => Ok(ReportFormat::Html),
            "both" => Ok(ReportFormat::Both),
            _ => Err(format!("Unknown format: {}. Expected: json, html, both", s)),
        }
    }
}

/// Handle the `goose test` command
pub async fn handle_test(
    target: Option<PathBuf>,
    url: Option<String>,
    spec: Option<PathBuf>,
    blueprint_name: Option<String>,
    format: String,
    output_dir: PathBuf,
    login_only: bool,
    ci: bool,
    headed: bool,
    provider: Option<String>,
    model: Option<String>,
) -> Result<()> {
    // Set provider/model env vars so agentic executor picks them up
    if let Some(ref p) = provider {
        std::env::set_var("WEBTEST_PROVIDER", p);
        info!("Provider override: {}", p);
    }
    if let Some(ref m) = model {
        std::env::set_var("WEBTEST_MODEL", m);
        info!("Model override: {}", m);
    }

    // Load app config
    let app_config = match (&target, &url) {
        (Some(path), _) => {
            info!("Loading target config from: {}", path.display());
            AppConfig::from_file(path)?
        }
        (_, Some(url)) => {
            info!("Using direct URL: {}", url);
            AppConfig::from_url(url)
        }
        _ => {
            anyhow::bail!("Either --target or --url must be specified");
        }
    };

    println!("🧪 goose-webtest v{}", env!("CARGO_PKG_VERSION"));
    println!("   Target: {}", app_config.target.name);
    println!("   URL: {}", app_config.target.base_url);
    if app_config.requires_auth() {
        println!("   Auth: form-based login");
    }
    println!();

    // Load test spec if provided
    let test_spec = if let Some(spec_path) = &spec {
        let ts = TestSpec::from_file(spec_path)?;
        println!("   Test spec: {} ({} scenarios)", ts.title, ts.scenarios.len());
        for (i, s) in ts.scenarios.iter().enumerate() {
            println!("     {}. {}", i + 1, s.description);
        }
        println!();
        Some(ts)
    } else {
        None
    };

    // Select blueprint
    let blueprint_path = if let Some(name) = &blueprint_name {
        // Look for blueprint in standard locations
        let paths = vec![
            PathBuf::from(format!("blueprints/{}.yaml", name)),
            PathBuf::from(format!("blueprints/{}.yml", name)),
        ];
        paths
            .into_iter()
            .find(|p| p.exists())
            .ok_or_else(|| anyhow::anyhow!("Blueprint not found: {}", name))?
    } else if login_only {
        PathBuf::from("blueprints/login-test.yaml")
    } else if spec.is_none() {
        // No test spec → use auto-discover (autonomous mode)
        info!("No test spec provided — using auto-discover blueprint");
        PathBuf::from("blueprints/auto-discover.yaml")
    } else {
        PathBuf::from("blueprints/explore-and-report.yaml")
    };

    let blueprint = Blueprint::from_file(&blueprint_path)?;
    println!("   Blueprint: {} ({})", blueprint.name, blueprint_path.display());
    println!("   Nodes: {} | Max retries: {} | Timeout: {}s",
        blueprint.nodes.len(),
        blueprint.settings.max_retries,
        blueprint.settings.timeout_seconds
    );
    println!();

    // Create report collector
    let mut report = ReportCollector::new(&app_config.target.name, &blueprint.name);
    if let Some(ref ts) = test_spec {
        report.test_spec = Some(ts.title.clone());
    }

    // Create and run blueprint engine
    let mut engine = BlueprintEngine::with_options(blueprint, app_config, headed)?;
    engine.set_report_dir(output_dir.clone());
    if let Some(ref ts) = test_spec {
        engine.set_test_spec(ts.clone());
    }
    println!("▶ Executing blueprint...\n");
    engine.execute(&mut report).await?;

    // Generate report
    let test_report = report.finalize();

    // Create output directory
    std::fs::create_dir_all(&output_dir)?;

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let report_format: ReportFormat = format.parse().unwrap_or(ReportFormat::Both);

    // Write JSON report
    match report_format {
        ReportFormat::Json | ReportFormat::Both => {
            let json_path = output_dir.join(format!("report_{}.json", timestamp));
            test_report.write_json(&json_path)?;
            println!("📄 JSON report: {}", json_path.display());
        }
        _ => {}
    }

    // Write HTML report
    match report_format {
        ReportFormat::Html | ReportFormat::Both => {
            let html_path = output_dir.join(format!("report_{}.html", timestamp));
            test_report.write_html(&html_path, &output_dir)?;
            println!("🌐 HTML report: {}", html_path.display());
        }
        _ => {}
    }

    // Print summary
    println!();
    println!("══════════════════════════════════════");
    println!("  RESULTS: {:?}", test_report.summary.result);
    println!("  Steps: {} total | {} passed | {} failed | {} errors",
        test_report.summary.total_steps,
        test_report.summary.passed,
        test_report.summary.failed,
        test_report.summary.errors
    );
    println!("  Duration: {:.1}s", test_report.metadata.duration_seconds);
    println!("══════════════════════════════════════");

    // CI mode: exit with appropriate code
    if ci {
        match test_report.summary.result {
            goose_webtest::report::OverallResult::Pass => std::process::exit(0),
            goose_webtest::report::OverallResult::Fail => std::process::exit(1),
            goose_webtest::report::OverallResult::Partial => std::process::exit(1),
            goose_webtest::report::OverallResult::Error => std::process::exit(2),
        }
    }

    Ok(())
}
