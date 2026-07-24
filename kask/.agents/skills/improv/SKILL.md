---
name: improv
visibility: public
description: "Composable interaction grammar for hKask agents. Five improv modes (Plussing, Yes And, Yes But, Freestyling, Riffing) provide constructive-by-default communication protocols for dual-presence chat, ensemble sessions, and kata coaching loops."
---

# Improv

Composable interaction grammar for hKask agents. Five improv modes — Plussing, Yes And, Yes But, Freestyling, and Riffing — provide constructive-by-default communication protocols for dual-presence chat, ensemble sessions, and kata coaching loops. A selector KnowAct evaluates conversation context and routes to the appropriate WordAct; a convergence checker closes the PDCA cycle.

## When to Use

- General conversation, kata observation drill, or coaching Q5 amplification → default to Plussing
- Brainstorming, kata five-questions drill, or building momentum → Yes And
- Coaching Q4 (next-step guidance), scope narrowing, or risk assessment → Yes But
- Creative problem-solving, architecture exploration, or ensemble ideation with multiple participants → Freestyling
- Deep-dive on a single contribution, "what if" tangents, or independent research threads → Riffing
- Dual-presence chat, ensemble sessions, and kata coaching loops where constructive-by-default posture is required
- When an agent is in a low-confidence regime (confidence < 0.5) and needs non-obvious paths to higher certainty — **Riffing** for divergent exploration of tangents that may surface higher-confidence findings, **Plussing** for constructive extraction of agreeable components from uncertain output
- When standard evidence-gathering has plateaued and a perspective-shift (not more data) is the path forward — improv modes reframe rather than accumulate
- After mode selection and application, run convergence-check to verify alignment and constructiveness

## Instructions

### Mode Selection (`improv-select`)
1. Evaluate conversation context, current contribution, active mode, and prior contributions.
2. Select the best-fit improv mode from {plussing, yes-and, yes-but, freestyling, riffing}.
3. Do NOT apply the mode — routing to individual WordActs is handled by the manifest flow.
4. If `active_mode` is provided and still context-appropriate, keep it.
5. Apply kata-specific overrides: Q4 → yes-but, Q5 → plussing, observation drill → plussing, five-questions drill → yes-and.
6. Default to `plussing` when no rule fires.
7. Return `{mode, rationale}`.

### Plussing (`improv-plussing`)
1. Extract agreeable components from the prior contribution; score each by agreeableness confidence (0.0–1.0).
2. Silently discard components with zero or negative agreeableness — do not mention or explain them.
3. Build constructively on the top 3 selected seeds, extending with new dimensions, implications, or next steps.
4. Never explicitly negate. Criticism is deletion-by-omission.
5. If nothing is agreeable, redirect constructively without referencing the disagreeable content.
6. Return `{selected_seeds, build, discarded_count, reg_span}`.

### Yes And (`improv-yes-and`)
1. Accept the whole contribution unchanged; acknowledge it explicitly.
2. Extend with a novel additive layer — a new dimension, implication, example, or next step.
3. Signal that the extension is additive, not substitutive; the accepted base must remain intact and visible.
4. Return `{accepted_base, extension, reg_span}`.

### Yes But (`improv-yes-but`)
1. Accept the whole contribution unchanged; acknowledge it explicitly.
2. Identify a boundary condition that narrows scope: resource constraint, compatibility requirement, risk to mitigate, or sequencing consideration.
3. Frame as additive guidance ("yes, and let's also account for…"), not rejection. Do not say "no," "wrong," "can't," or "impossible."
4. Ensure the constraint narrows without contradicting the accepted base.
5. Return `{accepted_base, constraint, reg_span}`.

### Freestyling (`improv-freestyling`)
1. Initiate the session with a declared time bound and participant list.
2. Cycle through participants in round-robin order; each turn is short (1–3 sentences), associative, and builds on the prior turn's energy.
3. Track time remaining; when the time bound is reached, signal session end and summarize emergent themes.
4. Record all turns for Regulation coherence analysis.
5. Mark each turn `[freestyle turn N by AGENT] content`.
6. Return `{turn, time_remaining, next_speaker, session_summary, reg_span}`.

### Riffing (`improv-riffing`)
1. Diverge from the seed contribution; identify an interesting dimension, implication, or "what if."
2. Explore the tangent independently — go deep, wide, or weird without group constraint.
3. Resolve per return policy: `ReturnToGroup` (synthesize and bridge back), `SpawnThread` (create new thread), or `ReturnAfterSteps { max_steps }` (explore up to N steps then return).
4. The riff must resolve — it cannot hang indefinitely.
5. Return `{tangent, outcome, synthesis, thread_id, steps_remaining, reg_span}`.

### Convergence Check (`improv-convergence-check`)
1. Start the metric at 1.0.
2. Subtract 0.3 if mode is present and in {plussing, yes-and, yes-but, freestyling, riffing}.
3. Subtract 0.4 if response content is present and mode-appropriate.
4. Subtract 0.2 if application output includes coherent Regulation signaling (e.g., `reg_spans` or `reg_span`).
5. If evidence indicates explicit contradiction/negation instead of constructive extension, keep metric ≥ 0.7.
6. Clamp to [0,1].
7. Return `{convergence_metric, convergence_method, rationale, blockers, unresolved_signals}`.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `improv-select.j2` | `KnowAct` | Pure mode selection. Evaluate conversation context and intent cues to select the best-fit improv mode. Does NOT apply the mode — routing to individual WordActs is handled by the manifest flow. |
| `improv-plussing.j2` | `WordAct` | Plussing (Catmull) — Extract agreeable components from a contribution, silently discard the remainder, and build constructively on selected seeds. Never explicitly negate. |
| `improv-yes-and.j2` | `WordAct` | Yes And — Accept the whole contribution and extend it with a novel, additive layer. Extension must be additive, not substitutive. |
| `improv-yes-but.j2` | `WordAct` | Yes But — Accept the whole contribution and append a constraint or redirect that narrows scope without contradicting. |
| `improv-freestyling.j2` | `WordAct` | Freestyling — Rapid collaborative short-response cycling among participants. Time-bounded, no single owner, round-robin turns. |
| `improv-riffing.j2` | `WordAct` | Riffing — Solo divergent exploration from a seed contribution. May return to group with synthesis or spawn a new thread. |
| `improv-convergence-check.j2` | `KnowAct` | Compute normalized convergence for improv mode-selection/application PDCA cycles and report unresolved constructiveness signals. |

## Constraints

- **Visibility:** All templates are `Public`.
- **Energy caps:** `improv-select` 4096; `improv-plussing` 4096; `improv-yes-and` 4096; `improv-yes-but` 4096; `improv-freestyling` 4096; `improv-riffing` 4096; `improv-convergence-check` 2048.
- **Never explicitly negate** (Plussing, and governing principle in selector). Criticism is deletion-by-omission.
- **Yes And extension must be additive, not substitutive** — the accepted base remains intact and visible.
- **Yes But constraint narrows, does not contradict** — do not use "no," "wrong," "can't," or "impossible."
- **Freestyling is time-bounded** with round-robin turns and no single owner.
- **Riffing must resolve** — return to group, spawn a thread, or complete within a declared step limit.
- **Convergence threshold** defaults to 0.15; max iterations default to 3; improvement target default 0.10.
- **Registry is authoritative** — when this SKILL.md disagrees with registry templates, the registry wins (P5.1).
