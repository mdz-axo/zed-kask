---
name: grounding-verify
description: "Verify that factual claims in a text are grounded in provided source data. Extracts claims, classifies provenance on a strength lattice (tool_verified > model_inference > unavailable), mechanically verifies citations via retrieve-cite-verify, scans narrative fields for leak rules, and computes a composite fact_score with nil-propagation. Usable standalone or as a composed component in analysis pipelines."
---

# Grounding Verify

Verify that factual claims in a text are grounded in provided source data.
Anchored to Fermi's four-contract trust system (grounding_trust, schema_trust,
rollup_trust, port_trust) and the listening skill's retrieve-cite-verify
process. The skill is domain-general — it works on any text + source pair —
but accepts domain-specific leak rules via context for narrative scanning.

## The provenance lattice

Every factual claim is classified into a provenance tier with a strength
ordinal. The lattice is the enforcement of the extraction ceiling: a claim
the LLM synthesizes from tool outputs is `model_inference` (strength 1),
never `tool_verified` (strength 2). Only direct citations — verbatim quotes
found via mechanical substring match, exact numbers found via `lisp_eval`
numeric match — can be `tool_verified`.

| Strength | Provenance Value | Meaning |
|----------|-----------------|---------|
| 2 | `tool_verified` | Value found in source output via mechanical match |
| 2 | `platform_derived` | Value computed by `lisp_eval` from sourced values |
| 1 | `model_inference` | LLM synthesized from source outputs (extraction ceiling) |
| 0 | `unavailable` | No source the pipeline called can supply this claim |
| 0 | `tool_no_match` | Source was consulted and had nothing for this subject |
| 0 | `pending_check` | Check exists but has not run yet |
| 0 | `rejected` | Checked and found wrong |

The provenance vocabulary is a closed set. The `lisp_eval` scoring call
rejects unrecognized values.

## When to Use

- When you need to verify that claims in a report, analysis, or pipeline
  output are grounded in the source data they cite.
- When you need a `fact_score` metric measuring the grounding quality of a
  text against its sources.
- When you are composing a larger pipeline (e.g., company-research-deep)
  and need a decoupled verification step that runs as a `spawn_agent` call.
- When you need a `verified_claims` registry — an append-only record of
  which claims are grounded, with provenance tier and source reference for
  each.
- When you need to scan narrative (prose) fields for claims that exceed
  what the sourced blocks can support (narrative leak detection).

## When NOT to Use

- When there are no source outputs to verify against — the fact_score will
  be nil (measurement meaningless, not zero).
- When the text contains no IS-mode declarative claims — OUGHT claims
  (recommendations), subjunctive claims (scenarios), and probabilistic
  claims (forecasts) are excluded from the fact score.
- For completeness checking — this skill checks factuality (is the claim
  real?), not completeness (are all relevant facts cited?).
- For reasoning quality checking — this skill checks grounding (does the
  source support the claim?), not logic (are the causal conclusions
  correct?).

## Instructions

### Step 1 — Extract and classify claims

1. Call `render_template` to render the claim extraction template:
   - template_ref: `grounding-verify/extract-claims`
   - context: `{ "target_text": "{{ target_text }}", "source_outputs": {{ source_outputs }} }`

2. Following the template's output schema, extract all declarative factual
   claims from `target_text`. Each claim is a `(subject, predicate, object)`
   tuple with:
   - `claim_id`: unique identifier within this verification run
   - `text`: the exact text of the claim
   - `char_offset`: character offset in `target_text`
   - `epistemic_mode`: IS / OUGHT / subjunctive / probabilistic (per
     pragmatic-semantics classification)
   - `source_reference`: the named source the claim cites (MCP tool name +
     output key, transcript chunk_id, URL), or `none` if uncited

3. Only IS-mode declarative claims proceed to verification. OUGHT,
   subjunctive, and probabilistic claims are recorded as `excluded_claims`
   with their epistemic mode — they are not fabrications, they are
   different kinds of statement.

4. Call `lisp_eval` to check structural invariants:
   - form: `"(if (= (length claims) 0) 'no_factual_claims 'ok)"`
   - env: `{ "claims": <your extracted claims> }`
   - If the result is `no_factual_claims`, emit `fact_score = nil` with
     `data_gap: "no_factual_claims_found"` and stop.

