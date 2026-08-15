# Gemba Loop: Human-in-the-Loop Guided Evolution Specification

## Status

Future work — specification, not yet implemented.

## Origin

From analysis of Trooskens et al. (2026), *Compiled AI* (arXiv:2604.05150v2).
The paper identifies two gaps: token economics as an explicit metric (gap 4)
and the evolutionary system loop (gap 5). Both share a root cause: kask has
feedback signal infrastructure but no structured human review process.

## The Gemba Concept

From Lean management / Kaizen: *gemba* (現場, "the actual place") is the
practice of going to where value is created to observe and improve. In kask's
digital context, the "actual place" is the running cybernetic regulation
system. The observer is the human operator with the Curator as companion.

**Key constraint: the gemba loop is not autonomous.** The Curator surfaces
signals and proposes actions; the human operator makes refinement decisions.

## Critical Prerequisite

`RegulationLedger::record_skill_span` is defined but **never called** in the
codebase. `SkillSpanStore` is empty at runtime. Before the gemba loop can
function, the emission path must be wired (Step 0 of the revised plan in
`compiled-ai-gaps-review.md`).

## The Six-Phase Loop

1. **Sense** (automated, continuous) — `CyberneticsLoop` + `MetacognitionLoop`
2. **Prepare** (Curator-assisted, on-demand) — `gemba-walk` skill synthesizes briefing
3. **Observe** (human + Curator, interactive) — conversational review
4. **Decide** (human, with Curator recommendations) — operator picks action
5. **Act** (Curator executes approved actions) — via `curator_directive`, `skill-maintenance`
6. **Verify** (automated, continuous) — next sensing cycle shows impact

## Token Economics (Gap 4)

Per-skill briefing: invocation count, token spend, average tokens/invocation,
operator feedback trend, amortization assessment.

## What Needs to Be Built

1. Wire `record_skill_span` emission (prerequisite — see `compiled-ai-gaps-review.md` Step 0)
2. Per-skill token aggregation (tag tool invocations with invoking skill ID)
3. The `gemba-walk` skill (retrieval + synthesis, uses existing tools)
4. Persist convergence metrics as spans (span type exists, needs payload data)

## References

- Trooskens, G., et al. (2026). *Compiled AI* (arXiv:2604.05150v2)
- `compiled-ai-gaps-review.md` — revised plan with verification
- kask `RegulationLedger` — `kask/crates/hkask-regulation/src/runtime.rs`
- kask `MetacognitionLoop` — `kask/crates/hkask-regulation/src/metacognition.rs`
- kask `SkillFeedbackSpan` — `kask/crates/hkask-regulation/src/skill_span.rs`
