---
name: pragmatic-semantics
core: true
description: "Epistemic discipline for classifying statements by certainty level, constraint force, and domain ontology anchoring. Distinguish IS from OUGHT, declarative from probabilistic from subjunctive. Resolve conflicts using OT ranking. Route computation steps to deterministic or probabilistic machines (D/P labelling, P8.4)."
audience: curator
---

# Pragmatic Semantics

Epistemic discipline for classifying statements by certainty level, constraint force, and domain ontology anchoring. Distinguish IS from OUGHT, declarative from probabilistic from subjunctive. Classify provenance of facts and their ontology tier (Core / Dual-Axis / Domain Supplement). Resolve conflicts using OT ranking with ontology anchoring. Route computation steps to the machine whose conditional entropy matches the task's — D/P labelling per P8.4.

## Reference models

Hume's is–ought distinction (*A Treatise of Human Nature*, 1739); Optimality Theory's strict-domination ranking (Prince & Smolensky, 1993) for conflict resolution; epistemic modality (Palmer, *Mood and Modality*, 1986) for declarative / probabilistic / subjunctive. Entropy-matched computation (P8.4): Shannon (1948) and Jaynes (1957) for conditional entropy; Turing (1936, 1939) for deterministic computation and oracles; Tetlock & Gardner (2015) and Brier (1950) for calibrated forecasts — the four derived-registry rulings (entropy, deterministic computation, verification oracle, calibrated forecast; rulings 2026-09-27, state as of 2026-11-01). Amortized inference (Gershman & Goodman 2014; Hu et al. 2024) for learned computation resolves coarsely on the 5W1H core rung with no derived entry — ruling pending (state as of 2026-11-01).

## D/P labelling

Statement classification (all axes of semantics-classify-statement, the five-tier classifications and the resolution strategy of semantics-conflict-resolve, provenance-trace steps 1–7) and semantics-route-step's regime classification, mismatch flags, and hybrid labeling are P — judgment, critiqued by the operator (a mismatch flag naming a nonexistent oracle is checkably wrong against the available_oracles input; a classification that contradicts its traced provenance is a finding). The confidence computation with its provenance caps (`PS-1`), the lexicographic ranking, winner and escalation cases (`PS-2`), the routing-table check (`PS-3`) and the convergence gate are D (`lisp_eval`, the forms below). A D tag on a step with no real oracle is checkably wrong; a P tag on a step with a cheap deterministic checker is a precision-improvement candidate.

## Initial and target condition

- **Initial condition:** the statement(s) or computation step, system context and available oracles, plus the direct source material required to classify, trace provenance or resolve the named conflict.
- **Target condition:** every requested analysis carries an honest status; the provenance gate has zero unverifiable gaps and `chain_confidence >= 0.8`; conflicts either produce a strict ranked resolution or an explicit tie/escalation; routing leaves zero unrouted and zero ungated P steps. Verification recommendations permit one provenance re-trace; persistent gaps or skipped analyses are reported rather than promoted to a pass.

## When to Use

- When a statement needs classification on ontological (IS/OUGHT), epistemic (declarative/probabilistic/subjunctive), and domain ontology anchoring axes.
- When routing a computation step to the machine whose conditional entropy matches the task's — deterministic oracle versus learned model — and labelling its D/P regime (P8.4).
- When determining the constraint force (Prohibition, Guardrail, Guideline, Evidence, Hypothesis) and provenance of a statement.
- When tracing the origin and evidentiary chain of a factual claim through hKask's data layers to its authoritative source.
- When identifying gaps in a claim's derivation chain and recommending verification steps.
- When resolving conflicts between statements using 5-tier OT ranking (ontological type, epistemic mode, constraint force, evidence provenance, and ontology anchoring).
- When ranking contradictory statements to determine a winner based on constraint force hierarchy and provenance weighting.

## When NOT to Use

- Feedback-loop and variety analysis — use `pragmatic-cybernetics`.
- Extracting structured data from text — use `structured-extraction`; classification is not extraction.
- Rendering final verdicts on code findings — `code-review` embeds this lens but owns the adjudication.

## Instructions

### semantics-classify-statement

