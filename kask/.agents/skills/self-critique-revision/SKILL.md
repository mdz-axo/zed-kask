---
name: self-critique-revision
visibility: public
description: "Iterative self-critique and revision cycle: generate an initial draft, critique it against quality criteria, then revise based on the critique.
"
---

# Self Critique Revision

Iterative self-critique and revision cycle: generate an initial draft, critique it against quality criteria, then revise based on the critique.


## When to Use

- When an initial draft response needs to be generated and evaluated against specific quality criteria.
- When a draft requires structured self-critique to identify issues, score them, and classify them by constraint-force severity.
- When a draft must be revised to address specific critique points while detecting and flagging regressions.
- When an iterative self-critique and revision cycle needs to measure whether quality improvement has plateaued or converged.
- When you need to self-critique a reasoning output for contradictions, unsupported claims, logical gaps, and confidence calibration (formerly the `review` skill — use `quality_criteria: [contradictions, unsupported_claims, logical_gaps, calibration]`).

## Instructions

### generate

1. Produce a thorough, well-structured response to the task prompt.
2. Focus on completeness, accuracy, clarity, and logical structure.
3. Address the task prompt directly and completely.
4. Provide an honest self-assessment of the output quality on a 0.0-1.0 scale based on the quality criteria, avoiding inflated values.

### critique

1. Critically evaluate the draft output against the specified quality criteria.
2. Score each criterion on a 1-5 scale based on the severity of gaps.
3. Classify each finding by constraint-force (Prohibition, Guardrail, Guideline, Evidence).
4. Cite exact text for each issue and provide an actionable suggestion.
5. Compute the overall and normalized quality scores honestly without minimizing real issues.

### revise

1. Address each critique point directly by making a specific change that fixes it, prioritizing Prohibition findings first, then Guardrail.
2. Preserve good content and only change what the critique identifies.
3. Improve overall quality so the revised draft scores higher on the quality criteria.
4. Compare each criterion score against the previous iteration and flag any criterion that dropped by ≥ 1 point.
5. Ensure the revised output is a complete, standalone response, not a diff.
6. Explain in `how_addressed` if a critique point cannot be resolved.

### self-critique-convergence-check

1. Extract the `normalized_quality_score` from `critique_result.per_criterion_scores` or compute it from `overall_quality_score × 5`.
2. Count unresolved critique issues by force, applying penalties for Prohibition, Guardrail, and Guideline findings.
3. Check the improvement trajectory, applying a penalty for regressions or a bonus if improvement plateaus below the target.
4. Verify critique coverage and apply penalties for unaddressed criteria.
5. Compute the convergence metric starting from the base formula, adding penalties and subtracting bonuses, then clamp to [0,1].

### iteration (child manifest)

When convergence is not reached, the parent invokes `self-critique-revision-iteration` — a single critique→revise→validate round that reuses the parent's `critique.j2` and `revise.j2` templates. The iteration child:
1. Critiques the current output against quality criteria (same `critique.j2` template).
2. Revises based on critique points (same `revise.j2` template).
3. Validates: compares quality improvement against the convergence threshold (default 0.15). If improvement < threshold, the process has converged.
4. Emits Regulation feedback for variety monitoring.

Max 3 iterations. Escalates to Curator if not converged after max iterations.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `generate.j2` | WordAct | Produce an initial draft response and an honest quality score.  |
| `critique.j2` | KnowAct | Evaluate a draft against quality criteria and return structured issues.  |
| `revise.j2` | KnowAct | Produce a revised draft that addresses each critique point.  |
| `self-critique-convergence-check.j2` | KnowAct | Compute normalized convergence metric for self-critique revision PDCA cycles. Returns convergence_metric plus rationale and blockers.  |

## Fusion Mode

This skill supports **fusion mode** via the `fusion:` block in its flow manifest.
When enabled, all analysis steps route through a multi-model panel — either with
LLM judge synthesis or the **algo / no-judge** path (`judge: algo`) for deterministic
JSON merge without an LLM judge call. This skill uses **deliberation mode** —
Multi-round refinement matches iterative critique.

The convergence check step has `fusion: false` to ensure deterministic rubric
evaluation uses single-model inference.

## Constraints

- `generate.j2`: Public.
- `critique.j2`: Public.
- `revise.j2`: Public.
- `self-critique-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
