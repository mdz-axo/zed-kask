---
name: pragmatic-semantics
core: true
description: "Epistemic discipline for classifying statements by certainty level, constraint force, and domain ontology anchoring. Distinguish IS from OUGHT, declarative from probabilistic from subjunctive. Resolve conflicts using OT ranking."
---

# Pragmatic Semantics

Epistemic discipline for classifying statements by certainty level, constraint force, and domain ontology anchoring. Distinguish IS from OUGHT, declarative from probabilistic from subjunctive. Classify provenance of facts and their ontology tier (Core / Dual-Axis / Domain Supplement). Resolve conflicts using OT ranking with ontology anchoring.

## Reference models

Hume's is–ought distinction (*A Treatise of Human Nature*, 1739); Optimality Theory's strict-domination ranking (Prince & Smolensky, 1993) for conflict resolution; epistemic modality (Palmer, *Mood and Modality*, 1986) for declarative / probabilistic / subjunctive.

## When to Use

- When a statement needs classification on ontological (IS/OUGHT), epistemic (declarative/probabilistic/subjunctive), and domain ontology anchoring axes.
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
6. Classify the provenance of the statement (Specification, Implementation, Observation, Inference, External, or Unknown).
7. Judge a base confidence (P), then compute the final confidence with `lisp_eval` (D) — tier modifier, clamp to [0,1], Unknown-provenance ceiling 0.3, Specification floor 0.8 only when the spec was actually checked as current:
   - form: `(let ((c (max 0 (min 1 (+ base (cond ((string= tier "fibo") 0.10) ((string= tier "sumo") 0.05) ((string= tier "unanchored") -0.15) (t 0))))))) (cond ((string= prov "unknown") (min c 0.3)) ((and (string= prov "specification") spec_checked) (max c 0.8)) (t c)))`
   - env: `{ "base": <judged base confidence>, "tier": "fibo|sumo|core|unanchored", "prov": <provenance, lower-case>, "spec_checked": <true only if the named spec was verified current> }`

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
6. Use ontology anchoring as the final tiebreaker, prioritizing higher-confidence ontologies (e.g., FIBO over SUMO, unanchored as lowest priority).
7. (D) Once each statement's five classifications are fixed, the ranking is a lexicographic comparison — compute it with `lisp_eval`, never judge it. Encode each statement as its rank on each tier (0 = strongest, in the orders of steps 2–6) and compare: `(begin (define cmp (lambda (a b) (cond ((is_null a) "tie") ((< (car a) (car b)) "first") ((> (car a) (car b)) "second") (t (cmp (cdr a) (cdr b)))))) (cmp a b))`, env `{ "a": [<5 tier ranks>], "b": [<5 tier ranks>] }`. The classifications themselves remain P.
8. Determine the winning statement and select a resolution strategy (Override, Scope, Defer, Escalate, or Confirm if no conflict exists).
9. Escalate to human review if two Prohibitions conflict or if the comparison returns `tie`.

### Convergence

9. Gate — call `lisp_eval` with:
   - form: `(and (= (length unverifiable_gaps) 0) (>= chain_confidence 0.8))`
   - env: `{ "unverifiable_gaps": <provenance-chain gaps that stayed unverifiable>,
             "chain_confidence": <overall confidence from semantics-provenance-trace step 5> }`
   If the classification source is unknown or the conflict result was skipped, carry that status into the final report; do not claim all three analyses passed on provenance alone. Bound: one re-trace — apply the verification recommendations
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

To render a template, call the `render_template` tool with the template ref (e.g., `pragmatic-semantics/semantics-classify-statement`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `semantics-conflict-resolve.j2`: `provenance_result`,`classification_result`


## Constraints

- `semantics-classify-statement.j2`: Public. IS-statements are never Prohibitions. Declarative OUGHT-statements map to Prohibition or Guardrail. Unknown provenance → confidence ≤ 0.3. Specification provenance → confidence ≥ 0.8 (verify spec is current). FIBO +0.10, SUMO +0.05, unanchored -0.15.
- `semantics-provenance-trace.j2`: Public. Every step must identify a concrete location. Unknown source → confidence ≤ 0.2. Direct spec quotes → confidence ≥ 0.9. Inference steps reduce confidence by ≥ 0.1.
- `semantics-conflict-resolve.j2`: Public. OUGHT never loses to IS. Two Prohibitions conflicting → escalate. Resolution enum: override, scope, defer, escalate, confirm.
- Conflict resolution runs only if `conflicts_detected == true`; otherwise record it as skipped, not a successful resolution. If true, require a ranked result or report the unresolved conflict; never default a missing result to `{}` and claim convergence.
- Convergence check incorporates all three analysis steps (classification, provenance, conflict resolution), not just classification.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
