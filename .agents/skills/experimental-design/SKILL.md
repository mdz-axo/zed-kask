---
name: experimental-design
description: "Design discriminating hypothesis tests before scientific spend: preserve provenance, compare candidate designs through evidence and limitations, specify analysis and stopping, bind a real executor, and produce versioned DesignSpec and FindingsSpec artifacts without machine admission."
---

# Experimental Design

Experimental design is the deep module between `falsifiability` and measurement. It determines exactly which observations would distinguish the live hypotheses and how to obtain them validly, efficiently, safely, and reproducibly. It does not frame the research question, invent rivals, execute measurements, operate the experiment registry, or resolve the human review.

## Ownership boundary

- `hypothesis-framer` owns the research question, H1/H0 and domain framing. PICO is an adapter for clinical questions, never a universal vocabulary.
- `falsifiability` owns admissibility, rival hypotheses, counterfactuals, falsifiers, and the test × hypothesis discrimination matrix.
- This skill owns candidate-design synthesis, measurement validity, identification, controls, allocation, information adequacy, preregistered analysis, stopping, executor binding, and the versioned `DesignSpec` / `FindingsSpec` artifacts.
- Domain harnesses, observation tools, or human-operated procedures execute measurements. Physical, biological, and human-subject procedures always stop at a human/operator boundary.
- `hypothesis-testing` owns the one end-to-end lifecycle and experimentation-registry adapter. This skill never calls `experiment_propose`, `variant_register`, `experiment_seal`, `fitness_record`, `final_evaluation_record`, or `selection_record`.
- `lean-prover` is optional and owns only exact finite or deterministic proof obligations; it cannot establish measurement validity or scientific intent.

## Reference models bound to the contract

| Authority | Operational binding |
|---|---|
| Platt, “Strong Inference,” DOI `10.1126/science.146.3642.347` | Rival-prediction traceability and discriminating observations. |
| NIST/SEMATECH Engineering Statistics Handbook, Process Improvement | Design-family selection, randomization/blocking/factorial structure, capable measurement systems, analysis and confirmation. |
| JCGM 100:2008, DOI `10.59161/JCGM100-2008E`, and VIM | Measurement model, calibration, metrological traceability and uncertainty. |
| Shadish–Cook–Campbell, *Experimental and Quasi-Experimental Designs for Generalized Causal Inference* (Houghton Mifflin, 2002; ISBN 0395615569) | Structural organizer for the four validity questions — statistical-conclusion, internal, construct, external/transport — mapped onto `DesignSpec` groups 4–7. **Partially verified** (research run `24d7fc791edc83f2`): only the publisher's bibliographic record and stated four-part structure were obtained; no accessible primary full text. Cite as an organizer, never quote, until the primary text is retrieved. |
| Wasserstein & Lazar, DOI `10.1080/00031305.2016.1154108`, and Greenland et al., DOI `10.1007/s10654-016-0149-3` | Outcome interpretation; a p-value is not effect size, truth probability or practical importance. |
| Lakens, Scheel & Isager, DOI `10.1177/2515245918770963` | Prespecified equivalence bounds and TOST-style bounded claims. |
| Lakens, DOI `10.1525/collabra.33267` | Sample-size justification by the inferential goal, not a universal power ritual. |
| Wald, DOI `10.1214/aoms/1177731118`, plus applicable anytime-valid guidance | Sequential designs require a prespecified stopping/error-control basis; naive repeated peeking is invalid. |
| Nosek et al., DOI `10.1073/pnas.1708274114` | Preregistration freezes confirmatory hypotheses, outcomes, analysis and stopping before confirmatory data inspection. |
| Munafò et al., DOI `10.1038/s41562-016-0021`, and NIH rigor guidance | Blinding, randomization, replication, transparency and reproducibility package. |
| NASA Glenn, “1901 Wind Tunnel Tests,” and Smithsonian Wright history | Measurement validity and transportability can fail while the focal hypothesis remains exposed: the Smeaton coefficient was revised; Lilienthal’s coefficients were valid for his geometry but did not transport to the Wrights’ different aspect ratio. |
| Mayo, *Statistical Inference as Severe Testing* (2018) | Optional severity check: a hypothesis is corroborated only when it passed a test that probably would not have passed so well were it false. Never an automatic verdict. |
| Lakatos, *The Methodology of Scientific Research Programmes* (1978) | The auxiliary-assumption ledger records progressive versus ad-hoc revisions as lineage context. It never protects a focal claim automatically. |
| Capabilities researcher corpus (`corpus:researcher:`) | Retrieve cross-domain reminders with source provenance: Henrich on transport to real life, Lidwell on artificial context and expectation effects, Miller & Page on alternative hypotheses, confounds and reproducibility. These inform design questions, not machine admission. Its same-named former skill is retired; query the corpus rather than activating it. |