### Step 2 — Assign provenance tier

1. Call `render_template` to render the provenance assignment template:
   - template_ref: `grounding-verify/assign-provenance`
   - context: `{ "claims": {{ step_1_result.claims }}, "source_outputs": {{ source_outputs }}, "pipeline_tool_log": {{ pipeline_tool_log }} }`

2. Following the template's output schema, assign a provenance tier to each
   factual claim:
   - `tool_verified`: the claim's value was found in a source output via
     mechanical match. The template identifies the source and the
     response field. (Elevation to this tier requires Step 3 — the
     mechanical check must pass.)
   - `platform_derived`: the claim's value was computed by a `lisp_eval`
     call from sourced values.
   - `model_inference`: the LLM synthesized the claim from source outputs.
     This is the extraction ceiling — legitimate, but not a retrieval.
   - `unavailable`: no source the pipeline called can supply this claim.
   - `tool_no_match`: the source was called but returned nothing for this
     subject.
   - `pending_check`: a check exists but has not run yet.

3. For each `tool_verified` claim, the template must also emit a
   `cross_check` specification — the `lisp_eval` form or substring match
   that will verify the cited value against the source output. A
   `tool_verified` claim with no `cross_check` is a claim nobody can
   falsify. If no cross-check is possible, the claim must be classified
   as `model_inference` with a `why` explaining why it cannot be
   mechanically verified.

4. Each claim entry carries a `why` field (minimum 40 characters)
   explaining its provenance status. Short justifications are rejected.

### Step 3 — Mechanically verify citations

1. For each claim provisionally classified as `tool_verified` in Step 2,
   perform the mechanical verification:
   - **Cited quotes**: verify that the exact substring exists in the
     referenced source chunk. This is a mechanical substring match —
     not model-mediated. The LLM found the quote and pointed to it;
     the process verifies the pointer.
   - **Cited numbers**: call `lisp_eval` to verify that the numeric value
     appears in the referenced source output. For derived numbers (e.g.,
     "revenue growth of 12%"), the `cross_check` form states the
     derivation and `lisp_eval` checks it against source values.

2. Call `lisp_eval` for each numeric cross-check:
   - form: the `cross_check` form from Step 2
   - env: the source output values

3. If the mechanical check passes, the claim stays `tool_verified`
   (strength 2). If it fails, the claim is reclassified as `rejected`
   (strength 0) and recorded in `hallucination_findings` with the source
   mismatch details.

4. Claims classified as `model_inference`, `platform_derived`,
   `unavailable`, or `tool_no_match` skip mechanical verification —
   they are not claiming direct citation.

### Step 4 — Scan narrative fields for leak rules

1. Identify all narrative (prose) fields in `target_text` — sections
   that are not structured data but prose written by the same model in
   the same turn.

2. Call `render_template` to render the narrative scan template:
   - template_ref: `grounding-verify/scan-narrative`
   - context: `{ "narrative_fields": {{ narrative_fields }}, "sourced_blocks": {{ sourced_blocks }}, "leak_rules": {{ leak_rules }} }`

3. Following the template's output schema, scan each narrative field
   against the leak rules table. Each rule pairs a source block with a
   pattern (`Word` for distinctive keywords, `Quantity` for
   number-preceded-by-unit patterns). If a narrative field contains a
   keyword from a block that is NOT sourced (provenance strength 0), it
   is a narrative leak.

4. The `Quantity` variant only fires when a number precedes the unit —
   a check that fires on correct output is worse than no check: it gets
   switched off, and the switching-off looks like cleanup.

5. Emit `narrative_leaks` — a list of (field, block, rule, matched_text)
   tuples for each leak detected.

### Step 5 — Compute fact_score

1. Compute the four sub-metrics from the verified claims:
   - **SAR** (Source-Anchored Ratio) = source_anchored_claims /
     total_factual_claims. A claim is source-anchored if its provenance
     strength is >= 1 (tool_verified, platform_derived, or
     model_inference).
   - **CVR** (Citation-Verified Ratio) = verified_citations /
     total_tool_verified_citations. Only claims provisionally classified
     as `tool_verified` are in the denominator.
   - **HFR** (Hallucination-Free Ratio) = surviving_claims /
     total_factual_claims. Claims reclassified as `rejected` in Step 3
     are eliminated.
   - **NLR** (Narrative-Leak Ratio) = clean_narrative_fields /
     total_narrative_fields. A field is clean if no leak rule fires, or
     if every fired rule is backed by a sourced block (strength >= 2).

