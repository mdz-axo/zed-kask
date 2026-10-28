---
name: superforecasting
core: true
description: "Superforecasting pipeline following Tetlock's Good Judgment Project methodology. Eight-stage process: question triage, Fermi decomposition, outside/inside views, Bayesian updating, dragonfly-eye synthesis, calibration, and forecast recording."
---

# Superforecasting

Superforecasting pipeline following Tetlock's Good Judgment Project methodology. Eight-stage process from question triage through Fermi decomposition, outside/inside views, Bayesian evidence updating, dragonfly-eye synthesis, probability calibration, and forecast recording.

## Goal responsibilities

Follow [GOAL_INTERFACE.md](../../../kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md).
Keep event-forecast declaration/resolution separate from human acceptance of the work. Ordinary goals need no probability; optional goal forecasts use only HumanAcceptance.

## Forecast interpretation and evidence limits

Binary Brier loss is mean((p−y)^2), range [0,1]; the original categorical
sum-over-outcomes convention has range [0,2] (two-category vector loss is
2×binary loss). State the convention. Constant .5 has binary loss .25,
not a universal climatology or quality threshold. Missing probability stays
missing; never synthesize a scoring prior or historical performance.
Murphy (1973), *A New Vector Partition of the Probability Score*, DOI
`10.1175/1520-0450(1973)012<0595:ANVPOT>2.0.CO;2`, separates reliability,
resolution and uncertainty: raw loss alone is not calibration. Compare a
declared reference on the same cases, weights and lead times; skill is
undefined when reference loss is zero. Report eligible/resolved/unresolved
counts and event dependence; versions are not independent outcomes. No
universal score bands, score labels, inverse-loss reliability weights or
automatic historical-bias correction. Ontology lookup names the concept; if
its gloss calls raw loss calibration, use the cited primary source for
interpretation rather than repeating it.

## Reference model

Tetlock & Gardner, *Superforecasting: The Art and Science of Prediction* (2015) — `onto_anchor` → derived `superforecasting`, `fermi_decomposition`, and `reference_class_forecasting` (operator ruling 2026-09-25). Brier (1950) → derived `brier_score`. Each stage below is labelled **D** (deterministic, oracle named) or **P** (probabilistic, calibration or critique named).

## D/P labelling

Every stage heading below carries its label. **D**: the nine `lisp_eval` forms
(the stage-0 triage gate `SF-1`, the stage-2 rate check and anchor check, the stage-4 posterior, the stage-5
weighted average, the stage-5/stage-6 compensation-masking and defensible-range gate `SF-2`, the
quality-gate weighted totals and pass rule `SF-3`, the market-prior gate); the
`scenario_*` server oracles (triage classification, quantify rejection,
calibration); `forecast_persist`; Brier at resolution; and the authored jq record-binding/assembly and raw-quality-shape filters below. **P**: triage,
Fermi decomposition, outside-view comparability, and dragonfly-eye
synthesis — each names its critique (`scenario_triage` cross-check,
falsifiability delegation, the gate's steelman step, the operator). The forecast probability itself is P — calibrated
judgment, scored only by Brier over resolved forecasts, never falsified by
one outcome.

## Initial and target condition

- **Initial condition (T1):** the admitted question, its resolution criteria and deadline, any sourced historical observations, `market_context` / `expert_prior`, the optional EQM-derived `overconfidence_bias`, and — when resolved forecasts exist — the `scenario_calibration` curve for the bucket. A historical base rate is not presumed to exist.
- **Target condition (T2):** either a sourced starting anchor followed by a probability with a defensible range and a record preserving the stage-0 resolution criteria/deadline, or an explicit evidence-gap exit with no numeric forecast. For a produced forecast, `SF-3`'s `gate_pass = true` (over `forecast-quality-gate`'s raw criterion judgments) is the local target; accuracy is judged only by Brier over resolved forecasts, never by one outcome.

## When to Use

- When you need to forecast the likelihood of a future event using a rigorous, structured methodology based on Tetlock's Good Judgment Project.
- When a forecasting question falls in the "Goldilocks zone" (not too easy, not too unpredictable) and warrants full pipeline investment.
- When you need to decompose a complex prediction into tractable sub-questions and establish base rates using the outside view.
- When you need to update a prior probability with new evidence using Bayesian methods and likelihood ratios.
- When you need to synthesize multiple causal models and dissenting views into a single calibrated probability.
- When you need to record a forecast with resolution criteria for later tracking, Brier scoring, and post-mortem analysis.
- When evaluating generated forecasts through an independent quality gate to assess calibration realism, confidence justification, evidence trail, and record completeness without self-assessment bias.
- When LEAP expert-judgment forecasts are available for the forecasting question or a close reference class, and you want to anchor the outside view on FRI's longitudinal expert panel rather than (or in addition to) market-implied probabilities.
- When the prediction-market calibration store needs a health check (scan cadence, per-bucket raw loss and observation coverage) or a resolution needs recording, or before relying on a market reliability tier (the Market-prior calibration check).

## When NOT to Use

- Questions outside the Goldilocks zone — already-resolved facts and pure-chance events get no value from the pipeline; answer directly.
- Reading a market's current probability — the market price IS the aggregate estimate; use the prediction-markets tools, don't re-derive it.
- Scoring a forecast with no recorded prior — post-hoc Brier scoring requires a persisted forecast (forecast_record); reconstructing one after the fact is fabrication.
- Quick one-off estimates where the question is trivially decomposable — the eight-stage pipeline costs more than the answer is worth.

