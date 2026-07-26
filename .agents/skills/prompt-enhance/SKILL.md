---
name: prompt-enhance
description: >-
  General-purpose prompt enhancement skill for prompts destined for the
  zed-kask platform. Typed routing over a 7-type prompt taxonomy (coding,
  reasoning, creative, classification, extraction, agent-task, meta) with a
  3-tier effort knob (low/medium/high). Composes pragmatic-semantics,
  pragmatic-cybernetics, essentialist, grill-me, and gpa-evolution as phase
  delegates. Default output is the enhanced prompt returned inline in a
  copyable code block; optional file save or both. Use when enhancing,
  refining, or optimizing any prompt that will be consumed by the zed-kask
  platform — skill .j2 templates, agent system prompts, chat/REPL prompts,
  or infrastructure Jinja2 templates.
---

# Prompt Enhance

General-purpose prompt enhancement skill for the zed-kask platform. Classifies prompts against a 7-type taxonomy, applies a typed rewrite with audit findings from three lenses (semantics, cybernetics, essentialist), verifies via a decoupled grill-me critic, and optionally evolves via gpa-evolution at high effort. Specialized leaf of the self-improvement family tree (Σ-pathway, p-component, intrinsic evaluative feedback).

## When to Use

- When you have a prompt destined for zed-kask (skill `.j2` template, agent system prompt, chat/REPL prompt, infrastructure Jinja2 template) and want it enhanced.
- When you want a typed rewrite that applies different moves based on prompt type (coding vs creative vs extraction vs agent-task vs meta).
- When you want to control effort: `low` for a fast single-pass rewrite, `medium` for audit + rewrite + critic, `high` for full evolutionary optimization.
- When you want the enhanced prompt returned inline (default), saved to a file, or both.
- When you want a decoupled critic to prevent the self-confirming loop (generator ≠ critic).
- When you want prompt-engineering best practices (typed taxonomy, textual gradients, beam search, critic decoupling) nested into a single composable skill.

## Inputs

| Input | Type | Default | Description |
|-------|------|---------|-------------|
| `prompt` | string | (required) | The prompt to enhance |
| `effort` | `low` \| `medium` \| `high` | `medium` | Effort tier — controls which phases run |
| `output_format` | `inline` \| `file` \| `both` | `inline` | How to deliver the result |
| `output_path` | string | (derived) | Explicit path for `file`/`both`; default `tasks/enhanced-<type>-<timestamp>.md` |
| `context` | object | (optional) | Target model, intended consumer, existing eval set |

## Effort Tiers

| Tier | Phases | Grill rounds | gpa iterations | Max PDCA | Cost target |
|------|--------|--------------|----------------|----------|-------------|
| `low` | 1 → 3 (single rewrite) | 0 | 0 | 3 | 1× baseline |
| `medium` | 1 → 2 (semantics + essentialist G1+G2) → 3 → 4 (1 round) | 1 | 0 | 6 | ~3× |
| `high` | 1 → 2 (full: +cybernetics) → 3 → 4 (3 escalating) → 5 (min 2 iters) | 3 | ≥2 | 9 | ~10×+ |

## The 7-Type Taxonomy

| Type | Taxonomy anchor | Phase 3 focus | Key risk |
|------|-----------------|---------------|----------|
| `coding` | reasoning & planning | contract clarity, I/O spec, error cases, test-first framing | vague acceptance criteria |
| `reasoning` | reasoning & planning | CoT structure, decomposition, self-verification | hidden assumptions |
| `creative` | profile & instruction | persona depth, constraints as creative tension, examples | over-constraining |
| `classification` | profile & instruction | label space, edge cases, few-shot balance, leakage prevention | label leakage in examples |
| `extraction` | knowledge | schema-first output, field definitions, missing-field policy | underspecified schema |
| `agent-task` | reliability | tool-use contracts, failure modes, context budget, bounded loops | unbounded tool loops |
| `meta` | reliability | self-reference safety, eval harness, convergence, critic decoupling | self-confirming loop |

## Instructions

### Phase 1 — Classify (enhance-classify, step 1)

