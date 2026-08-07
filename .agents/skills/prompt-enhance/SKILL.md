---
name: prompt-enhance
description: >-
  General-purpose prompt enhancement skill for prompts destined for the
  zed-kask platform. Typed routing over a 7-type prompt taxonomy (coding,
  reasoning, creative, classification, extraction, agent-task, meta) with a
  3-tier effort knob (low/medium/high). Single-pass pipeline: classify,
  rewrite (with inline audit), verify (decoupled critic at medium/high),
  output. Default output is the enhanced prompt returned inline in a
  copyable code block; optional file save or both. Use when enhancing,
  refining, or optimizing any prompt that will be consumed by the zed-kask
  platform — skill .j2 templates, agent system prompts, chat/REPL prompts,
  or infrastructure Jinja2 templates.
---

# Prompt Enhance

General-purpose prompt enhancement skill for the zed-kask platform. Classifies prompts against a 7-type taxonomy, applies a typed rewrite with an inline audit (placeholders, semantic fragility, structural accretion), verifies via a decoupled grill-me critic, and delivers the result. Specialized leaf of the self-improvement family tree (Σ-pathway, p-component, intrinsic evaluative feedback).

## When to Use

- When you have a prompt destined for zed-kask (skill `.j2` template, agent system prompt, chat/REPL prompt, infrastructure Jinja2 template) and want it enhanced.
- When you want a typed rewrite that applies different moves based on prompt type (coding vs creative vs extraction vs agent-task vs meta).
- When you want to control effort: `low` for a fast classify+rewrite+output (3 LLM calls), `medium` for +verify (4 calls), `high` for deeper verify (4 calls).
- When you want the enhanced prompt returned inline (default), saved to a file, or both.
- When you want a decoupled critic to prevent the self-confirming loop (generator ≠ critic).

## Inputs

| Input           | Type                         | Default    | Description                                                                     |
| --------------- | ---------------------------- | ---------- | ------------------------------------------------------------------------------- |
| `prompt`        | string                       | (required) | The prompt to enhance                                                           |
| `effort`        | `low` \| `medium` \| `high`  | `medium`   | Effort tier — controls whether the verify step runs                             |
| `output_format` | `inline` \| `file` \| `both` | `inline`   | How to deliver the result                                                       |
| `output_path`   | string                       | (derived)  | Explicit path for `file`/`both`; default `tasks/enhanced-<type>-<timestamp>.md` |
| `context`       | object                       | (optional) | Target model, intended consumer, existing eval set                              |

## Effort Tiers

| Tier     | Steps run                                                  | LLM calls | Cost target |
| -------- | ---------------------------------------------------------- | --------- | ----------- |
| `low`    | classify → rewrite → output                                | 3         | 1× baseline |
| `medium` | classify → rewrite → verify → output                       | 4         | ~1.5×       |
| `high`   | classify → rewrite → verify (3 escalating rounds) → output | 4         | ~2×         |

## The 7-Type Taxonomy

| Type             | Taxonomy anchor       | Rewrite focus                                                                          | Key risk                  |
| ---------------- | --------------------- | -------------------------------------------------------------------------------------- | ------------------------- |
| `coding`         | reasoning & planning  | contract clarity, I/O spec, error cases, test-first framing                            | vague acceptance criteria |
| `reasoning`      | reasoning & planning  | CoT structure, decomposition, self-verification, counterfactual stress                 | hidden assumptions        |
| `creative`       | profile & instruction | persona depth, constraints as creative tension, audience anchoring                     | over-constraining         |
| `classification` | profile & instruction | label space, edge cases, few-shot balance, tie-breaking policy                         | label leakage in examples |
| `extraction`     | knowledge             | schema-first output, field definitions, missing-field policy, type discipline          | underspecified schema     |
| `agent-task`     | reliability           | tool-use contracts, failure modes, context budget, bounded loops, termination criteria | unbounded tool loops      |
| `meta`           | reliability           | self-reference safety, eval harness, convergence criteria, critic decoupling           | self-confirming loop      |

## Instructions

### Step 1 — Classify (enhance-classify.j2)

1. Classify the input prompt against the 7-type taxonomy using pragmatic-semantics IS/OUGHT + epistemic-mode axes.
2. Validate the effort tier and output format (resolve defaults).
3. Synthesize a minimal proxy eval set (3-5 representative inputs) for medium/high tiers; empty at low.
4. Produce the routing decision and surface type-specific risks.

### Step 2 — Rewrite (enhance-rewrite.j2)

1. **Inline audit** (internal): scan for unresolved placeholders (Prohibition), semantic fragility (Guardrail), and structural accretion (essentialist G1+G2).
2. **Typed rewrite**: apply type-specific moves based on `prompt_type` from step 1.
3. **Mutation discipline**: each finding → at most one mutation; Prohibition findings must be addressed; Hypothesis-tier findings deferred.
4. Produce the enhanced prompt + acceptance criteria + mutations applied/deferred + audit findings.

### Step 3 — Verify (enhance-verify.j2, medium/high only)

1. Run grill-me self-challenge across Recall → Mechanism → Rationale → Edge Cases → Synthesis.
2. Decoupled from step 2 — do not defend the prompt you (didn't) write.
3. Tier-scaled rounds: 1 (Recall+Mechanism) at medium; 3 escalating at high.
4. Verdict: `pass`, `rewrite_needed`, or `fail`. No PDCA re-entry — the verdict is surfaced in the output change log.
5. Gated by `condition: step_1_result.effort_tier != 'low'`.

### Step 4 — Output (enhance-output.j2)

1. Format per `output_format`: `inline` (fenced code block, default), `file` (write to path), or `both`.
2. Always include a change log: summary, audit findings table by constraint force, grill verdict + ratings, mutations applied, deferred findings, residual risks.

## Registry Templates

| Template              | Type    | Purpose                                                         |
| --------------------- | ------- | --------------------------------------------------------------- |
| `enhance-classify.j2` | KnowAct | Classify prompt type, select effort tier, resolve output format |
| `enhance-rewrite.j2`  | KnowAct | Inline audit + typed rewrite (unified)                          |
| `enhance-verify.j2`   | KnowAct | Decoupled critic via grill-me self-challenge (medium/high)      |
| `enhance-output.j2`   | KnowAct | Format and deliver the enhanced prompt                          |

## Constraints

- All templates are `KnowAct` type with `Public` visibility.
- Default effort is `medium`; default output_format is `inline`.
- Single-pass pipeline — no PDCA loop. `max_iterations: 1` prevents re-entry.
- Verify step is decoupled from the rewrite step (self-improvement §9.1).
- Hypothesis-tier findings are never mutated — always deferred for user verification.
- Step conditions use `condition:` (not `skip_condition:`) — the step runs when the condition is true.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.

## Relationship to Other Skills

- **self-improvement**: theoretical parent. prompt-enhance is a specialized leaf (Σ-pathway, p-component, intrinsic evaluative feedback).
- **pragmatic-semantics**: classifier + provenance tracer (folded into the rewrite step's inline audit).
- **essentialist**: deletion test on prompt sections (folded into the rewrite step's inline audit).
- **grill-me**: decoupled critic (verify step).