## Instructions

### stage_0_triage (P — critique: `scenario_triage` classification as cross-check; operator sharpens missing criteria)

1. Evaluate whether a forecasting question is worth investing significant effort in.
2. Classify the question into "clocklike" (easy), "goldilocks" (just right), or "cloudlike" (unpredictable) — Tetlock's regularity axis, this skill's own judgment. Cross-check with `scenario_triage`'s specification-quality classification (`well_specified` / `goldilocks` / `needs_refinement`): the tool scores whether the question is specified enough to score, this step whether it is regular enough to forecast; proceed only when both land in the goldilocks zone.
3. Assess if there is sufficient publicly available information, if the outcome is determined by analyzable factors, if research would improve accuracy, and if the time horizon is appropriate.
4. Before proceeding, require a specified observable outcome, resolution criteria, and a deadline; if any are missing, ask for a sharper question instead of inventing them. Verify Stage 0 triage alignment with `scenario_triage` and non-empty `resolution_criteria` / `resolution_deadline` deterministically via `lisp_eval` gate **`SF-1`**:
   - form: `(and (string= triage_zone "goldilocks") (or (string= scenario_triage_class "well_specified") (string= scenario_triage_class "goldilocks")) (not (string= resolution_criteria "")) (not (string= resolution_deadline "")))`
   - env: `{ "triage_zone": <stage_0_triage zone>, "scenario_triage_class": <scenario_triage classification>, "resolution_criteria": <string>, "resolution_deadline": <string> }`
   If `SF-1` is false or no supported `scenario_type` or bounded `time_horizon` fits, report null and do not proceed into the scenario tree. A forecast probability is evaluated by calibration across resolved cases, not falsified by one outcome. Recommend proceeding only when `SF-1` passes; carry the criteria and deadline unchanged into stage 7's record.

### stage_1_fermi_decompose (P — critique: stage 3's falsifiability delegation; structural limits checked by `scenario_quantify` rejection)

1. Decompose the forecasting question into tractable sub-questions.
2. Unpack the question by asking what it would take for the answer to be yes or no.
3. Separate knowable from unknowable factors and expose assumptions.
4. Generate 3-7 sub-questions that are specific and answerable. Emit both a flat list (`sub_questions`, consumed by the Fermi calibration step) and a conditional probability tree (`sub_question_tree`) encoding how they combine into the outcome.
5. Structure the tree so correlated sub-questions share a common-cause parent (never co-parents of the same child — `marginalize` assumes parent independence). Keep fan-in ≤ 3 and depth ≤ 3.
6. List all assumptions, noting whether they are reasonable and what happens if they are false.
7. Identify established facts (knowns) and uncertain factors requiring estimation (unknowns).

### stage_2_outside_view (historical rates P identification / D arithmetic; market and expert priors P comparability)

1. Identify candidate reference classes for the main question and sub-questions. Record historical frequencies in `base_rates` only when comparable observed cases, outcome counts, sample size and a checkable source are available; otherwise leave that array empty. Market prices and expert medians belong in the separate `candidate_priors` array, never in historical `base_rates`.
2. For each reported historical rate, call `lisp_eval` with `(if (and (numberp outcomes) (numberp sample_size) (> sample_size 0) (<= 0 outcomes) (<= outcomes sample_size)) (/ outcomes sample_size) (quote invalid))`, binding `outcomes` to `outcome_count`. Reject `invalid`, rates that disagree with the computed value, or rates without a source. This deterministic check verifies arithmetic, not the comparability or truth of the source.
3. When a measured rate or a reliably matched market/expert prior fits the outcome, horizon, and conditioning, choose one source for `starting_probability` and record `anchor_source.kind`, source and rationale; do not average unlike sources. If none qualifies, require `starting_probability: null` and `anchor_source: null`. Stop before stage 3 and report the evidence gap; do not create a 0.5 or other synthetic prior.
4. Before invoking stage 3, check that `anchor_source` is present and its `kind` is `historical`, `market`, or `expert`, and call `lisp_eval` with `(and (numberp p) (<= 0 p) (<= p 1))` on the selected `starting_probability`. If false, stop; stage 3 requires a number and must never render `null * 100`.

> **MCP tool step (call `market_match` directly — no template):** fetch candidates for the question. A returned market price is an observed market prior, not a historical rate; inspect match confidence, reliability, spread and deadline before using it. An empty result means no matched market, not a 0.5 estimate. Before selecting a market prior as the anchor, run the "Market-prior calibration check" below on its bucket: a stale bucket means the reliability tier carries no calibration evidence — say so in `anchor_source.rationale`.

### stage_3_probability_estimate (delegated split — node estimates P, combination D via `scenario_quantify`)

The former single inside-view step is split into three steps. Generation and counterfactual analysis are delegated to the `falsifiability` skill; probability estimation stays in superforecasting.

