---
name: hypothesis-testing
description: "Orchestrate one human-gated hypothesis-testing lifecycle: framing, falsifiability, experimental design, preregistration, executor-backed measurement, sealed findings, independent grounding audit, and human review."
---

# Hypothesis Testing

Hypothesis testing is the one end-to-end scientific inquiry lifecycle in zed-kask. It composes the skills and instruments that own each phase; it never reimplements their methods. The operator remains principal investigator: holding the question, judgment and veto. The experimentation MCP server is the sealed registry and lab notebook, never the executor.

## Pipeline and ownership

`hypothesis-framer` → `falsifiability` → `experimental-design` → domain executor → experimentation registry → `grounding-verify` → algedonic review.

- `hypothesis-framer`: question, H1/H0 and framing.
- `falsifiability`: admissibility, rivals, counterfactuals, falsifiers and discrimination matrix.
- `experimental-design`: `HypothesisSpec`, candidate designs, validity/identification/information gates, selected `DesignSpec`, executor binding and `FindingsSpec` interpretation.
- Domain executor: measurements and report references. Physical, biological and human-subject work remains human-operated.
- Experimentation server: eight-tool registry (`experiment_propose`, `variant_register`, `experiment_seal`, `fitness_record`, `final_evaluation_record`, `selection_record`, `lineage_read`, `population_query`).
- `grounding-verify`: claims/source audit and material omissions.
- Algedonic review: human-chaired disposition and adoption.

## When to Use

- A real scientific hypothesis should move through framing, discriminating design, preregistration, measurement, findings audit and human review.
- A compatible scientific experiment needs the eight-tool experimentation registry used without losing scientific meaning.
- An inquiry must coordinate a domain executor and preserve real report references, design lineage and deviations.

## When NOT to Use

- Experimental design alone — invoke `experimental-design` directly.
- Question/hypothesis formulation alone — use `hypothesis-framer`.
- Rival generation or hard falsification alone — use `falsifiability`.
- Forecasting without measurement — use `superforecasting`.
- Artifact self-improvement experiments — use `self-improvement`; it shares the registry but owns that lifecycle.

## Initial and target condition

- **Initial condition:** a real research question and operator authority to begin. No confirmatory measurement for the proposed design has been inspected.
- **Target condition:** either a typed stop before scientific spend, or a resolved inquiry carrying a versioned preregistered `DesignSpec`, real executor reports, a `FindingsSpec`, independent findings audit, lineage, and human-review record. A genuinely binary resolved claim may also close its linked goal for Brier calibration.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|---|---|---|
| Framing, falsifiability and design | P + D gates | Owning skills and operator. |
| Registry compatibility classification | P + D | P evidence classification; `lisp_eval` HT-R is the only compatibility verdict. |
| Registry declaration mapping | P + D | Runs only after HT-R=`compatible`; `lisp_eval` HT-RM checks required mapping fields before any write. |
| Budget, seal citation, run-ref disjointness and fingerprint | D | Experimentation server typed refusals. |
| Execution | D/P by domain | Named harness/data source/human report. |
| Scientific interpretation | D over classified evidence | `experimental-design` ED-2. |
| Report audit | P + D | Decoupled `grounding-verify`. |
| Review and calibration | P + D | Human review; goal score only for genuinely binary resolved claims. |

## Instructions

### 1. Frame and admit

Run `hypothesis-framer`, then the full relevant `falsifiability` path: admission, rival generation, counterfactuals and discrimination matrix. A framing that already carries adequate rivals may reuse them; never collapse back to H1/H0 only when material rivals remain live.

Open a research run before literature-dependent framing or design. Retrieve concrete sources and annotate their verification state. Query the capabilities researcher corpus (`corpus:researcher:`) when cross-domain systems principles are relevant; its same-named former skill is retired, so use corpus retrieval rather than skill activation.

### 2. Design before declaration

Call `skill(name="experimental-design")` and follow it through a complete `DesignSpec` or typed stop. The design is complete **before** `experiment_propose`; proposal is the preregistration point. A failed design gate stops before registry declaration or execution.

Do not author an eval set or choose a harness directly from framing. The retired direct framing-to-harness path did this and is deleted.

### 3. Classify registry compatibility honestly

Render `hypothesis-testing/registry-adapter` to classify the current registry's semantic preconditions. It emits evidence-bearing `compatibility_inputs`, not a verdict or declaration arguments:

```json
{
  "hypothesis_spec": {},
  "design_spec": {},
  "feedback_partition": null,
  "selection_partition": null,
  "executor_report_schema": {},
  "binary_resolution_mapping": null
}
```

Compute HT-R:

```lisp
(if (and meaningful_baseline meaningful_binary_headroom genuinely_binary_claim rubric_expressible positive_max_runs partitions_valid) "compatible" "registry_schema_mismatch")
```

