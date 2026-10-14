---
name: improv
description: "Composable interaction grammar for hKask agents. Five improv modes (Plussing, Yes And, Yes But, Freestyling, Riffing) provide constructive-by-default communication protocols for dual-presence chat, ensemble sessions, and kata coaching loops."
---

# Improv

Composable interaction grammar for hKask agents. Five improv modes — Plussing, Yes And, Yes But, Freestyling, and Riffing — provide constructive-by-default communication protocols for dual-presence chat, ensemble sessions, and kata coaching loops. A selector step evaluates conversation context and routes to the appropriate mode; the mode then shapes one reply.

## Reference models

Johnstone, *Impro: Improvisation and the Theatre* (1979) — accepting and building on an offer (`onto_anchor` → derived `yes_and`); Catmull, *Creativity, Inc.* (2014) — plussing as Pixar's constructive-critique rule (`onto_anchor` → derived `plussing`); both operator rulings 2026-09-26. Yes But, Freestyling and Riffing are this skill's own extensions of those two sources — named modes with no independent published definition; their constraints below are the operative spec. Per-step: Yes And's accept-then-extend copies Johnstone's offer rule; Plussing's never-negate copies Catmull's rule, while its confidence ranking, silent discard and top-3 build are this skill's own mechanisms (unanchored); Yes But, Freestyling and Riffing are unanchored throughout.

## Initial and target condition

- **Initial condition:** the current contribution, prior contributions, the active mode (if any), and any kata question in play.
- **Target condition:** the reply follows the selected mode's constraints (see Constraints) as judged by the human participant.
- **PDCA exemption:** each mode is single-pass per contribution; the conversation continues with the human, who judges every reply. The only bounds are the modes' own: Freestyling's declared time bound and Riffing's `max_steps`.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|------|------|-------------------|
| Mode selection (`improv-select`); every mode's reply | P | the human participant |
| Kata pre-router (`IMP-1`) | D | `lisp_eval` deterministic mapping of `kata_stage` overrides before invoking `improv-select` |
| Yes But literal forbidden words (`IMP-2`, partial) | D | `lisp_eval` check below; catches literal words only, not contradiction |
| Plussing seed confidence ranking | P | a model estimate used only to rank seeds — never reported as a finding |
| Freestyling & Riffing bounds (`IMP-3`: `time_remaining`, `next_speaker`, `steps_remaining`) | D | `lisp_eval` arithmetic over `time_bound`, `elapsed_secs`, `participants`, `turn_index`, `max_steps`, and `steps_taken` |

## When to Use

- Mode routing is the selector's job: render `improv-select` — its rendered guide carries the full context-to-mode mapping, including the kata overrides (bound to kata-improvement's coaching-question numbering)
- Dual-presence chat, ensemble sessions, and kata coaching loops where constructive-by-default posture is required
- When the agent's own evidence is thin and it needs non-obvious paths to firmer ground — **Riffing** for divergent exploration of tangents that may surface higher-confidence findings, **Plussing** for constructive extraction of agreeable components from uncertain output
- When standard evidence-gathering has plateaued and a perspective-shift (not more data) is the path forward — improv modes reframe rather than accumulate


## When NOT to Use

- Shaping output for a reader — use `adhd-mode`; improv modes shape the conversation, not the rendering.
- Structured decision analysis — use `mcda`; modes build on contributions, they do not weight criteria.
- Factual lookups with one right answer — nothing to build on; answer directly.

## Instructions

### Mode Selection (`IMP-1` + `improv-select`)
1. First run deterministic `lisp_eval` pre-router `IMP-1` when a kata stage or override is present in context (`q4 -> yes-but`, `q5/observation-drill -> plussing`, `pdca-cycle -> yes-and`):
   - form: `(cond ((string= kata_stage "q4") "yes-but") ((or (string= kata_stage "q5") (string= kata_stage "observation-drill")) "plussing") ((string= kata_stage "pdca-cycle") "yes-and") (true "none"))`
   - env: `{ "kata_stage": <"q4"|"q5"|"observation-drill"|"pdca-cycle"|"none"> }`
   When `IMP-1` returns a mode other than `"none"`, use that mode directly and skip `improv-select.j2`.
2. Otherwise, render `improv/improv-select` to evaluate conversation context, current contribution, active mode, and prior contributions, and select the best-fit improv mode from {plussing, yes-and, yes-but, freestyling, riffing}.
3. Do NOT apply the mode in the selector — the agent then renders the selected mode's template.
4. If `active_mode` is provided and still context-appropriate, keep it.
5. Apply kata-specific overrides: Q4 → yes-but, Q5 → plussing, observation drill → plussing, pdca-cycle drill → yes-and.
6. Default to `plussing` when no rule fires.
7. Return `{mode, rationale}`.

### Plussing (`improv-plussing`)
1. Extract agreeable components from the prior contribution; give each an agreeableness `confidence` (0.0–1.0). This is a model estimate used only to rank seeds.
2. Silently discard components judged not agreeable — do not mention or explain them.
3. Build constructively on the top 3 seeds by that estimate, extending with new dimensions, implications, or next steps.
4. Never explicitly negate. Criticism is deletion-by-omission.
5. If nothing is agreeable, redirect constructively without referencing the disagreeable content.
6. Return `{selected_seeds, build, discarded_count}`.