2. Call `lisp_eval` to compute the fact_score with nil-propagation:
   - form: `"(if (or (member nil (list sar cvr hfr nlr)) (= claims_checked 0)) 'nil (let ((score (+ (* 0.30 sar) (* 0.25 cvr) (* 0.20 hfr) (* 0.25 nlr)))) score))"`
   - env: `{ "sar": <SAR value or nil>, "cvr": <CVR value or nil>, "hfr": <HFR value or nil>, "nlr": <NLR value or nil>, "claims_checked": <count> }`

3. If any sub-metric is nil or `claims_checked` is 0, `fact_score` is nil
   — not 0.0. A nil fact_score surfaces as
   `data_gap: "fact_score_measurement_failed"` with confidence penalty
   -0.20. This is the nil-propagation invariant: a failed measurement is
   a broken feedback loop, not a zero score.

4. Apply the threshold:
   - `fact_score >= 0.80`: meets bar, no penalty
   - `0.60 <= fact_score < 0.80`: below bar, confidence penalty -0.10
   - `fact_score < 0.60`: fails bar, block and re-enter with gaps
   - `fact_score = nil`: measurement failed, confidence penalty -0.20

### Step 6 — Derive confidence band from provenance floor

1. Compute the provenance floor — the minimum strength across all
   factual claims in the report. This is the Fermi `floor()` pattern:
   a report is only as strong as its weakest claim.

2. Call `lisp_eval`:
   - form: `"(min (mapcar (lambda (c) (assoc 'strength c)) claims))"`
   - env: `{ "claims": <verified claims with strength values> }`

3. Derive the confidence band from the floor:
   - Floor = 2 (all claims tool_verified or platform_derived): band = `high`
   - Floor = 1 (weakest claim is model_inference): band = `medium`
   - Floor = 0 (weakest claim is unavailable/tool_no_match/rejected): band = `flagged`

4. The confidence band is derived from the provenance floor — never
   accepted from the LLM's self-assessed confidence. If the weakest
   claim is `model_inference`, the band is `medium` regardless of what
   the report says about its own confidence.

### Step 7 — Emit verification report

1. Emit the verification report with:
   - `fact_score`: numeric or nil
   - `fact_score_breakdown`: { sar, cvr, hfr, nlr, claims_checked }
   - `confidence_band`: high / medium / flagged (from Step 6)
   - `confidence_adjustment`: numeric penalty (0 / -0.10 / -0.20)
   - `verified_claims`: append-only registry of all claims with
     provenance tier, source reference, `why`, cross_check result
   - `hallucination_findings`: list of claims reclassified as `rejected`
     with source mismatch details
   - `narrative_leaks`: list of (field, block, rule, matched_text)
   - `data_gaps`: list of failed verifications, missing sources, nil
     sub-metrics — never empty if anything failed
   - `verification_scope_limitations`: honest disclosure of what the
     fact score does not cover (completeness, reasoning quality,
     plausible fabrications, selective omission)

2. The `verified_claims` registry is append-only within a verification
   run. A claim verified as `tool_verified` stays `tool_verified`. A
   claim that failed verification stays `rejected` — it does not get
   re-classified when new sources are added. This prevents the
   un-stripping trap.

## Convergence

The skill is single-pass (sense → verify → report), not iterative. The
convergence signal is the fact_score itself:
- `fact_score >= 0.80`: verification passes, report proceeds.
- `fact_score < 0.60` or `fact_score = nil`: verification fails, the
  calling pipeline re-enters with fact-check gaps injected.

The calling pipeline (not this skill) handles the convergence loop —
this skill is the verifier, not the generator.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `extract-claims.j2` | Extract all declarative factual claims from target text. Each claim is a (subject, predicate, object) tuple with character offset, epistemic mode classification (IS/OUGHT/subjunctive/probabilistic per pragmatic-semantics), and source reference. Emits `claims` (IS-mode only) and `excluded_claims` (other modes). |
| `assign-provenance.j2` | Assign a provenance tier to each factual claim using the strength lattice. For each `tool_verified` claim, emits a `cross_check` specification (lisp_eval form or substring match). Each claim carries a `why` field (min 40 chars). Emits `provenance_assignments` with tier, source_reference, cross_check, why. |
| `scan-narrative.j2` | Scan narrative (prose) fields for leak rules — patterns that assert something only a sourced block could support. Uses `Word` and `Quantity` leak rule variants. Emits `narrative_leaks` with (field, block, rule, matched_text) tuples. |

