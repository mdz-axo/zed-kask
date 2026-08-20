---
name: diagnose
description: "Disciplined diagnosis loop for hard bugs and performance regressions. Cybernetic debugging: build feedback loop → reproduce → hypothesise → instrument → fix → regression-test. Aligned with Regulation sense→orient→decide→act."
---


# Diagnose

Disciplined diagnosis loop for hard bugs and performance regressions. Cybernetic debugging: build feedback loop → reproduce → hypothesise → instrument → fix → regression-test. Aligned with Regulation sense→orient→decide→act.

## When to Use

- A hard bug or performance regression resists quick fixes and needs disciplined root-cause analysis
: You need to anchor a bug to the code graph before debugging (Phase 0)
- You need to build a fast, deterministic feedback loop to reproduce the bug
- You need to generate multiple falsifiable hypotheses rather than anchoring on the first plausible idea
- You need to instrument code with targeted probes mapped to specific hypotheses
- You need to apply a fix with a regression test written *before* the fix, then clean up instrumentation
- You need to measure whether diagnosis convergence is sufficient to exit the loop
: The bug spans multiple Dublin Core entity types or PKO procedure paths and needs ontological classification
- A codegraph gap may be the real finding — the affected entity has no callers or is orphaned in the graph

## Instructions

1. **Anchor to the code graph (Phase 0 + step 1).** Step 0 is an `execute` step (no template) that pre-computes the blast radius via `codegraph_impact` (MCP) — deterministic, `on_failure: report` surfaces a broken codegraph channel without blocking the diagnosis. Step 1 (`diagnose-spec-anchor.j2`) then anchors the bug to the actual code structure using a dual ontology: it calls `codegraph_query` to find the affected symbol and `codegraph_traverse` to trace the call chain (callers + callees). Classify the THING using Dublin Core (`dcterms:type` = entity kind, `dcterms:identifier` = qualified name, `dcterms:source` = file:line, `dcterms:subject` = MDS category derived from graph position). Classify the FLOW using PKO (`pko:Procedure` = the call chain, `pko:Step` = each function, `pko:IssueOccurrence` = the bug at a specific `pko:StepExecution`). Every diagnosis must be anchored to a real codegraph entity or note that the graph is unavailable.

2. **Build the feedback loop and reproduce the bug.** Construct the fastest, most deterministic feedback loop. Try strategies in order: failing test at the reaching seam → `cargo test` with specific test name → CLI invocation with fixture input → HTTP script → replay captured input → throwaway harness → property/fuzz loop → `git bisect run` → differential loop. Prioritize speed, signal sharpness, and determinism — a 2-second deterministic loop beats a 30-second flaky loop. For non-deterministic bugs, the goal is a higher reproduction rate, not a clean repro: loop 100×, parallelise, add stress. Do not proceed to hypothesising without a loop you believe in.

3. **Generate falsifiable root-cause hypotheses (delegate to falsifiability).** Avoid single-hypothesis anchoring by delegating this step to the `falsifiability` skill's hypothesize stage — the shared Chamberlin/Platt method diagnose would otherwise reimplement. Invoke `falsifiability/falsifiability-hypothesize` with `admitted_target` = the symptom/bug description, `domain` = "bug diagnosis", `context` = code_context. falsifiability-hypothesize generates 3–7 ranked candidate root causes with forced diversity (≥1 unlikely, ≥1 challenging the obvious explanation, ≥1 embarrassing-if-true), each carrying a Platt-form prediction ("if X is the case, then observation Y under condition Z") and a falsifier; it discards any candidate that cannot be made falsifiable (a vibe) at generation, recording why. Map each returned hypothesis's `prediction` into the bug-debugging form ("if X is the cause, then changing Y will make the bug disappear") and treat its `falsifier` as the falsification condition that step 5's probes must be able to trigger. Rank by likelihood, not by ease of testing. Present the ranked list for user review before testing any hypothesis — the user often has domain knowledge that re-ranks instantly. Set `user_review_requested` to true and do not proceed to instrumenting until the user has reviewed.

4. **Hypothesis invariant check (step 4, compute/lisp.eval — no template, deterministic).** Before instrumenting, a lisp.eval compute step deterministically evaluates four structural invariants on the root-cause hypothesis set: count (3–7), completeness (every hypothesis has `prediction` + `falsifier` keys), diversity (≥2 distinct likelihoods), and mutual exclusivity (no duplicate hypothesis text). Returns a list of defect strings — no LLM round-trip. Step 5 (instrument) is gated on this check via `condition: NOT step_4_result`: if the hypothesis set has structural defects, the loop re-enters at step 3 (hypothesize) to repair them before any instrumentation runs.