### Yes And (`improv-yes-and`)
1. Accept the whole contribution unchanged; acknowledge it explicitly.
2. Extend with a novel additive layer — a new dimension, implication, example, or next step.
3. Signal that the extension is additive, not substitutive; the accepted base must remain intact and visible.
4. Return `{accepted_base, extension}`.

### Yes But (`improv-yes-but`)
1. Accept the whole contribution unchanged; acknowledge it explicitly.
2. Identify a boundary condition that narrows scope: resource constraint, compatibility requirement, risk to mitigate, or sequencing consideration.
3. Frame as additive guidance ("yes, and let's also account for…"), not rejection. Do not say "no," "wrong," "can't," or "impossible."
4. Ensure the constraint narrows without contradicting the accepted base.
5. Return `{accepted_base, constraint}`.
6. Partial check (`IMP-2`): call `lisp_eval` with `reply` bound to the reply text and `(begin (define bad (lambda (ws) (if (= (length ws) 0) (list) (if (string-contains (car ws) reply) (cons (car ws) (bad (cdr ws))) (bad (cdr ws)))))) (bad (list " no " " no," "No " "No," "wrong" "can't" "cannot" "impossible")))`. A non-empty list names the literal words to remove. An empty list does not prove the reply avoids contradiction; that stays the human's judgment.

### Freestyling (`improv-freestyling`)
1. Initiate the session with a declared time bound and participant list.
2. Cycle through participants in round-robin order; each turn is short (1–3 sentences), associative, and builds on the prior turn's energy.
3. Compute `time_remaining` and `next_speaker` deterministically via `lisp_eval` `IMP-3` (when time expires, signal session end and summarize emergent themes):
   - form: `(list (max 0 (- time_bound elapsed_secs)) (nth next_idx participants))`
   - env: `{ "time_bound": <secs>, "elapsed_secs": <secs>, "participants": <participant array>, "next_idx": <(turn_index + 1) mod length(participants)> }`
4. Mark each turn `[freestyle turn N by AGENT] content`.
5. Return `{turn, session_summary}` from the template plus `IMP-3`'s `{time_remaining, next_speaker}`.

### Riffing (`improv-riffing`)
1. Diverge from the seed contribution; identify an interesting dimension, implication, or "what if."
2. Explore the tangent independently — go deep, wide, or weird without group constraint.
3. Resolve per return policy: `ReturnToGroup` (synthesize and bridge back), `SpawnThread` (create new thread), or `ReturnAfterSteps { max_steps }` (explore up to N steps then return). Compute `steps_remaining` deterministically via `lisp_eval` `IMP-3`:
   - form: `(max 0 (- max_steps (length steps_taken)))`
   - env: `{ "max_steps": <max_steps>, "steps_taken": <steps_taken array> }`
4. The riff must resolve — it cannot hang indefinitely.
5. Return `{tangent, outcome, synthesis, thread_id}` from the template plus `IMP-3`'s `steps_remaining`.

## Regression case

Render `improv/improv-select` with `context`, `contribution`, `active_mode` (null or a mode), and `previous_contributions` — it returns `{mode, rationale}`. Render `improv/improv-yes-but` with `context` and a `contribution`, then run the forbidden-words check both ways with `lisp_eval`: a reply containing " no " returns a non-empty list naming it; a clean reply ("Yes — and let's also account for the deployment constraint") returns an empty list, which does NOT prove the reply avoids contradiction (that stays the human's judgment). The check is needle-first — `(string-contains (car ws) reply)`, the word is the needle — and is verified by the receipts above (executed live through `lisp_eval`). Also render `improv/improv-freestyling` with `participants`, `session`, `time_bound`, and `improv/improv-riffing` with `seed`, `steps_taken`, `return_policy`.

## Registry Templates

| Template | Purpose | Context |
|----------|---------|---------|
| `improv-select.j2` | Pure mode selection. Evaluate conversation context and intent cues to select the best-fit improv mode. Does NOT apply the mode — the agent then renders the selected mode's template. | `context`, `contribution`, `active_mode`, `previous_contributions` |
| `improv-plussing.j2` | Plussing (Catmull) — Extract agreeable components from a contribution, silently discard the remainder, and build constructively on selected seeds. Never explicitly negate. | `context`, `contribution` |
| `improv-yes-and.j2` | Yes And — Accept the whole contribution and extend it with a novel, additive layer. Extension must be additive, not substitutive. | `context`, `contribution` |
| `improv-yes-but.j2` | Yes But — Accept the whole contribution and append a constraint or redirect that narrows scope without contradicting. | `context`, `contribution` |
| `improv-freestyling.j2` | Freestyling — Rapid collaborative short-response cycling among participants. Time-bounded, no single owner, round-robin turns. | `participants`, `session`, `time_bound` |
| `improv-riffing.j2` | Riffing — Solo divergent exploration from a seed contribution. May return to group with synthesis or spawn a new thread. | `seed`, `steps_taken`, `return_policy` |

To render a template, call the `render_template` tool with the template ref (e.g., `improv/improv-select`) and a context object with the required variables.

## Constraints

- **Never explicitly negate** (Plussing, and governing principle in selector). Criticism is deletion-by-omission.
- **Yes And extension must be additive, not substitutive** — the accepted base remains intact and visible.
- **Yes But constraint narrows, does not contradict** — do not use "no," "wrong," "can't," or "impossible."
- **Freestyling is time-bounded** with round-robin turns and no single owner.
- **Riffing must resolve** — return to group, spawn a thread, or complete within a declared step limit.