Ontology anchors: derived-registry rulings are required for `experimental design`, `hypothesis testing`, `measurement uncertainty`, `randomized experiment`, `equivalence testing`, and `preregistration`. Until the runtime carrying those rulings is rebuilt, treat a coarse `5w1h_core` result as a routing state, never a private definition.

## When to Use

- A real hypothesis has passed framing/admissibility and needs a valid, discriminating study before scientific spend.
- Rival mechanisms predict potentially different observations, but measurement, identification, precision, controls or stopping remain undecided.
- An existing protocol needs review for construct validity, confounding, transportability, information adequacy or executor fit.
- A completed experiment needs a design-bound `FindingsSpec` and non-binary scientific outcome.

## When NOT to Use

- Generating the research question or H1/H0 — use `hypothesis-framer`.
- Generating rivals, counterfactuals or the discrimination matrix — use `falsifiability`.
- Running the complete lifecycle or writing the experiment registry — use `hypothesis-testing`.
- Executing physical, biological or human-subject procedures — bind a qualified human/operator executor.
- Forecasting without an intervention or measurement design — use `superforecasting`.

## Initial and target condition

- **Initial condition:** the outputs owned by `hypothesis-framer` and `falsifiability`, plus source provenance, unresolved assumptions, constraints, and executor inventory. No confirmatory observation may have been inspected for the design being authored.
- **Target condition:** a selected, versioned `DesignSpec` with an evidence-based rationale, strongest rejected alternative and disclosed limitations; or an explanation of what prevents a defensible design and who can supply the missing evidence. After execution, produce a `FindingsSpec` linked to the sealed design and real report references. Model-supplied assessments do not authorize execution or establish scientific validity.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|---|---|---|
| Contract normalization, candidate generation and auxiliary-failure predictions | P | Source record, operator, domain expert, and the inherited discrimination matrix. |
| Load-bearing-field ownership check | D over P-classified missing fields | `lisp_eval` ED-0. |
| Candidate-set cardinality | D over generated drafts | `lisp_eval` ED-C. |
| Design assessment and comparison | P | Cited evidence, measurement/identification arguments, domain expertise and the user's functional constraints; no boolean admission test. |
| Formal finite invariants | D when used | `lean_check` with axiom audit; no `sorry`. |
| Findings consistency and label mapping | D over P-classified inputs | ED-2I checks stated consistency/report presence; ED-2 maps those classifications to a label, not scientific truth or clearance. |
| Grounding and omissions audit | P + D | Decoupled `grounding-verify`, mechanical citation checks, operator review. |

## Canonical contracts

### `HypothesisSpec` — `hypothesis-spec-v1`

A normalized, provenance-carrying object with:

- `spec_version`, `spec_id`, `predecessor_spec_id`, `revision_reason`;
- `research_question`, `domain`;
- `hypotheses`: H1, H0 and relevant rivals with `id`, `statement`, `prediction`, `falsifier` and `provenance`; unresolved threats stay disclosed rather than being labeled tested;
- `unit_population_or_system`, `intervention_or_exposure`, `comparator`;
- `estimand`, `observable_outcomes`, `practical_effect_or_equivalence_margin` when applicable;
- `constraints`: ethical, safety, resource and feasibility;
- `falsifiability_inputs`: admitted target, counterfactuals and discrimination matrix by reference, never regenerated here;
- `source_provenance`, `unresolved_assumptions`.