5. **Instrument with targeted probes mapped to hypotheses.** Design probes where each probe maps to exactly one hypothesis — no scattergun logging. Change one variable at a time; never test multiple hypotheses simultaneously. Tool preference order: `rust-lldb`/`rust-gdb` breakpoint (one breakpoint beats ten logs) → targeted `tracing::debug!` with unique `[DIAG-xxxx]` prefix → `RUST_LOG` per-module tracing. Never "log everything and grep." Tag every diagnostic log with a unique `[DIAG-xxxx]` prefix so cleanup is a single grep. For performance bugs, use `cargo bench`, `criterion`, or `flamegraph` — measure first, fix second.

6. **Apply fix with regression test written before the fix.** If a correct seam exists: turn the minimised reproduction into a failing regression test at that seam, watch it fail, apply the fix, watch it pass, re-run the original feedback loop. If no correct seam exists, that itself is the finding — the architecture is preventing the bug from being locked down. Document this for architecture review. Do NOT write a shallow regression test that gives false confidence. Clean up: remove all `[DIAG-...]` instrumentation, delete throwaway prototypes, state the confirmed hypothesis in the commit/PR message. Verify `cargo clippy -p <crate> -- -D warnings` and `cargo test -p <crate>` pass. Write a post-mortem: what was the bug, root cause, fix, and what would have prevented it. If the fix reveals an architectural issue (no good test seam, tangled callers, hidden coupling), document it in an architecture note.

7. **Check convergence (step 7).** Measure whether root cause and fix confidence are sufficient to exit the diagnosis loop. Start at 1.0 and subtract for each satisfied check: root cause confidence (−0.25 if ambiguous), bug reproduced (−0.15 if not), fix validated (−0.20 if unvalidated), alternatives eliminated (−0.15 if not), contract strengthened (−0.10 if not). Clamp to [0,1]. Convergence threshold is 0.25 — diagnosis can't improve past evidence, so a looser threshold is appropriate. 0.00 = root cause confirmed, fix validated, regression tests pass. 0.50 = competing hypotheses, insufficient evidence to discriminate. 1.00 = no root cause identified, no fix proposed. If blockers remain, state the specific gap preventing convergence.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `diagnose-spec-anchor.j2` | KnowAct | Anchor a bug diagnosis to the actual code structure using a dual ontology: Dublin Core classifies the THING affected (entity type, identifier, source, subject domain derived from graph position) and PKO classifies the FLOW the bug occurs in (call chain as Procedure, each function as Step, bug as IssueOccurrence at a StepExecution). Uses codegraph_query/traverse/impact MCP tools. Phase 0 of the diagnosis pipeline. Replaces spec-anchoring — the bug is grounded in the real code graph, not a specification document. |
| `diagnose-loop.j2` | KnowAct | Build a feedback loop for the bug. Evaluate repro strategies, select the fastest deterministic signal, and confirm the bug reproduces before hypothesising. |
| `diagnose-instrument.j2` | KnowAct | Instrument the code with targeted probes mapped to specific hypotheses. Change one variable at a time. Use tagged diagnostic logs or breakpoints. |
| `diagnose-fix.j2` | KnowAct | Apply fix with regression test (before the fix). Verify original repro no longer reproduces. Clean up instrumentation. Write post-mortem. |

## Constraints

- All templates are `KnowAct` type with `Public` visibility
- Step 3 (hypothesize) is delegated to falsifiability/falsifiability-hypothesize
- Safety mode (when enabled): no file system access, no network calls, no environment variable access, strict Jinja2 sandbox enforcement
- Do not execute arbitrary Python code in Jinja2 expressions — sandboxed execution only
- Preserve original prompt structure and formatting; handle missing variables gracefully
- Hypothesis count: 3–7 (step 3 delegates to `falsifiability/falsifiability-hypothesize`) — every hypothesis must have a falsifiable prediction
- Every probe must map to exactly one hypothesis; every diagnostic log must have a unique `[DIAG-xxxx]` tag
- Write the regression test BEFORE the fix; if no correct seam exists, do not write a shallow test that gives false confidence
- All `[DIAG-xxxx]` instrumentation tags must be removed before declaring done
- The commit/PR message must state the confirmed hypothesis
- Do not fabricate codegraph entities — derive from actual `codegraph_query` results or note the graph is unavailable
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.