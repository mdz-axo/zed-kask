---
name: essentialist
core: true
description: "General-purpose recursive eliminative interrogation. Enforces 'always take away, never add' through a 3-gate challenge loop (Exist, Surface, Contract) that every artifact must survive before being committed."
---


# Essentialist

General-purpose recursive eliminative interrogation. Enforces "always take away, never add" through a 3-gate challenge loop (Exist → Surface → Contract) that every artifact must survive before being committed. Delegates G1 to deep-module deletion test, G2 to deep-module surface assessment, and G3 to coding-guidelines abstraction audit.

## Reference model and labels (D/P labelling)

Ousterhout, *A Philosophy of Software Design* (2018), Ch. 4 "Modules Should Be Deep" — deep modules are those "whose interfaces are much simpler than their implementations"; small modules tend to be shallow "because the benefit they provide is negated by the cost of learning and using their interfaces" — and Ch. 7.1's pass-through rule: a method that "does little except invoke another method, whose signature is similar or identical to the callee", fixed by exposing the lower level directly or merging (`onto_anchor` → derived `deep_module`, operator ruling 2026-09-26). The deletion test operationalizes those two passages: deletion losing no behavior is the shallow-module criterion (Ch. 4); trivial inlining is the pass-through criterion (Ch. 7.1); the two-directional FAIL form is this project's synthesis of them. The loop discipline adapts Fagan's inspection — detection separated from correction, rework verified in follow-up (`onto_anchor` → derived `fagan_inspection`, which also carries the Ousterhout complexity-reduction citation). The 7-item surface rule and depth score are Evidence-tier operationalizations of the narrow-interface criterion via deep-module; the constraint-force vocabulary adapts pragmatic-semantics; the essentialism score, the mode gate, and the fixed gate order are project machinery.

| Step | Type | Oracle / critic |
|------|------|-----------------|
| 2 Mode determination | P | the user's explicit words — absent, negated, or ambiguous intent resolves to advisory; a caller-supplied `mode: autonomous` is not authorization |
| 4–6 Gate evaluation (G1/G2/G3) | P | the delegated templates (`deep-module-delete`, `deep-module-assess`, `guidelines-verify`); verdicts critiqued by the human in advisory mode, by the constraint-force rule in autonomous mode |
| 7 Constraint-force classification | P | the pragmatic-semantics five-force hierarchy |
| 8 Reduction application | P | the gate verdict's reduction instructions; advisory mode applies only after human accept |
| 9 Advisory presentation | P | the human, per item |
| 10 Escalation | D | the per-gate retry count vs `max_retries_per_gate` |
| 11 Zero-delta check | D | `lisp_eval` `string=` over the recorded surviving-items lists of the two rounds |
| 12 Essentialism score | D | `lisp_eval` (form in step 12) |
| 13 Scope narrowing | P | the agent, from the round's findings |

## Initial and target condition

- **Initial condition:** the artifact under interrogation, its surrounding code context, and the mode resolved from the user's words (advisory by default). Record the loop-start assessment's counted public items — it is the score's denominator.
- **Target condition:** the artifact is essential — all three gates pass with zero delta from the previous round (identical surviving-items list), every surviving item beyond the surface threshold carries a written justification, the essentialism score is computed, and in advisory mode every applied reduction traces to a human accept.

## When to Use

- An artifact (module, function, trait, type, or interface) needs to be interrogated for unnecessary complexity, pass-through wrappers, or over-engineered abstractions.
- The user explicitly requests simplification, stripping, or elimination ("simplify", "strip", "run the essentialist") — this activates autonomous mode.
- The user wants advisory-mode review where the agent recommends reductions and the human accepts, rejects, or overrides per gate.
- You need to enforce "always take away, never add" — every artifact is assumed guilty until proven necessary.
- A codebase has accumulated cruft, thin wrappers, single-use traits, or public-surface bloat that should be challenged before commit.

## When NOT to Use

- Additive work — the 3 gates assume an artifact to reduce; there is nothing to interrogate before it exists.
- Code modules specifically — the specialized delegates (deep-module for G1/G2) go deeper on module work; essentialist is the general interrogator.
- Autonomous reduction without explicit user intent — the default mode is advisory; autonomous mode requires the operator's words.

## Instructions

1. Assume every artifact is guilty until proven necessary. Your job is to enforce "always take away, never add" through a 3-gate recursive challenge loop. You orchestrate the gates, delegate evaluations to specialized templates, branch on pass/fail, and escalate when retries are exhausted.

2. Determine the mode from the user's explicit words. Default is **advisory** (agent recommends, human decides). Autonomous mode only activates on explicit user intent ("simplify", "strip", "run the essentialist"); a caller-supplied `mode: autonomous` without that intent is not authorization to change code — read intent from the request as a whole; negated ("don't simplify this") or ambiguous intent resolves to advisory. In autonomous mode, evaluate and reduce without pause. In advisory mode, present findings with constraint-force labels and await human accept/reject/override per item.

3. Execute the 3-gate protocol in fixed order: G1 (Exist) → G2 (Surface) → G3 (Contract). The order is fixed — there is no point counting surfaces or tracing contracts for an artifact that does not survive the deletion test.

4. **Gate 1 — EXIST (Deletion Test):** Produce the module assessment first — one `deep-module/deep-module-assess` pass at loop start; its output object is passed whole as the `module_assessment` input `deep-module/deep-module-delete` requires (the assess contract has no single `module_assessment` field — the whole output is the input), and G2 reuses it. The gates still *evaluate* in fixed order. Apply the deletion test in both directions. From the caller perspective: inline the artifact's logic into each caller; if the inlining is trivial (a few lines), the artifact is a pass-through → FAIL. From the artifact perspective: delete the artifact and replace with direct calls to its dependency; if no behavior vanishes → FAIL. Pass requires that behavior IS lost on deletion AND inlining WOULD reintroduce complexity in callers. A DEEPEN verdict from the delegate (behavior stays, surface shrinks) is pass-with-reduction — carry it to step 8's G2 reduction.