1. **Generate causal hypotheses (delegate to falsifiability).** Invoke `falsifiability/falsifiability-hypothesize` with `admitted_target` = the resolvable event question admitted at stage 0, `domain` = "forecasting", `context` = the sub-questions, outside-view output, resolution criteria, and deadline. Apply falsifying observations to specific causal hypotheses; do not demand that one outcome falsify a probabilistic forecast or add a second Popper gate. Produces 3–7 ranked candidate causal pathways with the delegate's forced diversity (≥1 unlikely, ≥1 challenging the obvious explanation, ≥1 embarrassing-if-true — which for forecasting questions typically surface as alternative mechanisms, contamination/false-positive paths, and opposing-outcome pathways), each with a Platt-form prediction and a falsifier; discards vibes at generation.
2. **Construct counterfactuals / necessary conditions (delegate to falsifiability).** Invoke `falsifiability/falsifiability-counterfactual` with the generated `hypotheses`, `admitted_target`, `domain`. For each hypothesis construct the minimal do(not X) counterfactual, hold confounders fixed, and derive the testable consequence that distinguishes the counterfactual world from the factual one. Flag irreducible causes.
3. **Estimate probabilities and emit the tree (superforecasting).** Invoke `superforecasting/stage_3_probability_estimate` with the `hypotheses`, `counterfactuals`, `starting_probability` (the verified, source-labeled anchor), `outside_view_output`, and the stage-1 tree inputs (`sub_question_tree`, `topological_order`, `outcome_node_id`) — the template's `tree_nodes` output is what `scenario_quantify` consumes; without the tree inputs there is no `tree_combined_probability` to feed stage 4. For each hypothesis weigh evidence pro/con against its counterfactual's testable consequence, assign an individual probability, and enforce internal consistency. For each tree node estimate a marginal (roots) or a conditional table (dependents) — the combinator (AND/OR/mixture) is encoded structurally in the conditional values, not as a separate field. The invoking agent then calls `scenario_quantify` (hkask-mcp-scenarios) with the tree's nodes as ScenarioEvent objects — it topologically sorts the dependency graph and marginalizes via the shared `hkask_forecast::marginalize`, returning each node's `marginal_probability` plus the `joint_probability`. The outcome node's `marginal_probability` is `tree_combined_probability` — the exact inside-view posterior fed to stage 4 as the prior. The LLM no longer estimates `combined_probability`; the Rust tool owns that.

> **MCP tool step (after stage 3, call `scenario_quantify` directly — no template):** map the `sub_question_tree` nodes into ScenarioEvent objects (id, name, question, deadline, time_horizon, scenario_type, subject, probability, depends_on with parent_event_ids + conditionals, sub_questions, update_count) and call `scenario_quantify`. The outcome node's `marginal_probability` is `tree_combined_probability`, stage 4's prior.

### stage_4_evidence_update (likelihood ratios P; posterior D via `lisp_eval`)

