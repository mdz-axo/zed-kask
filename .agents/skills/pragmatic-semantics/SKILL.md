---
name: pragmatic-semantics
core: true
description: "Epistemic discipline for classifying statements by certainty level, constraint force, and domain ontology anchoring. Distinguish IS from OUGHT, declarative from probabilistic from subjunctive. Resolve conflicts using OT ranking. Route computation steps to deterministic or probabilistic machines (D/P labelling, P8.4)."
---

# Pragmatic Semantics

Epistemic discipline for classifying statements by certainty level, constraint force, and domain ontology anchoring. Distinguish IS from OUGHT, declarative from probabilistic from subjunctive. Classify provenance of facts and their ontology tier (Core / Dual-Axis / Domain Supplement). Resolve conflicts using OT ranking with ontology anchoring. Route computation steps to the machine whose conditional entropy matches the task's — D/P labelling per P8.4.

## Reference models

Hume's is–ought distinction (*A Treatise of Human Nature*, 1739); Optimality Theory's strict-domination ranking (Prince & Smolensky, 1993) for conflict resolution; epistemic modality (Palmer, *Mood and Modality*, 1986) for declarative / probabilistic / subjunctive. Entropy-matched computation (P8.4): Shannon (1948) and Jaynes (1957) for conditional entropy; Turing (1936, 1939) for deterministic computation and oracles; amortized inference (Gershman & Goodman 2014; Hu et al. 2024) for learned computation; Tetlock & Gardner (2015) and Brier (1950) for calibrated forecasts — the five derived-registry rulings.

## D/P labelling

Statement classification (all axes of semantics-classify-statement, the tiers and resolution/escalation steps of semantics-conflict-resolve, provenance-trace steps 1–7) and semantics-route-step's regime classification, mismatch flags, and hybrid labeling are P — judgment, critiqued by the operator (a mismatch flag naming a nonexistent oracle is checkably wrong against the available_oracles input; a classification that contradicts its traced provenance is a finding). The confidence computation (classify step 7), the lexicographic ranking (conflict-resolve step 7), and the convergence gate are D (`lisp_eval`, the forms below). A D tag on a step with no real oracle is checkably wrong; a P tag on a step with a cheap deterministic checker is a precision-improvement candidate.

## When to Use

- When a statement needs classification on ontological (IS/OUGHT), epistemic (declarative/probabilistic/subjunctive), and domain ontology anchoring axes.
- When routing a computation step to the machine whose conditional entropy matches the task's — deterministic oracle versus learned model — and labelling its D/P regime (P8.4).
- When determining the constraint force (Prohibition, Guardrail, Guideline, Evidence, Hypothesis) and provenance of a statement.
- When tracing the origin and evidentiary chain of a factual claim through hKask's data layers to its authoritative source.
- When identifying gaps in a claim's derivation chain and recommending verification steps.
- When resolving conflicts between statements using 5-tier OT ranking (ontological type, epistemic mode, constraint force, evidence provenance, and ontology anchoring).
- When ranking contradictory statements to determine a winner based on constraint force hierarchy and provenance weighting.

## When NOT to Use

- Feedback-loop and variety analysis — use `pragmatic-cybernetics`.
- Extracting structured data from text — use `structured-extraction`; classification is not extraction.
- Rendering final verdicts on code findings — `code-review` embeds this lens but owns the adjudication.

## Instructions

### semantics-classify-statement

