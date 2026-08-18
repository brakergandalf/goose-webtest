#!/usr/bin/env python3
"""
autoevolve.py — Autonomous improvement loop for goose-webtest.

Inspired by Karpathy's autoresearch pattern:
  1. Read INSTRUCTIONS.md + current code/configs
  2. LLM generates a hypothesis + code changes
  3. Apply changes, rebuild if needed
  4. Run gwt test with timeout
  5. Measure metric from JSON report
  6. If improved: git commit on feature branch
  7. If worse: revert changes
  8. Loop until deadline or max iterations

Usage:
    python3 autoevolve.py                              # defaults
    python3 autoevolve.py --max-iter 50 --timeout 180  # custom limits
    python3 autoevolve.py --dry-run                    # hypotheses only
    python3 autoevolve.py --provider local              # use local LLM for tests
"""

import importlib
import json
import os
import shutil
import signal
import subprocess
import sys
import time
from datetime import datetime, timedelta
from pathlib import Path

# ── Bootstrap: ensure anthropic SDK is available ─────────────────────
SCRIPT_DIR = Path(__file__).parent.resolve()
VENV_DIR = SCRIPT_DIR / ".venv-autoevolve"


def _ensure_venv():
    """Create venv + install deps if needed. Re-exec under venv python."""
    venv_python = VENV_DIR / "bin" / "python3"

    # Already running inside our venv (check sys.prefix, not sys.executable)
    if Path(sys.prefix).resolve() == VENV_DIR.resolve():
        return

    if not venv_python.exists():
        print("Setting up autoevolve venv...")
        uv = shutil.which("uv")
        if uv:
            subprocess.check_call([uv, "venv", str(VENV_DIR), "--python", "3.12"])
            subprocess.check_call(
                [uv, "pip", "install", "anthropic", "--python", str(venv_python)]
            )
        else:
            subprocess.check_call([sys.executable, "-m", "venv", str(VENV_DIR)])
            pip = VENV_DIR / "bin" / "pip"
            subprocess.check_call([str(pip), "install", "anthropic"])
        print("Venv ready.\n")

    # Re-exec under venv python, preserving all args
    os.execv(str(venv_python), [str(venv_python)] + sys.argv)


_ensure_venv()

import argparse  # noqa: E402 — after venv bootstrap

import anthropic  # noqa: E402

# ── Constants ────────────────────────────────────────────────────────
PROJECT_ROOT = Path(__file__).parent.resolve()
INSTRUCTIONS_FILE = PROJECT_ROOT / "INSTRUCTIONS.md"
EXPERIMENT_LOG = PROJECT_ROOT / "experiments" / "experiment_log.jsonl"
GWT = PROJECT_ROOT / "gwt"

# Files the agent may modify (grouped by rebuild requirement)
CONFIG_FILES = {
    "blueprints": sorted((PROJECT_ROOT / "blueprints").glob("*.yaml")),
    "targets": sorted((PROJECT_ROOT / "targets").glob("*.toml")),
    "test_specs": sorted((PROJECT_ROOT / "test-specs").glob("*.md")),
}
RUST_FILES = [
    PROJECT_ROOT / "crates/goose-webtest/src/steps/agentic.rs",
    PROJECT_ROOT / "crates/goose-webtest/src/steps/virtual_tools.rs",
    PROJECT_ROOT / "crates/goose-webtest/src/steps/deterministic.rs",
    PROJECT_ROOT / "crates/goose-webtest/src/blueprint/engine.rs",
]

# Hypothesis generation model
HYPOTHESIS_MODEL = "claude-sonnet-4-6"

# ── Globals for signal handling ──────────────────────────────────────
_active_backups: list = []
_shutting_down = False


def _signal_handler(sig, frame):
    global _shutting_down
    if _shutting_down:
        sys.exit(1)
    _shutting_down = True
    print("\n\nInterrupted — reverting any pending changes...")
    revert_changes(_active_backups)
    sys.exit(130)


signal.signal(signal.SIGINT, _signal_handler)
signal.signal(signal.SIGTERM, _signal_handler)


# ── Helpers ──────────────────────────────────────────────────────────
def read_instructions() -> str:
    if INSTRUCTIONS_FILE.exists():
        return INSTRUCTIONS_FILE.read_text()
    return "No INSTRUCTIONS.md found. Improve test reliability and assertion pass rate."


