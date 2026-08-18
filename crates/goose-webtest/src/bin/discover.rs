//! Discovery tool: step-by-step login flow with snapshots at each stage.

use anyhow::Result;
use goose_webtest::playwright::PlaywrightClient;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let url = args.get(1).map(|s| s.as_str()).unwrap_or(
        "https://www.sparkasse-hannover.de/de/home/login-online-banking/demo-online-banking-chiptan.html?n=true&stref=search&q=Demokonto"
    );

    eprintln!("=== LOGIN FLOW DISCOVERY ===\n");

    let pw = PlaywrightClient::start(true).await?;
    match pw.install_browser().await {
        Ok(_) => {}
        Err(_) => {}
    }

    // Step 1: Navigate
    eprintln!("[1] Navigating to login page...");
    pw.navigate(url).await?;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let snap = pw.snapshot().await?;

    // Check cookie banner
    if let Some(r) = find_ref(&snap, "link", "ablehnen") {
        eprintln!("[1b] Dismissing cookie banner (ref={})...", r);
        pw.click(&r).await?;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    // Step 2: Snapshot login form
    let snap = pw.snapshot().await?;
    eprintln!("[2] Login form snapshot:");
    // Print only the form-relevant lines
    for line in snap.lines() {
        let lower = line.to_lowercase();
        if lower.contains("textbox") || lower.contains("button") || lower.contains("anmeld")
            || lower.contains("pin") || lower.contains("online-banking")
        {
            eprintln!("   {}", line.trim());
        }
    }

    // Step 3: Find username textbox
    let username_ref = find_ref(&snap, "textbox", "anmeldename")
        .or_else(|| find_nth_ref(&snap, "textbox", 0));
    eprintln!("\n[3] Username textbox ref: {:?}", username_ref);

    if let Some(ref uref) = username_ref {
        eprintln!("   Clearing and filling username...");
        // First clear existing text, then type
        pw.fill(uref, "chipDEMO").await?;
        eprintln!("   Username filled");
    }

    // Step 4: Find password textbox
    let snap = pw.snapshot().await?;
    let password_ref = find_ref(&snap, "textbox", "pin")
        .or_else(|| find_ref(&snap, "textbox", "online-banking"))
        .or_else(|| find_nth_ref(&snap, "textbox", 1));
    eprintln!("[4] Password textbox ref: {:?}", password_ref);

    if let Some(ref pref) = password_ref {
        eprintln!("   Filling password...");
        pw.fill(pref, "12345").await?;
        eprintln!("   Password filled");
    }

    // Step 5: Snapshot before submit
    let snap = pw.snapshot().await?;
    eprintln!("\n[5] Form state before submit:");
    for line in snap.lines() {
        let lower = line.to_lowercase();
        if lower.contains("textbox") || lower.contains("anmelden") && lower.contains("button") {
            eprintln!("   {}", line.trim());
        }
    }

    // Step 6: Find and click Anmelden button
    let anmelden_ref = find_ref(&snap, "button", "anmelden");
    eprintln!("\n[6] Anmelden button ref: {:?}", anmelden_ref);

    if let Some(ref aref) = anmelden_ref {
        eprintln!("   Clicking Anmelden...");
        pw.click(aref).await?;
    }

    // Step 7: Wait and check result
    eprintln!("\n[7] Waiting 5s for page to load after login...");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let snap = pw.snapshot().await?;
    eprintln!("[7] Post-login page URL and first 40 lines:");
    for (i, line) in snap.lines().enumerate() {
        if i > 40 {
            eprintln!("   ... ({} more lines)", snap.lines().count() - 40);
            break;
        }
        eprintln!("   {}", line);
    }

    // Screenshot
    pw.save_screenshot(&std::path::PathBuf::from("discover_login_result.png")).await?;
    eprintln!("\n[8] Screenshot saved: discover_login_result.png");

    // Check for error messages
    let lower_snap = snap.to_lowercase();
    if lower_snap.contains("fehler") || lower_snap.contains("error") || lower_snap.contains("hinweis") {
        eprintln!("\n!! WARNING: Error/hint indicators found in post-login page");
    }
    if lower_snap.contains("finanzstatus") || lower_snap.contains("kontostand") || lower_snap.contains("umsätze") {
        eprintln!("\n** SUCCESS: Dashboard indicators found!");
    }

    Ok(())
}

fn find_ref(snapshot: &str, element_type: &str, keyword: &str) -> Option<String> {
    for line in snapshot.lines() {
        let lower = line.to_lowercase();
        if lower.contains(element_type) && lower.contains(keyword) {
            if let Some(start) = line.find("[ref=") {
                let after = &line[start + 5..];
                if let Some(end) = after.find(']') {
                    return Some(after[..end].to_string());
                }
            }
        }
    }
    None
}

fn find_nth_ref(snapshot: &str, element_type: &str, n: usize) -> Option<String> {
    let mut count = 0;
    for line in snapshot.lines() {
        if line.to_lowercase().contains(element_type) {
            if count == n {
                if let Some(start) = line.find("[ref=") {
                    let after = &line[start + 5..];
                    if let Some(end) = after.find(']') {
                        return Some(after[..end].to_string());
                    }
                }
            }
            count += 1;
        }
    }
    None
}
