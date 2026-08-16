# Compiled AI Gaps 1-3: Verification Audit and Implementation Plan

## Status

Analysis + proposed plan. Superseded by `compiled-ai-gaps-review.md` which
contains the revised plan after five-skill review.

## Origin

Analysis of Trooskens et al. (2026), _Compiled AI_ (arXiv:2604.05150v2)
identified three capabilities the paper suggests kask lacks:

1. **Explicit validation-stage framing** (Security / Syntax / Execution / Accuracy)
2. **Bounded tool call drift monitoring** (detecting semantic drift in schema-valid LLM outputs)
3. **Compile-time vs runtime failure distinction** (separating structural failures from execution failures)

## Verification Audit Summary

| Item                       | What kask already has                                                                                           | Genuine gap                                                                                                     |
| -------------------------- | --------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| 1 (validation stages)      | All four stages enforced by construction (OCAP, `deny_unknown_fields`, `ConvergenceTracker`, `result_feedback`) | Golden-output validation for deterministic-ish skills only                                                      |
| 2 (drift monitoring)       | Human drift channel (`result_feedback`, `SkillSpanStore`); schema validation via `output_schema.rs`             | Automated drift detection over `operator_feedback` trend — BUT `record_skill_span` is never called (see review) |
| 3 (failure classification) | Both failure classes exist (compile-time: manifest parse; runtime: inference, gas, convergence)                 | `execute_skill` returns `Result<String, String>` — can't distinguish classes                                    |

See `compiled-ai-gaps-review.md` for the revised plan.