def read_mutable_files(include_rust: bool = True) -> dict[str, str]:
    """Read all mutable files, return {relative_path: content}."""
    files = {}
    for paths in CONFIG_FILES.values():
        for p in paths:
            if p.exists():
                files[str(p.relative_to(PROJECT_ROOT))] = p.read_text()
    if include_rust:
        for p in RUST_FILES:
            if p.exists():
                files[str(p.relative_to(PROJECT_ROOT))] = p.read_text()
    return files


def extract_metric(report_path: Path) -> dict:
    """Extract the composite score from a JSON test report."""
    data = json.loads(report_path.read_text())
    s = data.get("summary", {})

    at = s.get("assertions_total", 0)
    ap = s.get("assertions_passed", 0)
    st = s.get("total_steps", 0)
    sp = s.get("passed", 0)
    dur = data.get("metadata", {}).get("duration_seconds", 0)

    assertion_rate = ap / max(at, 1)
    step_rate = sp / max(st, 1)
    pass_rate = 0.7 * assertion_rate + 0.3 * step_rate
    coverage = min(at / 20, 1.0)  # bonus for more assertions, caps at 20
    # Score: 70% pass rate + 30% coverage — rewards both quality AND breadth
    score = round(0.7 * pass_rate + 0.3 * coverage, 4)

    return {
        "score": score,
        "assertion_rate": assertion_rate,
        "step_rate": step_rate,
        "assertions_passed": ap,
        "assertions_total": at,
        "steps_passed": sp,
        "steps_total": st,
        "duration": dur,
        "overall_result": s.get("result", "unknown"),
    }


def find_latest_report(after: float = 0) -> Path | None:
    """Find the most recent JSON report, optionally created after a timestamp."""
    reports_dir = PROJECT_ROOT / "reports"
    if not reports_dir.exists():
        return None
    candidates = sorted(reports_dir.glob("report_*.json"), reverse=True)
    for c in candidates:
        if after and c.stat().st_mtime < after:
            continue
        return c
    return None


def load_history() -> list:
    if not EXPERIMENT_LOG.exists():
        return []
    entries = []
    for line in EXPERIMENT_LOG.read_text().strip().split("\n"):
        if line.strip():
            entries.append(json.loads(line))
    return entries


def append_history(entry: dict):
    EXPERIMENT_LOG.parent.mkdir(parents=True, exist_ok=True)
    with open(EXPERIMENT_LOG, "a") as f:
        f.write(json.dumps(entry) + "\n")


# ── Core functions ───────────────────────────────────────────────────
def generate_hypothesis(
    client: anthropic.Anthropic,
    instructions: str,
    mutable_files: dict[str, str],
    history: list,
    baseline: dict | None,
) -> dict:
    """Call Claude to generate one hypothesis with concrete file changes."""

    file_listing = ""
    for path, content in mutable_files.items():
        file_listing += f"\n--- {path} ---\n{content}\n"

    history_text = ""
    if history:
        for h in history[-15:]:
            tag = "KEPT" if h.get("kept") else "REVERTED"
            reason = h.get("reason", "")
            history_text += (
                f"- #{h['iteration']}: {h['hypothesis']} "
                f"-> score={h.get('score', 0):.3f} ({tag}, {reason})\n"
            )

    baseline_text = "No baseline yet — first run."
    if baseline:
        baseline_text = (
            f"Score: {baseline['score']:.3f}\n"
            f"Assertion pass rate: {baseline['assertion_rate']:.1%} "
            f"({baseline['assertions_passed']}/{baseline['assertions_total']})\n"
            f"Step pass rate: {baseline['step_rate']:.1%} "
            f"({baseline['steps_passed']}/{baseline['steps_total']})\n"
            f"Duration: {baseline['duration']:.1f}s\n"
            f"Overall: {baseline['overall_result']}"
        )

    prompt = f"""You are an autonomous agent improving goose-webtest, a web application testing system.

## Goal (from INSTRUCTIONS.md)
{instructions}

## Current Baseline
{baseline_text}

## Experiment History
{history_text if history_text else "No experiments yet."}

## Mutable Files
{file_listing}

## Scoring
score = 0.7 * (assertions_passed / assertions_total) + 0.3 * (steps_passed / steps_total)
Higher is better. Do NOT game the score by reducing total assertions.

## Task
1. Analyze current state and history (avoid repeating failed hypotheses).
2. Formulate ONE small, testable hypothesis.
3. Provide exact file changes.

## Response Format (strict JSON)
```json
{{
  "hypothesis": "One-line description",
  "reasoning": "Why this should improve the score",
  "changes": [
    {{
      "file": "relative/path",
      "action": "replace",
      "old": "exact text to find (including whitespace)",
      "new": "replacement text"
    }}
  ],
  "requires_rebuild": false
}}
```

Rules:
- ONE focused change per iteration. Small diffs.
- "old" must match EXACTLY (the script uses str.replace).
- Set requires_rebuild: true ONLY for .rs file changes.
- Do NOT change: login credentials, target URLs, autoevolve.py, scoring logic.
- Do NOT reduce assertion count to inflate the rate.
- Prefer YAML/TOML changes (fast) over Rust changes (rebuild ~60s).
- Focus on: prompt clarity, turn budgets, timeout tuning, assertion strategies."""

    resp = client.messages.create(
        model=HYPOTHESIS_MODEL,
        max_tokens=4096,
        messages=[{"role": "user", "content": prompt}],
    )

    text = resp.content[0].text
    # Extract JSON from markdown code block or raw
    if "```json" in text:
        json_str = text.split("```json", 1)[1].split("```", 1)[0].strip()
    elif "```" in text:
        json_str = text.split("```", 1)[1].split("```", 1)[0].strip()
    else:
        json_str = text.strip()

    return json.loads(json_str)