1. Classify the statement's ontological mode as either IS (descriptive) or OUGHT (prescriptive).
2. Determine the epistemic mode as declarative (high certainty), probabilistic (medium certainty), or subjunctive (low certainty).
3. Identify the domain ontology anchoring tier (core, dual_axis, or domain_supplement) and the specific ontology anchor.
4. Identify both the process axis (PKO) and state axis (DC+BIBO) if the statement is dual-axis.
5. Map the statement to its constraint force (Prohibition, Guardrail, Guideline, Evidence, or Hypothesis) based on its ontological and epistemic modes.
6. Classify the provenance of the statement (Specification, Design, Implementation, Runtime, Memory, Inference, External, or Unknown — the trace's data-layer hierarchy; `Runtime` replaces the former `Observation`, same concept).
7. The render emits unadjusted `base_confidence` and `spec_checked`. Resolve the anchor with `onto_anchor`, then compute the final confidence with `lisp_eval` gate `PS-1`, which applies each ontology modifier once and then the provenance caps:
   - form: `(let ((adjusted (min 1.0 (max 0.0 (+ base_confidence (if domain_anchored 0.05 0.0) (if sumo_anchored 0.03 0.0) (if spec_checked 0.05 0.0) (if unanchored -0.10 0.0)))))) (cond ((string= provenance "unknown") (min adjusted 0.3)) ((and (string= provenance "specification") (not spec_checked)) (min adjusted 0.3)) ((and (string= provenance "specification") spec_checked) (max adjusted 0.8)) (t adjusted)))`
   - env: `{ "base_confidence": <unadjusted base confidence from the render>, "provenance": <the render's provenance value>, "domain_anchored": <true|false from onto_anchor — true when the term resolves to the domain-supplement rung>, "sumo_anchored": <true|false from onto_anchor>, "spec_checked": <true only if the named spec was verified current>, "unanchored": <true|false> }`
   - Unknown provenance, or a specification claimed but not checked, is capped at 0.3; a checked specification is held at 0.8 or above. The form applies both; do not adjust the result by hand.

### semantics-provenance-trace

1. Start with the claim as stated and identify its most direct source (specification, design, implementation, runtime, memory, inference, or unknown).
2. Trace the claim back recursively through derivation steps until reaching a primary source or an unverifiable gap.
3. Record the source, location, derivation type, transform, and confidence delta for each step in the provenance chain.
4. Flag any gaps in the chain where sources cannot be verified.
5. Determine `chain_confidence` (0.0–1.0) from the verified chain, and its confidence level (high, medium, low, or unverifiable). List unverified links in `unverifiable_gaps`; never apply an authority floor to a source not actually checked.
6. Detect any conflicting sources or contradictory evidence within the provenance chain.
7. Provide specific, actionable recommendations for verifying and strengthening the provenance chain.

### semantics-conflict-resolve

Runs only when the trace returned `conflicts_detected: true` or the caller supplied several statements. When it does not run, its outcome for the convergence gate is `no_conflict` if `conflicts_detected` was explicitly false, and `skipped` if the field was missing or the step was otherwise not run.

1. Render `pragmatic-semantics/semantics-conflict-resolve`. The model classifies each statement on the five tiers — ontological mode, epistemic mode, constraint force, provenance authority, ontology anchor — and proposes a resolution strategy (override, scope, defer, confirm). It does not rank.
2. Rank with `lisp_eval` gate `PS-2`, passing the returned `statements` unchanged:
   - form: `(begin (define rank-in (lambda (value order n) (if (is_null order) 99 (if (string= (car order) value) n (rank-in value (cdr order) (+ n 1)))))) (define anchor-rank (lambda (anchor) (cond ((starts-with "omc:" anchor) 1) ((starts-with "golem:" anchor) 2) ((starts-with "sepio:" anchor) 3) ((starts-with "pko:" anchor) 4) ((starts-with "dcterms:" anchor) 5) ((starts-with "bibo:" anchor) 5) ((starts-with "mls:" anchor) 6) ((starts-with "sumo:" anchor) 7) ((starts-with "5w1h:" anchor) 8) ((string= anchor "unanchored") 9) (t 0)))) (define ranks (lambda (s) (list (rank-in (assoc! "ontological_mode" s) (list "OUGHT" "IS") 1) (rank-in (assoc! "epistemic_mode" s) (list "declarative" "probabilistic" "subjunctive") 1) (rank-in (assoc! "constraint_force" s) (list "Prohibition" "Guardrail" "Guideline" "Evidence" "Hypothesis") 1) (rank-in (assoc! "provenance" s) (list "specification" "design" "implementation" "runtime" "memory" "external" "inference" "unknown") 1)))) (define cmp (lambda (a b) (cond ((is_null a) "tie") ((< (car a) (car b)) "first") ((> (car a) (car b)) "second") (t (cmp (cdr a) (cdr b)))))) (define cmp5 (lambda (x y) (let ((upper (cmp (ranks x) (ranks y))) (ax (anchor-rank (assoc! "ontology_anchor" x))) (ay (anchor-rank (assoc! "ontology_anchor" y)))) (if (not (string= upper "tie")) upper (if (or (= ax 0) (= ay 0)) "unranked" (cmp (list ax) (list ay))))))) (define invalid (lambda (ss) (if (is_null ss) (list) (if (member 99 (ranks (car ss))) (cons (assoc! "id" (car ss)) (invalid (cdr ss))) (invalid (cdr ss)))))) (define best (lambda (ss lead) (if (is_null ss) lead (best (cdr ss) (if (string= (cmp5 (car ss) lead) "first") (car ss) lead))))) (define rivals (lambda (ss lead kind) (if (is_null ss) (list) (if (and (not (string= (assoc! "id" (car ss)) (assoc! "id" lead))) (string= (cmp5 (car ss) lead) kind)) (cons (assoc! "id" (car ss)) (rivals (cdr ss) lead kind)) (rivals (cdr ss) lead kind))))) (define prohibitions (lambda (ss) (if (is_null ss) 0 (+ (if (string= (assoc! "constraint_force" (car ss)) "Prohibition") 1 0) (prohibitions (cdr ss)))))) (let ((bad (invalid statements))) (if (not (is_null bad)) (list (list "outcome" "invalid_classification") (list "invalid" bad)) (if (< (length statements) 2) (list (list "outcome" "no_conflict") (list "winner" (if (is_null statements) "" (assoc! "id" (car statements))))) (let ((lead (best (cdr statements) (car statements)))) (let ((ties (rivals statements lead "tie")) (unranked (rivals statements lead "unranked"))) (list (list "outcome" (cond ((> (prohibitions statements) 1) "escalate_prohibitions") ((not (is_null ties)) "escalate_tie") ((not (is_null unranked)) "escalate_unranked_anchor") (t "ranked"))) (list "winner" (assoc! "id" lead)) (list "tied" ties) (list "unranked_anchor" unranked) (list "winner_ranks" (append (ranks lead) (list (anchor-rank (assoc! "ontology_anchor" lead))))))))))))`
   - env: `{ "statements": <the render's statements, each {id, ontological_mode, epistemic_mode, constraint_force, provenance, ontology_anchor}> }`
3. Read `outcome`. The tiers are compared in strict order — a lower tier is consulted only when every higher tier is equal (Optimality Theory's strict domination), so OUGHT never loses to IS:
   - `ranked` — `winner` strictly dominates; apply the proposed resolution strategy to the losers.
   - `escalate_prohibitions` — two or more Prohibitions conflict; escalate to human review and state the decision needed. Prohibitions are inviolable, so no ranking settles this.
   - `escalate_tie` — `tied` names statements equal to the winner on all five tiers; escalate rather than choose.
   - `escalate_unranked_anchor` — `unranked_anchor` names statements equal to the winner on tiers 1–4 where one side's anchor is outside the Tier-5 ranking (for example `qb:` or `schema:`); the adoption ranking does not cover it, so escalate rather than choose.
   - `invalid_classification` — `invalid` names statements carrying a value outside the tier 1–4 vocabularies; re-render once, then report the conflict unresolved.
   - `no_conflict` — fewer than two statements; nothing to rank.
   Tier 4 ranks External below every hKask layer and above inference; its domain authority flows through the Tier-5 anchoring (operator ruling 2026-09-30). Tier 5 is adoption-based (OMC, GOLEM, SEPIO, PKO, DC+BIBO, ML-Schema, SUMO, 5W1H, unanchored) and is not the confidence modifier; an anchor from any other published source is consulted only when tiers 1–4 tie, and then escalates.

### semantics-route-step

Standalone analysis: it classifies computation steps (P8.4), not statements, and is not part of the three-analysis convergence gate (one statement × three analyses).

1. Classify the step's regime by the entropy-matching rule:
   - **D** — the answer is pinned and a deterministic checker exists. Name the oracle (`lisp_eval`, `lean_check`, `cargo`, a server-side oracle).
   - **P, propose-verify** — knowledge is exhibited only in data; the model proposes and a D gate collapses. Name the gate.
   - **P, explicit probabilistic compute** — the answer is genuinely a distribution; a D server computes over P inputs. Name the server.
   - **P, calibrated forecast** — no characterizable posterior and no cheap oracle; judgment scored against external ground truth. Name the resolution mechanism (resolved outcomes, Brier).
2. Emit the tag: `{ "step": <step id>, "regime": "D" | "P:propose-verify" | "P:probabilistic" | "P:forecast", "oracle": <the named oracle, gate, server or resolution mechanism>, "critique": <what checks this label>, "mismatch_diseases": <the flagged diseases, each naming its step> }`.
3. Flag both mismatch diseases: a P step that could be D is a precision-improvement candidate; a D step where the answer has genuine multiplicity is false certainty.
4. Label hybrid steps at sub-step granularity (precedent: `falsifiability` splits its step 5 into a P call and a D verdict). `render_template` is D (deterministic render) feeding P (model consumption) — label the render and the consumption separately.
5. (D) Once every step of the procedure has a tag, run `lisp_eval` gate `PS-3` over the collected tags, unchanged:
   - form: `(begin (define regimes (list "D" "P:propose-verify" "P:probabilistic" "P:forecast")) (define has-text (lambda (v) (and (stringp v) (not (string= v ""))))) (define unrouted (lambda (ts) (if (is_null ts) (list) (if (member (assoc! "regime" (car ts)) regimes) (unrouted (cdr ts)) (cons (assoc! "step" (car ts)) (unrouted (cdr ts))))))) (define unnamed (lambda (ts) (if (is_null ts) (list) (if (has-text (assoc! "oracle" (car ts))) (unnamed (cdr ts)) (cons (assoc! "step" (car ts)) (unnamed (cdr ts))))))) (define count-regime (lambda (ts d) (if (is_null ts) 0 (+ (if (eq (string= (assoc! "regime" (car ts)) "D") d) 1 0) (count-regime (cdr ts) d))))) (let ((u (unrouted tags)) (n (unnamed tags))) (list (list "passed" (and (> (length tags) 0) (is_null u) (is_null n))) (list "unrouted" u) (list "unnamed_oracle" n) (list "d_steps" (count-regime tags t)) (list "p_steps" (- (- (length tags) (length u)) (count-regime tags t))))))`
   - env: `{ "tags": <every step's tag, each {step, regime, oracle}> }`
   `unrouted` names steps whose regime is not one of the four; `unnamed_oracle` names steps — D or P — with no named oracle, gate, server or resolution mechanism. Either list non-empty fails the table.

### Convergence

Gate — call `lisp_eval` with:
   - form: `(let ((gaps (length unverifiable_gaps))) (let ((reasons (append (if (string= classification_provenance "unknown") (list "classification_provenance_unknown") (list)) (append (if (> gaps 0) (list "unverifiable_gaps") (list)) (append (if (>= chain_confidence 0.8) (list) (list "chain_confidence_below_0.8")) (if (member conflict_outcome (list "no_conflict" "ranked")) (list) (list (if (string= conflict_outcome "skipped") "conflict_resolution_skipped" "conflict_unresolved")))))))) (list (list "converged" (is_null reasons)) (list "reasons" reasons))))`
   - env: `{ "classification_provenance": <the classify render's provenance>, "unverifiable_gaps": <the trace's unverifiable_gaps list>, "chain_confidence": <the trace's chain_confidence>, "conflict_outcome": <the PS-2 outcome, or "no_conflict" / "skipped" when conflict resolution did not run> }`
   `converged` is true only when all three analyses hold: a known classification provenance, a gap-free trace at `chain_confidence >= 0.8`, and a conflict outcome of `no_conflict` or `ranked`. `reasons` names each failing condition; report them. Bound: one re-trace — apply the verification recommendations
   (semantics-provenance-trace step 7), re-run the trace and this gate; gaps that
   survive the second pass are reported as unverifiable (the honest exit
   semantics-provenance-trace step 5 defines).

## Registry Templates

| Template | Purpose |
|----------|---------|
| `semantics-classify-statement.j2` | Classify a statement on three axes: ontological (IS/OUGHT), epistemic (declarative/probabilistic/subjunctive), and domain ontology anchoring (core/dual_axis/domain_supplement). Determine its constraint force and provenance, and emit the unadjusted base confidence `PS-1` adjusts. |
| `semantics-provenance-trace.j2` | Trace the provenance of a claim through hKask's data layers. Identify evidence sources, unverifiable gaps, chain confidence, and verification recommendations. |
| `semantics-conflict-resolve.j2` | Classify each conflicting statement on the five OT tiers and propose a resolution strategy. `PS-2` ranks them, names the winner and decides escalation. |
| `semantics-route-step.j2` | Classify a computation step into its D/P regime per P8.4 (entropy-matched routing): D (named oracle), P:propose-verify (named gate), P:probabilistic (named server), P:forecast (named resolution mechanism). Emits the step tag and flags both mismatch diseases. |

To render a template, call the `render_template` tool with the template ref (e.g., `pragmatic-semantics/semantics-classify-statement`) and a context object with the required variables.

Copyable `render_template` context blocks (variable names from each template's `[inference]` contract):

`pragmatic-semantics/semantics-classify-statement`:
```json
{ "statement": "<the statement>", "system_context": "<surrounding context, or an empty string>" }
```

`pragmatic-semantics/semantics-provenance-trace`:
```json
{ "classification_result": { "<the whole classify output>": "" }, "statement": "<the statement>", "system_context": "<surrounding context, or an empty string>" }
```

`pragmatic-semantics/semantics-conflict-resolve`:
```json
{ "provenance_result": { "<the whole trace output>": "" }, "classification_result": { "<the whole classify output>": "" }, "statements": null }
```
For the multi-statement case, `statements` is an array of `{ "id": "S1", "statement": "<text>" }` objects instead of `null`.

`pragmatic-semantics/semantics-route-step`:
```json
{ "step_description": "<the step, with its id>", "available_oracles": "<the oracles on hand, or an empty string>" }
```

## Regression case

The four forms are evaluated on passing and failing cases by the
`scientific_gate_ps_` tests (`bash kask/scripts/check-skill-lisp-forms.sh`):
`PS-1` holds unknown provenance and an unchecked specification at 0.3 and a
checked specification at 0.8 or above; `PS-2` never lets IS beat OUGHT, breaks
ties tier by tier down to the anchor, escalates two Prohibitions and a
five-tier tie, and rejects a value outside the tier vocabularies; `PS-3` names
an unrouted step and a D step with no oracle; the convergence gate fails on
unknown classification provenance or a skipped conflict resolution even over
a gap-free trace. `pragmatic_semantics_modifier_prose_matches_the_ps1_gate`
pins the modifier values stated in prose to the `PS-1` form.

## Constraints

- `semantics-classify-statement.j2`: IS-statements are never Prohibitions. Declarative OUGHT-statements map to Prohibition or Guardrail. Unknown provenance → confidence ≤ 0.3. Specification provenance → confidence ≥ 0.8 (verify spec is current). Ontology and spec modifiers are applied once, by gate `PS-1` only (domain_anchored +0.05, SUMO +0.03, spec_checked +0.05, unanchored -0.10), which also applies the provenance caps; this template emits the unadjusted `base_confidence` and never applies them itself.
- `semantics-provenance-trace.j2`: Every step must identify a concrete location. Unknown source → confidence ≤ 0.2. Direct spec quotes → confidence ≥ 0.9. Inference steps reduce confidence by ≥ 0.1.
- `semantics-conflict-resolve.j2`: OUGHT never loses to IS. Two Prohibitions conflicting → escalate. Both are decided by `PS-2`, not by the model. Strategy enum: override, scope, defer, confirm.
- `semantics-route-step.j2`: Every D tag names a real oracle; every P tag names its collapse path. A step tagged D where the answer has genuine multiplicity, or P where a deterministic checker exists, is flagged as a mismatch disease, never silently relabelled.
- Conflict resolution runs only if `conflicts_detected == true` or several statements were supplied. Its outcome for the convergence gate is `no_conflict` when the field was explicitly false, and `skipped` when the field was missing or the step did not run: a missing `conflicts_detected` field is not a no-conflict claim. If conflicts were detected, require a `ranked` outcome or report the unresolved conflict; never default a missing result and claim convergence.
- The convergence gate (D) covers all three analyses: classification provenance, the trace's gaps and chain confidence, and the conflict outcome.
