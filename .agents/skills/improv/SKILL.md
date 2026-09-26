---
shipped: false
name: improv
description: "Composable interaction grammar for hKask agents. Five improv modes (Plussing, Yes And, Yes But, Freestyling, Riffing) provide constructive-by-default communication protocols for dual-presence chat, ensemble sessions, and kata coaching loops."
---

# Improv

Composable interaction grammar for hKask agents. Five improv modes — Plussing, Yes And, Yes But, Freestyling, and Riffing — provide constructive-by-default communication protocols for dual-presence chat, ensemble sessions, and kata coaching loops. A selector step evaluates conversation context and routes to the appropriate mode; the mode then shapes one reply.

## Initial and target condition

- **Initial condition:** the current contribution, prior contributions, the active mode (if any), and any kata question in play.
- **Target condition:** the reply follows the selected mode's constraints (see Constraints) as judged by the human participant.
- **PDCA exemption:** each mode is single-pass per contribution; the conversation continues with the human, who judges every reply. The only bounds are the modes' own: Freestyling's declared time bound and Riffing's `max_steps`.

## Step types

| Step | Type | Oracle / critique |
|------|------|-------------------|
| Mode selection; every mode's reply | P | the human participant |
| Yes But literal forbidden words (partial) | D | `lisp_eval` check below; catches literal words only, not contradiction |

## When to Use

- General conversation, kata observation drill, or coaching Q5 amplification → default to Plussing
- Brainstorming, kata five-questions drill, or building momentum → Yes And
- Coaching Q4 (next-step guidance), scope narrowing, or risk assessment → Yes But
- Creative problem-solving, architecture exploration, or ensemble ideation with multiple participants → Freestyling
- Deep-dive on a single contribution, "what if" tangents, or independent research threads → Riffing
- Dual-presence chat, ensemble sessions, and kata coaching loops where constructive-by-default posture is required
- When the agent's own evidence is thin and it needs non-obvious paths to firmer ground — **Riffing** for divergent exploration of tangents that may surface higher-confidence findings, **Plussing** for constructive extraction of agreeable components from uncertain output
- When standard evidence-gathering has plateaued and a perspective-shift (not more data) is the path forward — improv modes reframe rather than accumulate


## When NOT to Use

- Shaping output for a reader — use `adhd-mode`; improv modes shape the conversation, not the rendering.
- Structured decision analysis — use `mcda`; modes build on contributions, they do not weight criteria.
- Factual lookups with one right answer — nothing to build on; answer directly.

## Instructions

### Mode Selection (`improv-select`)
1. Evaluate conversation context, current contribution, active mode, and prior contributions.
2. Select the best-fit improv mode from {plussing, yes-and, yes-but, freestyling, riffing}.
3. Do NOT apply the mode — the agent then renders the selected mode's template.
4. If `active_mode` is provided and still context-appropriate, keep it.
5. Apply kata-specific overrides: Q4 → yes-but, Q5 → plussing, observation drill → plussing, five-questions drill → yes-and.
6. Default to `plussing` when no rule fires.
7. Return `{mode, rationale}`.

### Plussing (`improv-plussing`)
1. Extract agreeable components from the prior contribution; give each an agreeableness `confidence` (0.0–1.0). This is a model estimate used only to rank seeds.
2. Silently discard components judged not agreeable — do not mention or explain them.
3. Build constructively on the top 3 seeds by that estimate, extending with new dimensions, implications, or next steps.
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
6. Partial check: call `lisp_eval` with `reply` bound to the reply text and `(begin (define bad (lambda (ws) (if (= (length ws) 0) (list) (if (string-contains (car ws) reply) (cons (car ws) (bad (cdr ws))) (bad (cdr ws)))))) (bad (list " no " " no," "No " "No," "wrong" "can't" "cannot" "impossible")))`. A non-empty list names the literal words to remove. An empty list does not prove the reply avoids contradiction; that stays the human's judgment.

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

## Registry Templates

| Template | Purpose |
|----------|---------|
| `improv-select.j2` | Pure mode selection. Evaluate conversation context and intent cues to select the best-fit improv mode. Does NOT apply the mode — the agent then renders the selected mode's template. |
| `improv-plussing.j2` | Plussing (Catmull) — Extract agreeable components from a contribution, silently discard the remainder, and build constructively on selected seeds. Never explicitly negate. |
| `improv-yes-and.j2` | Yes And — Accept the whole contribution and extend it with a novel, additive layer. Extension must be additive, not substitutive. |
| `improv-yes-but.j2` | Yes But — Accept the whole contribution and append a constraint or redirect that narrows scope without contradicting. |
| `improv-freestyling.j2` | Freestyling — Rapid collaborative short-response cycling among participants. Time-bounded, no single owner, round-robin turns. |
| `improv-riffing.j2` | Riffing — Solo divergent exploration from a seed contribution. May return to group with synthesis or spawn a new thread. |

To render a template, call the `render_template` tool with the template ref (e.g., `improv/improv-select`) and a context object with the required variables.

## Constraints

- **Visibility:** All templates are `Public`.
- **Never explicitly negate** (Plussing, and governing principle in selector). Criticism is deletion-by-omission.
- **Yes And extension must be additive, not substitutive** — the accepted base remains intact and visible.
- **Yes But constraint narrows, does not contradict** — do not use "no," "wrong," "can't," or "impossible."
- **Freestyling is time-bounded** with round-robin turns and no single owner.
- **Riffing must resolve** — return to group, spawn a thread, or complete within a declared step limit.

- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
