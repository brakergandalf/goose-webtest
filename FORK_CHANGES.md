# Fork Changes from upstream block/goose

This file tracks every modification made to upstream Goose files.
The `crates/goose-webtest/` crate is entirely new and won't conflict with upstream.

## Modified Files

### `crates/goose-cli/Cargo.toml`
- Added `goose-webtest` optional dependency
- Added `webtest` feature flag (included in default features)

### `crates/goose-cli/src/cli.rs`
- Added `Command::Test` variant (behind `#[cfg(feature = "webtest")]`)
- Added `--provider` and `--model` CLI flags for LLM provider selection
- Added command name mapping for "webtest"
- Added dispatch handler for `Command::Test` with provider/model forwarding

### `crates/goose-cli/src/commands/mod.rs`
- Added `pub mod webtest;` (behind `#[cfg(feature = "webtest")]`)

## New Files

### `crates/goose-cli/src/commands/webtest.rs`
- Test command handler: loads config, blueprint, runs engine, generates report
- Sets WEBTEST_PROVIDER/WEBTEST_MODEL env vars from CLI flags
- Defaults to auto-discover blueprint when no test spec is provided

### `crates/goose-webtest/` (entire new crate)
- `src/lib.rs` — Crate root (exports assertions, app_map modules)
- `src/app_map.rs` — AppMap, PageInfo, FormInfo, FieldInfo types for discovery output
- `src/assertions.rs` — Structured assertion framework (Assertion, AssertionType)
- `src/blueprint/mod.rs` — Blueprint types and YAML parser
- `src/blueprint/engine.rs` — Directed graph executor with context passing between nodes
- `src/blueprint/nodes.rs` — Node types with screenshots/assertions on NodeResult
- `src/blueprint/transitions.rs` — Transition conditions
- `src/config/mod.rs` — Config re-exports
- `src/config/app_config.rs` — TOML app config parser
- `src/config/test_spec.rs` — Markdown test spec parser
- `src/steps/mod.rs` — Step executors
- `src/steps/agentic.rs` — ReAct loop with virtual tool interception, screenshot/assertion collection
- `src/steps/deterministic.rs` — Deterministic action executor
- `src/steps/virtual_tools.rs` — Virtual tool definitions (assert_visible, assert_text_contains, etc.)
- `src/report/mod.rs` — Report collector with assertion summary
- `src/report/html_report.rs` — HTML report with assertion tables and multi-screenshot support

### `targets/sparkasse-demo.toml`
- First target app configuration

### `test-specs/sparkasse-basic.md`
- First natural language test specification

### `blueprints/login-test.yaml`
- Login-only blueprint

### `blueprints/explore-and-report.yaml`
- Full explore + test blueprint

### `blueprints/auto-discover.yaml`
- Autonomous discovery blueprint: discover app structure → generate + execute tests

### `gwt`
- Shell wrapper script with clean env, provider shortcuts, bare URL detection
