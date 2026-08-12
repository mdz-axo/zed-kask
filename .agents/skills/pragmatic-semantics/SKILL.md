---
name: pragmatic-semantics
description: "Epistemic discipline for classifying statements by certainty level, constraint force, and domain ontology anchoring. Distinguish IS from OUGHT, declarative from probabilistic from subjunctive. Resolve conflicts using OT ranking."
---

# Pragmatic Semantics

Epistemic discipline for classifying statements by certainty level, constraint force, and domain ontology anchoring. Distinguish IS from OUGHT, declarative from probabilistic from subjunctive. Classify provenance of facts and their ontology tier (Core / Dual-Axis / Domain Supplement). Resolve conflicts using OT ranking with ontology anchoring.

## When to Use

- When a statement needs classification on ontological (IS/OUGHT), epistemic (declarative/probabilistic/subjunctive), and domain ontology anchoring axes.
- When determining the constraint force (Prohibition, Guardrail, Guideline, Evidence, Hypothesis) and provenance of a statement.
- When tracing the origin and evidentiary chain of a factual claim through hKask's data layers to its authoritative source.
- When identifying gaps in a claim's derivation chain and recommending verification steps.
- When resolving conflicts between statements using 5-tier OT ranking (ontological type, epistemic mode, constraint force, evidence provenance, and ontology anchoring).
- When ranking contradictory statements to determine a winner based on constraint force hierarchy and provenance weighting.

## Instructions

### semantics-classify-statement

1. Classify the statement's ontological mode as either IS (descriptive) or OUGHT (prescriptive).
2. Determine the epistemic mode as declarative (high certainty), probabilistic (medium certainty), or subjunctive (low certainty).
3. Identify the domain ontology anchoring tier (core, dual_axis, or domain_supplement) and the specific ontology anchor.
4. Identify both the process axis (PKO) and state axis (DC+BIBO) if the statement is dual-axis.
5. Map the statement to its constraint force (Prohibition, Guardrail, Guideline, Evidence, or Hypothesis) based on its ontological and epistemic modes.
6. Classify the provenance of the statement (Specification, Implementation, Observation, Inference, External, or Unknown).
7. Calculate the confidence score from 0.0 to 1.0, applying tier-specific modifiers (e.g., +0.10 for FIBO, +0.05 for SUMO, -0.15 for unanchored). For Specification provenance, confidence should be ≥ 0.8, but verify the spec is current before applying this floor — specs can be stale.

### semantics-provenance-trace

1. Start with the claim as stated and identify its most direct source (specification, design, implementation, runtime, memory, inference, or unknown).
2. Trace the claim back recursively through derivation steps until reaching a primary source or an unverifiable gap.
3. Record the source, location, derivation type, transform, and confidence delta for each step in the provenance chain.
4. Flag any gaps in the chain where sources cannot be verified.
5. Determine the overall confidence level (high, medium, low, or unverifiable) based on the chain's completeness.
6. Detect any conflicting sources or contradictory evidence within the provenance chain.
7. Provide specific, actionable recommendations for verifying and strengthening the provenance chain.

### semantics-conflict-resolve

1. Rank the conflicting statements across five tiers: Ontological Mode, Epistemic Mode, Constraint Force, Provenance Authority, and Ontology Anchoring.
2. Apply the rule that OUGHT (prescriptive) overrides IS (descriptive) in conflicts.
3. Break ties within the same ontological mode by epistemic certainty (Declarative > Probabilistic > Subjunctive).
4. Break ties within the same epistemic mode by constraint force (Prohibition > Guardrail > Guideline > Evidence > Hypothesis).
5. Break ties within the same constraint force by provenance authority (Specification > Design > Implementation > Runtime > Memory > Inference > Unknown).
6. Use ontology anchoring as the final tiebreaker, prioritizing higher-confidence ontologies (e.g., FIBO over SUMO, unanchored as lowest priority).
7. Determine the winning statement and select a resolution strategy (Override, Scope, Defer, Escalate, or Confirm if no conflict exists).
8. Escalate to human review if two Prohibitions conflict or if all five tiers result in a genuine tie.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `semantics-classify-statement.j2` | KnowAct | Classify a statement on three axes: ontological (IS/OUGHT), epistemic (declarative/probabilistic/subjunctive), and domain ontology anchoring (core/dual_axis/domain_supplement). Determine its constraint force, provenance, and confidence with tier-specific modifiers. |
| `semantics-provenance-trace.j2` | KnowAct | Trace the provenance of a claim through hKask's data layers including ontology tier confidence modifiers. Identify evidence sources, confidence level, and verification recommendations. |
| `semantics-conflict-resolve.j2` | KnowAct | Resolve a conflict between statements using 5-tier OT ranking. Rank by ontological type, epistemic mode, constraint force, evidence provenance, and ontology anchoring (FIBO > SUMO > unanchored). |

## Constraints

- `semantics-classify-statement.j2`: Public. IS-statements are never Prohibitions. Declarative OUGHT-statements map to Prohibition or Guardrail. Unknown provenance → confidence ≤ 0.3. Specification provenance → confidence ≥ 0.8 (verify spec is current). FIBO +0.10, SUMO +0.05, unanchored -0.15.
- `semantics-provenance-trace.j2`: Public. Every step must identify a concrete location. Unknown source → confidence ≤ 0.2. Direct spec quotes → confidence ≥ 0.9. Inference steps reduce confidence by ≥ 0.1.
- `semantics-conflict-resolve.j2`: Public. OUGHT never loses to IS. Two Prohibitions conflicting → escalate. Resolution enum: override, scope, defer, escalate, confirm.
- Step 3 (conflict-resolve) is conditional — it only runs when `step_2_result.conflicts_detected == true`. Downstream steps must use `step_3_result | default({})` to handle the skipped case.
- Convergence check incorporates all three analysis steps (classification, provenance, conflict resolution), not just classification.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
