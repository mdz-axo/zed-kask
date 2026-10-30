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
| `high`   | classify → rewrite → verify (PE-State admits exact-next-level or current-level review within the three-attempt cap) → output (one focused correction; recheck only within that same attempt cap) | bounded; re-probes/rechecks share the verify budget | ~2.5× target, not measured usage |

## The 7-Type Taxonomy

The *Taxonomy anchor* column uses the four aspects of Liu et al., "A comprehensive taxonomy of prompt engineering techniques for large language models", *Frontiers of Computer Science* 20(3), 2003601 (2026; online 2025), doi:10.1007/s11704-025-50058-z. The seven types are this project's mapping onto those aspects; the paper does not define them. Resolve "prompt engineering" with `onto_anchor` at use time and retain the returned published source and alternative senses; request a ruling only for a genuinely coarse result.

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
| 3 Verify (`grill-me`) | P + D | critic grades are P; PE-State validates attempt/revision history, counts and threshold admission (D); verdict is reported, never upgraded |
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
3. **Unfold triage & optional forecast context (`metacognition`)**: validate `effort` and `output_format`, resolve `unfold_mode_resolved` (`rewrite_only` for trivial/single-step prompts, `inquiry` for reasoning/research prompts, `task_graph` for multi-step coding/agent-task prompts, or `clarify_first` when intent is missing), and leave routine `intake_confidence` null; a decision-relevant optional probability requires a separate prospective declaration and frozen resolver.
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

### Raw JSON preflight (before every PE-State evaluation)

PE-State is a two-part gate over the SAME raw env, not two state owners. First run `jq -e -f .agents/skills/prompt-enhance/raw-shape.jq` with the saved NEW review-state JSON file supplied on stdin, using its actual discovered invocation artifact path. Refuse on nonzero status; only then pass that exact parsed object as JSON text in `lisp_eval.env`. No filtered log, reconstructed findings or separately normalized arrays may cross this seam. The filter below matches the applied companion resource `raw-shape.jq`:

```jq
# Preserve raw JSON identity before the lossy JSON -> Lisp bridge.
# No review echo, scoring, call counter or shadow admission state.
def request_shape:
  type == "object" and
  keys == ["attempt_id","kind","level","reviewed_prompt","reviewed_revision"] and
  (.attempt_id | type == "number" and floor == .) and
  (.level | type == "number" and floor == .) and
  (.reviewed_revision | type == "number" and floor == .) and
  (.reviewed_prompt | type == "string") and
  (.kind | IN("initial","escalate","reprobe","correction","retry"));
type == "object" and has("effort_tier") and has("request") and
(.prompt_revisions | type == "array") and
(.critique_runs | type == "array") and
(.request == null or (.request | request_shape)) and
all(.critique_runs[];
  type == "object" and has("status") and has("outcome") and
  (if .status == "pending" then .outcome == null
   elif .status == "completed" then
    (.outcome | type == "object") and
    (.outcome.ratings | type == "array") and
    (.outcome.findings | type == "array") and
    all(.outcome.ratings[]; type == "object") and
    all(.outcome.findings[]; type == "object")
   else true end))

```

This preflight is necessary because the Lisp JSON bridge represents `null`, `[]` and `{}` identically when empty. It cannot recover their original JSON types. The raw filter requires pending `outcome` to be EXACT JSON null and distinguishes snapshot `request: null` from a complete known five-field request object. Empty objects/arrays are neither pending null nor snapshot null. Its numeric request fields must be integer-valued; the Lisp gate still checks legal level/revision/identity values. Other than these producer-boundary shape checks, PE-State owns record admission, identity, threshold arithmetic and budgets. Interpreter errors or raw shape refusal mean incomplete/invalid review, never another free critique. Neither component establishes semantic quality, actual dispatch provenance, independence, or that the agent appended before sending; the executor must obey that ordering and retain its tool receipts.

**Step budget and form extraction.** Supply `max_steps: 2000000` explicitly on every PE-State `lisp_eval` call — the 100000 default is exceeded by valid high-tier ledger states (two prompt revisions plus a bound critic outcome, ~19KB env; observed 2026-10-23, re-run at 2000000 passed), so a faithful first run of the documented form fails on valid input. Extract the PE-State form from this SKILL.md's source text, not from rendered skill content, and check paren balance (e.g. `awk`) before execution — transcribing the 92-line form from a rendered copy introduced paren errors (observed 2026-10-23: parse error, nothing consumed).