Missing ownership is strict: question/H1/H0/unit/intervention/comparator/estimand/outcomes/margin → return to `hypothesis-framer`; rivals/predictions/operational falsifiers/admission/counterfactuals/discrimination matrix → return to `falsifiability`; provenance/constraints/assumptions → block for the operator or source retrieval. Never fabricate a load-bearing value.

Preserve the owning `falsifiability` skill's unresolved threats and missing tests. A scoped control does not resolve its broader parent threat.

### `DesignSpec` — `design-spec-v1`

The canonical product contains:

1. identity, version and `hypothesis_spec_id`;
2. traceability from every hypothesis to distinct predicted observations;
3. selected design family and rationale;
4. units, sampling frame, factors, treatments, comparator, assignment, randomization, blocking, matching, blinding and counterbalancing, with justified omissions;
5. measurement model: construct→observable mapping, instruments, calibration, metrological traceability/uncertainty, scope/transportability, positive/negative/orthogonal controls, auxiliary-assumption ledger, and—when validity is uncertain—a bounded calibration sub-experiment with its trigger, acceptance criterion, budget and failure disposition;
6. identification: identifying contrast, causal model where applicable, assumptions and non-identifiable remainder;
7. information/precision justification: power/MDE, confidence-width target, equivalence margin, simulation, sensitivity analysis, or another sourced method appropriate to the design;
8. preregistered analysis: model/statistic, exclusions, missing data, multiplicity, robustness/sensitivity, thresholds, confirmatory/exploratory boundary;
9. stopping/adaptation rule, interim schedule, error-control basis and positive `max_runs`;
10. executor binding: exact harness/tool/human procedure, input/output contracts, report schema/provenance and typed failures (`executor_unavailable`, `protocol_deviation`, `measurement_failed`, `scientific_non_result`);
11. outcome rule, ethics/safety controls, limitations and reproducibility package;
12. candidate ledger including the selected design and strongest rejected alternative with rejection reason.

### `FindingsSpec` — `findings-spec-v1`

Contains `design_spec_id`, seal/fingerprint when registered, real executor report references, observations, effect/uncertainty, deviations, missingness, validity limitations, auxiliary-failure localization, per-hypothesis conclusion, typed scientific outcome, exploratory findings, and reproduction/audit references. Evidence used to invent a refinement is exploratory; a refinement creates a new `HypothesisSpec` with lineage and requires a fresh sealed experiment or disjoint confirmatory set.

## Instructions

### 1. Normalize without stealing ownership

Normalize the actual upstream artifacts using `experimental-design/normalize-hypothesis`. Its seven output fields feed ED-0R and ED-0 directly; missing arrays are errors, not implicit empty lists. Rendering a prompt is not executing normalization.

```json
{
  "framing_artifact": {},
  "falsifiability_artifact": {},
  "source_provenance": [],
  "constraints": {},
  "unresolved_assumptions": []
}
```

Classify missing fields from actual retained text/provenance, not inherited completeness flags. Every owner-return reason cites the artifact/pointer and absent evidence. Keep an explicit unknown unknown; keep disclosed unresolved scientific assumptions without mistaking them for missing disclosure, and do not require an inapplicable margin. Current PI amendments supersede only the constraints they address; authorization is not scientific validity. Preserve scoped rival limitations and prospective matrix semantics, with no invented observations or scientific verdicts.

Before routing, reconcile the unchanged semantic output with ED-0R. Pass the entire seven-key object as the gate environment, without reconstructing partitions or adding status keys. This form checks nullability, unique missing fields, unique owner rows, owner coverage and partition coverage directly over those arrays (the JSON-shape and owner-agreement checks above remain separate):

```lisp
(begin (define unique-strings (lambda (xs seen) (if (= 0 (length xs)) true (if (member (car xs) seen) false (unique-strings (cdr xs) (cons (car xs) seen)))))) (define unique-owner-fields (lambda (rows seen) (if (= 0 (length rows)) true (let ((field (assoc "field" (car rows)))) (if (or (is_null field) (member field seen)) false (unique-owner-fields (cdr rows) (cons field seen))))))) (define fields-covered (lambda (xs fields) (if (= 0 (length xs)) true (and (member (car xs) fields) (fields-covered (cdr xs) fields))))) (define owners-covered (lambda (rows fields) (if (= 0 (length rows)) true (and (member (assoc "field" (car rows)) fields) (owners-covered (cdr rows) fields))))) (let ((partition_fields (append framer_missing (append falsifiability_missing (append provenance_missing constraints_missing))))) (and (unique-strings missing_fields (list)) (unique-strings partition_fields (list)) (unique-owner-fields owner_returns (list)) (fields-covered partition_fields missing_fields) (owners-covered owner_returns missing_fields) (= (length missing_fields) (length owner_returns)) (= (length missing_fields) (length partition_fields)) (if (= (length missing_fields) 0) (not (is_null hypothesis_spec)) (is_null hypothesis_spec)))))
```

A false ED-0R result returns to normalization; no owner or missing field may disappear at the seam. Then run ED-0. The arrays are observations from the normalization result; do not reconstruct field completeness from memory.

```lisp
(cond ((> (length framer_missing) 0) "return_to_framer") ((> (length falsifiability_missing) 0) "return_to_falsifiability") ((> (length provenance_missing) 0) "blocked: provenance") ((> (length constraints_missing) 0) "blocked: constraints") ((is_null hypothesis_spec) "blocked: malformed_normalization") (t "ready"))
```

`ready` denotes normalization completeness, not scientific readiness. Keep bounded pilot/calibration objectives separate from powered confirmation of H1; design, measurement, precision and executor questions still require evidence. A return carries **all** missing field names to their owning skill or operator; the first status identifies the earliest pipeline owner, not permission to drop later owners.

### 2. Synthesize materially different designs

Render `experimental-design/synthesize-designs` to generate candidate drafts only:

```json
{
  "hypothesis_spec": {},
  "discrimination_matrix": {},
  "domain_evidence": [],
  "executor_inventory": [],
  "design_constraints": {}
}
```

Generate 2–5 materially different candidates when possible. If only one credible design exists, return the one candidate draft with a non-empty `only_credible_explanation` naming why the materially different alternatives cannot answer the question. ED-C checks the emitted drafts directly for unique, non-empty IDs and cardinality before any validity judgment:

```lisp
(begin (define unique-candidate-ids (lambda (drafts seen) (if (= 0 (length drafts)) true (let ((id (assoc "candidate_id" (car drafts)))) (if (or (is_null id) (string= "" id) (member id seen)) false (unique-candidate-ids (cdr drafts) (cons id seen))))))) (let ((candidate_count (length candidate_drafts))) (and (unique-candidate-ids candidate_drafts (list)) (cond ((and (>= candidate_count 2) (<= candidate_count 5)) true) ((and (= candidate_count 1) (not (is_null only_credible_explanation)) (not (string= "" only_credible_explanation))) true) (t false)))))
```

A false ED-C result returns to synthesis; it never proceeds by quietly dropping alternatives.

Deliberately vary the source of discrimination where justified: randomized intervention; blocked/factorial or fractional-factorial; matching/counterbalancing; calibration/orthogonal measurement; positive/negative controls; natural experiment/IV/RD/DID with a named causal model; sequential/adaptive with valid error control; measurement-first pilot for uncertain instruments or transportability; computational benchmark.

For every candidate, tabulate the expected observation under each rival **and** each material auxiliary failure: calibration error, leakage, confounding, model misspecification and domain shift. Reject any design whose possible outcomes cannot distinguish rivals or localize major failures. When measurement validity or transportability is uncertain, the draft includes a bounded calibration sub-experiment with its own acceptance criterion and `max_runs`; this localizes the fault without exonerating H1.

### 3. Assess and compare designs