1. Classify the statement's ontological mode as either IS (descriptive) or OUGHT (prescriptive).
2. Determine the epistemic mode as declarative (high certainty), probabilistic (medium certainty), or subjunctive (low certainty).
3. Identify the domain ontology anchoring tier (core, dual_axis, or domain_supplement) and the specific ontology anchor.
4. Identify both the process axis (PKO) and state axis (DC+BIBO) if the statement is dual-axis.
5. Map the statement to its constraint force (Prohibition, Guardrail, Guideline, Evidence, or Hypothesis) based on its ontological and epistemic modes.
6. Classify the provenance of the statement (Specification, Design, Implementation, Runtime, Memory, Inference, External, or Unknown — the trace's data-layer hierarchy; `Runtime` replaces the former `Observation`, same concept).
7. Judge a base confidence (P), then compute the final confidence with `lisp_eval` (D) — tier modifier, clamp to [0,1], Unknown-provenance ceiling 0.3 (a specification claimed but not checked is treated as unknown — the floor never applies to an unchecked spec), Specification floor 0.8 only when the spec was actually checked as current:
   - form: `(let ((c (max 0 (min 1 (+ base (cond ((string= tier "fibo") 0.10) ((string= tier "sumo") 0.05) ((string= tier "unanchored") -0.15) (t 0))))))) (cond ((or (string= prov "unknown") (and (string= prov "specification") (not spec_checked))) (min c 0.3)) ((and (string= prov "specification") spec_checked) (max c 0.8)) (t c)))`
   - env: `{ "base": <judged base confidence>, "tier": "fibo|sumo|core|unanchored" (derived from ontology_anchor: `fibo-*` → fibo, `sumo:*` → sumo, `unanchored` → unanchored, every other anchor → core, i.e. no modifier), "prov": <provenance, lower-case>, "spec_checked": <true only if the named spec was verified current> }`

### semantics-provenance-trace

1. Start with the claim as stated and identify its most direct source (specification, design, implementation, runtime, memory, inference, or unknown).
2. Trace the claim back recursively through derivation steps until reaching a primary source or an unverifiable gap.
3. Record the source, location, derivation type, transform, and confidence delta for each step in the provenance chain.
4. Flag any gaps in the chain where sources cannot be verified.
5. Determine `chain_confidence` (0.0–1.0) from the verified chain, and its confidence level (high, medium, low, or unverifiable). Count unverified links as `unverifiable_gaps`; never apply an authority floor to a source not actually checked.
6. Detect any conflicting sources or contradictory evidence within the provenance chain.
7. Provide specific, actionable recommendations for verifying and strengthening the provenance chain.

### semantics-conflict-resolve

1. Rank the conflicting statements across five tiers: Ontological Mode, Epistemic Mode, Constraint Force, Provenance Authority, and Ontology Anchoring.
2. Apply the rule that OUGHT (prescriptive) overrides IS (descriptive) in conflicts.
3. Break ties within the same ontological mode by epistemic certainty (Declarative > Probabilistic > Subjunctive).
4. Break ties within the same epistemic mode by constraint force (Prohibition > Guardrail > Guideline > Evidence > Hypothesis).
5. Break ties within the same constraint force by provenance authority (Specification > Design > Implementation > Runtime > Memory > Inference > Unknown).
6. Use ontology anchoring as the final tiebreaker, prioritizing the Tier-5 anchoring rank (FIBO over SUMO, unanchored as lowest priority — the rank is adoption-based, not the confidence modifier).
7. (D) Once each statement's five classifications are fixed, the ranking is a lexicographic comparison — compute it with `lisp_eval`, never judge it. Encode each statement as its rank on each tier (0 = strongest, in the orders of steps 2–6) and compare: `(begin (define cmp (lambda (a b) (cond ((is_null a) "tie") ((< (car a) (car b)) "first") ((> (car a) (car b)) "second") (t (cmp (cdr a) (cdr b)))))) (cmp a b))`, env `{ "a": [<5 tier ranks>], "b": [<5 tier ranks>] }`. The classifications themselves remain P.
8. Determine the winning statement and select a resolution strategy (Override, Scope, Defer, Escalate, or Confirm if no conflict exists).
9. Escalate to human review if two Prohibitions conflict or if the comparison returns `tie`.

### semantics-route-step

Standalone analysis: it classifies computation steps (P8.4), not statements, and is not part of the three-analysis convergence gate (one statement × three analyses).

1. Classify the step's regime by the entropy-matching rule:
   - **D** — the answer is pinned and a deterministic checker exists. Name the oracle (`lisp_eval`, `lean_check`, `cargo`, a server-side oracle).
   - **P, propose-verify** — knowledge is exhibited only in data; the model proposes and a D gate collapses. Name the gate.
   - **P, explicit probabilistic compute** — the answer is genuinely a distribution; a D server computes over P inputs. Name the server.
   - **P, calibrated forecast** — no characterizable posterior and no cheap oracle; judgment scored against external ground truth. Name the resolution mechanism (resolved outcomes, Brier).
2. Emit the tag: `{ "step": <step id>, "regime": "D" | "P:propose-verify" | "P:probabilistic" | "P:forecast", "oracle": <the named oracle, gate, server or resolution mechanism>, "critique": <what checks this label>, "mismatch_diseases": <the flagged diseases, each naming its step> }`.
3. Flag both mismatch diseases: a P step that could be D is a precision-improvement candidate; a D step where the answer has genuine multiplicity is false certainty.
4. Label hybrid steps at sub-step granularity (precedent: `falsifiability` splits its step 5 into a P call and a D verdict). `render_template` is D (deterministic render) feeding P (model consumption) — label the render and the consumption separately.

### Convergence

Gate — call `lisp_eval` with:
   - form: `(and (= (length unverifiable_gaps) 0) (>= chain_confidence 0.8))`
   - env: `{ "unverifiable_gaps": <provenance-chain gaps that stayed unverifiable>,
             "chain_confidence": <overall confidence from semantics-provenance-trace step 5> }`
   If the classification provenance is unknown or the conflict result was skipped, carry that status into the final report; do not claim all three analyses passed on provenance alone. Bound: one re-trace — apply the verification recommendations
   (semantics-provenance-trace step 7) and re-run the trace; gaps that
   survive the second pass are reported as unverifiable (the honest exit
   semantics-provenance-trace step 5 defines). This gate is the convergence
   check the Constraints require over all three analysis steps.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `semantics-classify-statement.j2` | Classify a statement on three axes: ontological (IS/OUGHT), epistemic (declarative/probabilistic/subjunctive), and domain ontology anchoring (core/dual_axis/domain_supplement). Determine its constraint force, provenance, and confidence with tier-specific modifiers. |
| `semantics-provenance-trace.j2` | Trace the provenance of a claim through hKask's data layers including ontology tier confidence modifiers. Identify evidence sources, confidence level, and verification recommendations. |
| `semantics-conflict-resolve.j2` | Resolve a conflict between statements using 5-tier OT ranking. Rank by ontological type, epistemic mode, constraint force, evidence provenance, and ontology anchoring (FIBO > SUMO > unanchored). |
| `semantics-route-step.j2` | Classify a computation step into its D/P regime per P8.4 (entropy-matched routing): D (named oracle), P:propose-verify (named gate), P:probabilistic (named server), P:forecast (named resolution mechanism). Emits the step tag and flags both mismatch diseases. |

To render a template, call the `render_template` tool with the template ref (e.g., `pragmatic-semantics/semantics-classify-statement`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `semantics-classify-statement.j2`: `statement`, `system_context`
- `semantics-provenance-trace.j2`: `classification_result`, `statement`, `system_context`
- `semantics-conflict-resolve.j2`: `provenance_result`, `classification_result`, `statements` (the multi-statement case)
- `semantics-route-step.j2`: `step_description`, `available_oracles`


## Constraints

- `semantics-classify-statement.j2`: IS-statements are never Prohibitions. Declarative OUGHT-statements map to Prohibition or Guardrail. Unknown provenance → confidence ≤ 0.3. Specification provenance → confidence ≥ 0.8 (verify spec is current). FIBO +0.10, SUMO +0.05, unanchored -0.15.
- `semantics-provenance-trace.j2`: Every step must identify a concrete location. Unknown source → confidence ≤ 0.2. Direct spec quotes → confidence ≥ 0.9. Inference steps reduce confidence by ≥ 0.1.
- `semantics-conflict-resolve.j2`: OUGHT never loses to IS. Two Prohibitions conflicting → escalate. Resolution enum: override, scope, defer, escalate, confirm.
- `semantics-route-step.j2`: Every D tag names a real oracle; every P tag names its collapse path. A step tagged D where the answer has genuine multiplicity, or P where a deterministic checker exists, is flagged as a mismatch disease, never silently relabelled.
- Conflict resolution runs only if `conflicts_detected == true`; the no-conflict outcome is resolution `confirm` (a valid result, not a failure). Record `skipped` only when the step did not run at all, and carry that status into the final report. If conflicts were detected, require a ranked result or report the unresolved conflict; never default a missing result to `{}` and claim convergence. A missing `conflicts_detected` field is not a no-conflict claim — state that the field is missing.
- The convergence gate (D) checks the provenance axis (`unverifiable_gaps` + `chain_confidence`); the classification-provenance and conflict statuses are carried into the final report by instruction — do not claim all three analyses passed on provenance alone.