def apply_changes(changes: list) -> list[tuple[Path, str]]:
    """Apply file changes. Returns backups [(path, original)] for rollback."""
    global _active_backups
    backups = []
    for ch in changes:
        path = PROJECT_ROOT / ch["file"]
        if not path.exists():
            print(f"  SKIP: {ch['file']} not found")
            continue

        original = path.read_text()
        if ch["action"] == "replace":
            if ch["old"] not in original:
                print(f"  SKIP: old text not found in {ch['file']}")
                # Show first 60 chars for debugging
                print(f"        looking for: {ch['old'][:60]}...")
                continue
            new_content = original.replace(ch["old"], ch["new"], 1)
            path.write_text(new_content)
            backups.append((path, original))
            print(f"  MODIFIED: {ch['file']}")
        elif ch["action"] == "write":
            path.write_text(ch["new"])
            backups.append((path, original))
            print(f"  WROTE: {ch['file']}")

    _active_backups = backups
    return backups


def revert_changes(backups: list[tuple[Path, str]]):
    """Restore original file contents."""
    global _active_backups
    for path, content in backups:
        path.write_text(content)
        print(f"  REVERTED: {path.relative_to(PROJECT_ROOT)}")
    _active_backups = []


def build_project() -> bool:
    """Cargo build. Returns True on success."""
    print("  Building (cargo)...")
    env = os.environ.copy()
    env["CARGO_HOME"] = os.path.expanduser("~/.cargo")
    env["RUSTUP_TOOLCHAIN"] = "stable"
    # Clean PATH like gwt does
    env["PATH"] = f"{env['CARGO_HOME']}/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"

    r = subprocess.run(
        ["cargo", "build", "--features", "webtest", "-p", "goose-cli"],
        cwd=PROJECT_ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=300,
    )
    if r.returncode != 0:
        print(f"  BUILD FAILED:\n{r.stderr[:500]}")
        return False
    print("  Build OK")
    return True


def run_test(timeout_s: int, target: str, provider: str) -> Path | None:
    """Run gwt and return path to the new JSON report, or None."""
    ts_before = time.time()
    print(f"  Running gwt (timeout {timeout_s}s, provider={provider})...")

    env = os.environ.copy()
    env["RUST_LOG"] = "warn"

    cmd = [str(GWT), "run", "--provider", provider, "--target", target]

    try:
        r = subprocess.run(
            cmd, cwd=PROJECT_ROOT, env=env,
            capture_output=True, text=True, timeout=timeout_s,
        )
    except subprocess.TimeoutExpired:
        print(f"  TIMEOUT after {timeout_s}s")
        return None

    if r.returncode not in (0, 1):
        print(f"  ERROR exit={r.returncode}")
        if r.stderr:
            print(f"  {r.stderr[:300]}")
        return None

    return find_latest_report(after=ts_before)


