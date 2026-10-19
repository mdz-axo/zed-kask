---
name: prompt-enhance
description: "General-purpose prompt enhancement for the zed-kask platform. Typed routing over a 7-type prompt taxonomy with a 3-tier effort knob. Use when enhancing prompts for skill templates, agent system prompts, or chat REPL prompts."
---

# Prompt Enhance

General-purpose prompt enhancement skill for the zed-kask platform. Classifies prompts against a 7-type taxonomy, applies a typed rewrite with an inline audit (placeholders, semantic fragility, structural accretion), verifies via a decoupled grill-me critic, and delivers the result. Specialized leaf of the self-improvement family tree (Σ-pathway, p-component, intrinsic evaluative feedback).

## Initial and target condition

- **Initial condition:** the user's original prompt and intended use, effort tier, output destination, and known risks. Record what must remain unchanged; missing user intent is a question, not permission to invent it.
- **Target condition:** an enhanced prompt preserving that intent, resolving identified Prohibition-tier defects, and honestly reporting proposed versus executed checks. Medium/high additionally target a decoupled critic `pass`; low has no independent critic and must report `skipped` rather than claiming verified quality. File output counts only after a successful write.

## When to Use

- When you have a prompt destined for zed-kask (skill `.j2` template, agent system prompt, chat/REPL prompt, infrastructure Jinja2 template) and want it enhanced.
- When you want a typed rewrite that applies different moves based on prompt type (coding vs creative vs extraction vs agent-task vs meta).
- When you want to control effort: `low` runs classify+rewrite, `medium` adds one critique, and `high` adds up to three gated critique rounds. The output render makes no LLM call.
- When you want the enhanced prompt returned inline (default), saved to a file, or both.
- When you want a decoupled critic to prevent the self-confirming loop (generator ≠ critic).

## When NOT to Use

- Training-config prompts — model training is out of scope for the co-researcher mandate.

## Inputs

| Input           | Type                         | Default    | Description                                                                     |
| --------------- | ---------------------------- | ---------- | ------------------------------------------------------------------------------- |
| `prompt`        | string                       | (required) | The prompt to enhance                                                           |
| `effort`        | `low` \| `medium` \| `high`  | `medium`   | Effort tier — controls whether the verify step runs                             |
| `output_format` | `inline` \| `file` \| `both` | `inline`   | How to deliver the result                                                       |
| `output_path`   | string                       | (derived)  | Explicit path for `file`/`both`; default `~/Documents/zk-data/skills/prompt-enhance/{date}-{run}/enhanced-<type>.md` |
| `context`       | object                       | (optional) | Target model, intended consumer, existing eval set                              |
| `unfold_mode`   | `auto` \| `rewrite_only` \| `inquiry` \| `task_graph` | `auto` | Controls whether the prompt is only rewritten or unfolded into a reference-model-anchored inquiry or dependency-ordered task graph |

## Effort Tiers

| Tier     | Steps run                                                  | LLM calls | Cost target |
| -------- | ---------------------------------------------------------- | --------- | ----------- |
| `low`    | classify → rewrite → output (one hard-defect correction if needed) | 2–3 | 1× baseline on first-pass success |
| `medium` | classify → rewrite → verify → output (one focused correction + recheck if needed) | 3–5 | ~1.5× on first-pass success |
| `high`   | classify → rewrite → verify (up to 3 rounds; escalation gated on prior-round Solid ≥80%, sub-40% re-probes the same level) → output (one focused correction + recheck if needed) | 5–7 typical (re-probes may add) | ~2.5× on first-pass success |

## The 7-Type Taxonomy

The *Taxonomy anchor* column uses the four aspects of Liu et al., "A comprehensive taxonomy of prompt engineering techniques for large language models", *Frontiers of Computer Science* 20(3), 2003601 (2026; online 2025), doi:10.1007/s11704-025-50058-z. The seven types are this project's mapping onto those aspects; the paper does not define them. The term "prompt engineering" reaches only the core rung of `onto_anchor`; a derived-registry ruling is pending with the operator.

| Type             | Taxonomy anchor       | Rewrite focus                                                                          | Key risk                  |
| ---------------- | --------------------- | -------------------------------------------------------------------------------------- | ------------------------- |
| `coding`         | reasoning & planning  | contract clarity, I/O spec, error cases, test-first framing                            | vague acceptance criteria |
| `reasoning`      | reasoning & planning  | CoT structure, decomposition, self-verification, counterfactual stress                 | hidden assumptions        |
| `creative`       | profile & instruction | persona depth, constraints as creative tension, audience anchoring                     | over-constraining         |
| `classification` | profile & instruction | label space, edge cases, few-shot balance, tie-breaking policy                         | label leakage in examples |
| `extraction`     | knowledge             | schema-first output, field definitions, missing-field policy, type discipline          | underspecified schema     |
| `agent-task`     | reliability           | tool-use contracts, failure modes, context budget, bounded loops, termination criteria | unbounded tool loops      |
| `meta`           | reliability           | self-reference safety, eval harness, convergence criteria, critic decoupling           | self-confirming loop      |

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|------|------|-------------------|
| 1 Classify (`pragmatic-semantics` + `hypothesis-framer` + `metacognition`) | P | decoupled critic in step 3 (medium/high); none at low, reported `skipped` |
| 1.5 Anchor (`onto_anchor` + `zk-ref-open` via `curator_federated_search`) | D+P | `onto_anchor` (D) + `curator_federated_search` filtered to `source_id == "zk-ref-open"` / `source_kind == "corpus"` (D retrieval, P selection); skipped at `low` or `rewrite_only`/`clarify_first` |
| 2 Rewrite & Unfold (`pragmatic-cybernetics` + `improv` + `essentialist`) | P | decoupled critic in step 3 (medium/high); none at low, reported `skipped` |
| 3 Verify (`grill-me`) | P | decoupled grill-me critic; its verdict is reported, never upgraded |
| 4 Output render (`kata-improvement`) | D | `render_template` (no model call) |
| File write (`file`/`both`) | D | the write tool's success receipt |
| Local PDCA Check (`kata-improvement`) | P | medium/high: the verify verdict; low: generator self-inspection of the inline audit — no independent critic, the independent verdict stays `skipped` |
| Local PDCA Act (`kata-improvement`) | P | re-verify after the one corrective re-render (medium/high); bound: one correction per invocation. A later explicit user amendment starts a new invocation, never a second Act pass. |

Anchors: the 7-type taxonomy adapts Liu et al. (2026) — §The 7-Type Taxonomy; the verify escalation adapts the grill-me protocol (grounding: Socratic questioning, core-rung `onto_anchor`, operator ruling pending); the local PDCA loop copies Deming's cycle.

## Instructions

`render_template` only returns a stage's instructions; it does not classify, rewrite, verify, or save anything. Produce the JSON requested by each rendered template before calling the next one. A template contract validates the *inputs*, not whether the agent completed its output. Never treat a proposed check in an enhanced prompt as an executed check.

Array-valued context fields are plain JSON arrays — never stringified JSON. A string passed where a template iterates an array renders character-by-character into placeholder rows that look like real output; the output template now surfaces this as `input_errors`, but the correct form everywhere is a plain array.

### Step 1 — Classify (enhance-classify.j2)

