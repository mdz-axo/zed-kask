---
name: essentialist
visibility: public
description: "General-purpose recursive eliminative interrogation. Enforces 'always take away, never add' through a 3-gate challenge loop (Exist → Surface → Contract) that every artifact must survive before being committed. Delegates G1 to deep-module deletion test, G2 to deep-module surface assessment, and G3 to coding-guidelines abstraction audit."
---


# Essentialist

General-purpose recursive eliminative interrogation. Enforces "always take away, never add" through a 3-gate challenge loop (Exist → Surface → Contract) that every artifact must survive before being committed. Delegates G1 to deep-module deletion test, G2 to deep-module surface assessment, and G3 to coding-guidelines abstraction audit.

## When to Use

- An artifact (module, function, trait, type, or interface) needs to be interrogated for unnecessary complexity, pass-through wrappers, or over-engineered abstractions.
- The user explicitly requests simplification, stripping, or elimination ("simplify", "strip", "run the essentialist") — this activates autonomous mode.
- The user wants advisory-mode review where the agent recommends reductions and the human accepts, rejects, or overrides per gate.
- You need to enforce "always take away, never add" — every artifact is assumed guilty until proven necessary.
- A codebase has accumulated cruft, thin wrappers, single-use traits, or public-surface bloat that should be challenged before commit.

## Instructions

1. Assume every artifact is guilty until proven necessary. Your job is to enforce "always take away, never add" through a 3-gate recursive challenge loop. You orchestrate the gates, delegate evaluations to specialized templates, branch on pass/fail, and escalate when retries are exhausted.

2. Determine the mode. Default is **advisory** (agent recommends, human decides). Autonomous mode only activates on explicit user intent ("simplify", "strip", "run the essentialist"). In autonomous mode, evaluate and reduce without pause. In advisory mode, present findings with constraint-force labels and await human accept/reject/override per item.

3. Execute the 3-gate protocol in fixed order: G1 (Exist) → G2 (Surface) → G3 (Contract). The order is fixed — there is no point counting surfaces or tracing contracts for an artifact that does not survive the deletion test.

4. **Gate 1 — EXIST (Deletion Test):** Delegate to `deep-module/deep-module-delete`. Apply Ousterhout's deletion test in both directions. From the caller perspective: inline the artifact's logic into each caller; if complexity reappears and the replacement is trivial (a few lines), the artifact is a pass-through → FAIL. From the artifact perspective: delete the artifact and replace with direct calls to its dependency; if no behavior vanishes → FAIL. Pass requires that behavior IS lost on deletion AND complexity WOULD reappear in callers.

5. **Gate 2 — SURFACE (Interface Count):** Delegate to `deep-module/deep-module-assess`. Count every public item (function, type, trait, constant). Apply the 7-function rule: ≤ 7 public items passes; each item beyond 7 requires a written justification explaining why it cannot be merged. Compute the depth score: `implementation_lines / (public_functions + public_types + public_traits)`. Challenge the actor: "What if this artifact had exactly one public function? What would it be? Why do the others need to exist separately?"

6. **Gate 3 — CONTRACT (Abstraction Trace):** Delegate to `coding-guidelines/guidelines-verify` with focus on Simplicity First violations. Trace every abstraction boundary: for every trait, count implementors (if 1 → single-use, can it be inlined?); for every wrapper/adapter, identify added behavior beyond a direct call (if none → pass-through, delete); for every config struct, check if passed through untouched (if yes → unnecessary indirection); for every error type, check if it wraps exactly one inner error (if yes → pass-through); for every generic parameter, count concrete types using it (if 1 → unnecessary generality). Pass requires every abstraction encodes genuine behavior beyond a direct call.

7. Classify every finding by constraint-force per the pragmatic-semantics hierarchy: Prohibition, Guardrail, Guideline, Evidence, Hypothesis. In autonomous mode, only Prohibition and Guardrail findings cause gate failure; Guideline, Evidence, and Hypothesis are informational. In advisory mode, present Prohibitions as REQUIRED (rejection causes immediate escalation), Guardrails as REQUIRED (overridable with reason), Guidelines as SUGGESTED, Evidence as INFO, and Hypotheses as SPECULATIVE.