def git_ensure_branch() -> str:
    """Create an autoevolve feature branch if on main."""
    r = subprocess.run(
        ["git", "branch", "--show-current"],
        cwd=PROJECT_ROOT, capture_output=True, text=True,
    )
    branch = r.stdout.strip()

    if branch in ("main", "master"):
        ts = datetime.now().strftime("%Y%m%d-%H%M")
        new_branch = f"autoevolve/{ts}"
        subprocess.run(
            ["git", "checkout", "-b", new_branch],
            cwd=PROJECT_ROOT, capture_output=True,
        )
        print(f"  Branch: {new_branch}")
        return new_branch

    print(f"  Branch: {branch}")
    return branch


def git_commit(hypothesis: str, metrics: dict, iteration: int, changed_files: list[str]):
    """Commit only the autoevolve-modified files."""
    for f in changed_files:
        subprocess.run(
            ["git", "add", str(PROJECT_ROOT / f)],
            cwd=PROJECT_ROOT, capture_output=True,
        )

    msg = (
        f"autoevolve #{iteration}: {hypothesis}\n\n"
        f"Score: {metrics['score']:.3f} "
        f"(assertions: {metrics['assertion_rate']:.1%}, "
        f"steps: {metrics['step_rate']:.1%})\n"
        f"Duration: {metrics['duration']:.1f}s | "
        f"Result: {metrics['overall_result']}"
    )
    subprocess.run(
        ["git", "commit", "-m", msg],
        cwd=PROJECT_ROOT, capture_output=True,
    )
    print(f"  COMMITTED: autoevolve #{iteration}")