### Step 3 — Verify (enhance-verify.j2, medium/high only)

1. Use the decoupled `grill-me` critic, preserving the existing challenge, verdict and findings policy. `low` skips the critic (`skipped`, empty ratings/findings); `medium` and `high` both start at level 1 (Recall + Mechanism). Level 2 adds Rationale; level 3 adds Edge Cases + Synthesis. Ratings contain exactly one row for each cumulative area at that level, no duplicate or unknown area; row order is irrelevant. Keep `Solid|Partial|Gap` unchanged. The independent critic supplies ratings, not threshold arithmetic.
2. Initialize `critique_runs = []` and `prompt_revisions = [enhanced_prompt]` after the first rewrite. `prompt_revisions` freezes the exact initial text and, only after the one named-gap correction, its revised text. No separate critique execution count or completed-results list exists. Each attempt is one record:
   ```json
   { "attempt_id": 1, "kind": "initial", "level": 1,
     "reviewed_revision": 0, "reviewed_prompt": "<exact initial rewrite text>",
     "status": "pending", "outcome": null }
   ```
   `attempt_id` is the one-based log position, never reused. `kind` is `initial|escalate|reprobe|correction|retry`; `status` is `pending|completed|failed`. Only status/outcome change after dispatch; identity, level, kind and reviewed revision/text are immutable. Bind the returned raw outcome to the captured pending attempt, not a later prompt or caller-typed count. Completed outcomes retain the full critic result, including ratings, findings and verdict unchanged. On transport failure or invalid critic output, keep that SAME attempt as `failed`, preserving its raw outcome/error; never delete it from the budget.
