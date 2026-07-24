---
name: caveman
visibility: public
description: "Ultra-compressed communication mode. Cuts token usage ~75% by dropping filler, articles, and pleasantries while keeping full technical accuracy. Active every response once triggered."
---


# Caveman

Ultra-compressed communication mode. Cuts token usage ~75% by dropping filler, articles, and pleasantries while keeping full technical accuracy. Once triggered, caveman mode stays active on every response until explicitly disabled.

## When to Use

- A draft response needs to be compressed into caveman mode to reduce token usage while preserving all technical substance
- You want to evaluate whether a caveman-compressed output has converged (sufficiently compressed while preserving clarity constraints)
- Caveman mode has been triggered and is active for the current response cycle
- A PDCA (Plan-Do-Check-Act) convergence check is needed after compression to decide if further compression passes are required
- Auto-clarity exceptions apply: security warnings, irreversible action confirmations, multi-step sequences where fragment order risks misread, or the user asks to clarify / repeats a question

## Instructions

### Compression (caveman-compress)

1. Drop articles (a/an/the), filler (just/really/basically/actually/simply), pleasantries (sure/certainly/of course/happy to), hedging (I think/it seems like/perhaps), and redundant phrasing.
2. Keep all technical terms exact and unchanged.
3. Keep all code blocks exact and unchanged — code is sacred.
4. Keep all error messages quoted exact — error text is sacred.
5. Keep all URLs exact and unchanged — URLs are sacred.
6. Use fragments. Prefer short synonyms (big not extensive, fix not "implement a solution for").
7. Abbreviate common terms: DB, auth, config, req, res, fn, impl.
8. Follow the pattern: `[thing] [action] [reason]. [next step].`
9. Drop caveman temporarily for security warnings (must be clear and unambiguous), irreversible action confirmations (must be explicit about consequences), multi-step sequences where fragment order risks misread, and when the user asks to clarify or repeats the question.
10. Resume caveman mode after the clarity-requiring part is done.
11. Estimate tokens as word count × 1.3.
12. Compute `compression_ratio` = `compressed_token_estimate / original_token_estimate` (lower is better).
13. Report any sections where caveman was temporarily dropped as `clarity_exceptions`.

### Convergence Check (caveman-convergence-check)

1. Given a compression output, compute a normalized convergence metric on [0,1].
2. Score 0 = fully converged (output is sufficiently compressed while preserving clarity constraints); 1 = not converged.
3. Score how much work remains toward the convergence threshold (default 0.15).
4. Use LLM-assessed saturation detection as the convergence method.
5. Consider iteration count (default max 3) and improvement target (default 0.10) when scoring.
6. Return only JSON with `convergence_metric`, `convergence_method`, and `rationale`.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `caveman-compress.j2` | `WordAct` | Compress a draft response into caveman mode: drop articles, filler, pleasantries, hedging. Preserve all technical substance. Insert clarity exceptions for security and irreversible actions. |
| `caveman-convergence-check.j2` | `KnowAct` | Compute normalized convergence metric for caveman PDCA cycles. |

## Constraints

- Both templates run at `visibility: Public` with `energy_cap: 2048`.
- All technical substance must survive compression — no loss of meaning permitted.
- Code blocks, error messages, and URLs are sacred and must not be modified.
- Do not execute arbitrary Python code in Jinja2 expressions — sandboxed execution only.
- Preserve the original prompt structure and formatting.
- Handle missing variables gracefully (leave as-is or use default if specified).
- Convergence check runs with `thinking_budget: off` and `max_tokens: 900` — keep rationale terse.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
