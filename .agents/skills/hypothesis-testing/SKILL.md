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
| Registry compatibility classification | P + D | P evidence classification; `lisp_eval` HT-R is the only compatibility verdict, and it returns which arm. |
| Registry declaration mapping | P + D | Runs only after HT-R returned a compatible arm; `lisp_eval` HT-RM checks required mapping fields for that arm before any write. |
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

Compute HT-R. The registry carries two seal attestations, so compatibility has two arms:

```lisp
(let ((common (and meaningful_baseline rubric_expressible positive_max_runs
                   partitions_valid design_spec_versioned executor_bound)))
  (cond ((not common) "registry_schema_mismatch")
        ((and binary_headroom genuinely_binary_claim) "compatible_binary")
        (scientific_headroom_criterion "compatible_scientific")
        (t "registry_schema_mismatch")))
```

The semantic booleans come from the adapter's cited evidence; the verdict does not.

The arms are a **preference order, not mutual exclusion**. A design carrying both a meaningful binary pass rate and a design-family headroom criterion is admissible science: it takes the binary arm, because that arm also yields pass-rate eligibility and Brier calibration. The server's `SealAttestationConflict` refuses supplying *both attestations at seal* — it does not refuse a design *capable* of either. `genuinely_binary_claim` gates only the binary arm; a non-binary claim with a scientific criterion routes to `compatible_scientific` rather than to mismatch.

Calibration, single-arm measurement, observational estimation and practical equivalence are **not** incompatible — they are the `compatible_scientific` arm, attested by a tagged scientific headroom criterion (`design_family`, `criterion`, `evidence`). `registry_schema_mismatch` means what it says: a missing baseline arm, inexpressible rubrics, no run ceiling, absent or non-disjoint partitions, an unversioned `DesignSpec`, an unbound executor, or neither headroom attestation available. On mismatch, preserve the complete DesignSpec outside a fake experiment, keep all failed-condition evidence, do not render the declaration mapper, and do not call a registry write tool. **Never fabricate a pass rate to force the binary arm.**

### 4. Declare, baseline and seal (HT-R compatible designs only)

Render `hypothesis-testing/registry-declaration` only after HT-R returned `compatible_binary` or `compatible_scientific`, passing the arm it returned:

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

The mapper uses `layer="scientific_hypothesis"`; passes the versioned `DesignSpec` as the **top-level `design_spec` argument** of `experiment_propose`, never nested inside `eval_set` and never written to both; serializes `{ "schema": "experimental-design-registry-adapter-v1", "hypothesis_spec": ..., "feedback_partition": ..., "selection_partition": ..., "executor_report_schema": ... }` inside `eval_set`; maps uncertainty/indifference regions to `noise_band`; carries positive `budget.max_runs`; and links a kanban goal only for a genuinely binary claim. Rubrics include ≥1 `metric` and ≥1 `implementation` or `grounded_report`, stable snake_case ids, and verification in `contains` / `not_contains` / `regex` / `report_ref`.

`design_spec` must carry a non-empty `spec_version`; the server refuses it otherwise. No freeze coverage is lost by the top-level position — the seal fingerprints `(eval_set, fitness_fn, noise_band, rubrics, design_spec)` together, so post-seal design drift yields a different fingerprint from either position.

Before any write, run HT-RM over the rendered mapping, binding `headroom_arm` to the **verbatim HT-R result token** — `compatible_binary` or `compatible_scientific`, copied unchanged, never abbreviated to `binary` / `scientific` and never re-encoded:

```lisp
(and (string= layer "scientific_hypothesis")
     design_spec_toplevel
     (not design_spec_in_eval_set)
     spec_version_nonempty
     (> max_runs 0)
     (> metric_rubric_count 0)
     (> impl_count 0)
     feedback_partition_present
     selection_partition_present
     (cond ((string= headroom_arm "compatible_binary")
            (and meaningful_pass_rate_definition binary_resolution_present))
           ((string= headroom_arm "compatible_scientific")
            (and design_family_nonempty criterion_nonempty))
           (t (not t))))
```

The arm check is a **closed-world `cond`, not two independent `if`s**. Each arm clause is reached only by its own token, and any unrecognised `headroom_arm` — `registry_schema_mismatch`, an abbreviated `binary` / `scientific`, a stale or misspelled token, an empty string — falls to the `(t (not t))` branch and fails the gate. An independent-`if` form cannot express this: a token matching neither arm skips both tests, each `if` yields its `t` default, and the conjunction authorises a write with **no** arm precondition ever checked. Write no fallback arm and no default-permit branch; an unknown arm is a refusal, never a pass.

A false result returns to mapping once; a second failure stops as `registry_mapping_invalid` and performs no registry write.

Then:

1. Create the linked goal only for a genuinely binary claim; prediction confidence comes from `superforecasting` / `scenario_calibrate` or an externally calibrated method, never unaided self-assessment.
2. `experiment_propose` with the versioned eval-set object, the top-level `design_spec`, rubrics, fitness function, noise band, budget and (binary arm only) goal id.
3. `variant_register` **every arm the DesignSpec predeclared**, before measurement: the control as `role="baseline"` (exactly one), each comparator as `role="challenger"`, and each ablation as `role="ablation"` naming the `parent_variant_id` whose component it ablates — the server refuses a parentless ablation. Registration is not measurement: pre-seal challenger *fitness* is refused by the server (`ExperimentNotSealed`), so registering the full arm set early cannot leak a pre-seal measurement. Freeze the arm-selection rule here, before any feedback result is read.
4. Execute the baseline arm through the DesignSpec-bound executor and `fitness_record(seal_id=null)` its real report references. Pre-seal fitness is baseline-only.
5. `experiment_seal` with **exactly one** attestation, matching the HT-R arm: `baseline_pass_rate` for `compatible_binary` (the scientifically meaningful measured rate), or `scientific_headroom` — `{design_family, criterion, evidence}` with recorded report refs — for `compatible_scientific`. Supplying both, or neither, is a typed refusal. A binary pass rate at 1.0 is a typed no-headroom refusal: close with first reject reason containing literal `no-headroom` (for information inadequacy, `no-headroom: no_information`), redesign and re-declare; never weaken evaluators.

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