The semantic booleans come from the adapter's cited evidence; the verdict does not. For calibration, single-arm measurement, observational estimation, non-binary evidence, practical equivalence without a genuinely binary preregistered resolution, or any design lacking a meaningful pass rate/baseline, HT-R must return `registry_schema_mismatch`. **Do not fabricate one.** Preserve the complete DesignSpec outside a fake experiment, keep all failed-condition evidence, and issue schema-evolution evidence for first-class `design_spec`, scientific headroom and typed scientific outcomes. Do not render the declaration mapper or call a registry write tool.

### 4. Declare, baseline and seal (HT-R compatible designs only)

Render `hypothesis-testing/registry-declaration` only after HT-R returned `compatible`:

```json
{
  "hypothesis_spec": {},
  "design_spec": {},
  "feedback_partition": {},
  "selection_partition": {},
  "executor_report_schema": {},
  "binary_resolution_mapping": {},
  "compatibility_evidence": {}
}
```

The mapper uses `layer="scientific_hypothesis"`; serializes `{ "schema": "experimental-design-registry-adapter-v1", "hypothesis_spec": ..., "design_spec": ..., "feedback_partition": ..., "selection_partition": ..., "executor_report_schema": ... }` inside `eval_set`; maps uncertainty/indifference regions to `noise_band`; carries positive `budget.max_runs`; and links the preregistered binary prediction to a kanban goal. Rubrics include ≥1 `metric` and ≥1 `implementation` or `grounded_report`, stable snake_case ids, and verification in `contains` / `not_contains` / `regex` / `report_ref`.

Before any write, run HT-RM over the rendered mapping:

```lisp
(and (string= layer "scientific_hypothesis") design_spec_present (> max_runs 0) (> metric_rubric_count 0) (> implementation_or_report_rubric_count 0) meaningful_pass_rate_definition binary_resolution_present feedback_partition_present selection_partition_present)
```

A false result returns to mapping once; a second failure stops as `registry_mapping_invalid` and performs no registry write.

Then:

1. Create the linked goal only for a genuinely binary claim; prediction confidence comes from `superforecasting` / `scenario_calibrate` or an externally calibrated method, never unaided self-assessment.
2. `experiment_propose` with the versioned eval-set object, rubrics, fitness function, noise band, budget and goal id.
3. `variant_register(role="baseline")` for the predeclared control/baseline arm.
4. Execute that arm through the DesignSpec-bound executor and `fitness_record(seal_id=null)` its real report references. Pre-seal fitness is baseline-only.
5. `experiment_seal` with the scientifically meaningful baseline pass rate. At 1.0, close with first reject reason containing literal `no-headroom`; for information inadequacy use `no-headroom: no_information`. Redesign and re-declare; never weaken evaluators.

Treat proposal as immutable preregistration. Seal fingerprints the already-preregistered eval contract; it is not permission to design after seeing baseline results.

### 5. Execute exactly the bound protocol

Execute only through the `DesignSpec.executor_binding`. Every post-seal `fitness_record` cites the active `seal_id`; the budget ceiling is terminal. Record typed operational failures separately from scientific outcomes:

- `executor_unavailable` — no measurement occurred;
- `protocol_deviation` — measurement does not match the sealed procedure;
- `measurement_failed` — instrument/report unusable;
- `scientific_non_result` — valid observation with no decisive conclusion.

A missing report or failed executor becomes a visible measurement void, never a refuted hypothesis. Physical, biological and human-subject procedures are handed to the operator; never report them as performed by the agent.

Use `final_evaluation_record` only with run references disjoint from all prior records; the server enforces this mechanically. Do not reimplement the split.

### 6. Interpret and audit

Return the sealed design and real reports to `experimental-design` Step 6 for `FindingsSpec` + ED-2 outcome. Preserve its vocabulary: `rejected`, `corroborated`, `evidence_of_practical_equivalence`, `inconclusive`, `measurement_void`, `design_void`.

Render `hypothesis-testing/report` with the design, findings, registry lineage and proposed-vs-executed check ledger:

```json
{
  "hypothesis_spec": {},
  "design_spec": {},
  "findings_spec": {},
  "registry_lineage": null,
  "audit_findings": {},
  "proposed_checks": [],
  "executed_checks": []
}
```

Run `grounding-verify` as a decoupled audit over the exact report and retained source outputs. Findings are a working-paper record; never label the whole report verified. An unavailable audit is a visible blocker, not an empty audit result.

### 7. Resolve through human review

The algedonic review chairs disposition. Today’s registry can honestly call `selection_record(selected|rejected)` only for a genuinely binary preregistered claim. Never encode `corroborated`, `inconclusive` or `evidence_of_practical_equivalence` merely by renaming them selected/rejected.

