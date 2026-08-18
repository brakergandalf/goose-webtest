# autoevolve Instructions

## Objective

Improve goose-webtest's autonomous web testing on the Sparkasse demo banking app.
Maximize the **assertion pass rate** while maintaining or increasing test coverage.

## Success Metric

```
score = 0.7 * (assertions_passed / assertions_total) + 0.3 * (steps_passed / steps_total)
```

Higher is better. Do NOT reduce assertion count to inflate the rate.

## What To Optimize

1. **Blueprint prompts** — Make LLM instructions clearer, more structured, reduce ambiguity
2. **Turn budgets** — Allocate turns based on task complexity (discovery vs. execution)
3. **Timeout values** — Tune wait times for page loads, element detection, login
4. **Assertion strategies** — Improve how virtual tools detect pass/fail (text matching, snapshot parsing)
5. **Context passing** — Improve information flow between agentic nodes
6. **Node transitions** — Optimize when to proceed vs. retry on partial failures

## Constraints

- Do NOT change login credentials (chipDEMO / 12345)
- Do NOT change the target URL (sparkasse-hannover.de)
- Max turns per agentic node: 60 (cost limit)
- Test run timeout: 5 minutes max
- No form submissions — read-only testing only
- No navigation to external URLs or localhost

## Known Issues

- Discovery nodes sometimes get stuck re-visiting the same pages
- Assertion text matching is case-insensitive but doesn't handle partial matches well
- The generate_and_execute node sometimes runs out of turns before completing all tests
- Login occasionally fails on first attempt (cookie consent overlay)

## Anti-Patterns to Avoid

- Prompts that are too long (>500 words) — LLM attention degrades
- Redundant assertion checks on the same element
- Reducing discovery depth just to make steps pass faster
- Over-specifying element selectors that break on minor UI changes