1. Classify the input prompt against the 7-type taxonomy using pragmatic-semantics IS/OUGHT + epistemic-mode axes.
2. Validate the effort tier and output format (resolve defaults).
3. Synthesize a minimal proxy eval set (3-5 representative inputs) for medium/high tiers; empty at low.
4. Produce the routing decision (which phases to run) and surface type-specific risks.

### Phase 2 — Audit (enhance-audit, step 2, medium/high only)

1. **Pragmatic-semantics lens**: classify every claim, default, and hardcoded reference by IS/OUGHT, epistemic mode, constraint force, and provenance. Flag Inference-tier claims (confidence ≤ 0.3) as fragile.
2. **Pragmatic-cybernetics lens** (high only): treat prompt→model→output as a feedback loop; assess polarity, delay, gain, closure, fidelity; diagnose broken loops.
3. **Essentialist lens**: G1 (deletion test on prompt sections — does complexity reappear in model failures if deleted?) + G2 (≤7 instruction blocks). G3 (contract trace) at high only.
4. Classify every finding by constraint force (Prohibition → Guardrail → Guideline → Evidence → Hypothesis).
5. Gated by `condition: step_1_result.effort_tier != 'low'`.

### Phase 3 — Typed Rewrite (steps 3-9, one runs per cascade)

1. Seven parallel steps, each gated by `condition: step_1_result.prompt_type == '<type>'`. Only one executes.
2. Apply audit findings as targeted mutations — one mutation per finding, Prohibition findings first.
3. Defer Hypothesis-tier findings for user verification (do not mutate based on speculation).
4. If `pdca_iteration > 1`, incorporate Phase 4 grill feedback from the prior iteration (bound as `grill_feedback` from `step_11_result`).
5. Produce the enhanced prompt + acceptance criteria + mutation log.

### Collector (step 10, compute action)

1. Canonicalizes the active rewrite step's output into `enhanced_prompt`, `acceptance_criteria`, `mutations_applied`, `mutations_deferred` context keys.
2. No LLM call — pure context coalescing via the `coalesce_rewrite_output` compute function.
3. Eliminates the need for nested `default()` chains in downstream steps.

### Phase 4 — Verify (enhance-verify, step 11, medium/high only)