# ── Main loop ────────────────────────────────────────────────────────
def main():
    parser = argparse.ArgumentParser(
        description="Autonomous improvement loop for goose-webtest"
    )
    parser.add_argument("--max-iter", type=int, default=100)
    parser.add_argument("--timeout", type=int, default=180,
                        help="Seconds per test run")
    parser.add_argument("--target", default="targets/sparkasse-demo.toml")
    parser.add_argument("--provider", default="claude",
                        help="LLM provider for test execution")
    parser.add_argument("--max-hours", type=float, default=12.0)
    parser.add_argument("--dry-run", action="store_true",
                        help="Generate hypotheses without executing")
    parser.add_argument("--include-rust", action="store_true",
                        help="Include Rust source in mutable scope")
    args = parser.parse_args()

    client = anthropic.Anthropic()
    deadline = datetime.now() + timedelta(hours=args.max_hours)

    print("=" * 60)
    print("  autoevolve — goose-webtest self-improvement loop")
    print("=" * 60)
    print(f"  Max iterations : {args.max_iter}")
    print(f"  Test timeout   : {args.timeout}s")
    print(f"  Target         : {args.target}")
    print(f"  Provider       : {args.provider}")
    print(f"  Rust scope     : {'yes' if args.include_rust else 'no'}")
    print(f"  Deadline       : {deadline.strftime('%Y-%m-%d %H:%M')}")
    print("=" * 60)

    instructions = read_instructions()
    print(f"\nINSTRUCTIONS.md: {len(instructions)} chars")

    branch = git_ensure_branch()
    history = load_history()

    # ── Establish baseline ───────────────────────────────────────────
    baseline = None
    latest = find_latest_report()
    if latest:
        baseline = extract_metric(latest)
        print(f"Baseline from existing report: score={baseline['score']:.3f}")
    else:
        print("No existing report — running baseline test...")
        report_path = run_test(args.timeout, args.target, args.provider)
        if report_path:
            baseline = extract_metric(report_path)
            print(f"Baseline established: score={baseline['score']:.3f}")
        else:
            print("Baseline test failed. Starting with score=0.")

    best_score = baseline["score"] if baseline else 0.0
    kept_count = 0

    # ── Main loop ────────────────────────────────────────────────────
    for iteration in range(1, args.max_iter + 1):
        if _shutting_down:
            break
        if datetime.now() >= deadline:
            print(f"\nDeadline reached after {iteration - 1} iterations.")
            break

        remaining = deadline - datetime.now()
        h, m = divmod(int(remaining.total_seconds()), 3600)
        m //= 60

        print(f"\n{'=' * 60}")
        print(f"  Iteration {iteration}/{args.max_iter}  |  "
              f"best={best_score:.3f}  |  {h}h{m:02d}m left")
        print(f"{'=' * 60}")

        # 1. Read current files
        mutable = read_mutable_files(include_rust=args.include_rust)

        # 2. Generate hypothesis
        print("\n[1] Generating hypothesis...")
        try:
            hyp = generate_hypothesis(
                client, instructions, mutable, history, baseline
            )
        except Exception as e:
            print(f"  FAILED: {e}")
            time.sleep(5)
            continue

        print(f"  Hypothesis : {hyp['hypothesis']}")
        print(f"  Changes    : {len(hyp['changes'])} file(s)")
        print(f"  Rebuild    : {hyp.get('requires_rebuild', False)}")

        if args.dry_run:
            print("  [DRY RUN] skipping execution")
            continue

        # 3. Apply changes
        print("\n[2] Applying changes...")
        backups = apply_changes(hyp["changes"])
        if not backups:
            print("  No changes applied — skipping iteration")
            entry = {
                "iteration": iteration,
                "timestamp": datetime.now().isoformat(),
                "hypothesis": hyp["hypothesis"],
                "score": 0, "kept": False, "reason": "no_changes",
            }
            history.append(entry)
            append_history(entry)
            continue

        # 4. Rebuild if needed
        if hyp.get("requires_rebuild", False):
            print("\n[3] Rebuilding...")
            if not build_project():
                revert_changes(backups)
                entry = {
                    "iteration": iteration,
                    "timestamp": datetime.now().isoformat(),
                    "hypothesis": hyp["hypothesis"],
                    "score": 0, "kept": False, "reason": "build_failed",
                }
                history.append(entry)
                append_history(entry)
                continue

        # 5. Run test
        step = 4 if hyp.get("requires_rebuild") else 3
        print(f"\n[{step}] Running test...")
        report_path = run_test(args.timeout, args.target, args.provider)

        if not report_path:
            print("  No report produced — reverting")
            revert_changes(backups)
            entry = {
                "iteration": iteration,
                "timestamp": datetime.now().isoformat(),
                "hypothesis": hyp["hypothesis"],
                "score": 0, "kept": False, "reason": "test_failed",
            }
            history.append(entry)
            append_history(entry)
            continue

        # 6. Measure
        metrics = extract_metric(report_path)
        new_score = metrics["score"]

        print(f"\n  Score     : {new_score:.3f}  (baseline: {best_score:.3f})")
        print(f"  Assertions: {metrics['assertions_passed']}/{metrics['assertions_total']} "
              f"({metrics['assertion_rate']:.1%})")
        print(f"  Steps     : {metrics['steps_passed']}/{metrics['steps_total']} "
              f"({metrics['step_rate']:.1%})")
        print(f"  Duration  : {metrics['duration']:.1f}s")
        print(f"  Result    : {metrics['overall_result']}")

        # 7. Decide: keep or revert
        changed_files = [c["file"] for c in hyp["changes"]]
        if new_score > best_score:
            delta = new_score - best_score
            print(f"\n  >>> IMPROVEMENT +{delta:.3f} — KEEPING <<<")
            best_score = new_score
            baseline = metrics
            git_commit(hyp["hypothesis"], metrics, iteration, changed_files)
            kept = True
            kept_count += 1
        else:
            delta = new_score - best_score
            print(f"\n  --- no improvement ({delta:+.3f}) — REVERTING ---")
            revert_changes(backups)
            kept = False

        # 8. Log
        entry = {
            "iteration": iteration,
            "timestamp": datetime.now().isoformat(),
            "hypothesis": hyp["hypothesis"],
            "reasoning": hyp.get("reasoning", ""),
            "changes": changed_files,
            "score": new_score,
            "best_score": best_score,
            "metrics": metrics,
            "kept": kept,
            "reason": "improved" if kept else "no_improvement",
        }
        history.append(entry)
        append_history(entry)

    # ── Summary ──────────────────────────────────────────────────────
    total = len([h for h in history if "iteration" in h])
    print(f"\n{'=' * 60}")
    print(f"  autoevolve complete")
    print(f"{'=' * 60}")
    print(f"  Iterations : {total}")
    print(f"  Kept       : {kept_count}")
    print(f"  Reverted   : {total - kept_count}")
    print(f"  Best score : {best_score:.3f}")
    print(f"  Branch     : {branch}")
    print(f"{'=' * 60}")


if __name__ == "__main__":
    main()