To render a template, call `render_template` with the template ref (e.g.,
`grounding-verify/extract-claims`) and a context object with the required
variables.

## Composition

This skill is designed as a **delegation target**, mirroring the
architectural role of `essentialist` and `falsifiability`:

- **company-research-deep** invokes `grounding-verify` as a `spawn_agent`
  call at two pipeline points (early anchor after company-8part, late gate
  after thesis-essentialist). The spawned agent receives the stage output
  + source outputs as inputs — it has no shared conversation history with
  the generator. This is the self-improvement §9.1 decoupling enforcement.
- **company-research-flash** can invoke `grounding-verify` as a late gate
  before the LENS consistency audit.
- Any pipeline that produces claims against source data can compose this
  skill as a verification step.

The skill is also usable **standalone** — a user provides a text and its
source outputs, and the skill produces a fact_score and verified_claims
registry.

## Cross-Skill Composition

- Step 1 reuses `structured-extraction` (claim extraction as
  entity/relation tuples with character offsets).
- Step 1 classifies claims by epistemic mode using `pragmatic-semantics`
  (IS/OUGHT, declarative/probabilistic/subjunctive).
- Step 3 reuses the `listening` skill's retrieve-cite-verify process
  (mechanical substring match for cited quotes).
- Step 3 uses `lisp_eval` for deterministic numeric verification.
- The provenance lattice and extraction ceiling are adapted from Fermi's
  `grounding_trust.rs` (strength ordinal, `EXTRACTION_CEILING`).
- The narrative leak scan is adapted from Fermi's `NARRATIVE_LEAKS`
  pattern (`LeakRule::Word` and `LeakRule::Quantity`).
- The confidence band from provenance floor is adapted from Fermi's
  `hud_contract.rs` (`confidence_for` derived from provenance verdict,
  never accepted from the model).

## Constraints

- This SKILL.md body is the authoritative methodology. Jinja2 templates
  in the registry are structured reference versions of the same content.
- The provenance vocabulary is a closed set: `tool_verified`,
  `platform_derived`, `model_inference`, `unavailable`, `tool_no_match`,
  `pending_check`, `rejected`. The `lisp_eval` scoring call rejects
  unrecognized values.
- The extraction ceiling: LLM-synthesized claims are `model_inference`
  (strength 1), never `tool_verified` (strength 2). Only direct citations
  verified via mechanical match can be `tool_verified`.
- Citation verification is mechanical (substring match for quotes,
  `lisp_eval` numeric match for numbers), not model-mediated. The LLM
  finds and points; the process verifies.
- Every `tool_verified` claim must carry a `cross_check` specification.
  A `tool_verified` claim with no cross-check is a claim nobody can
  falsify — reclassify as `model_inference` with a `why`.
- Each claim entry carries a `why` field (minimum 40 characters). Short
  justifications are rejected — an unexplained disposition is how a
  contract rots.
- No `unwrap_or(0)` on fact_score sub-metrics. If any sub-metric is nil,
  fact_score is nil. A nil fact_score surfaces as
  `data_gap: "fact_score_measurement_failed"` with confidence penalty
  -0.20.
- `claims_checked` count accompanies the fact_score. A fact score of 1.0
  with zero claims checked is nil — zero mismatches over zero rows is
  unknown, not clean.
- The `verified_claims` registry is append-only within a verification
  run. Claims do not get re-classified when new sources are added.
- The confidence band is derived from the provenance floor, never
  accepted from the LLM's self-assessed confidence.
- `verification_scope_limitations` must be disclosed in the output. The
  fact score covers factuality, not completeness or reasoning quality.
- When composed as a `spawn_agent` call, the verifier has no shared
  conversation history with the generator (self-improvement §9.1).
- Corroborated is not confirmed. Never output "proven" or "verified true."
  Use "survived," "withstood," "grounded."