The algedonic review chairs disposition. Pass `FindingsSpec.typed_scientific_outcome` **verbatim** into `selection_record`'s `scientific_outcome` field — never renamed, never re-encoded as a binary verdict. The six values are the ED-2 vocabulary exactly: `rejected`, `corroborated`, `evidence_of_practical_equivalence`, `inconclusive`, `measurement_void`, `design_void`.

Supply `scientific_outcome` **alone**. Do not also supply `verdict`: the tool layer synthesizes the stored resolution token from whichever single field is given, which is what satisfies the store's `verdict == outcome` equality check; supplying both is a typed `SelectionResolutionConflict` refusal. A `rejected` outcome still requires non-empty `reject_reasons`.

- `rejected` and `corroborated` enter the Brier calibration record. A linked goal exists only for a genuinely binary claim, so only these two can judge and score one.
- `evidence_of_practical_equivalence`, `inconclusive`, `measurement_void` and `design_void` are excluded from calibration **on the typed field itself** — no literal marker string is needed, and none should be invented. Preserve the FindingsSpec and the review decision; the fossil states honestly that the claim was neither held nor refuted.

Literal `no-headroom` / `measurement-void` markers remain the closure path for **artifact-layer experiments only** (`self-improvement` owns those; they resolve through `verdict`, which has no typed field). A scientific design never needs them.

Refinement requires a new versioned `HypothesisSpec` with lineage. Evidence that suggested it is exploratory and cannot confirm it; use a fresh sealed experiment or disjoint confirmatory set.

### 8. Metacognitive check and bounded improvement

Before execution, record the design’s expected outcome and confidence. After execution, compare expectation with observation. Only `rejected` and `corroborated` outcomes carry a linked goal to score. Periodically inspect calibration across experiments and revise the design method when systematic error appears; never count `design_void`, `measurement_void` or `inconclusive` as refutations.

Apply the conclusions-update gate HT-1 after each completed measurement batch:

```lisp
(cond ((or (string= trigger "contradiction") (string= trigger "new_category") (string= trigger "flip") (string= trigger "refinement")) "update") (t "keep"))
```

Simple reconfirmation keeps the prior conclusion. At most three rejected updates per cycle; then preserve the prior conclusion and disclose the skipped update.

## Registry Templates

| Template | Purpose |
|---|---|
| `registry-adapter.j2` | Classify the evidence-bearing semantic preconditions for mapping a complete `DesignSpec` onto the current experimentation contract, for both the binary and scientific headroom arms; it emits HT-R inputs, never a compatibility verdict or declaration args. |
| `registry-declaration.j2` | After HT-R returned a compatible arm, map the versioned scientific artifacts, partitions, report schema and (binary arm) resolution into proposed registry arguments for HT-RM validation. |
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
- Binary arm: meaningful control + binary rubric rate + genuinely binary claim + expressible reports + positive run budget + real disjoint partitions + versioned DesignSpec + bound executor → HT-R `compatible_binary`; seal attests `baseline_pass_rate`.
- Scientific arm: single-arm calibration, observational estimation or equivalence with a tagged `{design_family, criterion, evidence}` criterion and the common inputs → HT-R `compatible_scientific`; seal attests `scientific_headroom`. These designs are admitted, not refused.
- Both attestations available → `compatible_binary` (preference order, not mutual exclusion); binary headroom with a non-binary claim and no scientific criterion → `registry_schema_mismatch`.
- Genuine mismatch: missing baseline arm, inexpressible rubrics, zero run budget, missing partitions, unversioned DesignSpec, unbound executor, or neither attestation → HT-R `registry_schema_mismatch`; declaration mapper remains uncalled and no declaration args exist.
- Declaration shape: `design_spec` is the top-level `experiment_propose` argument with non-empty `spec_version` and is never duplicated inside `eval_set`; HT-RM refuses a dual write.
- HT-RM arm vocabulary: `headroom_arm` carries the verbatim HT-R token. `compatible_binary` with a meaningful pass-rate definition and present binary resolution → true; `compatible_scientific` with non-empty `design_family` and `criterion` → true. Each arm fails when its own precondition is false. Every other token — `registry_schema_mismatch`, abbreviated `binary` / `scientific`, empty or unrecognised — fails closed-world, with no arm precondition consulted and no registry write.
- Arm registration: every predeclared arm registered before measurement — one baseline, challengers, ablations each naming `parent_variant_id`; a parentless ablation is refused.
- Resolution: the ED-2 outcome passes verbatim into `scientific_outcome`, supplied alone; all six values round-trip unchanged; `rejected` carries non-empty reasons; the four non-calibrating outcomes are excluded from Brier on the typed field, with no literal marker invented.
- Server constraints: pre-seal challenger fitness refused; post-seal missing/wrong seal refused; overlapping final refs refused; both-or-neither seal attestation refused; binary pass rate 1.0 refused; changed DesignSpec produces a different fingerprint/new experiment.
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