3. **PE-State replaces both old PE-3 and PE-2 forms.** Run the exact form below with JSON-text env `{ "effort_tier": <classified tier>, "prompt_revisions": <frozen texts>, "critique_runs": <all attempts>, "request": <planned attempt identity or null for snapshot> }`. A planned request contains the identity fields and `kind`, not a model-typed count. Require `input_valid: true` AND `admit: true`; interpreter errors also refuse dispatch. Then append the request with `status: "pending", outcome: null` BEFORE dispatch, and snapshot-check that log. A pending attempt blocks concurrent dispatch and counts toward exhaustion. Never append a completed result as a new attempt. Snapshot-check again after settling it.

   ```lisp
(begin
  (define pe-get (lambda (r k) (assoc! k r)))
  (define pe-level (lambda (n) (and (numberp n) (or (= n 1) (= n 2) (= n 3)))))
  (define pe-revision (lambda (n) (and (numberp n) (or (= n 0) (= n 1)))))
  (define pe-areas (lambda (n) (if (= n 1) (quote ("Recall" "Mechanism")) (if (= n 2) (quote ("Recall" "Mechanism" "Rationale")) (quote ("Recall" "Mechanism" "Rationale" "Edge Cases" "Synthesis"))))))
  (define pe-rating-rows (lambda (rs remaining)
    (if (is_null rs) (= (length remaining) 0)
      (let ((r (car rs)))
        (and (listp r) (stringp (pe-get r "area")) (member (pe-get r "area") remaining)
          (member (pe-get r "rating") (quote ("Solid" "Partial" "Gap")))
          (stringp (pe-get r "evidence"))
          (pe-rating-rows (cdr rs) (pe-remove remaining (pe-get r "area"))))))))
  (define pe-remove (lambda (xs value) (if (is_null xs) (list) (if (string= (car xs) value) (cdr xs) (cons (car xs) (pe-remove (cdr xs) value))))))
  (define pe-ratings (lambda (rs level) (and (listp rs) (= (length rs) (length (pe-areas level))) (pe-rating-rows rs (pe-areas level)))))
  (define pe-solid (lambda (rs) (if (is_null rs) 0 (+ (if (string= (pe-get (car rs) "rating") "Solid") 1 0) (pe-solid (cdr rs))))))
  (define pe-findings (lambda (xs) (and (listp xs) (if (is_null xs) true
    (and (listp (car xs)) (stringp (pe-get (car xs) "area")) (> (length (pe-get (car xs) "area")) 0)
      (stringp (pe-get (car xs) "gap")) (> (length (pe-get (car xs) "gap")) 0)
      (member (pe-get (car xs) "severity") (quote ("Prohibition" "Guardrail" "Guideline")))
      (stringp (pe-get (car xs) "suggested_fix_direction")) (pe-findings (cdr xs)))))))
  (define pe-blocking (lambda (xs) (if (is_null xs) 0 (+ (if (string= (pe-get (car xs) "severity") "Prohibition") 1 0) (pe-blocking (cdr xs))))))
  (define pe-outcome (lambda (r)
    (let ((o (pe-get r "outcome")))
      (and (listp o) (pe-ratings (pe-get o "ratings") (pe-get r "level"))
        (pe-findings (pe-get o "findings"))
        (member (pe-get o "verdict") (quote ("pass" "rewrite_needed" "fail")))
        (if (string= (pe-get o "verdict") "pass") (and (= (pe-solid (pe-get o "ratings")) (length (pe-get o "ratings"))) (= (pe-blocking (pe-get o "findings")) 0)) true)))))
  (define pe-last (lambda (xs) (nth (- (length xs) 1) xs)))
  (define pe-limit (if (string= effort_tier "low") 0 3))
  (define pe-identity (lambda (r index)
    (and (listp r) (= (pe-get r "attempt_id") (+ index 1))
      (pe-level (pe-get r "level")) (pe-revision (pe-get r "reviewed_revision"))
      (< (pe-get r "reviewed_revision") (length prompt_revisions))
      (stringp (pe-get r "reviewed_prompt"))
      (string= (pe-get r "reviewed_prompt") (nth (pe-get r "reviewed_revision") prompt_revisions)))))
  (define pe-transition (lambda (prior r)
    (and (< (length prior) pe-limit) (pe-identity r (length prior))
      (if (is_null prior)
        (and (string= (pe-get r "kind") "initial") (= (pe-get r "level") 1) (= (pe-get r "reviewed_revision") 0))
        (let ((p (pe-last prior)))
          (and (= (pe-get p "reviewed_revision") 0) (not (string= (pe-get p "status") "pending"))
            (if (string= (pe-get r "kind") "retry")
              (and (string= (pe-get p "status") "failed")
                (= (pe-get r "level") (pe-get p "level"))
                (= (pe-get r "reviewed_revision") (pe-get p "reviewed_revision")))
              (and (string= (pe-get p "status") "completed")
                (not (string= (pe-get (pe-get p "outcome") "verdict") "pass"))
                (if (string= (pe-get r "kind") "correction")
                  (and (= (pe-get p "reviewed_revision") 0) (= (pe-get r "reviewed_revision") 1)
                    (= (pe-get r "level") (pe-get p "level"))
                    (> (length (pe-get (pe-get p "outcome") "findings")) 0))
                  (and (string= effort_tier "high")
                    (= (pe-get r "reviewed_revision") (pe-get p "reviewed_revision"))
                    (let ((rs (pe-get (pe-get p "outcome") "ratings")))
                      (if (string= (pe-get r "kind") "escalate")
                        (and (= (pe-get r "level") (+ 1 (pe-get p "level"))) (>= (* 5 (pe-solid rs)) (* 4 (length rs))))
                        (and (string= (pe-get r "kind") "reprobe") (= (pe-get r "level") (pe-get p "level"))
                          (< (* 5 (pe-solid rs)) (* 2 (length rs))))))))))))))))
  (define pe-history (lambda (remaining prior)
    (if (is_null remaining) true
      (let ((r (car remaining)))
        (and (pe-transition prior r)
          (member (pe-get r "status") (quote ("pending" "failed" "completed")))
          (if (string= (pe-get r "status") "completed") (pe-outcome r)
            (if (string= (pe-get r "status") "pending") (and (is_null (cdr remaining)) (is_null (pe-get r "outcome"))) true))
          (pe-history (cdr remaining) (append prior (list r))))))))
  (define pe-last-completed (lambda (xs found)
    (if (is_null xs) found (pe-last-completed (cdr xs) (if (string= (pe-get (car xs) "status") "completed") (car xs) found)))))
  (define pe-input (and (member effort_tier (quote ("low" "medium" "high")))
    (listp prompt_revisions) (or (= (length prompt_revisions) 1) (= (length prompt_revisions) 2))
    (stringp (car prompt_revisions)) (> (length (car prompt_revisions)) 0)
    (if (= (length prompt_revisions) 2) (and (stringp (nth 1 prompt_revisions)) (> (length (nth 1 prompt_revisions)) 0) (not (string= (car prompt_revisions) (nth 1 prompt_revisions)))) true)
    (listp critique_runs) (<= (length critique_runs) pe-limit)
    (pe-history critique_runs (list))))
  (if (not pe-input)
    (list (list "input_valid" false) (list "admit" false) (list "verified_current_revision" false) (list "attempts_used" (if (listp critique_runs) (length critique_runs) 0)) (list "selected_attempt_id" 0) (list "delivery_verdict" "fail") (list "error" "invalid_review_state"))
    (let ((review (pe-last-completed critique_runs (list))))
      (let ((verified (and (> (length critique_runs) 0)
              (string= (pe-get (pe-last critique_runs) "status") "completed")
              (= (pe-get (pe-last critique_runs) "reviewed_revision") (- (length prompt_revisions) 1))
              (string= (pe-get (pe-get (pe-last critique_runs) "outcome") "verdict") "pass"))))
        (list (list "input_valid" true)
          (list "admit" (and (not (is_null request))
            (pe-transition critique_runs request)
            (= (pe-get request "reviewed_revision") (- (length prompt_revisions) 1))))
          (list "attempts_used" (length critique_runs))
          (list "attempts_remaining" (- pe-limit (length critique_runs)))
          (list "verified_current_revision" verified)
          (list "selected_attempt_id" (if (is_null review) 0 (pe-get review "attempt_id")))
          (list "delivery_verdict" (if (string= effort_tier "low") "skipped" (if verified "pass" (if (is_null review) "fail" (if (string= (pe-get (pe-get review "outcome") "verdict") "pass") "fail" (pe-get (pe-get review "outcome") "verdict"))))))
          (list "verification_gap" (if (or verified (string= effort_tier "low")) "" "Current prompt has no final accepted clean pass; report failed, pending, invalid, stale or budget-exhausted review explicitly.")))))))

   ```

   The form replays every recorded admission before accepting the current state. Medium and high share the existing maximum of THREE critic attempts per invocation; low permits zero. Medium still has one completed primary review and at most one correction recheck, never semantic escalation/reprobe; failed-primary retries spend that same three-attempt budget, not a new allowance. Failed and in-flight attempts, same-level re-probes, and correction verification consume those same allowances. A failed attempt may retry only the same revision/level with remaining budget; no hidden recovery call. After a correction recheck, deliver; do not restart escalation, retry or reprobe.
   At high, a valid non-pass review with >=80% Solid admits ONLY `level = previous level + 1`; <40% admits ONLY a same-level re-probe with different angles. Exact 40% does not admit a reprobe. Invalid/empty ratings and failed/pending outcomes cannot authorize semantic escalation. A `pass` already satisfies the target and admits no extra call. These are the existing project threshold adaptations, not a new semantic scoring policy. Arithmetic eligibility says nothing about rating correctness or actual critic independence.
