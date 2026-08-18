# goose-webtest

Fork of [block/goose](https://github.com/block/goose) with a web application testing agent built on the Goose AI framework.

## Quick Start

```bash
# Build
CARGO_HOME="$HOME/.cargo" RUSTUP_TOOLCHAIN=stable cargo build --features webtest -p goose-cli

# Auto-discover mode (no test spec needed)
./gwt run --target targets/sparkasse-demo.toml

# With test spec
./gwt run --target targets/sparkasse-demo.toml --spec test-specs/sparkasse-basic.md

# Bare URL (no auth)
./gwt run https://example.com

# Choose provider
./gwt run --provider claude --target targets/sparkasse-demo.toml
./gwt run --provider local --headed https://example.com

# Verbose logging
RUST_LOG=info ./gwt run --target targets/sparkasse-demo.toml
```

## Architecture

### Hybrid Deterministic + Agentic Pipeline

The engine executes a directed graph (petgraph) of nodes. Deterministic nodes run Rust code directly (login, navigation). Agentic nodes run a lightweight ReAct loop where an LLM uses Playwright MCP tools to interact with the browser.

```
BlueprintEngine (engine.rs)
  ├── DeterministicExecutor (deterministic.rs)
  │     └── PlaywrightClient (playwright.rs) — MCP subprocess
  └── execute_agentic_full (agentic.rs)
        ├── LLM Provider (goose providers)
        ├── Playwright tools (22 browser automation tools)
        └── Virtual tools (assert_visible, assert_text_contains, etc.)
```

### Context Passing

Agentic node outputs are stored in `context_history` and prepended to subsequent agentic node prompts:
```
=== Context from previous steps ===
### Discover Application Structure (node: discover)
{JSON app map...}
=== End previous context ===
{current node prompt}
```

### Virtual Tools

The agentic executor intercepts these tool calls locally instead of forwarding to Playwright:
- `assert_visible(text)` — check text in accessibility snapshot
- `assert_text_contains(selector, expected)` — check element contains text
- `assert_url_matches(pattern)` — check current URL
- `assert_element_exists(description, text)` — check element in snapshot
- `log_step(message, status)` — record test progress

Each assertion produces a structured `Assertion` record included in reports.

## Key Files

### Crate: `crates/goose-webtest/`
| File | Purpose |
|------|---------|
| `src/blueprint/engine.rs` | Graph executor, context passing, orchestration |
| `src/blueprint/nodes.rs` | Node types (Deterministic/Agentic), NodeResult with screenshots/assertions |
| `src/blueprint/transitions.rs` | OnSuccess/OnFailure/Always transition conditions |
| `src/steps/agentic.rs` | ReAct loop, virtual tool interception, evidence collection |
| `src/steps/deterministic.rs` | Login, navigation, form filling (no LLM) |
| `src/steps/virtual_tools.rs` | Virtual tool definitions for LLM tool list |
| `src/assertions.rs` | Assertion/AssertionType structs |
| `src/app_map.rs` | AppMap/PageInfo/FormInfo types for discovery |
| `src/playwright.rs` | PlaywrightClient MCP wrapper |
| `src/config/app_config.rs` | TOML target config parser |
| `src/config/test_spec.rs` | Markdown test spec parser |
| `src/report/mod.rs` | ReportCollector, TestReport, assertion summary |
| `src/report/html_report.rs` | Self-contained HTML with embedded screenshots, assertion tables |

### CLI: `crates/goose-cli/src/commands/webtest.rs`
- Handles `goose test` command
- Sets `WEBTEST_PROVIDER`/`WEBTEST_MODEL` env vars from `--provider`/`--model` flags
- Defaults to `auto-discover` blueprint when no `--spec` provided

### Blueprints
| Blueprint | Nodes | Use Case |
|-----------|-------|----------|
| `auto-discover.yaml` | launch→login→discover(20t)→generate_and_execute(30t)→report | No test spec needed |
| `explore-and-report.yaml` | launch→login→explore(15t)→execute_tests(25t)→report | With test spec |
| `login-test.yaml` | launch→login→screenshot→verify(5t)→report | Verify login only |
| `performance-audit.yaml` | launch→login→measure→navigate(20t)→responsiveness(10t)→report | Performance checks |

### Targets & Specs
- `targets/sparkasse-demo.toml` — Sparkasse demo online banking (German, form login)
- `test-specs/sparkasse-basic.md` — 5 basic test scenarios

## `gwt` Wrapper

Shell script at project root. Handles:
- Clean PATH (bypasses conda/pyenv)
- Provider shortcuts: `local`→`vllm_local`, `claude`→`anthropic`, `openrouter`/`or`→`openrouter`
- Bare URL detection: `./gwt run https://x.com` → `--url https://x.com`
- Default provider: `vllm_local`

## Provider Configuration

Priority order for LLM provider resolution:
1. `--provider`/`--model` CLI flags (sets `WEBTEST_PROVIDER`/`WEBTEST_MODEL`)
2. `GOOSE_PROVIDER`/`GOOSE_MODEL` env vars
3. `~/.config/goose/config.yaml`
4. Default: `anthropic` / `claude-sonnet-4.6`

## Adding a New Target

```toml
# targets/my-app.toml
[target]
name = "My Application"
base_url = "https://app.example.com"

[auth]
type = "form"
login_url = "https://app.example.com/login"
username = "testuser"
password = "testpass"

[timeouts]
page_load_ms = 15000
element_wait_ms = 5000
login_wait_ms = 10000
```

## Adding a Test Spec

```markdown
# My App - Basic Tests

- After login, check that the dashboard shows user name
- Navigate to settings and verify all form fields are present
- Check that the navigation menu has all expected sections
```

## Reports

Generated in `reports/` as JSON + HTML. The HTML report includes:
- Step timeline with pass/fail indicators
- Assertion tables (green/red rows per assertion)
- Embedded base64 screenshots
- Duration and summary stats

## Common Issues

### Build fails
```bash
# Ensure stable toolchain and correct cargo home
CARGO_HOME="$HOME/.cargo" RUSTUP_TOOLCHAIN=stable cargo build --features webtest -p goose-cli
```

### Provider errors
```
Failed to create provider 'vllm_local'...
```
Make sure the vllm-mlx server is running on port 8800, or use `--provider claude`.

### Login fails on German sites
The login handler knows German patterns (Anmeldename, PIN, Einloggen). If it still fails, check that the target TOML `login_url` goes directly to the login form, not a landing page.

### No output / seems stuck
Agentic nodes take time (LLM reasoning + browser automation). Use `RUST_LOG=info` to see progress. Each turn is logged.
