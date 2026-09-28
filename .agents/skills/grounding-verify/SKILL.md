---
name: grounding-verify
description: "Verify that factual claims in a text are grounded in provided source data, in the discipline of published verification practice: ISA 500 assertion-orientation and the evidence reliability hierarchy, vouching (claim → source) and tracing (source → text) direction of testing, ISA 320 materiality scoping, ISA 230 working-paper documentation. Extracts load-bearing claims, classifies provenance on a strength lattice (tool_verified > platform_derived > model_inference > unavailable), verifies citations and arithmetic via lisp_eval, traces sources for material omissions, detects cross-source conflicts with a precedence hierarchy, and emits findings with severity — never a verified label. Standalone or as a pipeline component."
---

# Grounding Verify

Verify that factual claims in a text are grounded in provided source data.
Anchored to published verification practice (operator rulings 2026-09-27):
ISA 500 (*Audit Evidence*) for assertion-orientation, the evidence
reliability hierarchy and inquiry-is-not-corroboration; vouching
(claim → source) and tracing (source → text) for direction of testing;
ISA 320 for materiality-scoped checking; ISA 230 for the working-paper
record; W3C PROV for provenance shape (the origins check's derivation
graph). Conflict adjudication follows the Verification Commons Protocol's
precedence hierarchy; proportionality follows ISA 320 materiality plus
the VCP graduated-severity scale — both cited where implemented. (Heuer's
ACH, Caulfield's SIFT and IEEE 1012 were named here with no operational
consumer — the roles they claimed were already anchored to the sources
that implement them — and were removed 2026-09-28.) Earlier
anchors retained: Fermi's four-contract trust system (grounding_trust,
schema_trust, rollup_trust, port_trust), the listening skill's
retrieve-cite-verify process, and the Verification Commons Protocol
(Ostrom institutional design: bounded provenance, decoupled monitoring,
graduated sanctions, conflict precedence, nested verification layers).
The skill is domain-general — it works on any text + source pair — but
accepts domain-specific leak rules and congruence rules via context.

## The provenance lattice

Every factual claim is classified into a provenance tier with a strength
ordinal. The adapted axis is falsifiability-through-execution, not ISA
500.A31's external-over-internal ordering: both strength-2 tiers carry a
`cross_check` form the interpreter runs, and checker-independence (ISA
500's inquiry-is-not-corroboration: an uncited opinion stays
`model_inference` regardless of who holds it) supplies the reliability
discipline. The .A31 documentary/oral and original/copy distinctions have
no counterpart here — all tool output is documentary, and the match runs
against the retained bytes. The lattice is the enforcement of the extraction
ceiling: a claim
the LLM synthesizes from tool outputs is `model_inference` (strength 1),
never `tool_verified` (strength 2). Only direct citations — verbatim quotes
found via mechanical substring match, exact numbers found via `lisp_eval`
numeric match — can be `tool_verified`.

The extraction ceiling applies to arithmetic too. Derived numbers (EV from
market cap + net debt, medians, discount percentages) are `platform_derived`
— strength 2 only when the derivation is stated as a `cross_check` form the
interpreter evaluates against input values. Arithmetic presented without a
falsifiable derivation is `model_inference`: legitimate, but unverified,
and the provenance floor will price it accordingly.

| Strength | Provenance Value | Meaning |
|----------|-----------------|---------|
| 2 | `tool_verified` | Value found in source output via mechanical match |
| 2 | `platform_derived` | Value computed by `lisp_eval` from sourced values |
| 1 | `model_inference` | LLM synthesized from source outputs (extraction ceiling) |
| 0 | `unavailable` | No source the pipeline called can supply this claim — the `why` must name which: no source exists, or a source exists but was never consulted |
| 0 | `tool_no_match` | Source was consulted and had nothing for this subject |
| 0 | `pending_check` | Check exists but has not run yet |
| 0 | `rejected` | Checked and found wrong |

The provenance vocabulary is a closed set. The Step 2 validation call
counts assignments whose value is outside the set; a count > 0
re-enters Step 2.

Absence is ambiguous until the record settles it: `tool_no_match` (asked,
empty), `unavailable` with a source that was never consulted (not asked —
the remedy is to run it and find out), and `unavailable` with no source that
can supply it (no tool exists) are three different states with three
different remedies. An `unavailable` claim whose `why` does not say which is
an unfinished classification, not a verdict. A compliant absence (the source
cannot supply the field, so the null is correct) must not be reported as a
failure.

A checker can lower a claim with authority and cannot raise one. An LLM
reviewer's agreement adds nothing — an uncited opinion sits at
`model_inference` strength regardless of who holds it, and the checker and
the claimant are likely the same model. Only a mechanical match (Step 3)
elevates to `tool_verified`; a checker's falsification is a reproducible
comparison over retained bytes and belongs in `rejected` with the evidence
cited.

## Initial and target condition