1. Run grill-me self-challenge across Recall → Mechanism → Rationale → Edge Cases → Synthesis.
2. Decoupled from Phase 3 — do not defend the prompt you (didn't) write.
3. Tier-scaled rounds: 1 (Recall+Mechanism) at medium; 3 escalating at high.
4. Cross-check against acceptance criteria and proxy eval set.
5. Verdict: `pass` → proceed; `rewrite_needed` → convergence metric forced above threshold (drives PDCA re-entry); `fail` → escalate to user.
6. Gated by `condition: step_1_result.effort_tier != 'low'`.

### Phase 5 — Evolve (enhance-evolve, step 12, high only)

1. Delegate to gpa-evolution's pattern: sample trajectories → reflect (textual gradient) → propose mutations + crossover → test → update Pareto frontier.
2. Min 2 iterations. Multi-objective: (quality, cost) — a prompt that is 5% better but 3× longer is often a loss.
3. Maintain critic decoupling: the reflection step is the critic, the mutation step is the generator.
4. Return the frontier's best member as the final enhanced prompt.
5. Gated by `condition: step_1_result.effort_tier == 'high'`.

### Convergence (enhance-convergence-check, step 13)

1. Tier-scaled thresholds: low (0.30), medium (0.20), high (0.10).
2. Tier-scaled max iterations: low (3), medium (6), high (9).
3. Start at 1.0; subtract for each completed phase.
4. **Grill verdict incorporation**: `rewrite_needed` forces metric above threshold (drives re-entry); `fail` forces `next_action=escalate_to_user`; `pass` uses computed metric.
5. Materiality guard: force convergence if metric delta < 0.02 for ≥ half the tier max (low=1, medium=3, high=4) iterations.
6. `next_action`: `exit` | `re-enter` | `exhausted` | `escalate_to_user`.

### PDCA Loop (step 14)

1. Single loop step. Gated by `condition: step_13_result.next_action == 're-enter'`.
2. If `next_action` is `exit`, `exhausted`, or `escalate_to_user`, the condition is false and the cascade proceeds to output.
3. Carries state via `input_mapping`: `prior_convergence_metric` (feeds materiality guard), `pdca_iteration` (increments counter).
4. Tier max enforced by the convergence-check template returning `exhausted` when the tier cap is hit.

### Output (enhance-output, step 15)

1. Format per `output_format`: `inline` (fenced code block, default), `file` (write to path), or `both`.
2. Always include a change log: summary, audit findings table by constraint force, grill ratings, mutations applied, deferred findings, residual risks.
3. If convergence returned `exhausted` or `escalate_to_user`, surface that in the change log.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `enhance-classify.j2` | KnowAct | Phase 1 — Classify prompt type, select effort tier, synthesize proxy eval set |
| `enhance-audit.j2` | KnowAct | Phase 2 — Audit via semantics, cybernetics, essentialist |
| `enhance-rewrite-coding.j2` | KnowAct | Phase 3 (coding) — Typed rewrite for coding prompts |
| `enhance-rewrite-reasoning.j2` | KnowAct | Phase 3 (reasoning) — Typed rewrite for reasoning prompts |
| `enhance-rewrite-creative.j2` | KnowAct | Phase 3 (creative) — Typed rewrite for creative prompts |
| `enhance-rewrite-classification.j2` | KnowAct | Phase 3 (classification) — Typed rewrite for classification prompts |
| `enhance-rewrite-extraction.j2` | KnowAct | Phase 3 (extraction) — Typed rewrite for extraction prompts |
| `enhance-rewrite-agent-task.j2` | KnowAct | Phase 3 (agent-task) — Typed rewrite for agent task prompts |
| `enhance-rewrite-meta.j2` | KnowAct | Phase 3 (meta) — Typed rewrite for meta-prompts |
| `enhance-collector.j2` | KnowAct | Collector — Coalesce the active rewrite step's output into a canonical context key |
| `enhance-verify.j2` | KnowAct | Phase 4 — Decoupled critic via grill-me self-challenge |
| `enhance-evolve.j2` | KnowAct | Phase 5 — Evolutionary optimization via gpa-evolution delegate (high only) |
| `enhance-convergence-check.j2` | KnowAct | Convergence gate — tier-scaled thresholds + grill verdict incorporation |
| `enhance-output.j2` | KnowAct | ACT phase — Format and deliver the enhanced prompt |

## Constraints

- All templates are `KnowAct` type with `Public` visibility.
- Default effort is `medium`; default output_format is `inline`.
- Phase 5 (evolve) fires only at `high` tier.
- Phase 4 critic is decoupled from Phase 3 generator (self-improvement §9.1).
- Hypothesis-tier findings are never mutated — always deferred for user verification.
- Single PDCA loop (step 14) handles all re-entry; no separate rewrite loop. Grill verdict is incorporated into the convergence metric (step 13) to drive re-entry.
- Max PDCA iterations tier-scaled: low=3, medium=6, high=9; materiality guard forces convergence on irreducible gaps at half the tier max.
- Step conditions use `condition:` (not `skip_condition:`) — the step runs when the condition is true. Supported operators: `==`, `!=`, `<`, `<=`, `>`, `>=`, `AND`, `OR`, `NOT`, dot-paths.
- Loop steps do not evaluate `loop_condition` — they check `convergence_field` + `threshold` + `max_iterations`. Use `condition:` on the loop step to gate re-entry.
- Proxy eval set is auto-synthesized at medium/high; user may override via `context.existing_eval_set`.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.

## Relationship to Other Skills

- **self-improvement**: theoretical parent. prompt-enhance is a specialized leaf (Σ-pathway, p-component, intrinsic evaluative feedback). It borrows the Kata shape (baseline → target → experiment → measure) but not the 10-iteration outer loop.
- **gpa-evolution**: Phase 5 engine. prompt-enhance delegates the evolutionary loop but adds the typed-routing layer and critic decoupling that gpa-evolution alone doesn't enforce.
- **pragmatic-semantics**: Phase 1 classifier + Phase 2 provenance tracer.
- **pragmatic-cybernetics**: Phase 2 loop diagnostician (high tier).
- **essentialist**: Phase 2 deletion test on prompt sections.
- **grill-me**: Phase 4 decoupled critic.
- **task-breakdown**: not used directly — prompt-enhance is a single-prompt service skill, not a multi-task planner. The PDCA shape replaces the Kata outer loop.
