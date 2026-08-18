GitHub Digest 2026-03-26 | 13 repos + 3 local clones

🚨 [ACTION REQUIRED]
• hermes-agent: 106 commits behind origin/main
  → Your local ~/.hermes/hermes-agent/ is severely outdated
  → Risk: Running patched version without latest agent features/fixes
  → Fix: `cd ~/.hermes/hermes-agent && git pull origin main`

⚠️ [DRIFT DETECTED]
• CoPaw: 0 behind, 2 commits ahead of upstream
  → Local modifications detected but fully in sync
  → Patches ahead: 2 commits ready to merge upstream

✅ Local clones current
• autoresearch-mlx: 0 behind/ahead — perfectly synced

📊 Remote Activity (last 24h)
• anthropics/claude-code: v2.1.84 released (March 25)
  → Added managed-settings.d/ drop-in directory for team policy fragments
  → Added CwdChanged/FileChanged hook events for reactive environments
  → sandbox.failIfUnavailable to exit on sandbox failure instead of running unsandboxed
  → Added CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1 for credential stripping
  → Transcript search (/ key, n/N to step)
  → Agents can declare initialPrompt in frontmatter to auto-submit first turn
  → Fixed: voice mode audio recovery, Remote Control session titles, Bash ghost-text suggestions
  → Improved: plugin freshness, MCP OAuth with Client ID Metadata Document
  
• ml-explore/mlx: v0.31.1 on March 11 (outside 24h window)
  → CUDA improvements: FSDP, Quantized GEMV, 3/5/6-bit quants, Hadamard transform
  → Batch support in QMV, faster compilation
  → LayerNorm VJP fix, memory leak fixes in save/load
  → Active development continues with recent commits

⚠️ Notable repos with activity outside 24h window:
• vllm-project/vllm: Recent commits (local server updates)
• lightningnetwork/lnd: Lightning Network core updates
• fedimint/fedimint: MINTMAKER dependency — check if relevant
• agentscope-ai/CoPaw: Memory system your agent-stack uses

📌 Recommendations
1. CRITICAL: Pull hermes-agent fix immediately (106 commits!)
2. CoPaw: Merge your 2 local patches to upstream when ready
3. Consider updating claude-code to v2.1.84 for security improvements

Full data logged. Run `hermes status hermes-agent` to see full diff.
