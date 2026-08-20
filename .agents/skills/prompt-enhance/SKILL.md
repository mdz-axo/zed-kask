---
name: prompt-enhance
description: "General-purpose prompt enhancement for the zed-kask platform. Typed routing over a 7-type prompt taxonomy with a 3-tier effort knob. Use when enhancing prompts for skill templates, agent system prompts, or chat REPL prompts."
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
5. Gated by the result of step 1's `effort_tier != 'low'`.

### Step 4 — Output (enhance-output-render.j2, deterministic render)

1. Format per `output_format`: `inline` (fenced code block, default), `file` (write to path), or `both`.
2. Always include a change log: summary, audit findings table by constraint force, grill verdict + ratings, mutations applied, deferred findings, residual risks.
3. This step is a `render` action (no LLM call) — the change log is templated from structured data. The former LLM-call version was the single largest source of process failures (empty output → JSON parse error after 4 successful LLM calls).

## Registry Templates

| Template | Purpose |
|----------|---------|
| `enhance-classify.j2` |  | Classify the input prompt against the 7-type taxonomy (coding, reasoning, creative, classification, extraction, agent-task, meta) using pragmatic- semantics IS/OUGHT + epistemic-mode axes. Select the effort tier (low/medium/high) and validate the output_format (inline/file/both, default inline). Synthesize a minimal proxy eval set (3-5 representative inputs) for medium/high tiers so downstream phases have a signal to optimize against. Produces the routing decision that drives step 2. |
| `enhance-rewrite.j2` |  | Inline audit + typed rewrite. Scans for unresolved placeholders, semantic fragility, and structural accretion, then applies type-specific rewrite moves based on the prompt_type from step 1. Folds the former separate audit step and 7 typed rewrite variants into a single LLM call. Produces the enhanced prompt, audit findings, and mutations applied. |
| `enhance-verify.j2` |  | Decoupled critic. Runs grill-me self-challenge against the enhanced prompt across Recall -> Mechanism -> Rationale -> Edge Cases -> Synthesis. Decoupled from step 2 to prevent the self-confirming loop. Tier-scaled: 1 round (Recall+Mechanism) at medium, 3 escalating rounds at high. Skipped at low tier. Produces a Solid/Partial/Gap rating per area. |
| `enhance-output.j2` |  | Format the final enhanced prompt per output_format (inline/file/both). inline (default): return the prompt in a fenced code block with a one-line summary of changes and the audit findings table. file: write the prompt to a path the user specifies (or a derived default). both: inline + file. Always include a change-log section. |
| `enhance-output-render.j2` |  | Render-only variant of enhance-output for programmatic delivery without an LLM round-trip. Formats the enhanced prompt per output_format. |
| `enhance-audit.j2` |  | Audit the input prompt through three lenses: pragmatic-semantics (classify claims by IS/OUGHT, epistemic mode, constraint force), pragmatic-cybernetics (feedback loop properties), and essentialist (deletion test + surface count). Not referenced by the current process manifest — the audit is folded into enhance-rewrite.j2. Retained for potential future re-decomposition. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- All templates are prompt templates with `Public` visibility.
- Default effort is `medium`; default output_format is `inline`.
- Single-pass pipeline — no PDCA loop. `max_iterations: 1` prevents re-entry.
- Verify step is decoupled from the rewrite step (self-improvement §9.1).
- Hypothesis-tier findings are never mutated — always deferred for user verification.
- Step conditions use a condition check (the step runs when the condition is true).
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.

## Relationship to Other Skills

- **self-improvement**: theoretical parent. prompt-enhance is a specialized leaf (Σ-pathway, p-component, intrinsic evaluative feedback).
- **pragmatic-semantics**: classifier + provenance tracer (folded into the rewrite step's inline audit).
- **essentialist**: deletion test on prompt sections (folded into the rewrite step's inline audit).
- **grill-me**: decoupled critic (verify step).