4. Dispatch `enhance-verify` with its full existing context, adding `attempt_id` and `reviewed_revision` from the pending record; `round` is that record's `level` and `enhanced_prompt` is its `reviewed_prompt`. The original prompt, acceptance criteria, proxy set and checkability context stay frozen across reviews and the correction:
   ```json
   { "enhanced_prompt": "<pending reviewed_prompt>", "original_prompt": "<original prompt>",
     "prompt_type": "<step 1>", "effort_tier": "<step 1>",
     "proxy_eval_set": [], "acceptance_criteria": [], "checkability_map": [],
     "unfold_mode_resolved": "<step 1>", "reference_models": [], "unfolded_plan": [],
     "round": 1, "attempt_id": 1, "reviewed_revision": 0 }
   ```
   The empty arrays above illustrate types, not permission to discard populated stage outputs: carry the actual stage arrays. Critic output remains the current `ratings`, `findings`, `verdict` plus its other existing fields. Validate it before marking completed. A claimed `pass` requires ALL required area ratings to be `Solid` AND no Prohibition findings, matching the existing producer verdict contract. A pass with Partial/Gap or a Prohibition is inconsistent: keep that same consumed attempt as `failed` with the raw outcome intact; never delete/refund it or claim verified. A mistakenly completed inconsistent record is rejected with consumed attempts and failed delivery visible. Finding `area` and `gap` must be nonempty strings to support a named-gap correction; this is a length check, not a semantic scorer or a new finding-area taxonomy. Empty suggested-fix text alone is not a new rejection policy.