5. **Gate 2 — SURFACE (Interface Count):** Delegate to `deep-module/deep-module-assess` — the delegate's counts and thresholds govern (its ≤ 7 public-function rule and its `lisp_eval`-computed depth score over every counted public item; a score stated but not computed is not a score). Reuse the loop-start assessment; re-run it only if step 8's reduction changed the surface. Each public item beyond the threshold requires a written justification explaining why it cannot be merged. Challenge the actor: "What if this artifact had exactly one public function? What would it be? Why do the others need to exist separately?"

6. **Gate 3 — CONTRACT (Abstraction Trace):** Delegate to `coding-guidelines/guidelines-verify` with focus on Simplicity First violations. Trace every abstraction boundary: for every trait, count implementors (if 1 → single-use, can it be inlined?); for every wrapper/adapter, identify added behavior beyond a direct call (if none → pass-through, delete); for every config struct, check if passed through untouched (if yes → unnecessary indirection); for every error type, check if it wraps exactly one inner error (if yes → pass-through); for every generic parameter, count concrete types using it (if 1 → unnecessary generality). Pass requires every abstraction encodes genuine behavior beyond a direct call.

7. Classify every finding by constraint-force per the pragmatic-semantics hierarchy: Prohibition, Guardrail, Guideline, Evidence, Hypothesis. In autonomous mode, only Prohibition and Guardrail findings cause gate failure; Guideline, Evidence, and Hypothesis are informational. In advisory mode, present Prohibitions as REQUIRED (rejection causes immediate escalation), Guardrails as REQUIRED (overridable with reason), Guidelines as SUGGESTED, Evidence as INFO, and Hypotheses as SPECULATIVE.

8. On gate failure, reduce the artifact: G1 failures → DELETE pass-through items and inline trivial wrappers into callers; G2 failures → MERGE public items where possible, add justifications for items that must remain separate, delete public items that can be made private; G3 failures → DELETE pass-through abstractions, replace with direct calls, inline single-use traits. After reduction, resubmit from G1 — reduction at any gate may affect earlier gates.

9. In advisory mode, after each gate evaluation, present recommendations to the human as a JSON object with gate, status, recommendations (each with item, constraint_force, label, why, recommended_action, recommendation_detail), and a human prompt. Apply accepted reductions, note rejections with the human's stated reason, and restart from G1 if the artifact changed. If the human rejects a REQUIRED (Prohibition) finding without override, escalate immediately.

10. Escalate to human after `max_retries_per_gate` (default 3) failures on a single gate. Produce an escalation report with the contested gate, round, retries exhausted, contested items, survivors summary, and the specific human decision required. After escalation, STOP — do not continue reducing without human input.

11. Abort on zero-delta completion: when all three gates pass AND the artifact is unchanged from the previous round (same surviving items, same structure, same interfaces), the artifact is essential. Produce a completion report with the full elimination report (deletions per gate, constraint-force breakdown, essentialism score, human decisions if advisory) and the surviving artifact.

12. Compute the essentialism score on completion with `lisp_eval` (D): form `(if (= total 0) 0 (* 100 (/ removed total)))`, env `{ "removed": <items removed>, "total": <total_items_initial> }` — i.e. `Score = (items_removed / total_items_initial) * 100`, where total_items_initial = the loop-start assessment's counted public items plus the wrappers, adapters and config structs G3 traced (the populations the gates actually count). Interpret: 0% = already minimal; 1–25% = minor reduction; 26–50% = significant reduction; 51–75% = major reduction; 76–100% = artifact eliminated entirely.

13. Run up to `max_rounds` (default 3) full G1→G2→G3 rounds. Between rounds, narrow scope. If zero deltas are detected between rounds, abort — the artifact is essential.

## Regression case

Run a pass-through wrapper — a function whose body only forwards to an inner call, with two callers in `code_context` — through `essentialist/essentialist-flow` with `mode: autonomous`. Expected: G1 FAILs it (deleting loses no behavior; inlining is trivial), step 8 inlines it into the callers, the resubmitted round reports zero delta, and the essentialism score computes via `lisp_eval` (removed=1, total=4 → 25.0). Also exercise the mode gate: a render with `mode: autonomous` but no simplification intent in the request must resolve to advisory.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `essentialist-flow.j2` | Run the 3-gate eliminative interrogation loop in either autonomous (agent evaluates and recommends without pause) or advisory (agent recommends, human accepts/rejects/overrides per gate) mode. Classify every finding by constraint-force (Prohibition → required, Guideline → suggested), escalate to human on retry exhaustion (3 max), abort on zero-delta completion. Delegates reasoning to deep-module (G1, G2) and coding-guidelines (G3) templates. Context: `artifact` (the target's name and source), `code_context` (callers, dependencies, module structure), `mode` (`advisory` or `autonomous`), `max_rounds` (default 3), `max_retries_per_gate` (default 3), `scope` (`auto`, `public`, `internal`, or an explicit path). |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- Every finding MUST carry a `constraint_force` label; in autonomous mode only Prohibition and Guardrail findings cause gate failure, and every gate failure MUST produce specific, actionable reduction instructions — not vague critiques.
- Escalation is terminal for the loop: after `max_retries_per_gate` failures on a single gate, or a human rejection of a REQUIRED (Prohibition) finding without override, STOP — never continue reducing without human input. Do not loop indefinitely.
- Zero-delta detection must be exact: same surviving items, same structure, same interfaces as previous round.