- **Initial condition:** the exact target text, server-observed source outputs and tool log, plus any caller-provided leak/congruence rules. A source that was not called is not an empty source result.
- **Target condition:** every load-bearing factual claim is either mechanically checked or explicitly downgraded with its reason; material omissions from the sources are surfaced; narrative source flags reflect *surviving* verified claims, not provisional assignments. The verifier classifies immutable input—it does not rewrite the report to pass, and its output is a working-paper record of what was checked and what was found (ISA 230) — never a verified label.

## When to Use

- When you need to verify that claims in a report, analysis, or pipeline
  output are grounded in the source data they cite.
- When you need a record of which claims are grounded, with provenance tier
  and source reference for each.
- When you are composing a larger pipeline that produces claims against
  source data and need a decoupled verification step that runs as a
  `spawn_agent` call.
- When you need a `verified_claims` registry — an append-only record of
  which claims are grounded, with provenance tier and source reference for
  each.
- When you need to scan narrative (prose) fields for claims that exceed
  what the sourced blocks can support (narrative leak detection).
- When two sources in the same pipeline supply conflicting values for the
  same quantity (a live quote vs. a derived metric, TTM vs. annual periods)
  and the report must disclose and resolve the conflict by precedence
  rather than silently pick one.

## When NOT to Use

- When there are no source outputs to verify against — there is nothing
  to check the claims against.
- When the text contains no IS-mode declarative claims — OUGHT claims
  (recommendations), subjunctive claims (scenarios), and probabilistic
  claims (forecasts) are excluded from verification.
- For reasoning quality checking — this skill checks grounding (does the
  source support the claim?), not logic (are the causal conclusions
  correct?).

## D/P labelling

| Step | Type | Oracle / critique |
|------|------|-------------------|
| 1 item 0 (prediction) | P | scored by the Step 7 item 0 reconciliation (D) — the prediction is a calibration claim, not a finding |
| 1 extract and classify | P | the operator; the mandatory `decoupling` field (an `in_thread` run is flagged as a self-check); the append-only error log |
| 1 item 4 (structural invariants) | D | `lisp_eval` (the zero-claim guard — pinned) |
| 2 provenance assignment | P | the lattice is fixed, the assignment is judgment — critiqued by the Step 2 validation calls (D) and the operator |
| 2 item 5 (validation calls) | D | `lisp_eval` (closed-vocabulary + why-min-40 counts — pinned) |
| 3 mechanical verification (drivers, grading wrapper, empty-env, origins, consistency) | D | `lisp_eval` — a claim's verification status comes from the form's output, never the model's assertion |
| 4 narrative scan | D render feeding P | `render_template` (D); leak-rule interpretation P — critiqued by the operator |
| 4 item 1 (block marking) | D | `lisp_eval` (the any-verified/mark pair — pinned) |
| 4.5 omission materiality | P | the operator; the record of what was examined and what was not |
| 5 counts | D | computed from the run's own records — never a composite score |
| 6 provenance floor + confidence band | D | `lisp_eval` (the floor form — pinned); the band derives from floor + conflicts + decoupling, never from the model |
| 7 item 0 (reconciliation) | D | `lisp_eval` (the gap form — pinned) |
| 7 severity + emission | P | the operator; graduated sanctions; high/critical findings surface immediately |

`render_template` renders are D (deterministic render) feeding P (the
assignment judgment over the rendered output). The output never says
verified: the counts and the floor are facts for the reader, not a verdict.

## Instructions

### Step 1 — Extract and classify claims

0. **Predict before extracting (calibration, P).** From the target text's length and the number of source outputs alone, state the expected number of load-bearing claims and the expected number that will fail mechanical verification (Step 3), with one sentence of basis. Record both before rendering; Step 7 reconciles them.
1. Call `render_template` to render the claim extraction template:
   - template_ref: `grounding-verify/extract-claims`
   - context: `{ "target_text": "{{ target_text }}", "source_outputs": {{ source_outputs }} }`
   - `source_outputs` is an array of `{ tool_name, description, output_key }`
     objects — one per MCP tool output; this is the shape `extract-claims.j2`
     renders (it reads `source.tool_name`, `source.description`,
     `source.output_key`). A bare string list renders an empty source list.

2. Following the template's output schema, extract all declarative factual
   claims from `target_text`. Each claim is a `(subject, predicate, object)`
   tuple with:
   - `claim_id`: unique identifier within this verification run
   - `text`: the exact text of the claim
   - `char_offset`: character offset in `target_text`
   - `materiality`: `load_bearing` or `incidental` — a claim is load-bearing
     if the text's conclusions would change if it were wrong (ISA 320); this
     drives finding severity and the tracing scope, not whether the cheap
     mechanical checks run (they run on everything)
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
   - For any `lisp_eval` call in this skill that walks the claims list with
     a recursive helper form, pass `max_depth` ≥ 8× the list length —
     helpers consume 2–4 depth frames per element, so the 1024 default
     only covers lists of a few hundred claims (observed 2026-09-03: a
     134-element assignments list needed ~300 depth frames and the former
     64 default failed both validation calls on first attempt).
   - If the result is `no_factual_claims`, emit the empty record with
     `data_gap: "no_factual_claims_found"` and stop.

### Step 2 — Assign provenance tier

