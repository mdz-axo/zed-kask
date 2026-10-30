---
name: improv
description: "Composable interaction grammar for hKask agents. Five improv modes (Plussing, Yes And, Yes But, Freestyling, Riffing) provide constructive-by-default communication protocols for dual-presence chat, ensemble sessions, and kata coaching loops."
audience: user
---

# Improv

Composable interaction grammar for hKask agents. Five improv modes — Plussing, Yes And, Yes But, Freestyling, and Riffing — provide constructive-by-default communication protocols for dual-presence chat, ensemble sessions, and kata coaching loops. A selector step evaluates conversation context and routes to the appropriate mode; the mode then shapes one reply.

## Reference models

Johnstone, *Impro: Improvisation and the Theatre* (1979) — accepting and building on an offer (`onto_anchor` → derived `yes_and`); Catmull, *Creativity, Inc.* (2014) — plussing as Pixar's constructive-critique rule (`onto_anchor` → derived `plussing`); both operator rulings 2026-09-26. Yes But, Freestyling and Riffing are this skill's own extensions of those two sources — named modes with no independent published definition; their constraints below are the operative spec. Per-step: Yes And's accept-then-extend copies Johnstone's offer rule; Plussing's never-negate copies Catmull's rule, while its confidence ranking, silent discard and top-3 build are this skill's own mechanisms (unanchored); Yes But, Freestyling and Riffing are unanchored throughout.

## Initial and target condition

- **Initial condition:** the current contribution, prior contributions, the active mode (if any), and any kata question in play.
- **Target condition:** the reply follows the selected mode's constraints (see Constraints) as judged by the human participant.
- **Per-contribution Check/Act:** select the mode and draft one reply; check it against that mode's named Constraints and `IMP-2`/`IMP-3` where applicable. On a passing Check, deliver for the human participant's judgment. On a named failure, revise that reply once and recheck; if it still fails, report the unmet constraint and stop. Freestyling's time bound and Riffing's `max_steps` remain additional bounds. This adapts PDCA (Lean Enterprise Institute's Plan, Do, Check, Act); it checks the contribution, never evaluates or accepts the skill itself.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|------|------|-------------------|
| Mode selection (`improv-select`); every mode's reply | P | the human participant |
| Kata pre-router (`IMP-1`) | D | `lisp_eval` deterministic mapping of `kata_stage` overrides before invoking `improv-select` |
| Yes But literal forbidden words (`IMP-2`, partial) | D | `lisp_eval` check below; catches literal words only, not contradiction |
| Plussing seed confidence ranking | P | a model estimate used only to rank seeds — never reported as a finding |
| Freestyling & Riffing bounds (`IMP-3`: `time_remaining`, `next_speaker`, `steps_remaining`) | D | `lisp_eval` arithmetic over `time_bound`, `elapsed_secs`, `participants`, `turn_index`, `max_steps`, and `steps_taken` |
| Per-contribution Check/Act | D + P | `IMP-2`/`IMP-3` supply partial mechanical evidence; mode adherence is a semantic assessment, with the human participant retaining judgment |

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

### Per-contribution Check/Act (all selected modes)
1. **Plan/Do:** select the mode above, render its template with the complete context below, and draft one reply using that mode's instructions. For Freestyling, run `IMP-3` before another turn; at zero time remaining, end and summarize instead. For `ReturnAfterSteps`, run `IMP-3` before exploring; at zero steps remaining, return with synthesis instead.
2. **Check:** name any unmet constraint from that mode's Constraints and instructions. Check mode adherence semantically; run `IMP-2` for Yes But and `IMP-3` for the applicable time/step bounds. The passing threshold is zero named unmet constraints and no failed mechanical check. An empty `IMP-2` list is only a literal-word pass, not proof of semantic adherence.
3. **Act:** if the first Check passes, deliver for human judgment without a gratuitous second run. Otherwise re-enter that mode's draft step once, revising the reply against the named failure, then repeat Check. If the second Check fails, report the unmet constraint and stop; do not silently deliver the failed draft. Never restart the revision budget by reselecting the mode. The human may identify a semantic failure even after a mechanical pass.
4. An invalid-input result or tool error is a visible blocked Check, never a passing reply: report its reason and stop pending corrected inputs. Do not fabricate a speaker, remaining bound, thread identifier, or successful check. Revision cannot extend the declared time/step limits.

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
   - form: `(begin (define names-ok (lambda (xs) (if (= (length xs) 0) true (and (stringp (car xs)) (not (string= (car xs) "")) (names-ok (cdr xs)))))) (define wrap-index (lambda (i n) (if (< i n) i (if (< (- i n) n) (- i n) (let ((r (wrap-index i (+ n n)))) (if (>= r n) (- r n) r)))))) (cond ((not (listp participants)) "invalid_participants") ((or (= (length participants) 0) (not (names-ok participants))) "invalid_participants") ((not (numberp turn_index)) "invalid_turn_index") ((or (not (eq (- turn_index turn_index) 0)) (< turn_index 0)) "invalid_turn_index") ((not (and (numberp time_bound) (numberp elapsed_secs))) "invalid_time") ((or (< time_bound 0) (< elapsed_secs 0)) "invalid_time") (true (let ((n (length participants)) (r (wrap-index turn_index (length participants)))) (list (max 0 (- time_bound elapsed_secs)) (nth (if (= r (- n 1)) 0 (+ r 1)) participants))))))`
   - env: `{ "time_bound": 60, "elapsed_secs": 10, "participants": ["a", "b", "c"], "turn_index": 0 }`. Pass as JSON text to `lisp_eval`, substituting the raw session values.
   - `turn_index` is the zero-based ordinal of the turn just completed, not a precomputed participant index. The result is `[time_remaining, next_speaker]`, equivalent to advancing one participant after that completed turn. Participants must be a nonempty array of nonempty strings; `turn_index` must be a nonnegative JSON integer (even `1.0` is rejected); times must be nonnegative numbers. Invalid values return the named string above and block the contribution; missing bindings or exhausted interpreter budgets surface as tool errors, never success.
   - Integer admission uses type-sensitive `eq`: subtracting an integer from itself yields integer zero, while a float yields float zero. The remainder helper doubles only when `2*n <= i`, avoiding integer overflow; reduction precedes the increment so even the maximum signed 64-bit index is supported. Recursion is logarithmic in the index; participant validation traverses the list and remains subject to the interpreter's finite budgets. At zero time remaining the speaker is bookkeeping only: do not emit another turn.
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

1. Render `improv/improv-select` with the complete context below, select Yes But for scope narrowing, then render `improv/improv-yes-but` with its complete context. The renderer returns a guide, not the generated `{mode, rationale}` or reply: the agent produces those outputs by following the guide.
2. Execute Check/Act on a clean draft, `Yes — and let's also account for the deployment constraint`: `IMP-2` with JSON-text env `{"reply":"Yes — and let's also account for the deployment constraint"}` must return `[]`; deliver for human judgment without revision. For `Yes, no deployment before the checklist is ready`, it must return `[" no "]`; name the failure, revise once to the clean draft, and recheck to `[]`. Reusing the failing draft on the recheck must instead report the unmet constraint and stop. These are literal-word controls, not human semantic acceptance. A tool error (including a needle longer than the reply) blocks Check visibly.
3. Run the exact Freestyling `IMP-3` form against each row below, merging overrides into the example env. Compare actual results to the fixed expectations; do not supply a model-computed index. Also run Riffing's unchanged form with `{"max_steps":2,"steps_taken":["a","b"]}` → `0` and with one step → `1`. Render both bounded modes using the contexts below; verify that zero remaining time/steps routes to summary/return, not more exploration.

| Freestyling env override | Expected result |
|---|---|
| none | `[50,"b"]` |
| `turn_index: 2` | `[50,"a"]` |
| `participants: ["a"], elapsed_secs: 80` | `[0,"a"]` (end session) |
| `elapsed_secs: 60` | `[0,"b"]` (end session) |
| `turn_index: 1000000` | `[50,"c"]` |
| `turn_index: 9223372036854775807` | `[50,"c"]` |
| `turn_index: -1`, `1.5`, `1.0`, `"1"`, `true`, `null`, `[]`, or `{}` (each separately) | `"invalid_turn_index"` |
| `participants: []`, `null`, `"a"`, `{ "a": 1 }`, `[1]`, or `[""]` (each separately) | `"invalid_participants"` |
| `time_bound: "60"`, `elapsed_secs: true`, or `elapsed_secs: -1` (each separately) | `"invalid_time"` |
| `time_bound: 0.5, elapsed_secs: 0.25` | `[0.25,"b"]` |

## Registry Templates

| Template | Purpose | Context |
|----------|---------|---------|
| `improv-select.j2` | Pure mode selection. Evaluate conversation context and intent cues to select the best-fit improv mode. Does NOT apply the mode — the agent then renders the selected mode's template. | `context`, `contribution`, `active_mode`, `previous_contributions` |
| `improv-plussing.j2` | Plussing (Catmull) — Extract agreeable components from a contribution, silently discard the remainder, and build constructively on selected seeds. Never explicitly negate. | `context`, `contribution` |
| `improv-yes-and.j2` | Yes And — Accept the whole contribution and extend it with a novel, additive layer. Extension must be additive, not substitutive. | `context`, `contribution` |
| `improv-yes-but.j2` | Yes But — Accept the whole contribution and append a constraint or redirect that narrows scope without contradicting. | `context`, `contribution` |
| `improv-freestyling.j2` | Freestyling — Rapid collaborative short-response cycling among participants. Time-bounded, no single owner, round-robin turns. | `participants`, `session`, `time_bound` |
| `improv-riffing.j2` | Riffing — Solo divergent exploration from a seed contribution. May return to group with synthesis or spawn a new thread. | `seed`, `steps_taken`, `return_policy` |

Call `render_template` with the selected `template_ref` and `context` as complete JSON text. Substitute actual context values into these copyable shapes; template rendering supplies instructions, not a generated contribution or a Check verdict. Keep computed bounds in the caller's handoff; the mode templates do not compute them.

`improv/improv-select` (`active_mode` is a string; use `""` when unset):
```json
{"context":{"intent":"scope narrowing"},"contribution":"Deploy the pilot","active_mode":"","previous_contributions":[]}
```
`improv/improv-plussing`, `improv/improv-yes-and`, or `improv/improv-yes-but`:
```json
{"context":{"intent":"scope narrowing"},"contribution":"Deploy the pilot"}
```
`improv/improv-freestyling` (`session` is a string):
```json
{"participants":["a","b","c"],"session":"Turn 0 by a completed; 10 seconds elapsed of 60. IMP-3 returned [50, b]; b speaks next.","time_bound":60}
```
`improv/improv-riffing` (supply `max_steps` separately to `IMP-3` for `ReturnAfterSteps`):
```json
{"seed":{"contribution":"Deploy the pilot"},"steps_taken":["Check deployment constraints"],"return_policy":"ReturnAfterSteps { max_steps: 2 }"}
```

## Constraints

- **Never explicitly negate** (Plussing, and governing principle in selector). Criticism is deletion-by-omission.
- **Yes And extension must be additive, not substitutive** — the accepted base remains intact and visible.
- **Yes But constraint narrows, does not contradict** — do not use "no," "wrong," "can't," or "impossible."
- **Freestyling is time-bounded** with round-robin turns and no single owner.
- **Riffing must resolve** — return to group, spawn a thread, or complete within a declared step limit.

