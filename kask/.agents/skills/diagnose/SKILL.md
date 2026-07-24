---
name: diagnose
visibility: public
description: "Disciplined diagnosis loop for hard bugs and performance regressions. Cybernetic debugging: build feedback loop → reproduce → hypothesise → instrument → fix → regression-test. Aligned with Regulation sense→orient→decide→act."
---


# Diagnose

Disciplined diagnosis loop for hard bugs and performance regressions. Cybernetic debugging: build feedback loop → reproduce → hypothesise → instrument → fix → regression-test. Aligned with Regulation sense→orient→decide→act.

## When to Use

- A hard bug or performance regression resists quick fixes and needs disciplined root-cause analysis
- You need to anchor a bug to functional requirements before debugging (Phase 0)
- You need to build a fast, deterministic feedback loop to reproduce the bug
- You need to generate multiple falsifiable hypotheses rather than anchoring on the first plausible idea
- You need to instrument code with targeted probes mapped to specific hypotheses
- You need to apply a fix with a regression test written *before* the fix, then clean up instrumentation
- You need to measure whether diagnosis convergence is sufficient to exit the loop
- The bug spans multiple MDS categories (Domain, Composition, Trust, Lifecycle, Curation) and needs classification
- A spec gap may be the real finding — no functional requirement governs the misbehaving code

## Instructions