- Binary claim held/refuted: record selected/rejected honestly; linked goal may be judged and operator-scored.
- `measurement_void`, `design_void`, `inconclusive`, corroboration without a binary decision, or practical-equivalence result: preserve the FindingsSpec and review decision; if the registry cannot type it, close only with an applicable literal void marker or stop at schema mismatch. Never pollute the Brier record.

Refinement requires a new versioned `HypothesisSpec` with lineage. Evidence that suggested it is exploratory and cannot confirm it; use a fresh sealed experiment or disjoint confirmatory set.

### 8. Metacognitive check and bounded improvement

Before execution, record the design’s expected outcome and confidence. After execution, compare expectation with observation. Score only genuinely binary resolved claims. Periodically inspect calibration across experiments and revise the design method when systematic error appears; never count `design_void`, `measurement_void` or `inconclusive` as refutations.

Apply the conclusions-update gate HT-1 after each completed measurement batch:

```lisp
(cond ((or (string= trigger "contradiction") (string= trigger "new_category") (string= trigger "flip") (string= trigger "refinement")) "update") (t "keep"))
```

Simple reconfirmation keeps the prior conclusion. At most three rejected updates per cycle; then preserve the prior conclusion and disclose the skipped update.

## Registry Templates

| Template | Purpose |
|---|---|
| `registry-adapter.j2` | Classify the evidence-bearing semantic preconditions for mapping a complete `DesignSpec` onto the current artifact-oriented experimentation contract; it emits HT-R inputs and schema-evolution evidence, never a compatibility verdict or declaration args. |
| `registry-declaration.j2` | After HT-R=`compatible`, map the versioned scientific artifacts, partitions, report schema and binary resolution into proposed registry arguments for HT-RM validation. |
| `report.j2` | Produce the hypothesis-testing study report from sealed design, real run reports, FindingsSpec, registry lineage and audit results, with proposed checks separated from executed checks. |
| `conclusions.j2` | Apply HT-1 to update the distilled conclusion only on contradiction, new category, recommendation flip or meaningful refinement; reconfirmation keeps the prior text. |

Copy these complete contexts at render call sites; do not reconstruct them from prose.

`hypothesis-testing/registry-adapter`:

```json
{"hypothesis_spec": {}, "design_spec": {}, "feedback_partition": null, "selection_partition": null, "executor_report_schema": {}, "binary_resolution_mapping": null}
```

`hypothesis-testing/registry-declaration`:

```json
{"hypothesis_spec": {}, "design_spec": {}, "feedback_partition": {}, "selection_partition": {}, "executor_report_schema": {}, "binary_resolution_mapping": {}, "compatibility_evidence": {}}
```

`hypothesis-testing/report`:

```json
{"hypothesis_spec": {}, "design_spec": {}, "findings_spec": {}, "registry_lineage": null, "audit_findings": {}, "proposed_checks": [], "executed_checks": []}
```

`hypothesis-testing/conclusions`:

```json
{"cycle_events": [], "prior_conclusion": null}
```

## Regression case

- HT-1: contradiction/new_category/flip/refinement → `update`; reconfirmation → `keep`.
- Compatible registry path: meaningful control + binary rubric rate + binary claim + expressible reports + positive run budget + real disjoint partitions → HT-R `compatible`; registry declaration renders only then, and HT-RM must pass before `experiment_propose`.
- Incompatible paths: single-arm calibration, observational estimation without binary pass rate, non-binary corroboration, or missing partitions → HT-R `registry_schema_mismatch`; declaration mapper remains uncalled and no declaration args exist.
- Server constraints: pre-seal challenger fitness refused; post-seal missing/wrong seal refused; overlapping final refs refused; baseline 1.0 refused; changed DesignSpec produces a different fingerprint/new experiment.
- End-to-end functional trial: a real operator-supplied hypothesis runs design → honest executor boundary → FindingsSpec → grounding audit → human review. A synthetic case does not satisfy this trial.

## Constraints

- One lifecycle only. `experimental-design` returns artifacts and vetoes; this skill orchestrates.
- No design after `experiment_propose`; no confirmatory-data peeking before preregistration.
- Recorded runs name real reports. Never simulate fitness.
- Nothing auto-adopts; human review owns disposition.
- The study report separates proposed checks from checks actually executed.
- Curator memory stays human-gated; reports propose memory candidates rather than writing them automatically.
- Semantic templates classify evidence; only `lisp_eval` HT-R/HT-RM authorize compatibility and mapping. Never carry a model-typed compatibility verdict across the registry seam.
- `BudgetExhausted` ends the declaration. A changed mechanism or design is a fresh declaration with lineage.