5. Before output, use a snapshot (`request: null`). The gate returns only `selected_attempt_id` (0 means none), NEVER a review echo. For a positive ID, select `critique_runs[id - 1]` from the ORIGINAL raw JSON object that passed preflight, confirm its `attempt_id` equals the selected ID, and take ratings/findings from that raw record's `outcome` unchanged. Do not serialize them through Lisp, reconstruct them from ratings, or reject otherwise allowed nested extra data. For ID 0 use empty ratings/findings and keep the gate's failed/skipped delivery status. Set `grill_verdict` to `delivery_verdict`, never manufacture a pass from an old-revision review or pending/failed attempt. Keep any original raw critic verdict in the same log. Deliver `verification_gap` and the attempt status/revision/budget alongside the rendered result when current verification is missing, invalid or exhausted. No accepted result is not a skipped critic at medium/high; it is an unresolved failure. Invalid history blocks further calls and any verification claim; deliver the last prompt with the state error visible, not a repaired or fabricated history.

### Local PDCA — Check and Act before output

1. **Plan/Do:** use the initial condition and target above; classify and rewrite once, then run the tier's verification stage. Do not call a model again when the target is already satisfied.
2. **Check:** at medium/high, a `pass` with no unresolved Prohibition-tier issue meets the local target; `rewrite_needed` or `fail` carries the critic's specific gaps. At low, inspect the inline audit for unresolved Prohibition-tier issues, but keep the independent verdict `skipped` even when none is visible. A proposed `lisp_eval`, Lean, or empirical check in the prompt is not evidence it ran.
3. **Act (one correction maximum):** on a named gap, rerender `enhance-rewrite` with the *original* `prompt`/`task`, previous `enhanced_prompt` as `prior_enhanced_prompt`, and the verify step's `findings` as `critic_feedback`. Preserve already satisfied constraints. Append the exact revised text as `prompt_revisions[1]` (only one correction). At medium/high, request a `correction` attempt at the last completed review's level, binding revision 1 and that exact text; run it only if PE-State admits it within the existing attempt budget. If exhausted or a prior attempt is still pending, do not dispatch: deliver the unresolved/unreviewed correction visibly, never reuse revision 0's pass. At low, recheck only the hard issue and retain `skipped`. If the target still fails, deliver with the failed verdict and remaining gap visible—never turn a failure into a `pass`. Do not loop on new stylistic suggestions or perform a second correction.
4. **Post-delivery user amendment (new invocation, not Act re-entry):** Step 4 delivery terminates this invocation. If a later user message explicitly changes or adds a requirement, begin a new prompt-enhance invocation: pass the prior delivered enhanced prompt as `prompt`, pass the user's literal follow-on as `task`, and reset `critique_runs` to `[]` and start a fresh one-text `prompt_revisions` after its first rewrite. Preserve the prior run's verdict and change log as provenance. Only an explicit user turn may open this path; critic feedback, stylistic suggestions, or agent initiative never do. The new invocation retains its own one-correction maximum and independent verification.

### Step 4 — Output (enhance-output-render.j2, deterministic render)

1. Render with the full context shape — copy it, do not reconstruct it from prose:
   ```json
   { "enhanced_prompt": "<from step 2>", "prompt_type": "<from step 1>", "effort_tier": "<from step 1>",
     "output_format_resolved": "<from step 1>", "output_path_resolved": "<from step 1>",
     "audit_findings": <from step 2>, "grill_ratings": <step 3 ratings>,
     "grill_findings": <step 3 findings unchanged, or [] when verification is skipped>, "grill_verdict": "<from step 3>",
     "mutations_applied": <from step 2>, "mutations_deferred": <from step 2>,
     "unfold_mode_resolved": "<from step 1>", "reference_models": <from step 1.5>,
     "unfolded_plan": <from step 2> }
   ```
   Encode this whole context object once as JSON text for `render_template.context`; keep its seven array fields (`audit_findings`, `grill_ratings`, `grill_findings`, `mutations_applied`, `mutations_deferred`, `reference_models`, `unfolded_plan`) as arrays inside that object, never individually stringified. Use PE-State's scalar selected ID to select the ORIGINAL raw JSON review, and pass that raw review's `findings` unchanged as `grill_findings`, including on `rewrite_needed` or `fail`; do not reconstruct them from ratings or the verdict. At low effort pass `grill_verdict: "skipped"`, `grill_ratings: []`, `grill_findings: []`, `reference_models: []`, and `unfolded_plan: []` (if `rewrite_only`). A missing `grill_findings` field fails admission; a non-array value surfaces as `input_errors` and requires a corrected render. Do not pass a nested `verification` object or the retired `schema_defects` and `convergence_metric` fields.
2. Deliver the rendered `delivered_output` to the user. `inline` displays it; `file` and `both` additionally require a separate file-write action. A render-only step NEVER writes a file (`output_path_written` stays empty). Report a path only after a successful write.
3. Always include the change log: audit findings, critique verdict, ratings and specific findings, applied and deferred mutations. Preserve the selected critic's complete structured findings in both `delivered_output` and `change_log.grill_findings`; disclose PE-State's verification gap and selected review revision separately, never by rewriting or erasing those findings; a `rewrite_needed` or `fail` verdict must remain visible, never upgraded to verified.
4. This step makes no LLM call. The skill is not complete until its enhanced prompt is delivered, not merely rendered.

## Executor discipline (operator-approved 2026-10-19, from the live lockout run)

`render_template` supplies stage instructions; the agent is the executor. These bounds come from a live run (2026-10-19) where the executor burned ~20 calls emitting an empty `context` against the classify contract before falling back to reading the templates — and misfiled its own emission failure as a tool schema defect:

- **Pre-send gate (Prohibition):** before any `render_template` call, verify the stage contract's required keys are present in `context` — never emit an empty `context` object. An empty context yields `missing required input field(s): ...`, which reads like a schema defect but is an empty-argument emission.
- **Fallback:** if `render_template` fails for a reason that is not the context's content (tool hard-capped, populated context rejected, base-path error), read the stage template directly from `kask/registry/templates/prompt-enhance/<stage>.j2` and execute the stage against it — the stage's contract is the template's `[inference]` header wherever the instructions come from. Report the tool failure in the change log; never silently skip a stage's contract.
- **Stop bound:** stop after 3 consecutive failures of the same call shape and report the blocker; the retry tracker hard-refuses the 5th identical input, so retrying into the cap is wasted turns.

## Regression case

Run a mixed request (for example: improve a proof skill, check a finite invariant with `lisp_eval`, check an example with Lean, and decide which additions are useful) through classify → rewrite → verify → output at `medium` effort. Include one run whose verify verdict is `rewrite_needed`: it must drive exactly one corrective re-render (`prior_enhanced_prompt` plus `critic_feedback` taken from the verify `findings`) and one re-verify. Check that the classifier emits distinct `finite`, `formal`, `empirical`, and `judgment` obligations; the rewrite proposes checks without claiming they ran; the critic challenges whether those checks establish the claims; and the final `delivered_output` contains the enhanced prompt and an honest verdict. Exercise low-effort `file` output separately: `grill_verdict` must be `skipped` and `output_path_written` must remain empty until a separate write succeeds. A failed verdict must remain visible, not be silently changed to `pass`. Exercise the post-delivery boundary: (a) an agent-generated extra suggestion after one correction does not reopen the loop; (b) an explicit user amendment starts a new invocation with a fresh attempt log and revision history and the prior delivered prompt as baseline; (c) PE-State replays NEW-data attempt logs: initial low/medium/high routing, exact-next escalation, same-level reprobe, failed/pending exhaustion, one revision-bound correction, exact area coverage, invalid vocabulary and inclusive/exclusive threshold boundaries. Scalar-selected raw findings still reach the actual finalizer unchanged; no malformed or stale result certifies the current prompt.

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