1. **Anchor to functional requirements (Phase 0).** Before building a feedback loop, classify the bug by MDS category (Domain, Composition, Trust, Lifecycle, Curation). Map the symptom to functional requirement references (FR#). State the requirement text, which criterion the bug violates, and whether the violation is partial or complete. If no functional requirement covers the misbehavior, flag it as a spec gap — this is a finding, not a failure. Recommend what requirement should exist. Every diagnosis must trace to a spec requirement or note the gap explicitly. Do not fabricate FR# references.

2. **Build the feedback loop and reproduce the bug.** Construct the fastest, most deterministic feedback loop. Try strategies in order: failing test at the reaching seam → `cargo test` with specific test name → CLI invocation with fixture input → HTTP script → replay captured input → throwaway harness → property/fuzz loop → `git bisect run` → differential loop. Prioritize speed, signal sharpness, and determinism — a 2-second deterministic loop beats a 30-second flaky loop. For non-deterministic bugs, the goal is a higher reproduction rate, not a clean repro: loop 100×, parallelise, add stress. Do not proceed to hypothesising without a loop you believe in.

3. **Generate falsifiable root-cause hypotheses (delegate to falsifiability).** Avoid single-hypothesis anchoring by delegating this step to the `falsifiability` skill's hypothesize stage — the shared Chamberlin/Platt method diagnose would otherwise reimplement. Invoke `falsifiability/falsifiability-hypothesize` with `admitted_target` = the symptom/bug description, `domain` = "bug diagnosis", `context` = code_context. falsifiability-hypothesize generates 3–7 ranked candidate root causes with forced diversity (≥1 unlikely, ≥1 challenging the obvious explanation, ≥1 embarrassing-if-true), each carrying a Platt-form prediction ("if X is the case, then observation Y under condition Z") and a falsifier; it discards any candidate that cannot be made falsifiable (a vibe) at generation, recording why. Map each returned hypothesis's `prediction` into the bug-debugging form ("if X is the cause, then changing Y will make the bug disappear") and treat its `falsifier` as the falsification condition that step 4's probes must be able to trigger. Rank by likelihood, not by ease of testing. Present the ranked list for user review before testing any hypothesis — the user often has domain knowledge that re-ranks instantly. Set `user_review_requested` to true and do not proceed to instrumenting until the user has reviewed.

4. **Instrument with targeted probes mapped to hypotheses.** Design probes where each probe maps to exactly one hypothesis — no scattergun logging. Change one variable at a time; never test multiple hypotheses simultaneously. Tool preference order: `rust-lldb`/`rust-gdb` breakpoint (one breakpoint beats ten logs) → targeted `tracing::debug!` with unique `[DIAG-xxxx]` prefix → `RUST_LOG` per-module tracing. Never "log everything and grep." Tag every diagnostic log with a unique `[DIAG-xxxx]` prefix so cleanup is a single grep. For performance bugs, use `cargo bench`, `criterion`, or `flamegraph` — measure first, fix second.

5. **Apply fix with regression test written before the fix.** If a correct seam exists: turn the minimised reproduction into a failing regression test at that seam, watch it fail, apply the fix, watch it pass, re-run the original feedback loop. If no correct seam exists, that itself is the finding — the architecture is preventing the bug from being locked down. Document this for architecture review. Do NOT write a shallow regression test that gives false confidence. Clean up: remove all `[DIAG-...]` instrumentation, delete throwaway prototypes, state the confirmed hypothesis in the commit/PR message. Verify `cargo clippy -p <crate> -- -D warnings` and `cargo test -p <crate>` pass. Write a post-mortem: what was the bug, root cause, fix, and what would have prevented it. If the fix reveals an architectural issue (no good test seam, tangled callers, hidden coupling), document it in an architecture note.

6. **Check convergence.** Measure whether root cause and fix confidence are sufficient to exit the diagnosis loop. Start at 1.0 and subtract for each satisfied check: root cause confidence (−0.25 if ambiguous), bug reproduced (−0.15 if not), fix validated (−0.20 if unvalidated), alternatives eliminated (−0.15 if not), contract strengthened (−0.10 if not). Clamp to [0,1]. Convergence threshold is 0.25 — diagnosis can't improve past evidence, so a looser threshold is appropriate. 0.00 = root cause confirmed, fix validated, regression tests pass. 0.50 = competing hypotheses, insufficient evidence to discriminate. 1.00 = no root cause identified, no fix proposed. If blockers remain, state the specific gap preventing convergence.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `diagnose-spec-anchor.j2` | `KnowAct` | Anchor a bug diagnosis to functional requirements: classify by MDS category, map symptom to FR# references, flag spec gaps. Phase 0 of the diagnosis pipeline. Every diagnosis traces to a spec or notes the gap. |
| `diagnose-loop.j2` | `KnowAct` | Build a feedback loop for the bug. Evaluate repro strategies, select the fastest deterministic signal, and confirm the bug reproduces before hypothesising. |
| `diagnose-instrument.j2` | `KnowAct` | Instrument the code with targeted probes mapped to specific hypotheses. Change one variable at a time. Use tagged diagnostic logs or breakpoints. |
| `diagnose-fix.j2` | `KnowAct` | Apply fix with regression test (before the fix). Verify original repro no longer reproduces. Clean up instrumentation. Write post-mortem. |
| `diagnose-convergence-check.j2` | `KnowAct` | Compute normalized convergence metric for diagnosis cycles. Outputs `convergence_metric` in [0,1], where 0 means root cause and fix confidence are sufficient for exit. |

## Fusion Mode

This skill supports **fusion mode** via the `fusion:` block in its flow manifest.
When enabled, all analysis steps route through a multi-model panel — either with
LLM judge synthesis or the **algo / no-judge** path (`judge: algo`) for deterministic
JSON merge without an LLM judge call. This skill uses **critique mode** — Draft →
panel critiques → revise matches the diagnosis loop.

The convergence check step has `fusion: false` to ensure deterministic rubric
evaluation uses single-model inference.

## Constraints

- All templates are `KnowAct` type with `Public` visibility
- Energy caps: spec-anchor (3072), loop (6144), instrument (4096), fix (6144), convergence-check (2048); step 3 (hypothesize) is delegated to `falsifiability/falsifiability-hypothesize` (energy_cap 4096)
- Safety mode (when enabled): no file system access, no network calls, no environment variable access, strict Jinja2 sandbox enforcement
- Do not execute arbitrary Python code in Jinja2 expressions — sandboxed execution only
- Preserve original prompt structure and formatting; handle missing variables gracefully
- Hypothesis count: 3–7 (step 3 delegates to `falsifiability/falsifiability-hypothesize`) — every hypothesis must have a falsifiable prediction
- Every probe must map to exactly one hypothesis; every diagnostic log must have a unique `[DIAG-xxxx]` tag
- Write the regression test BEFORE the fix; if no correct seam exists, do not write a shallow test that gives false confidence
- All `[DIAG-xxxx]` instrumentation tags must be removed before declaring done
- The commit/PR message must state the confirmed hypothesis
- Do not fabricate FR# references — derive from actual specification documents or explicitly note the gap
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins