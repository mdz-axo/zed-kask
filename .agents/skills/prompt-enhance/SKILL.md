---
name: prompt-enhance
description: "General-purpose prompt enhancement for the zed-kask platform. Typed routing over a 7-type prompt taxonomy with a 3-tier effort knob. Use when enhancing prompts for skill templates, agent system prompts, or chat REPL prompts."
---

# Prompt Enhance

General-purpose prompt enhancement skill for the zed-kask platform. Classifies prompts against a 7-type taxonomy, applies a typed rewrite with an inline audit (placeholders, semantic fragility, structural accretion), verifies via a decoupled grill-me critic, and delivers the result. Specialized leaf of the self-improvement family tree (Σ-pathway, p-component, intrinsic evaluative feedback).

Single-pass by design (DR-S13a exempt class: documented single-pass) — the verify verdict is surfaced in the output change log, not re-entered.

## When to Use

- When you have a prompt destined for zed-kask (skill `.j2` template, agent system prompt, chat/REPL prompt, infrastructure Jinja2 template) and want it enhanced.
- When you want a typed rewrite that applies different moves based on prompt type (coding vs creative vs extraction vs agent-task vs meta).
- When you want to control effort: `low` runs classify+rewrite, `medium` adds one critique, and `high` adds three critique rounds. The output render makes no LLM call.
- When you want the enhanced prompt returned inline (default), saved to a file, or both.
- When you want a decoupled critic to prevent the self-confirming loop (generator ≠ critic).

## When NOT to Use

- Training-config prompts — `lora-training` owns that domain.

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
| `low`    | classify → rewrite → output                                | 2         | 1× baseline |
| `medium` | classify → rewrite → verify → output                       | 3         | ~1.5×       |
| `high`   | classify → rewrite → verify (3 escalating rounds) → output | 5         | ~2.5×       |

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

`render_template` only returns a stage's instructions; it does not classify, rewrite, verify, or save anything. Produce the JSON requested by each rendered template before calling the next one. A template contract validates the *inputs*, not whether the agent completed its output. Never treat a proposed check in an enhanced prompt as an executed check.

### Step 1 — Classify (enhance-classify.j2)

1. Classify the input prompt against the 7-type taxonomy using pragmatic-semantics IS/OUGHT + epistemic-mode axes.
2. Validate the effort tier and output format (resolve defaults).
3. Synthesize a minimal proxy eval set (3-5 representative inputs) for medium/high tiers; empty at low.
4. Produce `prompt_type`, `effort_tier`, `output_format_resolved`, `output_path_resolved`, `proxy_eval_set`, `risks`, `routing`, and `checkability_map`. For each consequential obligation choose `finite` (supported `lisp_eval`), `formal` (Lean 4 over stated assumptions), `empirical` (test or observation), `judgment` (evidence-based interpretation), or `none`. Leave the map empty if not applicable. These are *candidate* checks, not claims that any tool ran.
5. Render with `prompt`, `task`, and optional `effort`, `output_format`, `output_path`, `context`. The template contract treats `task` as required even though it is not a user-facing skill input; use the user's task description. Do not invent missing values for optional inputs.

### Step 2 — Rewrite (enhance-rewrite.j2)

1. **Inline audit** (internal): scan for unresolved placeholders (Prohibition), semantic fragility (Guardrail), and structural accretion (essentialist G1+G2).
2. **Typed rewrite**: apply type-specific moves based on `prompt_type` from step 1.
3. **Mutation discipline**: each finding → at most one mutation; Prohibition findings must be addressed; Hypothesis-tier findings deferred.
4. Render with original `prompt`, `task`, plus classify outputs `prompt_type`, `effort_tier`, `risks`, `checkability_map`. Produce `enhanced_prompt`, `acceptance_criteria`, `mutations_applied`, `mutations_deferred`, and `audit_findings` before proceeding. Assign specific checks only where they serve the request; a formal proof checks its proposition, not whether that proposition captures user intent, and a computed result depends on input provenance. Unrun checks remain explicitly proposed.

### Step 3 — Verify (enhance-verify.j2, medium/high only)

1. Run grill-me self-challenge across Recall → Mechanism → Rationale → Edge Cases → Synthesis.
2. Decoupled from step 2 — do not defend the prompt you (didn't) write.
3. Tier-scaled rounds: 1 (Recall+Mechanism) at medium; 3 escalating at high.
4. Render with `enhanced_prompt`, original `original_prompt`, `prompt_type`, `effort_tier`, `proxy_eval_set`, `acceptance_criteria`, `checkability_map`, and `round` (1 at medium, 1–3 at high). Produce `ratings` and `verdict` (`pass`, `rewrite_needed`, or `fail`). Challenge whether each proposed check actually establishes its claim and whether unrun checks are mislabeled as verified.
5. No PDCA re-entry: surface a non-pass verdict and its findings in the change log; do not claim the rewrite passed. Skip this stage at low effort and set `grill_verdict = "skipped"`, `grill_ratings = []`.

### Step 4 — Output (enhance-output-render.j2, deterministic render)

1. Render with `enhanced_prompt`, `prompt_type`, `effort_tier`, `output_format_resolved`, `output_path_resolved`, `audit_findings`, `mutations_applied`, `mutations_deferred`, and flat `grill_verdict`/`grill_ratings` from verification (or skipped/empty at low effort). Do not pass a nested `verification` object. The retired `schema_defects` and `convergence_metric` fields have no producing stage and must not be required.
2. Deliver the rendered `delivered_output` to the user. `inline` displays it; `file` and `both` additionally require a separate file-write action. A render-only step NEVER writes a file (`output_path_written` stays empty). Report a path only after a successful write.
3. Always include the change log: audit findings, critique verdict and ratings, applied and deferred mutations. A `rewrite_needed` or `fail` verdict must remain visible; do not report it as verified.
4. This step makes no LLM call. The skill is not complete until its enhanced prompt is delivered, not merely rendered.

## Regression case

Run a mixed request (for example: improve a proof skill, check a finite invariant with `lisp_eval`, check an example with Lean, and decide which additions are useful) through classify → rewrite → verify → output. Check that the classifier emits distinct `finite`, `formal`, `empirical`, and `judgment` obligations; the rewrite proposes checks without claiming they ran; the critic challenges whether those checks establish the claims; and the final `delivered_output` contains the enhanced prompt and an honest verdict. Exercise low-effort `file` output separately: `grill_verdict` must be `skipped` and `output_path_written` must remain empty until a separate write succeeds. A failed verdict must remain visible, not be silently changed to `pass`.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `enhance-classify.j2` | Classify the input prompt against the 7-type taxonomy (coding, reasoning, creative, classification, extraction, agent-task, meta) using pragmatic-semantics IS/OUGHT + epistemic-mode axes. Select the effort tier (low/medium/high) and validate the output_format (inline/file/both, default inline). Synthesize a minimal proxy eval set (3-5 representative inputs) for medium/high tiers so downstream phases have a signal to optimize against. Produces the routing decision that drives step 2. |
| `enhance-rewrite.j2` | Inline audit + typed rewrite. Scans for unresolved placeholders, semantic fragility, and structural accretion, then applies type-specific rewrite moves based on the prompt_type from step 1. Folds the former separate audit step and 7 typed rewrite variants into a single LLM call. Produces the enhanced prompt, audit findings, and mutations applied. |
| `enhance-verify.j2` | Decoupled critic. Runs grill-me self-challenge against the enhanced prompt across Recall -> Mechanism -> Rationale -> Edge Cases -> Synthesis. Decoupled from step 2 to prevent the self-confirming loop. Tier-scaled: 1 round (Recall+Mechanism) at medium, 3 escalating rounds at high. Skipped at low tier. Produces a Solid/Partial/Gap rating per area. |
| `enhance-output.j2` | Legacy LLM-formatting reference; the active output step is `enhance-output-render.j2`. Do not use this template in the skill run. |
| `enhance-output-render.j2` | Render-only variant of enhance-output for programmatic delivery without an LLM round-trip. Formats the enhanced prompt per output_format. |
| `enhance-audit.j2` | Audit the input prompt through three lenses: pragmatic-semantics (classify claims by IS/OUGHT, epistemic mode, constraint force), pragmatic-cybernetics (feedback loop properties), and essentialist (deletion test + surface count). Not referenced by the current process manifest — the audit is folded into enhance-rewrite.j2. Retained for potential future re-decomposition. |

To render a template, call the `render_template` tool with the template ref (e.g., `prompt-enhance/enhance-classify`) and a context object with the required variables.

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
