# Scenario Builder Pipeline

**Location:** `registry/manifests/scenario-builder.yaml`  
**Templates:** `registry/templates/scenario-builder/`  
**Version:** 0.31.0

## Overview

This pipeline implements Schwartz's scenario planning methodology. It refines a focal question, maps micro and macro forces, constructs divergent 2x2 scenario narratives, runs an independent quality gate, extracts implications and early-warning indicators, and computes convergence for iterative refinement.

## Pipeline Stages

| Step | Template | Purpose | Gas Cap |
|------|----------|---------|---------|
| 1 | `focal-question.j2` | Refine and bound the focal question | 3,000 |
| 2 | `key-forces.j2` | Enumerate microenvironment forces | 5,000 |
| 3 | `driving-forces.j2` | Map STEEP driving forces, identify critical uncertainties | 5,000 |
| 4 | `axes-and-narratives.j2` | Generate 2x2 axes and four scenario narratives | 8,000 |
| 5 | `scenario-quality-gate.j2` | Independent quality gate (divergence, consistency, coverage) | 4,000 |
| 6 | `implications-indicators.j2` | Extract implications, robust/contingent strategies, indicators | 5,000 |
| 7 | `scenario-convergence-check.j2` | Compute convergence metric + stall detection | 3,000 |
| 8 | *(loop)* | Restart at step 4 if convergence not met | — |

**Total Gas Budget:** 100,000 tokens

## Theoretical Foundation

Based on Schwartz's **The Art of the Long View**:

1. **Focal Question** — Decision-relevant, time-bounded, scope-bounded
2. **Key Forces** — Microenvironment: market, regulatory, technology, competitors, resources
3. **Driving Forces** — Macro STEEP: society, technology, economy, environment, politics
4. **Axes & Narratives** — Two critical uncertainty axes → 2x2 quadrant scenarios
5. **Quality Gate** — Independent divergence/consistency/coverage evaluation (no self-assessment)
6. **Implications** — Robust strategies (all scenarios) + contingent strategies + tripwires
7. **Convergence** — Gate scores + parametric_variation_flag + stall detector

## Convergence & Loop Behavior

- **Threshold:** 0.15 (penalty metric; 0 = converged, 1 = not converged)
- **Max iterations:** 3
- **Loop target:** step 4 (narrative generation) — preserves the refined focal question, key forces, and driving-forces/axes across iterations. The quality gate diagnoses narrative divergence, consistency, and coverage, which are fixed by regenerating scenarios, not by re-rolling the focal question.
- **Stall detector:** If the convergence metric does not improve by ≥ 0.03 vs the prior iteration, a blocker is emitted signaling that regeneration is vacuous — escalation is recommended.
- **On not reached:** escalate to human

## Convergence Method

The convergence check consumes the independent quality gate's output directly:

| Penalty | Condition | Weight |
|---------|-----------|--------|
| Gate failed | `gate_pass = false` | +0.50 |
| Divergence | `(1.0 - divergence_score) × 0.30` | up to +0.30 |
| Consistency | `(1.0 - consistency_score) × 0.15` | up to +0.15 |
| Coverage | `(1.0 - coverage_score) × 0.15` | up to +0.15 |
| Parametric variation | `parametric_variation_flag = true` (from gate) | +0.20 |
| Implications | `robust_strategies` empty or < 2 | +0.10 |
| Indicators | `early_indicators` empty or < 2 | +0.10 |

The `parametric_variation_flag` is consumed directly from the quality gate — it is NOT re-derived via word-overlap (the gate's semantic check is strictly stronger than any heuristic).

## Regulation Integration

The pipeline emits Regulation spans for monitoring:
- `hkask.template.select` — Pipeline selection
- `hkask.template.render` — Template execution at each step
- `hkask.template.outcome` — Scenario set finalized

## OCAP Requirements

The pipeline requires template render permissions for all 7 templates plus manifest execution permission. All capabilities are template-scoped and expire after 3600 seconds.

## Error Handling

| Error Type | Behavior |
|------------|----------|
| Gas exceeded | Abort |
| Timeout | Retry (max 1, 1s backoff) |
| Validation failure | Abort |
| Capability denied | Escalate to Curator |

,## Fusion Mode

This skill supports **fusion mode** via the `fusion: true` field in the flow
manifest. When fusion is globally enabled (env vars or `/fusion on`), all
analysis steps route through a multi-model panel — either with LLM judge
synthesis or the **algo / no-judge** path (`judge: algo`) for deterministic
JSON merge without an LLM judge call.

The quality gate (step 5) and convergence check (step 7) have `fusion: false`
set explicitly — deterministic rubric evaluations use single-model inference.

See the superforecasting README for full fusion configuration details.

## Future Enhancements

- [ ] Conditional loop target based on `gate_findings` (restart at forces/axes if coverage fails, narratives if divergence fails)
- [ ] Delegate axis-independence verification to a dedicated check
- [ ] Wire `constraint_force` classification to `pragmatic-semantics` for validation
- [ ] Human-in-the-loop checkpoint after quality gate

## References

- Schwartz, P. (1996). *The Art of the Long View*
- Schoemaker, P. (1995). "Scenario Planning: A Tool for Strategic Thinking"
- van der Heijden, K. (2005). *Scenarios: The Art of Strategic Conversation*