1. Incorporate new evidence and update probabilities using likelihood ratios and Bayesian reasoning.
2. Assess the strength (weak/moderate/strong) and direction (supports/contradicts/neutral) of each piece of evidence.
3. Calculate or estimate the likelihood ratio (P(E|H) / P(E|~H)) for each evidence item.
4. Compute each update via `lisp_eval` — the posterior of one update is the prior of the next; the `lisp_eval` posterior is the authoritative D value (the template's `updated_probability` is a cross-check):
   - form: "(/ (* prior likelihood_ratio) (+ (* prior likelihood_ratio) (- 1 prior)))"
   - env: `{ "prior": <current prior>, "likelihood_ratio": <P(E|H) / P(E|~H)> }`

   > **MCP tool step (when the forecast is tracked in the scenarios server's tree — call `scenario_update` directly):** pass the prior, the template's `evidence_likelihood` (P(E|H)) and `evidence_base_rate` (P(E)), and an evidence description — this returns a calculation only, not a forecast-store write. Retain its evidence and calculation as working artifacts. A changed admitted probability needs a new declaration identity. The two Bayesian parameterizations agree only for coherent evidence probabilities.
5. Make many small updates most of the time, and occasional large updates when evidence is very strong.
6. Update the prior probability to the posterior probability based on the accumulated evidence.

### stage_5_synthesis (model probabilities and criterion scores P; weighted average D via `lisp_eval`)

1. Integrate multiple causal models and perspectives into a "dragonfly eye" view.
2. Identify clashing causal forces pushing toward YES vs. NO.
3. Steelman the strongest opposing arguments, making them as persuasive as possible — the book's Commandment 9 perspective-taking discipline ("understanding the arguments of the other side so well that you can reproduce them to the other's satisfaction"). The team-management halves of Commandment 9 (precision questioning, constructive confrontation) are DEFERRED, not dismissed: the book's finding is that teams were 23% more accurate than individuals, a superforecaster on a superteam became 50% more accurate, and superteams beat prediction markets by 15-30% — a single-agent pipeline cannot capture the interpersonal half; the platform's swarm machinery is the future path.
4. Generate 3-5 distinct causal models, each with an implied probability.
5. Apply MCDA-style weighted aggregation: score each model against evidence alignment, reference class stability, causal mechanism clarity, and model confidence criteria. Keep only the four raw criterion scores and `implied_probability` in each causal-model row; `compensation_warning` remains the model's semantic warning.
6. Synthesize an integrated probability with `lisp_eval` directly over the raw `causal_models` array. The form validates 3–5 models, implied probabilities in [0,1], and every criterion score in [1,10]; it derives each composite, the normalized model weights, and the weighted probability. No caller- or model-typed `c_i`/weight is accepted:
   - form: `(begin (define score-ok (lambda (x) (and (numberp x) (>= x 1) (<= x 10)))) (define composite (lambda (m) (let ((s (assoc "mcda_criteria_scores" m))) (+ (assoc "evidence_alignment" s) (assoc "reference_class_stability" s) (assoc "causal_mechanism_clarity" s) (assoc "model_confidence" s))))) (define valid (lambda (m) (let ((s (assoc "mcda_criteria_scores" m)) (p (assoc "implied_probability" m))) (and (listp m) (listp s) (stringp (assoc "model_name" m)) (> (length (assoc "model_name" m)) 0) (numberp p) (>= p 0) (<= p 1) (score-ok (assoc "evidence_alignment" s)) (score-ok (assoc "reference_class_stability" s)) (score-ok (assoc "causal_mechanism_clarity" s)) (score-ok (assoc "model_confidence" s)))))) (define all-valid (lambda (ms) (if (is_null ms) true (and (valid (car ms)) (all-valid (cdr ms)))))) (define weighted (lambda (ms) (if (is_null ms) 0 (+ (* (assoc "implied_probability" (car ms)) (composite (car ms))) (weighted (cdr ms)))))) (define total (lambda (ms) (if (is_null ms) 0 (+ (composite (car ms)) (total (cdr ms)))))) (define collect (lambda (ms den) (if (is_null ms) (list) (cons (list (assoc "model_name" (car ms)) (composite (car ms)) (/ (composite (car ms)) den) (assoc "compensation_warning" (car ms))) (collect (cdr ms) den))))) (if (and (>= (length causal_models) 3) (<= (length causal_models) 5) (all-valid causal_models)) (let ((den (total causal_models))) (list (/ (weighted causal_models) den) (collect causal_models den))) (list nil "invalid_models")))`
   - env: `{ "causal_models": <stage_5_synthesis.causal_models verbatim> }`
   - returns `[synthesized_probability, [[model_name, composite, normalized_weight, compensation_warning], ...]]`; `[null, "invalid_models"]` blocks stage 6 and names a stage-5 repair.
   The template's `synthesized_probability_judgment` is a non-authoritative holistic cross-check only; compare it with the computed value and report the disagreement, never average them.
7. Aggregate the judgments of different models, noting where they agree and diverge.

### stage_6_calibration (P — critiqued by source evidence and later matched-cohort outcomes; `scenario_calibrate` performs no automatic historical correction)

1. Assign an evidence-justified probability using the full 0–100% scale. Calibration is an empirical cohort property, not a label this generation can certify.
2. Anchor the assignment on `scenario_calibrate` (hkask-mcp-scenarios): call it with the question and per-sub-question estimates (constructed from the stage-3 tree nodes' marginals paired with the stage-1 sub-questions, each with an estimate + confidence) — the server computes the calibrated probability via Tetlock's methodology. Pass its output as `server_calibrated_probability` to the stage-6 render and anchor `final_probability` on it; depart only with explicit justification.
3. Avoid hedge words and use specific percentages matched to evidence quality.
4. Assess confidence level (low, medium, high) based on evidence quality, model agreement, and reference class stability.
5. Justify the specific probability and precision against the pipeline's evidence trail.
6. Define a defensible range of probabilities (`[range_low, range_high]`) that would also be reasonable, and verify Stage 5 compensation-masking detection plus Stage 6 defensible-range containment deterministically via `lisp_eval` gate **`SF-2`**:
   - form: `(list (or (< min_criterion_score 4) (> (- max_criterion_score min_criterion_score) 5)) (and (<= range_low final_probability) (<= final_probability range_high) (< range_low range_high)))`
   - env: `{ "min_criterion_score": <lowest criterion score 1..10 on a model>, "max_criterion_score": <highest criterion score 1..10 on that model>, "range_low": <defensible range lower bound>, "final_probability": <stage 6 final_probability>, "range_high": <defensible range upper bound> }`
   The first element flags compensation masking on the evaluated model; the second must be `true` (`final_probability` within the inclusive bounds of a non-degenerate `[range_low, range_high]`) before proceeding to stage 7.

### stage_7_record (P narrative; caller-owned record and admission)

1. Render stage 7 for only `reasoning_summary` and `key_assumptions`, passing the actual stage 0–6 outputs/evidence in `pipeline_summary`, not counts. The caller owns the admitted question, criteria/deadline, final probability/confidence and new `forecast_id`; never regenerate them through a model. `scenario_update` does not persist them.
2. Bind the following handoff directly from held originals and tool call/results; `stage0` is the caller's admitted frame, not a rewritten question. Run each headed jq filter below with `jq -ce '<filter>'` on this object. A nonzero exit blocks the handoff; never manufacture missing data. `quantify.response` and `receipt` are unwrapped tool content, not model output.
   ```json
   {"stage0":{"forecasting_question":"<original question>","resolution_criteria":"<stage-0 criteria>","resolution_deadline":"<stage-0 ISO date>"},
    "stage6":{"final_probability":0.42,"confidence_level":"medium"},
    "narrative":{"reasoning_summary":"<actual stage-7 summary>","key_assumptions":[]},
    "outcome_node_id":"<selected outcome ID>",
    "quantify":{"arguments":{"events":[]},"response":{"nodes":[]}},
    "declaration":{"forecast_id":"<caller ID>","events":[],"resolution_at":{},"resolver":"<named source>"},"receipt":null}
   ```
   The numeric example and empty collections show types, not defaults: replace them from the actual stage/tool objects before execution. Before declaration, `Record assembly` must bind the exact final events. `scenario_quantify` persists a tree: do not invoke it as a read-only audit probe. A changed final estimate requires a coherent revised tree or an explicit binding-gap exit, never silent replacement of p or its conditional table.
3. Run `Record assembly` with `receipt:null` for a working record, then the quality gate. A passing quality gate is not admission. Only then call `scenario_declare` with the assembly filter's exact `declaration` output; replace null only with its actual successful receipt and rerun assembly. Missing discovery or failed declaration stays explicitly unadmitted; never substitute `scenario_score`. Retain exact successful arguments/receipt for later scoring. Gate-driven revisions need a new identity.

### Record assembly

```jq
def text: type == "string" and length > 0;
. as $input | .stage0 as $s | .stage6 as $c | .narrative as $n | .declaration as $d | .receipt as $r |
[.quantify.response.nodes[] | select(.id == $input.outcome_node_id)] as $nodes |
[$d.events[] | select(.id == $input.outcome_node_id)] as $events |
if (.quantify.arguments.events == $d.events) and ($nodes | length) == 1 and ($events | length) == 1 and
   $nodes[0].marginal_probability == $c.final_probability and
   $nodes[0].question == $s.forecasting_question and $events[0].question == $s.forecasting_question and
   $nodes[0].deadline == $s.resolution_deadline and $events[0].deadline == $s.resolution_deadline and
   has("receipt") and ($s.forecasting_question | text) and ($s.resolution_criteria | text) and
   ($s.resolution_deadline | text) and ($d.forecast_id | text) and
   ($c.final_probability | type == "number" and . >= 0 and . <= 1) and
   ($c.confidence_level | IN("low","medium","high")) and
   ($n | type == "object" and keys == ["key_assumptions","reasoning_summary"]) and
   ($n.reasoning_summary | text) and ($n.key_assumptions | type == "array" and all(.[]; text)) and
   ($r == null or ($r.status == "declared" and $r.forecast_id == $d.forecast_id and
     $r.resolver == $d.resolver and $r.resolution_at == $d.resolution_at and
     $r.event_count == ($d.events | length)))
then {declaration:$d, record_result:{forecast_record:{question:$s.forecasting_question, probability:$c.final_probability,
      confidence:$c.confidence_level, resolution_criteria:$s.resolution_criteria,
      expiration_date:$s.resolution_deadline, reasoning_summary:$n.reasoning_summary,
      key_assumptions:$n.key_assumptions}, tracking_id:$d.forecast_id, admission_receipt:$r}}
else error("forecast/tree mismatch, invalid narrative or declaration receipt") end
```

Receipt matching verifies supplied fields, not authenticity: only the caller's real tool result may supply it. Admission time belongs to the store; no model timestamp is recorded.

> **Declare before resolution.** A template record is not durable admission. Call `scenario_declare` with a new `forecast_id`, complete `events`, exact event-ID → UTC timestamp `resolution_at` map and named `resolver` before outcome visibility; it freezes tree marginals, propositions and deadlines. At resolution `scenario_score` requires the same ID, exact frozen events and resolver plus observed outcomes. A revised forecast needs a new identity. For equity questions, `forecast_persist` freezes the return-accuracy target; missing `forecast_probability` remains unscored, never a synthetic prior.

Scenario admission must precede each exact `resolution_at` cutoff; map keys must
match event IDs exactly, and the UTC cutoff date must match descriptive deadline.
Score only at/after cutoff. The whole manifest is one synced journal event;
identical same-ID retries, including after cutoff, preserve original intake.
Changed declarations conflict; old date-only records are unscored, not retrofitted.

### forecast-quality-gate (P — decoupled critic: a separate render that evaluates, never generates; pass rule D)

1. Evaluate the forecast across four independent dimensions: calibration realism, confidence justification, evidence trail, and record completeness.
2. Emit only raw criterion scores (0–1), in one `crit` object with four named arrays of four scores in table order, plus specific evidence-backed findings. No aggregate dimension score is an independent judgment.
3. Compute the per-criterion weighted totals and the pass rule deterministically via `lisp_eval` gate **`SF-3`** — the template emits only raw criterion scores and findings; it never emits `gate_pass`:
   - form: `(begin (define valid-scores (lambda (xs) (if (is_null xs) true (and (numberp (car xs)) (<= 0 (car xs)) (<= (car xs) 1) (valid-scores (cdr xs)))))) (define valid-dimension (lambda (xs) (and (listp xs) (= (length xs) 4) (valid-scores xs)))) (define wsum (lambda (s w) (/ (+ (* (nth 0 s) (nth 0 w)) (* (nth 1 s) (nth 1 w)) (* (nth 2 s) (nth 2 w)) (* (nth 3 s) (nth 3 w))) 100))) (if (and (listp crit) (= (length crit) 4) (valid-dimension (assoc "calibration_realism" crit)) (valid-dimension (assoc "confidence_justification" crit)) (valid-dimension (assoc "evidence_trail" crit)) (valid-dimension (assoc "record_completeness" crit))) (let ((cr (wsum (assoc "calibration_realism" crit) (list 30 25 20 25))) (cj (wsum (assoc "confidence_justification" crit) (list 35 30 20 15))) (et (wsum (assoc "evidence_trail" crit) (list 35 30 20 15))) (rc (wsum (assoc "record_completeness" crit) (list 35 25 20 20)))) (list cr cj et rc (and (>= cr 0.6) (>= cj 0.6) (>= et 0.6) (>= rc 0.6)))) (list nil "invalid_criteria")))`
   - env: `{ "crit": {"calibration_realism": [1.0, 1.0, 1.0, 1.0], "confidence_justification": [1.0, 1.0, 1.0, 1.0], "evidence_trail": [1.0, 1.0, 1.0, 1.0], "record_completeness": [1.0, 1.0, 1.0, 1.0]} }` — each list holds the four criterion judgments (0–1) for that dimension, in the table order above. Returns `[cr, cj, et, rc, gate_pass]` on valid input; `[null, "invalid_criteria"]` blocks delivery on malformed scores; `gate_pass` is the conjunction of all four weighted totals ≥ 0.60.
4. First run `Quality input shape` below on the raw critic object; feed only its emitted `crit` to SF-3 (no retyped scores). Percentage-integer weights preserve the same table weights without an epsilon at the inclusive .60 boundary. Missing/invalid input blocks delivery, never defaults to a passing score. JSON parsing is lossy for repeated object keys; do not claim this shape gate detects duplicate raw JSON keys after parsing.
5. Map computed failing dimensions to the critic's specific fix notes, never its aggregate verdict. Evaluation remains separate from forecast generation.

### Quality input shape

```jq
if type == "object" and (.crit | type == "object" and
   keys == ["calibration_realism","confidence_justification","evidence_trail","record_completeness"]) and
   all(.crit[]; type == "array" and length == 4 and all(.[]; type == "number" and . >= 0 and . <= 1)) and
   (.gate_findings | type == "array")
then .crit else error("quality critic must emit four named arrays of four scores and findings") end
```

### Loop (PDCA over the gate)

- **Plan:** the gate's fix notes predict which stage's output, if revised, lifts the failing dimension to ≥ 0.60.
- **Do:** re-run only the stage(s) the fix notes name, then every downstream stage.
- **Check (D):** re-render the gate and re-run `SF-3`; pass iff `gate_pass` (the conjunction of all four weighted totals ≥ 0.60) is `true`.
- **Act:** stop on pass, or after 2 gate cycles. On a second failure, deliver the forecast with the failing dimensions and their scores recorded as the remaining gap.

### Market-prior calibration check (formerly calibration-stewardship)

Market reliability tiers describe liquidity/spread, not empirical forecast accuracy. Read snapshots and raw loss/counts to inspect observation coverage; no raw-Brier demotion is supported. Tool arithmetic is D, interpretation and scan-cadence recommendations are P. A named source rule is provenance, not authenticated ground truth.

Why it is two-phase: `market_check_resolutions` (1) snapshots every OPEN market's current price as the pre-resolution probability-at-observation (the earliest snapshot per market is kept) and (2) scores newly resolved markets against their snapshot. A market that resolves before its first scan is counted in `resolved_without_snapshot` and skipped — its terminal price is the outcome declaration, and scoring it would give Brier ≈ 0 by construction. Scans must therefore run often enough that open markets are snapshotted before they resolve; a high too-late rate is the primary signal this check manages.

1. **Scan.** Call `market_check_resolutions` (default limit 100; raise toward 500 for a catch-up after a gap; `series` scopes the Kalshi phases only). Read `snapshotted`, `recorded`, `resolved_without_snapshot`, `skipped_ambiguous` (50-50 resolutions, never fabricated), `already_known`, `zero_scan_reason` and `warnings`. The scan is idempotent.
2. **Read the signal.** Call `market_calibration` for the market prior's bucket (a domain such as "politics" or a series ticker) and any other bucket of interest. `stale: true` means no resolved data — report it as unknown, never as well calibrated. For a non-stale bucket report raw binary loss, sample size, observation horizon and missing-resolution limitations. No universal quality band or calibration certification follows.
3. **Record resolutions honestly.** A `market_subscribe_resolutions` notification carries NO pre-resolution probability. To record one, take the probability-at-observation from the scan's snapshot or the operator's own recorded pre-resolution observation, then call `market_record_resolution` (bucket, probability, outcome). Never pass the terminal price as the probability.
4. **Separate liquidity from accuracy.** Inspect volume/spread as market access signals only. Preserve raw forecast history for a matched-cohort analysis; do not demote or certify reliability from a raw score.

5. **Gate.** Call `lisp_eval` with form `(and (= stale_buckets 0) (< without_snapshot_rate 0.2))`, env `{ "stale_buckets": <buckets of interest reporting stale>, "without_snapshot_rate": <resolved_without_snapshot / (recorded + resolved_without_snapshot)> }`. True: this scan-coverage heuristic passes, not a calibration certificate. An empty denominator is unknown, not a zero too-late rate. False: recommend a more frequent scan cadence to the operator and re-run step 1 once; if the rate is still ≥ 0.2, report the cadence problem instead of re-running. When a bucket stays stale, a market prior from it may still anchor stage 2, but its tier is volume/spread-only and the rationale must say so.

Never record a post-resolution price as the probability-at-observation, and never retry an ambiguous (50-50) resolution hoping for a different result. This check scores market buckets; scoring your own forecasts is the Brier loop below.

Across questions, inspect `scenario_calibration` as descriptive eligible-history bins with counts and uncertainty, not a certificate or an automatic correction. `scenario_calibrate` does not apply learned historical bias. Review both successful and failed forecasts; lower loss alone cannot demonstrate learning across a changed task mix.

## LEAP Integration

The superforecasting process accepts an optional `expert_prior` array input
alongside `market_context`. Before invoking the process, the invoking agent
should:

1. Call `rss_list_subscriptions` to find the actual LEAP feed stream IDs. Use
   `rss_get_entries` with each matching `stream_id`, then check entry titles
   and content against the forecasting question. `rss_search` is global and
   cannot search within named streams; do not infer a feed from a global match.
2. If source-identified entries match, parse the LEAP forecast distribution
   from those entries into the `expert_prior` array. Each element carries:
   `question`, `median_probability`, `superforecaster_median` (if available),
   `public_median` (if available), `interquartile_range` (if available),
   `time_horizon`, `scenario` ('rapid AI progress' / 'slow AI progress' /
   'unconditional'), `rationale_excerpt` (optional), `source_url`, `wave`.
3. Pass the `expert_prior` array alongside `market_context` when invoking the
   superforecasting skill.

The process does not fetch LEAP itself — the invoking agent does, because the
process is a single skill invocation and should not reach out to MCP servers
mid-process (per the existing `market_context` pattern). If no matching
subscription or source-identified entry exists, `expert_prior` is empty and
the process runs without it. Do not fabricate LEAP data.

## EQM Feedback Integration

The superforecasting process accepts an optional `overconfidence_bias` number
input. **Not yet wired:** `scenario_calibrate` derives its bias only from its own resolved-forecast store (`hkask_mcp_scenarios.rs` calibration block) and has no caller input for an EQM-derived bias. Until it does, apply the EQM signal as an explicit, justified departure at stage 6 step 2 and record it; do not claim the server applied it. Before re-invoking superforecasting on a prior
iteration's forecast, the invoking agent should:

1. Score the prior iteration's rationale (stage_7_record output) against the
   Explanation Quality Markers (Karvetski et al. 2026): check for red flags
   (`extreme_confidence`, `forecast_rationale_misalign`) and grounding failures
   (excessive `speculative_terms` without evidence).
2. Derive the `overconfidence_bias` value from that scoring. The signal is
   signed: positive = overconfident (red flags dominate), negative =
   underconfident (speculation without grounding).
3. Pass the `overconfidence_bias` value when re-invoking superforecasting.

The process does not score the rationale itself — the invoking agent does,
because the process is a single skill invocation and should not reach out to
other skills mid-process (per the existing `market_context` and
`expert_prior` pattern). If the scoring is not performed,
`overconfidence_bias` defaults to 0.0 (no adjustment). Do not fabricate the
bias value.

For rationale-level improvement (not just calibration feedback), have the
invoking agent run the `eqm` skill's Improve loop on the prior rationale —
`market_score_rationale` (the single source of truth for EQM scoring, never
re-implemented by the agent) identifies which red flags dominate, and the
bounded rewrite raises the EQM passage rate while preserving the forecast
probability (alignment invariant) — before re-invoking superforecasting.

## Regression case

Render `superforecasting/stage_0_triage` twice — once with a representative
goldilocks-zone question carrying resolution criteria and a deadline (pass),
once with the deadline omitted (the triage must refuse to proceed rather
than invent one). Run the nine pinned `lisp_eval` forms with representative
envs: `SF-1` passes only when `triage_zone` is `"goldilocks"`, `scenario_triage_class` is `"well_specified"` or `"goldilocks"`, and `resolution_criteria`/`resolution_deadline` are non-empty; the stage-2 rate check returns `invalid` on a zero sample and the
computed rate otherwise; the anchor check bounds `p` to [0, 1]; the stage-4
posterior form updates a prior; the stage-5 raw-model form derives composites,
normalized weights and synthesized probability for valid 3-, 4- and 5-model
arrays, and returns `[null, "invalid_models"]` for an out-of-range score,
probability, blank model name or wrong model count; `SF-2` flags compensation masking and verifies `range_low <= final_probability <= range_high` with `range_low < range_high`; `SF-3` computes the four weighted dimension totals from the criterion judgments and passes only
when all four are >= 0.60 (a below-threshold dimension returns `gate_pass` false); the market-prior gate requires
`stale_buckets = 0` and `without_snapshot_rate < 0.2`. SF-3 also refuses malformed/out-of-range scores and preserves the exact .60 boundary. Run both authored jq filters on positive and negative handoffs: source-bound dependent marginal, stale events/changed final estimate, metadata injection, null versus mismatched receipts, and wrong raw critic shapes. Native renderer and authored-rule tests pin these boundaries; they do not certify semantic forecasting quality.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `stage_0_triage.j2` | Triage a forecasting question to determine difficulty level and whether it falls in the Goldilocks zone warranting full pipeline investment. |
| `stage_1_fermi_decompose.j2` | Fermi-decompose the forecasting question into independent, tractable sub-questions. Separate knowns from unknowns and document assumptions. |
| `stage_2_outside_view.j2` | Separate sourced historical rates from market/expert priors; return a labeled starting probability or null and stop before numeric estimation. |
| `stage_3_probability_estimate.j2` | Inside-view probability estimation. Takes pre-generated hypotheses (from falsifiability/falsifiability-hypothesize) and their counterfactual necessary-conditions (from falsifiability/falsifiability-counterfactual), weighs evidence pro/con against each counterfactual's testable consequence, assigns individual probabilities, enforces internal consistency, and combines to adjust from the outside-view anchor. Replaces the probability half of the former stage_3_inside_view step. |
| `stage_4_evidence_update.j2` | Incorporate new evidence via Bayesian updating with likelihood ratios. Revise the prior probability based on evidence strength. |
| `stage_5_synthesis.j2` | Synthesize a dragonfly-eye view by integrating multiple causal models and perspectives. Steel-man dissenting views and produce a synthesized probability. |
| `stage_6_calibration.j2` | Calibrate the final probability using the full 0-100% scale. Justify precision against known calibration principles and the pipeline's evidence trail. |
| `stage_7_record.j2` | Synthesize reasoning and assumptions; the caller assembles the original record fields and actual declaration evidence deterministically. |
| `forecast-quality-gate.j2` | Independent quality gate that evaluates forecast calibration realism, confidence justification, evidence trail completeness, and record quality without self-assessment bias. Receives calibration and record outputs and emits only raw criterion scores plus findings; `SF-3` (`lisp_eval`) computes the weighted totals and the pass rule, never the model. |

To render a template, call the `render_template` tool with the template ref (e.g., `superforecasting/stage_0_triage`) and a context object with the required variables.

Copyable `render_template` context blocks (variable names from each template's `[inference] contract`):

`superforecasting/stage_0_triage`:
```json
{
  "forecasting_question": "<the question being forecast>",
  "domain": "<the domain>",
  "time_horizon": "<tactical|strategic|long_term>"
}
```

`superforecasting/stage_1_fermi_decompose`:
```json
{
  "forecasting_question": "<the question being forecast>",
  "triage_output": { "difficulty_level": "goldilocks", "rationale": "<stage 0 triage rationale>" }
}
```

`superforecasting/stage_2_outside_view`:
```json
{
  "forecasting_question": "<the question being forecast>",
  "sub_questions": ["<tractable sub-question>"],
  "knowns": ["<established fact>"],
  "market_context": [{ "market": { "source": "<venue>", "question": "<market question>", "probability": 0.4, "reliability_tier": "<high|low>", "spread": 0.02, "deadline": "<date>" }, "match_confidence": "<high|medium|low>" }],
  "expert_prior": [{ "wave": "<wave>", "question": "<question>", "median_probability": 0.4, "interquartile_range": "<IQR>", "time_horizon": "<horizon>", "scenario": "<conditioning>" }]
}
```

`superforecasting/stage_3_probability_estimate`:
```json
{
  "forecasting_question": "<the question being forecast>",
  "hypotheses": [{ "<hypothesis field>": "<value>" }],
  "counterfactuals": [{ "<counterfactual field>": "<value>" }],
  "starting_probability": 0.35,
  "outside_view_output": { "reference_classes": [{ "class_name": "<class>", "description": "<description>" }], "anchor_source": { "kind": "<historical|market|expert>", "source": "<source>", "rationale": "<comparability rationale>" } },
  "sub_question_tree": [{ "id": "sq1", "question": "<sub-question>", "parent_id": null, "role": "<necessary|sufficient|contributing>" }],
  "topological_order": ["sq1"],
  "outcome_node_id": "sq1"
}
```

`superforecasting/stage_4_evidence_update`:
```json
{
  "forecasting_question": "<the question being forecast>",
  "prior_probability": 0.35,
  "hypothesis_analysis": { "primary_hypothesis": "<highest-probability hypothesis>", "primary_probability": 0.4 },
  "new_evidence": ["<genuinely new evidence item>"],
  "market_context": [{ "market": { "source": "<venue>", "question": "<market question>", "probability": 0.4, "reliability_tier": "<high|low>", "spread": 0.02, "volume": 1000, "deadline": "<date>" }, "match_confidence": "<high|medium|low>" }],
  "expert_prior": [{ "wave": "<wave>", "question": "<question>", "scenario": "<conditioning>", "time_horizon": "<horizon>", "rationale_excerpt": "<expert rationale>" }]
}
```

`superforecasting/stage_5_synthesis`:
```json
{
  "forecasting_question": "<the question being forecast>",
  "updated_probability": 0.4,
  "hypothesis_analysis": { "primary_hypothesis": "<highest-probability hypothesis>" }
}
```

`superforecasting/stage_6_calibration`:
```json
{
  "forecasting_question": "<the question being forecast>",
  "synthesized_probability": 0.42,
  "synthesis_output": { "causal_models": [{ "model_name": "<model>" }], "dissenting_views": [{ "view": "<view>" }] },
  "server_calibrated_probability": 0.42
}
```

`superforecasting/stage_7_record`:
```json
{
  "forecasting_question": "<the question being forecast>",
  "final_probability": 0.42,
  "confidence_level": "<low|medium|high>",
  "pipeline_summary": { "stage0": {}, "stage1": {}, "stage2": {}, "stage3": {}, "stage4": {}, "stage5": {}, "stage6": {} },
  "resolution_criteria": "<observable rule for yes/no resolution>",
  "expiration_date": "<ISO date the forecast should be evaluated>"
}
```

`superforecasting/forecast-quality-gate`:
```json
{
  "forecasting_question": "<the question being forecast>",
  "calibration_result": { "final_probability": 0.42, "confidence_level": "<low|medium|high>", "precision_justification": "<justification>", "defensible_range": { "lower": 0.35, "upper": 0.5 } },
  "record_result": { "forecast_record": { "question": "<original question>", "probability": 0.42, "resolution_criteria": "<stage-0 criteria>", "expiration_date": "<stage-0 date>", "reasoning_summary": "<stage-7 narrative>", "key_assumptions": [] }, "tracking_id": "<caller ID>", "admission_receipt": null },
  "synthesis_output": { "causal_models": [{ "model_name": "<model>" }], "dissenting_views": [{ "view": "<view>" }] }
}
```


## Constraints

- `stage_4_evidence_update.j2`: (Only genuinely new evidence after the selected stage-2 anchor warrants an update; current quotes and repeated expert medians do not.)
- If a prediction-markets tool call fails during the Market-prior calibration check, call `curator_report_skill_use_issue` with skill_name "superforecasting", the tool name and the error, and continue with the best available information.