1. Call `render_template` to render the provenance assignment template:
   - template_ref: `grounding-verify/assign-provenance`
   - context: `{ "claims": {{ step_1_result.claims }}, "source_outputs": {{ source_outputs }}, "pipeline_tool_log": {{ pipeline_tool_log }}, "congruence_rules": {{ congruence_rules }} }`
   - `congruence_rules` (optional, pass `[]` when none): an array of
     `{ quantity, primary_source, cross_check_sources, tolerance, resolution }`
     objects declaring which source is authoritative for which quantity and
     what relative tolerance applies (e.g., 0.05 for 5%). When empty, the
     Step 3 consistency pass is skipped — a scope limitation to disclose in
     Step 7, not a data gap.

2. Following the template's output schema, assign a provenance tier to each
   factual claim:
   - `tool_verified`: the claim's value was found in a source output via
     mechanical match. The template identifies the source and the
     response field. (Elevation to this tier requires Step 3 — the
     mechanical check must pass.)
   - `platform_derived`: the claim's value was computed by a `lisp_eval`
     call from sourced values (or values already verified in this run).
     The assignment must carry the derivation as a value-returning
     `cross_check` form, plus `claimed_value` (the number the report
     states), `unit` (the unit of the claim's last decimal place), and
     an `inputs` array naming each input's origin (a source output key,
     or an earlier claim's id) — Step 3 evaluates the form, grades the
     difference, and checks the origins. Prefer the canonical derivation
     forms from the template's library over authoring your own —
     selection over authoring shrinks the gaming surface — and record
     the selection in `library_form` (null when authored); library
     forms skip Step 3's empty-env check.
   - `model_inference`: the LLM synthesized the claim from source outputs.
     This is the extraction ceiling — legitimate, but not a retrieval.
   - `unavailable`: no source the pipeline called can supply this claim.
     The `why` must name which sub-case: no source exists, or a source
     exists but was never consulted (the remedy is to run it, not to
     report a gap).
   - `tool_no_match`: the source was called but returned nothing for this
     subject.
   - `pending_check`: a check exists but has not run yet.

3. For each `tool_verified` or `platform_derived` claim, the template
   must also emit a `cross_check` specification — the `lisp_eval` form
   that will verify the cited value against the source output
   (`string-contains` for quotes, value-returning derivation forms for
   arithmetic). A strength-2 claim with no `cross_check` is a claim
   nobody can falsify. If no cross-check is possible, the claim must be
   classified as `model_inference` with a `why` explaining why it cannot
   be mechanically verified. Record the infeasibility in
   `cross_check_exemptions` (the disclosure the template emits:
   `{claim_id, reason}`) — the exemption is a disclosure of why the check
   could not run, never a strength-2 pass; the claim's tier is still
   `model_inference`.

4. Each claim entry carries a `why` field (minimum 40 characters)
   explaining its provenance status. Short justifications are rejected. Each
   `sourced_blocks` entry carries the claim IDs provisionally associated with
   that block; its `is_sourced` flag is not final until Step 3 completes.

5. Validate the assignments before any claim is provisionally trusted —
   two `lisp_eval` calls over the Step 2 output. A count > 0 on either
   call re-enters Step 2: fix the flagged assignments before Step 3.
   These two calls are the enforcement line for the closed-vocabulary and
   why-min-40 constraints. Re-enter assignment at most once on a malformed
   result — the bound counts malformed results, not which check failed
   (a first-pass vocabulary failure followed by a re-entered why-length
   failure is one re-entry consumed; the next malformed result blocks).
   A blocked run emits the partial report with
   `verification_scope_limitations` naming the blockage — a consumer must
   be able to distinguish a blocked run from a crashed one, and the
   degradation rule applies: a failed check degrades visibly, never
   silently.
   - Closed vocabulary — count assignments whose `provenance` value is
     outside the closed set (a missing `provenance` counts as bad —
     fail-closed):
     `(let ((vocab (list "tool_verified" "platform_derived" "model_inference" "unavailable" "tool_no_match" "pending_check" "rejected")) (bad (lambda (lst) (cond ((is_null lst) 0) ((member (assoc "provenance" (car lst)) vocab) (bad (cdr lst))) (t (+ 1 (bad (cdr lst)))))))) (bad assignments))`
     env: `{ "assignments": <step_2_result.provenance_assignments> }`
   - why-min-40 — count assignments whose `why` is under 40 characters
     (a missing `why` counts as short — `length` of nil is 0):
     `(let ((short (lambda (lst) (cond ((is_null lst) 0) ((>= (length (assoc "why" (car lst))) 40) (short (cdr lst))) (t (+ 1 (short (cdr lst)))))))) (short assignments))`
     same env.

### Step 3 — Mechanically verify citations

Minimize the calls: `lisp_eval` executes locally (a sandboxed
in-process interpreter — no LLM, no API cost per execution), but every
call is an emission the model must write and a result it must read.
So each data-driven check family runs as ONE call — the canonical
library's drivers take all claims as data and return per-claim results.
The derive+grade wrapper (item 2) is the one per-claim call: iterate it
and emit all wrappers in parallel in a single turn. The only per-form
exception is the empty-env check (item 3), which cannot batch — a
combined empty-env call errors on the first unbound symbol and masks
constant derivations. If a driver call errors, fall back to per-claim
calls for that family to isolate the malformed entry. The only
sequential rule is item 4's cascade, which applies after the batch
returns.

1. For all claims provisionally classified as `tool_verified` in Step 2,
   run the mechanical verification as one call per family, using the
   canonical library's data-driven drivers:
   - **Cited quotes**: one call with the quote driver over all quoted
     claims — env: `{ "records": [{ "claim_id", "quote", "source_text" }, ...] }`.
     This is a mechanical substring match — not model-mediated. The LLM
     found the quote and pointed to it; the process verifies the pointer.
     An empty `quote` errors the call — an empty needle would verify
     anything, and a check that fires on correct output is worse than no
     check. For stored transcripts, `educt_locate` is the deterministic
     word-aligned locator — prefer it per quote when the source is an
     educt-stored transcript (it cannot batch).
   - **Cited numbers**: one call with the number driver over all cited
     numbers — env: `{ "records": [{ "claim_id", "value", "values" }, ...] }`
     where `values` is the referenced source output's value list. A
     claim whose `cross_check` form is not a plain membership check
     runs individually with its own form.
   A failed match (position i in the driver's result) reclassifies that
   claim as `rejected` (strength 0) and records it in
   `hallucination_findings` with the source mismatch details.

2. For each claim provisionally classified as `platform_derived`, verify
   the derivation — the deterministic arithmetic audit. The interpreter,
   not an LLM, re-derives every computed number in the report. One
   lightweight call per claim: wrap the claim's `cross_check` form as
   the `computed` binding in the grading wrapper, which returns
   `[computed_value, grade]`:
   - form: `"(let ((computed <cross_check form>)) (list computed (let ((diff (abs (- computed claimed)))) (cond ((<= diff (* 0.5 unit)) 'pass) ((<= diff (* 1.001 unit)) 'warn) (t 'fail)))))"`
   - env: the input values the derivation names, plus `claimed` (the
     value the report states) and `unit` (from Step 2)
   - If the `cross_check` form is already provided pre-wrapped (the library
     median), call it as-is — do not wrap it again.
   - The `1.001` slack on the warn boundary absorbs binary64
     representation error: `(- 18.0 17.9)` computes to
     `0.10000000000000142`, and without the slack the canonical
     rounding case grades `fail` instead of `warn` (probed live
     2026-09-08). A real 1.005-unit error still grades `fail`.
   - `pass`: the claim stays `platform_derived` (strength 2).
   - `warn`: the difference is at most one unit of the claim's last
     decimal — a rounding difference. The claim stays `platform_derived`;
     record a `rounding_notes` entry (class W, severity trivial) with
     both values.
   - `fail`: reclassify as `rejected` (strength 0); record in
     `hallucination_findings` (class E) with the prior (claimed) and
     corrected (computed) values.

3. Run the empty-env dependency check on every `platform_derived`
   `cross_check` form that was NOT copied verbatim from the canonical
   library — the anti-gaming falsifier:
   - A library form references its inputs by construction; the
     assignment's `library_form` field records which one was selected.
     Verify the declaration first — one call with the library driver
     over every claim with a non-null `library_form`:
     - env: `{ "records": [{ "claim_id", "library_form", "cross_check" }, ...] }`
     - `ok`: skip the empty-env check for this claim.
     - `mismatch` or `unknown_name`: the declaration is false — record
       a finding (class W, severity low: `library_form` did not match
       the recorded `cross_check`) and treat the form as authored:
       the empty-env check below applies to it.
     - The driver is a substring check: it catches false declarations,
       not engineered decoys — a form built to contain a library text
       while computing something else is a doctored form, and the
       registry records it verbatim for review.
   - For every authored form: call `lisp_eval` with the form and
     env `{}`. The call MUST fail with an unbound-symbol error: a real
     derivation references its input values, and against an empty env
     those references are unbound. A form that returns a value without
     the input env embeds its answer as a literal — a constant
     masquerading as a derivation. Reclassify the claim as
     `model_inference` and record the finding: the derivation is not
     falsifiable against the inputs.
   - Run the check on the bare `cross_check` form, not the grading
     wrapper — the wrapper's own `claimed`/`unit` bindings error
     against an empty env and would mask a constant derivation. This
     is also why the check cannot batch: a combined call errors on the
     first unbound symbol and masks the rest.
   - This check is what decouples the arithmetic audit: the derivation
     is stated once, and the interpreter — which has no incentive to
     confirm the report — both evaluates it and proves it depends on
     the cited inputs.

4. Run the origins check on all `platform_derived` claims in ONE call —
   the anchoring falsifier. The check builds a W3C PROV-shaped derivation
   graph: each claim an entity, each input an attribution to a source or an
   earlier entity, acyclic by the no-forward-references rule (PROV's
   provenance shape, cited in the frontmatter). Dependency is not anchoring: two forms can
   reference each other's outputs and pass every check above while
   grounded in no source at all (counterexample probed live
   2026-09-08: `x = value_b + 1`, `y = value_a + 1`). Each claim's
   `inputs` (from Step 2) name their origin — a source output key or
   an earlier claim's id:
   - Call `lisp_eval` once with the library's origins driver:
     - env: `{ "claims": [{ "claim_id", "inputs": [{ "name", "origin" }, ...] }, ...] in extraction order, "source_allowed": <only the origin strings of observed source outputs> }`. The driver itself adds `claim:<id>` only *after* processing that claim; do not give it one global list of claim IDs.
   - A claim whose bad-origin count > 0 has an input anchored in
     neither a source nor an earlier claim — a forward reference or an
     unanchored loop. Reclassify as `model_inference` with a `why`
     naming the unanchored input.
   - No-forward-references makes the derivation graph acyclic by
     construction — a cycle must cross the id order somewhere.
   - Cascade rule: when a claim grades `fail`, every later claim whose
     inputs reference it is demoted to `model_inference` (`why`: input
     claim rejected). Apply after the batch returns.

5. Run the cross-source consistency pass when `congruence_rules` were
   provided. For each rule `{ quantity, primary_source,
   cross_check_sources, tolerance, resolution }`:
   - Locate the primary value and each cross-check value in the source
     outputs (the location is model-mediated; the comparison is not).
   - Call `lisp_eval` once with the library's consistency driver over
     every rule × cross-check pair:
     - env: `{ "rules": [{ "quantity", "primary", "cross", "tolerance" }, ...] }`
   - On `conflict` (per record in the driver's result), emit a
     `source_conflicts` finding: both values, both
     sources, both periods/definitions, the relative difference, and the
     precedence disposition. The default hierarchy is `primary source
     (audited filing) > live market quote > derived metric > model
     inference`; a rule's `resolution` field overrides it for that
     quantity. The disposition: prefer the more direct source, disclose
     both values with their periods in the report appendix, and use the
     more conservative value in the main analysis when the difference
     is material and unresolved.
   - A conflict is a data note, not a hallucination: neither claim is
     `rejected`, the closed provenance vocabulary is unchanged, and the
     finding surfaces through the confidence band (Step 6) and the
     error log (Step 7) — never through a composite score.

6. Claims classified as `model_inference`, `unavailable`, or
   `tool_no_match` skip mechanical verification — they are not claiming
   direct citation or a falsifiable derivation.

### Step 4 — Scan narrative fields for leak rules

1. Identify all narrative (prose) fields in `target_text` — sections
   that are not structured data but prose written by the same model in
   the same turn. After Step 3 demotions and cascading rejects, discard
   the provisional `is_sourced` flags from Step 2. Let `verified_ids` contain
   only claims whose final tier is `tool_verified` or `platform_derived`.
   Compute each block's final flag from its `claim_ids` with `lisp_eval`:
   `(define any-verified (lambda (ids verified) (if (is_null ids) nil (or (member (car ids) verified) (any-verified (cdr ids) verified))))) (define mark (lambda (blocks verified) (if (is_null blocks) '() (cons (list (assoc "block_name" (car blocks)) (any-verified (assoc "claim_ids" (car blocks)) verified)) (mark (cdr blocks) verified))))) (mark blocks verified_ids)`.
   Require block IDs to refer to actual extracted claims; missing/unmapped
   blocks make the scan unverified, not clean. The block-to-claim association
   remains a model-mediated classification; the Boolean is computed from
   mechanically surviving claims rather than trusted from Step 2.

2. Call `render_template` to render the narrative scan template:
   - template_ref: `grounding-verify/scan-narrative`
   - context: `{ "narrative_fields": {{ narrative_fields }}, "sourced_blocks": {{ post_step_3_sourced_blocks }}, "leak_rules": {{ leak_rules }} }` — pass final `{block_name,is_sourced}` values, never the provisional Step 2 flags.

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

### Step 4.5 — Trace sources for material omissions

Vouching (Steps 1–4) tests whether the text's claims are supported.
Tracing runs the other direction of testing: for each source output, what
material facts does it disclose that the text's conclusions should engage
but do not? An omission is a finding even when every present claim is
supported.

1. For each source output, list its material facts — those bearing on the
   text's subject and conclusions, not every detail it contains. A source
   you do not examine is a recorded scope limitation, never a clean result.
2. Mark each fact `engaged` (the text addresses it), `omitted` (the text's
   conclusions should engage it but do not), or `immaterial` (outside the
   text's scope).
3. An `omitted` material fact is a finding with severity by the same
   graduated scale — a material omission the conclusions depend on is
   high severity.
4. This step is judgment over the sources' actual content; it cannot be
   mechanical. Record what you examined and what you did not.

### Step 5 — Report the counts

Report the raw counts, not a composite score: claims extracted, claims
checked, claims at each provenance tier, claims rejected, rounding notes,
narrative leaks, source conflicts, material omissions found. A composite
score invites threshold-gaming and implies a precision the measurement
does not have; the counts and findings are the working-paper record
(ISA 230). A zero-claim run reports zero claims checked — zero mismatches
over zero rows is unknown, not clean.

### Step 6 — Report the provenance floor

Compute the provenance floor — the minimum strength across all factual
claims (the Fermi `floor()` pattern: a report is only as strong as its
weakest claim) — and report it with the `decoupling` field
(`spawn_agent` or `in_thread`). An `in_thread` run by the text's generator
is a self-check — say so; the party being monitored cannot be the sole
monitor. The floor and the decoupling field are facts for the reader, not
a verdict on the text; the output never says verified.

1. Call `lisp_eval` to compute the floor (a claim record missing
   `strength` must error loudly — an unfinished classification surfaces,
   it does not silently floor):
   - form: `"(define floor-strength (lambda (cs) (if (= (length cs) 1) (assoc "strength" (nth 0 cs)) (let ((rest_min (floor-strength (cdr cs)))) (let ((this (assoc "strength" (car cs)))) (if (< this rest_min) this rest_min)))))) (floor-strength claims)"`
   - env: `{ "claims": <the Step 2 provenance assignments, each with "strength"> }`
   - Pass `max_depth` ≥ 8× the claims count (the recursive helper — see
     Step 1 item 4's depth note).

2. Derive and report the confidence band from the floor, the source
   conflicts, and the decoupling field (the Fermi `hud_contract.rs`
   `confidence_for` adaptation — derived from the provenance verdict,
   never accepted from the model): the band caps at `medium` for an
   `in_thread` run (a self-check, no matter how strong the claims) and at
   `medium` when unresolved material `source_conflicts` are present; a
   high or critical finding surfaces immediately regardless of the band.
   The band is a derived fact for the reader, like the floor — never a
   verdict.

### Step 7 — Emit verification report

0. **Reconcile the prediction (D).** Report `predicted_load_bearing` / `found_load_bearing` and `predicted_failures` / `found_failures` from Step 1 item 0, gaps computed by `lisp_eval` `(list (- found_load_bearing predicted_load_bearing) (- found_failures predicted_failures))`. A large gap on the failure count means the verifier's prior about this source mix is miscalibrated — report it; it is not a finding against the text.
1. Emit the verification report with:
   - `counts`: claims extracted, load-bearing claims, claims checked, claims
     at each provenance tier, claims rejected — the raw counts from Step 5
   - `provenance_floor`: the minimum claim strength, and `decoupling`:
     spawn_agent | in_thread — mandatory. A consumer must
     be able to tell a decoupled audit from a self-check.
   - `verified_claims`: append-only registry of all claims with
     provenance tier, source reference, `why`, cross_check result
   - `hallucination_findings`: claims reclassified as `rejected`, with
     source mismatch details, prior and corrected values, severity
   - `material_omissions`: the Step 4.5 tracing findings — omitted facts
     with their source, severity, and what the text's conclusions should
     engage
   - `rounding_notes`: (claim, claimed, computed) entries that passed
     within one unit of the last decimal (class W)
   - `source_conflicts`: cross-source findings with both values, both
     sources/periods, relative difference, precedence disposition,
     severity (class N)
   - `narrative_leaks`: list of (field, block, rule, matched_text)
   - `data_gaps`: list of failed verifications, missing sources —
     never empty if anything failed
   - `error_log`: the graduated-sanctions log compiled per item 3
   - `verification_scope_limitations`: honest disclosure of what the
     verification does not cover (reasoning quality, plausible
     fabrications, selective framing) and which sources the tracing
     pass did not examine — plus whether the consistency pass ran (were
     `congruence_rules` provided?)

2. Every finding carries a severity and a recommended disposition —
   graduated sanctions, not uniform punishment:
   - trivial (rounding): note in the error log; no correction
   - low (single figure, conclusion unaffected): note; correction
     optional
   - medium (changes a supporting figure): the calling pipeline
     corrects inline with attribution
   - high (changes the primary conclusion): surface immediately — do
     not hold it back for the full report; the pipeline must re-examine
     the conclusion
   - critical (invalidates a section): surface immediately; the pipeline
     must withdraw the section
   The skill emits severity and disposition; the calling pipeline
   executes corrections. This skill is the verifier, not the generator.

3. Compile the error log: call `render_template` with
   - template_ref: `grounding-verify/compile-error-log`
   - context: `{ "hallucination_findings": {{ hallucination_findings }}, "rounding_notes": {{ rounding_notes }}, "source_conflicts": {{ source_conflicts }} }`
   The log is append-only within the run — a correction records both
   the prior and the corrected value; it never erases the original.

4. The `verified_claims` registry is append-only within a verification
   run. A claim verified as `tool_verified` stays `tool_verified`. A
   claim that failed verification stays `rejected` — it does not get
   re-classified when new sources are added. This prevents the
   un-stripping trap.

## Convergence

The local PDCA operates on the *same immutable text and observed sources*:

1. **Plan:** record the initial condition and target above, including the applicable source and narrative checks.
2. **Do:** execute Steps 1–7. The verifier does not rewrite the user's claims to obtain a higher score.
3. **Check:** reject malformed provenance classifications and missing claim-to-block mappings rather than treating them as empty-success. A run with zero claims checked reports zero — unknown, not clean. Surface a high/critical finding immediately regardless of the counts.
4. **Act (bounded):** one correction of malformed *verification input* may re-enter Step 2, followed by its downstream checks; a second malformed result stops as blocked. A sourced claim falsified by Step 3 stays rejected in this run. Provide specific fact-check gaps to the calling pipeline; only the caller may revise the report or gather new source evidence in a new run. Do not loop locally to improve a model-produced score.

This is a bounded verifier loop, not skill-effectiveness evaluation; the operator judges the quality of this verifier in the algedonic-review gemba.

## Regression case

Renders (declared contexts): `grounding-verify/extract-claims` with `target_text` (a short report citing one verbatim quote and one derived number) and `source_outputs` (two `{tool_name, description, output_key}` objects); `assign-provenance` with the extracted claims, the source outputs, a `pipeline_tool_log`, and one `congruence_rules` entry; `scan-narrative` with `narrative_fields`, post-Step-3 `sourced_blocks`, and `leak_rules` (one `Word` rule from an unsourced block); `compile-error-log` with one finding of each class (E, W, N). Forms: the six SKILL.md-pinned forms are executed by the pin suite (`test_canonical_provenance_floor_form`, `test_canonical_vocabulary_check_form`, `test_canonical_why_length_check_form`, `test_canonical_step1_structural_form`, `test_canonical_narrative_mark_form`, `test_canonical_prediction_reconciliation_form` — the last two added 2026-09-28). Full-run-only behaviors a render cannot check (a case that claims them over-claims): the extraction ceiling's correct operation (a synthesized claim held at strength 1), the append-only registry, and the decoupling field's presence in the emitted report — asserted by the process, not by any render.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `extract-claims.j2` | Extract all declarative factual claims from target text. Each claim is a (subject, predicate, object) tuple with character offset, epistemic mode classification (IS/OUGHT/subjunctive/probabilistic per pragmatic-semantics), and source reference. Emits `claims` (IS-mode only) and `excluded_claims` (other modes). |
| `assign-provenance.j2` | Assign a provenance tier to each factual claim using the strength lattice. For each `tool_verified` or `platform_derived` claim, emits a `cross_check` specification (a lisp_eval form — `string-contains` for quotes, value-returning derivation forms for arithmetic) plus, for `platform_derived`, the `claimed_value`, `unit`, origin-tagged `inputs`, and `library_form`. Includes a canonical derivation form library (EV, multiples, growth, discount, median) plus data-driven batch drivers (cited numbers, cited quotes, origins, consistency, library verification) that run each check family as ONE call — select and bind instead of authoring. Consumes `congruence_rules` when provided. Each claim carries a `why` field (min 40 chars). Emits `provenance_assignments` with provenance, strength, source_reference, cross_check, claimed_value, unit, inputs, library_form, why. |
| `scan-narrative.j2` | Scan narrative (prose) fields for leak rules — patterns that assert something only a sourced block could support. Uses `Word` and `Quantity` leak rule variants. Emits `narrative_leaks` with (field, block, rule, matched_text) tuples. |
| `compile-error-log.j2` | Compile the graduated-sanctions error log from all findings — `hallucination_findings` (class E), `rounding_notes` (class W), `source_conflicts` (class N) — into one append-only table with ID, class, claim, description, prior value, corrected value, severity, disposition, and a materiality `why`. |

To render a template, call `render_template` with the template ref (e.g.,
`grounding-verify/extract-claims`) and a context object with the required
variables.

## Composition

This skill is designed as a **delegation target**, mirroring the
architectural role of `essentialist` and `falsifiability`:

- **company-research-deep** and **company-research-flash** each run their
  own author-side evidence review (`company-research/evidence-review`) over
  their frozen drafts instead of a decoupled verifier; neither invokes this
  skill (observed 2026-09-27; if a sibling adopts this skill, update this
  note in the same change).
- Any pipeline that produces claims against source data can compose this
  skill as a verification step.

The skill is also usable **standalone** — a user provides a text and its
source outputs, and the skill produces the findings, counts and
verified_claims registry.

**This skill is one layer of a nested verification stack, not the whole
stack.** Each layer catches a different error type, and no single layer
is expected to catch everything:

- Inline arithmetic — the `platform_derived` cross_check forms ARE the
  deterministic math audit: the derivation is stated once and the
  interpreter evaluates it. No separate math-auditor agent is needed;
  an LLM re-deriving LLM arithmetic is a weaker check than `lisp_eval`.
- Claim grounding — this skill (provenance lattice, mechanical checks,
  tracing, findings).
- Decoupled re-execution — composing this skill as a `spawn_agent` call
  (no shared conversation history) for publication-grade reports.
- User review — the operator is the final authority; grounded claims mean
  "the sources support them," never "the report is correct."

**When composing as `spawn_agent`, pass the source outputs in the task.**
Spawned agents do not reliably reach external MCP tools; a verifier
asked to pull its own data will silently fall back to training-data
estimates that look like tool output (observed 2026-09-07, VCP testing).
A verifier that reports values absent from the passed source outputs is
fabricating, not verifying.

**Congruence rules live with the composing pipeline.** This skill is
domain-general; the domain-specific table (which tool is authoritative
for which quantity, what tolerance applies) is supplied via context by
the pipeline that knows the domain — e.g., the equity table
(`income_statement` over `key_metrics` back-calculation, `stock_quote`
for market cap) belongs to company-research-deep/flash's context
construction, not to this skill's body.

**Deliberate scope boundaries (operator ratification, 2026-09-08):**
verification tiers (depth selection) and the primary source pull live
in the composing pipelines, not in this skill — the skill is
single-pass by design and verifies against provided sources only.

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
- The conflict precedence hierarchy, graduated finding severity, and
  decoupling observability are adapted from the Verification Commons
  Protocol v1.0 (2026-09-07), which grounds research verification in
  Ostrom's design principles for governing common-pool resources.

## Constraints

- The provenance vocabulary is a closed set: `tool_verified`,
  `platform_derived`, `model_inference`, `unavailable`, `tool_no_match`,
  `pending_check`, `rejected`. The Step 2 validation call counts
  assignments outside this set; a count > 0 re-enters Step 2.
- The extraction ceiling: LLM-synthesized claims are `model_inference`
  (strength 1), never `tool_verified` (strength 2). Only direct citations
  verified via mechanical match can be `tool_verified`.
- Citation verification is mechanical (`lisp_eval` `string-contains` for
  quotes, `lisp_eval` numeric match for numbers), not model-mediated. The
  LLM finds and points; the process verifies.
- Every `tool_verified` and `platform_derived` claim must carry a
  `cross_check` specification. A strength-2 claim with no cross-check
  is a claim nobody can falsify — reclassify as `model_inference` with
  a `why`.
- Each claim entry carries a `why` field (minimum 40 characters). Short
  justifications are rejected by the Step 2 validation call (count > 0
  re-enters Step 2) — an unexplained disposition is how a contract rots.
- No composite score is emitted. The raw counts, findings and provenance
  floor are the record; a zero-claim run reports zero claims checked —
  unknown, not clean.
- The `verified_claims` registry is append-only within a verification
  run. Claims do not get re-classified when new sources are added.
- The provenance floor and the `decoupling` field are reported facts, not
  a verdict; the output never says verified. An `in_thread` run by the
  text's generator is reported as a self-check.
- `verification_scope_limitations` must be disclosed in the output,
  including which sources the tracing pass did not examine. The
  verification covers grounding and material omissions, not reasoning
  quality.
- When composed as a `spawn_agent` call, the verifier has no shared
  conversation history with the generator (self-improvement §9.1).
- The empty-env dependency check applies to authored `platform_derived`
  cross_check forms only — a form copied verbatim from the canonical
  library (recorded in `library_form`) references its inputs by
  construction. An authored form evaluated against env `{}` must fail
  with an unbound-symbol error; a form that returns a value without
  the input env embeds its answer as a literal — reclassify the claim
  as `model_inference` and record the finding. Run it on the bare
  form, never the grading wrapper; it cannot batch.
- Each data-driven check family (cited numbers, cited quotes, origins,
  consistency, library verification) executes as ONE `lisp_eval` call
  via a canonical library driver; the derive+grade wrapper is the only
  per-claim call. A driver error falls back to per-claim calls for that
  family.
- The warn boundary carries a `1.001`×unit slack: binary64
  representation error at the boundary (`18.0 − 17.9` computes to
  `0.10000000000000142`) must not turn a rounding difference into a
  rejection.
- Every `platform_derived` input names its origin — a source output
  key or an earlier claim's id. Forward references and unanchored
  loops reclassify as `model_inference`; a rejected claim cascades to
  its dependents.
- A text with no narrative fields reports zero narrative leaks and
  discloses `no narrative fields` in `verification_scope_limitations`.
  A table-only report is not a failed measurement.
- `source_conflicts` are findings, not provenance values. The closed
  vocabulary is unchanged; a conflict rejects neither claim. Conflicts
  cap the confidence band and enter the error log; they never enter
  a composite score.
- The `decoupling` field is mandatory in the verification report. An
  `in_thread` run by the report's generator is a self-check — its
  confidence band caps at `medium` no matter how strong the claims
  are.
- The error log is append-only within a run: every correction records
  both the prior and the corrected value. Findings carry severity
  (trivial/low/medium/high/critical) and a recommended disposition;
  the calling pipeline executes corrections — this skill emits
  findings, it does not rewrite the report.
- Findings with severity high or critical surface immediately,
  regardless of the provenance floor.
- If any tool call fails (`render_template`, `lisp_eval`), call
  `curator_report_skill_use_issue` with `skill_name: "grounding-verify"`,
  the failed tool, and the error — then continue with the best available
  verification. A failed check degrades the report's claims (mark
  unverified), never silently passes them through.
- Corroborated is not confirmed. Never output "proven" or "verified true."
  Use "survived," "withstood," "grounded."