For each candidate, examine the actual evidence: can the proposed observations distinguish rivals or test a prespecified equivalence claim? What identifies the contrast? Are instruments and reference data calibrated for this population or system? What precision is achievable, which biases remain, and can the analysis, stopping rule and procedure be reproduced? Address ethics, safety and feasibility with concrete controls and constraints, not asserted completeness flags.

A suspect instrument, reference table or transport assumption calls for the bounded calibration sub-experiment. Localize instrument error, invalid reference data, analysis error or domain shift; none automatically exonerates H1. Disclose unresolved validity questions rather than converting them to permission through a boolean formula.

Render `experimental-design/select-design` over the retained candidate drafts and source-backed design constraints:

```json
{
  "hypothesis_spec": {},
  "candidate_drafts": [],
  "design_constraints": {}
}
```

Compare discrimination/elimination power, expected information, measurement and identification strength, robustness to auxiliary failure, cost, time, risk and reversibility. Efficiency cannot compensate for a known validity failure. Explain the selected design and strongest rejected alternative using their evidence and limitations. Preserve candidate provenance; a revision must say what changed and why, never silently rewrite the comparison.

If no defensible design is available, report the missing evidence or unavailable executor and the next useful action; do not fabricate a selection. The human retains functional decisions, judgment and veto. This design comparison neither authorizes measurements nor creates a new mandatory approval loop.

### 4. Specify precision, analysis and stopping

Choose an information justification that matches the estimand. Failure to reject without adequate precision is a design void or inconclusive observation, never support. Equivalence/non-inferiority requires a prespecified practically meaningful margin and adequate interval precision.

Freeze primary outcomes, analysis, exclusions, missing-data handling, multiplicity, stopping and adaptation before confirmatory data. Fixed-N designs stop at N. Sequential/adaptive designs cite the applicable theorem/procedure and state alpha/error spending or anytime-valid evidence. Naive optional stopping is rejected.

Use `lean-prover` only when an exact finite proposition is useful (for example outcome-enum disjointness or a deterministic stopping-state invariant). Run `lean_check`, audit axioms and reject `sorry`. General optional-stopping validity stays with published statistical theory plus analytic checks or deterministic simulation unless its probability model/library is actually formalized.

### 5. Bind the executor and hand off

Name the exact executable seam and its typed report. Software/agent designs may bind test harnesses; economic designs may bind FRED/DBnomics/World Bank/market observations; physical, biological and human-subject designs bind a human-operated protocol and stop. An unavailable executor is not a negative scientific result.

Return the complete `DesignSpec` to `hypothesis-testing`. Do not call the registry here.

### 6. Interpret executed evidence

After execution, render `experimental-design/interpret-findings` with the sealed `DesignSpec`, real reports, observations, deviations and audit sources:

```json
{
  "design_spec": {},
  "registry_identity": null,
  "run_reports": [],
  "observations": [],
  "deviations": [],
  "audit_sources": []
}
```

The template proposes evidence classifications with cited bases; it does not establish their truth. ED-2I checks internal consistency and report presence only:

```lisp
(and (not (and prediction_contradicted equivalence_met)) (if equivalence_met equivalence_prespecified true) (if (= (length run_reports) 0) (not measurement_valid) true))
```

A false result returns to findings reconciliation. `run_reports` is the exact array supplied to the template, so report presence is computed rather than model-typed. A missing report must set `measurement_valid=false`; an invalidating protocol breach or failed measurement does the same. A valid observation with no decisive result is `scientific_non_result`, not a measurement failure.

Then call ED-2:

```lisp
(cond ((not design_valid) "design_void") ((not measurement_valid) "measurement_void") ((and prediction_contradicted precision_adequate) "rejected") ((and equivalence_prespecified equivalence_met precision_adequate) "evidence_of_practical_equivalence") ((and severe precision_adequate) "corroborated") (t "inconclusive"))
```

Attach the exact ED-2 result as `FindingsSpec.typed_scientific_outcome`, alongside the evidence bases and uncertainty of its input classifications. This deterministic mapping is not independent scientific adjudication, execution clearance or human acceptance; a label cannot strengthen its evidence.