8. On gate failure, reduce the artifact: G1 failures → DELETE pass-through items and inline trivial wrappers into callers; G2 failures → MERGE public items where possible, add justifications for items that must remain separate, delete public items that can be made private; G3 failures → DELETE pass-through abstractions, replace with direct calls, inline single-use traits. After reduction, resubmit from G1 — reduction at any gate may affect earlier gates.

9. In advisory mode, after each gate evaluation, present recommendations to the human as a JSON object with gate, status, recommendations (each with item, constraint_force, label, why, recommended_action, recommendation_detail), and a human prompt. Apply accepted reductions, note rejections with the human's stated reason, and restart from G1 if the artifact changed. If the human rejects a REQUIRED (Prohibition) finding without override, escalate immediately.

10. Escalate to human after `max_retries_per_gate` (default 3) failures on a single gate. Produce an escalation report with the contested gate, round, retries exhausted, contested items, survivors summary, and the specific human decision required. After escalation, STOP — do not continue reducing without human input.

11. Abort on zero-delta completion: when all three gates pass AND the artifact is unchanged from the previous round (same surviving items, same structure, same interfaces), the artifact is essential. Produce a completion report with the full elimination report (deletions per gate, constraint-force breakdown, essentialism score, human decisions if advisory) and the surviving artifact.

12. Compute the essentialism score on completion: `Score = (items_removed / total_items_initial) * 100`, where total_items_initial = public functions + public types + public traits + wrappers + adapters + config structs. Interpret: 0% = already minimal; 1–25% = minor reduction; 26–50% = significant reduction; 51–75% = major reduction; 76–100% = artifact eliminated entirely.

13. Run up to `max_rounds` (default 3) full G1→G2→G3 rounds. Between rounds, narrow scope. If zero deltas are detected between rounds, abort — the artifact is essential.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `essentialist-flow.j2` | `KnowAct` | Run the 3-gate eliminative interrogation loop in either autonomous (agent evaluates and recommends without pause) or advisory (agent recommends, human accepts/rejects/overrides per gate) mode. Classify every finding by constraint-force (Prohibition → required, Guideline → suggested), escalate to human on retry exhaustion (3 max), abort on zero-delta completion. Delegates reasoning to deep-module (G1, G2) and coding-guidelines (G3) templates. |

,## Fusion Mode

This skill supports **fusion mode** via the `fusion:` block in its flow manifest.
When enabled, all analysis steps route through a multi-model panel — either with
LLM judge synthesis or the **algo / no-judge** path (`judge: algo`) for deterministic
JSON merge without an LLM judge call. The essentialist uses **critique mode** (draft →
panel critiques → revise) to match the 3-gate challenge loop.

The convergence check step has `fusion: false` to ensure deterministic rubric
evaluation uses single-model inference.

## Constraints

- Visibility: Public — this skill's templates are shared across agents.
- Energy cap: 8192 tokens per template invocation.
- The default mode is **advisory** (agent recommends, human decides). Autonomous mode only activates on explicit user intent ("simplify", "strip", "run the essentialist").
- The G1→G2→G3 order is FIXED. G1 (Exist) must come first.
- Every finding MUST carry a `constraint_force` label. Only Prohibition and Guardrail cause gate failure in autonomous mode.
- Every gate failure MUST produce specific, actionable reduction instructions — not vague critiques.
- Escalate to human after `max_retries_per_gate` failures on a single gate. Do not loop indefinitely.
- In advisory mode, human rejections of REQUIRED (Prohibition) findings cause immediate ESCALATE. Guardrail rejections are allowed with stated reason.
- After escalation, STOP. Do not continue reducing without human input.
- Zero-delta detection must be exact: same surviving items, same structure, same interfaces as previous round.
- Do not execute arbitrary Python code in Jinja2 expressions (sandboxed execution).
- Handle missing variables gracefully (leave as-is or use default if specified).
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.