1. **Semantic classification (`pragmatic-semantics`)**: classify the input prompt against the 7-type taxonomy using IS/OUGHT + epistemic-mode + provenance axes, and extract 2–4 domain/methodology `anchor_terms` when unfolding applies.
2. **Inquiry & objective framing (`hypothesis-framer`)**: apply the FINER admissibility gate and PICO/counterfactual structure (`finer_pico_framing`) to extract explicit and implied objectives, target conditions, and counterfactual falsifiers. If user intent is missing or untestable, set `unfold_mode_resolved = "clarify_first"` and populate `clarifying_questions` rather than inventing scope.
3. **Unfold triage & calibration prior (`metacognition`)**: validate `effort` and `output_format`, resolve `unfold_mode_resolved` (`rewrite_only` for trivial/single-step prompts, `inquiry` for reasoning/research prompts, `task_graph` for multi-step coding/agent-task prompts, or `clarify_first` when intent is missing), and record `intake_confidence` (`0.0–1.0`) for downstream Brier calibration.
4. Synthesize a minimal proxy eval set (3-5 representative inputs) for medium/high tiers; empty at low.
5. Produce `prompt_type`, `semantic_classification`, `effort_tier`, `output_format_resolved`, `output_path_resolved`, `unfold_mode_resolved`, `intake_confidence`, `anchor_terms`, `finer_pico_framing`, `proxy_eval_set`, `risks`, `routing`, and `checkability_map`. For each consequential obligation choose `finite` (supported `lisp_eval`), `formal` (Lean 4 over stated assumptions), `empirical` (test or observation), `judgment` (evidence-based interpretation), or `none`. Leave the map empty if not applicable. These are *candidate* checks, not claims that any tool ran.
6. Render with the full context shape — copy it, do not reconstruct it from prose (the 2026-10-07..13 rejections dropped fields in varying subsets):
   ```json
   { "prompt": "<the prompt>", "task": "<the user's task description>",
     "effort": "<low|medium|high>", "output_format": "<inline|file|both>",
     "unfold_mode": "<auto|rewrite_only|inquiry|task_graph>",
     "output_path": "<omit unless user-supplied>", "context": "<omit unless supplied>" }
   ```
   `prompt` and `task` are required (the contract rejects their absence; `task` is not a user-facing skill input but the contract requires it — use the user's task description). `effort` defaults to `medium`, `output_format` to `inline`, and `unfold_mode` to `auto`; `output_path` and `context` are optional — do not invent values for them.

### Step 1.5 — Anchor (`onto_anchor` + `zk-ref-open`, medium/high when `unfold_mode_resolved` is `inquiry` or `task_graph`)

1. When `effort_tier` is `medium` or `high` AND `unfold_mode_resolved` is `inquiry` or `task_graph`:
   - Call `onto_anchor` in parallel on the 2–4 `anchor_terms` from Step 1.
   - Run one bounded `curator_federated_search(query, limit=5)` call (or `corpus_query`) and filter results to `source_id == "zk-ref-open"` (or `source_kind == "corpus"`) so episodic chat turns are never cited as published reference models.
   - Assemble `reference_models` as a plain JSON array of objects (`reference_model_id`, `provenance`, `discovery_capability_unlocked`, `execution_capability_unlocked`, `status`). If `onto_anchor` terminates at `5w1h_core` or `zk-ref-open` has no relevant passage, record `status: "coarse_anchor"` / `"unanchored_in_zk_ref_open"` and require an operator ruling — never fabricate a reference model citation.
2. When `effort_tier` is `low` OR `unfold_mode_resolved` is `rewrite_only` or `clarify_first`, skip tool calls and pass `reference_models: []`.

### Step 2 — Rewrite & Unfold (enhance-rewrite.j2)

1. **Inline audit** (internal): scan for unresolved placeholders (Prohibition), semantic fragility (Guardrail), and structural accretion (`essentialist` Exist/Surface/Contract gates, ≤ 7 top-level blocks and ≤ 7 unfolded items).
2. **Reference-model binding & controlled unfolding (`pragmatic-cybernetics` + `improv` + `essentialist`)**:
   - Bind each entry in `reference_models` to concrete discovery capabilities (blind spots, missing variables, counterfactuals) and execution capabilities (sensors, attenuators, amplifiers, D-gates). Honor the `coarse_anchor` non-fabrication prohibition.
   - Apply constructive *Plussing* (`improv`) according to `unfold_mode_resolved`: unfold into falsifiable hypothesis branches (`inquiry`), a dependency-ordered task DAG with `depends_on`, `d_p_mode`, and `verification_oracle` (`task_graph`), a clarifying-question prompt (`clarify_first`), or a tight single-task rewrite (`rewrite_only`).
   - Prune via `essentialist` 3-gate interrogation: remove any unfolded task/branch or instruction block whose deletion does not degrade the target condition.
3. **Typed rewrite & mutation discipline**: apply type-specific moves based on `prompt_type` from step 1; each finding → at most one mutation; Prohibition findings must be addressed; Hypothesis-tier findings deferred.
4. Render with the full context shape — copy it, do not reconstruct it from prose:
   ```json
   { "prompt": "<the original prompt>", "prompt_type": "<from step 1>", "effort_tier": "<from step 1>",
     "risks": <from step 1>, "checkability_map": <from step 1>, "task": "<the user's task>",
     "unfold_mode_resolved": "<from step 1>", "finer_pico_framing": <from step 1>,
     "reference_models": <from step 1.5>, "prior_enhanced_prompt": "", "critic_feedback": [] }
   ```
   On a corrective re-render, `prior_enhanced_prompt` carries the previous rewrite and `critic_feedback` the verify step's `findings`. Produce `enhanced_prompt`, `unfolded_plan`, `acceptance_criteria`, `mutations_applied`, `mutations_deferred`, and `audit_findings` before proceeding. Assign checks only where they serve the request; proof does not establish that a proposition captures user intent, and computation depends on input provenance. Unrun checks remain explicitly proposed.

### Step 3 — Verify (enhance-verify.j2, medium/high only)

1. Run `grill-me` self-challenge across Recall → Mechanism → Rationale → Edge Cases → Synthesis, plus the **Reference-Model Governance & Unfolding Challenge** (verifying that cited `zk-ref-open` models govern tasks rather than decorative name-dropping, that `coarse_anchor` fallbacks are unfabricated, and that `unfolded_plan` is acyclic and ≤ 7 items).
2. Decoupled from step 2 — do not defend the prompt you (didn't) write (self-improvement §9.1).
3. Tier-scaled rounds: 1 (Recall+Mechanism) at medium; up to 3 at high, escalation gated on prior-round Solid ≥80% (below 40% re-probes the same level).
4. Render with the full context shape — copy it, do not reconstruct it from prose:
   ```json
   { "enhanced_prompt": "<from step 2>", "original_prompt": "<the original prompt>",
     "prompt_type": "<from step 1>", "effort_tier": "<from step 1>",
     "proxy_eval_set": <from step 1>, "acceptance_criteria": <from step 2>,
     "checkability_map": <from step 1>, "unfold_mode_resolved": "<from step 1>",
     "reference_models": <from step 1.5>, "unfolded_plan": <from step 2>, "round": 1 }
   ```
   `round` is 1 at medium, 1–3 escalating at high. Produce `ratings`, `findings` (the failed-gap list that feeds a corrective Act), and `verdict` (`pass`, `rewrite_needed`, or `fail`). Challenge whether each proposed check actually establishes its claim and whether unrun checks are mislabeled as verified.
5. Preserve the critic's specific failed checks and verdict. Skip this stage at low effort and set `grill_verdict = "skipped"`, `grill_ratings = []`; a skipped critic is not a `pass`.
6. Run deterministic `lisp_eval` gate `PE-2` after the critique step to enforce the escalation bound. `blocking_defects` is **computed from the critic's structured `findings`**, never typed as a count — `enhance-verify.j2` emits a `severity` field (`Prohibition|Guardrail|Guideline`) per finding, so the count is deterministic and a `pass` cannot be self-certified over an unresolved Prohibition:
   - form: `(let ((count-prohibitions (lambda (items) (if (= 0 (length items)) 0 (+ (if (string= "Prohibition" (assoc "severity" (car items))) 1 0) (count-prohibitions (cdr items))))))) (let ((blocking_defects (count-prohibitions findings))) (list blocking_defects (and (<= rounds_executed 3) (<= pdca_corrections 1) (if (string= verdict "pass") (= blocking_defects 0) true)))))`
   - env: `{ "rounds_executed": <1..3>, "pdca_corrections": <0..1>, "verdict": "<pass|rewrite_needed|fail|skipped>", "findings": <the critic's findings array verbatim from enhance-verify, each with its severity field> }`
   - Returns `[blocking_defects, gate_result]`. A `false` gate result on a `pass` verdict means the critic passed a prompt carrying an unresolved Prohibition-tier defect — the verdict does not stand; carry the failed verdict and the remaining gap into the output.

### Local PDCA — Check and Act before output

1. **Plan/Do:** use the initial condition and target above; classify and rewrite once, then run the tier's verification stage. Do not call a model again when the target is already satisfied.
2. **Check:** at medium/high, a `pass` with no unresolved Prohibition-tier issue meets the local target; `rewrite_needed` or `fail` carries the critic's specific gaps. At low, inspect the inline audit for unresolved Prohibition-tier issues, but keep the independent verdict `skipped` even when none is visible. A proposed `lisp_eval`, Lean, or empirical check in the prompt is not evidence it ran.
3. **Act (one correction maximum):** on a named gap, rerender `enhance-rewrite` with the *original* `prompt`/`task`, previous `enhanced_prompt` as `prior_enhanced_prompt`, and the verify step's `findings` as `critic_feedback`. Preserve already satisfied constraints. At medium/high, re-run `enhance-verify` once against the revised prompt and the same acceptance criteria; at low, recheck only the hard issue and retain `skipped`. If the target still fails, deliver with the failed verdict and remaining gap visible—never turn a failure into a `pass`. Do not loop on new stylistic suggestions or perform a second correction.
4. **Post-delivery user amendment (new invocation, not Act re-entry):** Step 4 delivery terminates this invocation. If a later user message explicitly changes or adds a requirement, begin a new prompt-enhance invocation: pass the prior delivered enhanced prompt as `prompt`, pass the user's literal follow-on as `task`, and reset `rounds_executed` and `pdca_corrections` to zero. Preserve the prior run's verdict and change log as provenance. Only an explicit user turn may open this path; critic feedback, stylistic suggestions, or agent initiative never do. The new invocation retains its own one-correction maximum and independent verification.

### Step 4 — Output (enhance-output-render.j2, deterministic render)

1. Render with the full context shape — copy it, do not reconstruct it from prose:
   ```json
   { "enhanced_prompt": "<from step 2>", "prompt_type": "<from step 1>", "effort_tier": "<from step 1>",
     "output_format_resolved": "<from step 1>", "output_path_resolved": "<from step 1>",
     "audit_findings": <from step 2>, "grill_ratings": <from step 3>, "grill_verdict": "<from step 3>",
     "mutations_applied": <from step 2>, "mutations_deferred": <from step 2>,
     "unfold_mode_resolved": "<from step 1>", "reference_models": <from step 1.5>,
     "unfolded_plan": <from step 2> }
   ```
   At low effort pass `grill_verdict: "skipped"`, `grill_ratings: []`, `reference_models: []`, and `unfolded_plan: []` (if `rewrite_only`). Do not pass a nested `verification` object. The retired `schema_defects` and `convergence_metric` fields have no producing stage and must not be required. Pass the six array fields (`audit_findings`, `grill_ratings`, `mutations_applied`, `mutations_deferred`, `reference_models`, `unfolded_plan`) as plain arrays of objects with the fields the template reads — never stringified JSON (the 2026-10-08 receipts: the change-log sections rendered placeholder rows while the summary line interpolated, because a stringified array iterates character-by-character; the template now rejects this loudly as `input_errors`).
2. Deliver the rendered `delivered_output` to the user. `inline` displays it; `file` and `both` additionally require a separate file-write action. A render-only step NEVER writes a file (`output_path_written` stays empty). Report a path only after a successful write.
3. Always include the change log: audit findings, critique verdict and ratings, applied and deferred mutations. A `rewrite_needed` or `fail` verdict must remain visible; do not report it as verified.
4. This step makes no LLM call. The skill is not complete until its enhanced prompt is delivered, not merely rendered.

## Regression case

Run a mixed request (for example: improve a proof skill, check a finite invariant with `lisp_eval`, check an example with Lean, and decide which additions are useful) through classify → rewrite → verify → output at `medium` effort. Include one run whose verify verdict is `rewrite_needed`: it must drive exactly one corrective re-render (`prior_enhanced_prompt` plus `critic_feedback` taken from the verify `findings`) and one re-verify. Check that the classifier emits distinct `finite`, `formal`, `empirical`, and `judgment` obligations; the rewrite proposes checks without claiming they ran; the critic challenges whether those checks establish the claims; and the final `delivered_output` contains the enhanced prompt and an honest verdict. Exercise low-effort `file` output separately: `grill_verdict` must be `skipped` and `output_path_written` must remain empty until a separate write succeeds. A failed verdict must remain visible, not be silently changed to `pass`. Exercise the post-delivery boundary: (a) an agent-generated extra suggestion after one correction does not reopen the loop; (b) an explicit user amendment starts a new invocation with fresh counters and the prior delivered prompt as baseline; (c) `PE-2` uses `string=` and passes corrections 0/1 while rejecting correction 2 within one invocation.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `enhance-classify.j2` | Classify the input prompt against the 7-type taxonomy (coding, reasoning, creative, classification, extraction, agent-task, meta) using pragmatic-semantics IS/OUGHT + epistemic-mode axes. Select the effort tier (low/medium/high) and validate the output_format (inline/file/both, default inline). Synthesize a minimal proxy eval set (3-5 representative inputs) for medium/high tiers so downstream phases have a signal to optimize against. Produces the routing decision that drives step 2. |
| `enhance-rewrite.j2` | Inline audit + typed rewrite in one LLM call. Scans for unresolved placeholders, semantic fragility, and structural accretion, then applies type-specific moves from step 1's `prompt_type`. Produces the enhanced prompt, audit findings, and mutations. |
| `enhance-verify.j2` | Decoupled critic. Runs grill-me self-challenge across Recall -> Mechanism -> Rationale -> Edge Cases -> Synthesis. Decoupled from step 2 to prevent the self-confirming loop. Tier-scaled: 1 round (Recall+Mechanism) at medium, up to 3 at high (escalation-gated). Skipped at low tier. Produces a Solid/Partial/Gap rating per area. |
| `enhance-output-render.j2` | Deterministic output formatter (no LLM call): renders `delivered_output` from the pipeline's results per output_format; file delivery requires the separate write action. |


To render a template, call the `render_template` tool with the template ref (e.g., `prompt-enhance/enhance-classify`) and a context object with the required variables.

## Constraints

- A requested effort tier is never silently downgraded to save cost — the classify stage surfaces the cost estimate in `risks` and the orchestrator decides.