- `rejected`: an adequate discriminating design contradicted the prediction.
- `corroborated`: the hypothesis survived a realistic opportunity for rejection; never proved.
- `evidence_of_practical_equivalence`: prespecified bound met with adequate precision.
- `inconclusive`: inadequate precision, materially broad interval, or neutral observation.
- `measurement_void`: invalid instrument, missing/untrustworthy measurement or protocol breach.
- `design_void`: non-identifiable contrast, no information headroom or unusable design.

`design_void`, `measurement_void` and `inconclusive` are never scored as refuted scientific claims.

### 7. Check and improve (bounded PDCA)

Run `bash kask/scripts/check-skill-lisp-forms.sh`. After corpus parsing, the same gate runs the `scientific_gate_` behavioral regressions in the Lisp checker against the **actual authored forms**, using the production evaluator; zero matching tests is a failure.

The suite exercises the authored ED-0R/ED-0 ownership reconciliation and routing, ED-C cardinality/unique ids, ED-2I report-presence/consistency checks, and all six ED-2 label mappings. Negative cases include imprecise contradiction, equivalence and contradictory findings. These checks do not establish whether the supplied scientific classifications are correct.

These tests check decision logic over classified inputs, not scientific validity or study execution. JSON null and empty objects/arrays are null-like in Lisp; ED-0R tests use a nonempty versioned spec for the nonnull case.

Revise a design when new evidence or an identified repair warrants it, preserving the reason and lineage. If no useful next step is available, report the limitation rather than looping or weakening the scientific question.

## Registry Templates

| Template | Purpose |
|---|---|
| `normalize-hypothesis.j2` | Normalize upstream framing and falsifiability artifacts into `hypothesis-spec-v1`, preserving provenance and returning missing load-bearing fields to their owner rather than inventing them. |
| `synthesize-designs.j2` | Generate materially different candidate drafts with rival-and-auxiliary predictions, evidence bases and limitations. |
| `select-design.j2` | Compare retained drafts and produce the selected `design-spec-v1`, strongest rejected alternative and evidence-based rationale. |
| `interpret-findings.j2` | Reconcile real executor reports against the sealed `DesignSpec` and emit `findings-spec-v1` with observations, uncertainty, deviations, limitations and ED-2 inputs; never turn imprecision or invalid measurement into support. |

Copy these complete contexts at the render call sites; do not reconstruct them from prose.

`experimental-design/normalize-hypothesis`:

```json
{"framing_artifact": {}, "falsifiability_artifact": {}, "source_provenance": [], "constraints": {}, "unresolved_assumptions": []}
```

`experimental-design/synthesize-designs`:

```json
{"hypothesis_spec": {}, "discrimination_matrix": {}, "domain_evidence": [], "executor_inventory": [], "design_constraints": {}}
```

`experimental-design/select-design`:

```json
{"hypothesis_spec": {}, "candidate_drafts": [], "design_constraints": {}}
```

`experimental-design/interpret-findings`:

```json
{"design_spec": {}, "registry_identity": null, "run_reports": [], "observations": [], "deviations": [], "audit_sources": []}
```

## Constraints

- No confirmatory data inspection before the design is complete and handed to `hypothesis-testing` for declaration. `experiment_propose` is preregistration; `experiment_seal` is the later fingerprint/headroom attestation.
- Never “blame the instrument first.” Localize faults among hypothesis, instrument, reference data, analysis and domain shift using calibration, controls, orthogonal measurements and rival predictions.
- Observational causal claims require a causal model and explicit residual-confounding limitations.
- Cost and speed do not compensate for known validity, ethics or feasibility problems; state the evidence and unresolved constraints.
- Recorded runs name real executor reports. Synthetic cases are smoke tests, not capability validation.
- Proposed checks and executed checks are reported separately.
- Semantic templates propose designs and evidence classifications. Structural checks establish only the invariants they compute; ED-2 maps supplied classifications to labels. None certifies scientific validity or grants execution authority.
- A findings audit uses `grounding-verify` and is never labeled simply “verified.”
