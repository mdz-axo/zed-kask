---
name: hypothesis-testing
description: "Orchestrate one human-gated hypothesis-testing lifecycle: framing, falsifiability, experimental design, preregistration, executor-backed measurement, sealed findings, independent grounding audit, and human review."
audience: user
---

# Hypothesis Testing

Hypothesis testing is the one end-to-end scientific inquiry lifecycle in zed-kask. It composes the skills and instruments that own each phase; it never reimplements their methods. The operator remains principal investigator: holding the question, judgment and veto. The experimentation MCP server is the sealed registry and lab notebook, never the executor.

## Goal responsibilities

Follow [GOAL_INTERFACE.md](../../../kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md).
Keep scientific findings and the human judgment of the research goal separate. Neither a corroborated hypothesis nor a successful executor run supplies acceptance.

## Pipeline and ownership

`hypothesis-framer` → `falsifiability` → `experimental-design` → domain executor → experimentation registry → `grounding-verify` → algedonic review.

- `hypothesis-framer`: question, H1/H0 and framing.
- `falsifiability`: admissibility, rivals, counterfactuals, falsifiers and discrimination matrix.
- `experimental-design`: `HypothesisSpec`, materially different candidate designs, assessment against design constraints, the selected versioned `DesignSpec` with precision, analysis and stopping, executor binding and `FindingsSpec` interpretation.
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
- **Target condition:** either a typed stop before scientific spend, or a resolved inquiry carrying a versioned preregistered `DesignSpec`, real executor reports, a `FindingsSpec`, independent findings audit, lineage, and human-review record. Linked goals track verification regardless of scientific outcome; human closure is separate. An eligible forecast needs its own frozen proposition and resolver.

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
| Review and calibration | P + D | Human review and native closure separate; optional forecast resolution requires a complete prospective declaration, not binarity alone. |

## Instructions

### 1. Frame and admit

Run `hypothesis-framer`, then the full relevant `falsifiability` path: admission, rival generation, counterfactuals and discrimination matrix. A framing that already carries adequate rivals may reuse them; never collapse back to H1/H0 only when material rivals remain live.

Open a research run before literature-dependent framing or design. Retrieve concrete sources and annotate their verification state. Query the capabilities researcher corpus (`corpus:researcher:`) when cross-domain systems principles are relevant; its same-named former skill is retired, so use corpus retrieval rather than skill activation.

### 2. Design before declaration

Call `skill(name="experimental-design")` and follow it through a complete `DesignSpec` or typed stop. The design is complete **before** `experiment_propose`; proposal is the preregistration point. A typed design stop halts before registry declaration or execution.

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

The arms are a **preference order, not mutual exclusion**. A design carrying both a meaningful binary pass rate and a design-family headroom criterion is admissible science: it takes the binary arm, because that arm also yields pass-rate eligibility. The server's `SealAttestationConflict` refuses supplying *both attestations at seal* — it does not refuse a design *capable* of either. Within HT-R, `genuinely_binary_claim` gates only the binary pass-rate arm; independently, forecast eligibility requires a prospective frozen declaration and resolver, not binarity alone. Thus a genuinely binary single-arm study may take `compatible_scientific` without fabricating a pass rate and still carry a linked goal.

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
  "binary_resolution_mapping": null,
  "compatibility_evidence": {},
  "headroom_arm": "compatible_scientific"
}
```

The mapper uses `layer="scientific_hypothesis"`; copies the supplied `headroom_arm` token verbatim into `registry_mapping`; passes the versioned `DesignSpec` as the **top-level `design_spec` argument** of `experiment_propose`, never nested inside `eval_set` and never written to both; serializes `{ "schema": "experimental-design-registry-adapter-v1", "hypothesis_spec": ..., "feedback_partition": ..., "selection_partition": ..., "executor_report_schema": ... }` inside `eval_set`; maps uncertainty/indifference regions to `noise_band`; carries positive `budget.max_runs`; and links a kanban goal only for a genuinely binary claim. Rubrics include ≥1 `metric` and ≥1 `implementation` or `grounded_report`, stable snake_case ids, and verification in `contains` / `not_contains` / `regex` / `report_ref`.

`design_spec` must carry a non-empty `spec_version`; the server refuses it otherwise. No freeze coverage is lost by the top-level position — the seal fingerprints `(eval_set, fitness_fn, noise_band, rubrics, design_spec)` together, so post-seal design drift yields a different fingerprint from either position.

Before any write, run HT-RM over the rendered mapping. Bind `expected_headroom_arm` to the **verbatim HT-R result token**, `mapped_headroom_arm` to the token emitted in `registry_mapping`, and `rubric_kinds` to the rendered rubrics' exact `kind` strings. The form validates and counts the rubric kinds itself — never pass model-authored rubric counts. Both arm tokens must be `compatible_binary` or `compatible_scientific`, copied unchanged, never abbreviated to `binary` / `scientific` and never re-encoded:

```lisp
(begin
  (define valid-rubric-kinds
    (lambda (ks)
      (if (= 0 (length ks)) t
          (and (or (string= (car ks) "metric")
                   (string= (car ks) "implementation")
                   (string= (car ks) "grounded_report"))
               (valid-rubric-kinds (cdr ks))))))
  (define count-kind
    (lambda (ks kind)
      (if (= 0 (length ks)) 0
          (+ (if (string= (car ks) kind) 1 0)
             (count-kind (cdr ks) kind)))))
  (and (string= layer "scientific_hypothesis")
       design_spec_toplevel
       (not design_spec_in_eval_set)
       spec_version_nonempty
       (> max_runs 0)
       (valid-rubric-kinds rubric_kinds)
       (> (count-kind rubric_kinds "metric") 0)
       (> (+ (count-kind rubric_kinds "implementation")
             (count-kind rubric_kinds "grounded_report")) 0)
       feedback_partition_present
       selection_partition_present
       (string= expected_headroom_arm mapped_headroom_arm)
       (cond ((string= expected_headroom_arm "compatible_binary")
              (and meaningful_pass_rate_definition binary_resolution_present))
             ((string= expected_headroom_arm "compatible_scientific")
              (and design_family_nonempty criterion_nonempty))
             (t (not t)))))
```

The arm check is a **closed-world `cond`, not two independent `if`s**, and the mapped token must equal the original HT-R token before dispatch. Each arm clause is reached only by its own expected token; any unrecognised token — `registry_schema_mismatch`, an abbreviated `binary` / `scientific`, a stale or misspelled token, an empty string — falls to the `(t (not t))` branch and fails the gate. A mapper that changes one valid arm into the other also fails before either arm's fields are consulted. An independent-`if` form cannot express this: a token matching neither arm skips both tests, each `if` yields its `t` default, and the conjunction authorises a write with **no** arm precondition ever checked. Write no fallback arm and no default-permit branch; an unknown or changed arm is a refusal, never a pass.

A false result returns to mapping once; a second failure stops as `registry_mapping_invalid` and performs no registry write.

Then:

1. Create a linked verification goal when useful, with confirmed criteria and actual source. Omit routine goal forecast; a binary claim alone is not a complete prospective declaration. Do not count linked experiment and goal as independent forecast events.
2. `experiment_propose` with the versioned eval-set object, the top-level `design_spec`, rubrics, fitness function, noise band, budget and optional linked verification goal id.
3. `variant_register` **every arm the DesignSpec predeclared**, before measurement: the control as `role="baseline"` (exactly one), each comparator as `role="challenger"`, and each ablation as `role="ablation"` naming the `parent_variant_id` whose component it ablates — the server refuses a parentless ablation. Registration is not measurement: pre-seal challenger *fitness* is refused by the server (`ExperimentNotSealed`), so registering the full arm set early cannot leak a pre-seal measurement. Freeze the arm-selection rule here, before any feedback result is read.
4. Execute the baseline arm through the DesignSpec-bound executor and `fitness_record(seal_id=null)` its real report references. Pre-seal fitness is baseline-only.
5. `experiment_seal` with **exactly one** attestation, matching the HT-R arm: `baseline_pass_rate` for `compatible_binary` (the scientifically meaningful measured rate), or `scientific_headroom` — `{design_family, criterion, evidence}` with recorded report refs — for `compatible_scientific`. Supplying both, or neither, is a typed refusal. A binary pass rate at 1.0 is a typed no-headroom refusal. For a scientific design, resolve that information-inadequate study through human review as `scientific_outcome="design_void"`, with the no-headroom reason preserved in `reject_reasons` or the FindingsSpec; redesign and re-declare, never weaken evaluators. Literal `no-headroom` closure remains only for artifact-layer experiments.

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

- `rejected` and `corroborated` enter the Brier calibration record. This experiment score never automatically closes a linked goal. Any scientific outcome may supply worker progress evidence; human closure and optional goal-forecast resolution remain separate.
- `evidence_of_practical_equivalence`, `inconclusive`, `measurement_void` and `design_void` are excluded from calibration **on the typed field itself** — no literal marker string is needed, and none should be invented. Preserve the FindingsSpec and the review decision; the fossil states honestly that the claim was neither held nor refuted.

Literal `no-headroom` / `measurement-void` markers remain the closure path for **artifact-layer experiments only** (`self-improvement` owns those; they resolve through `verdict`, which has no typed field). A scientific design never needs them.

Refinement requires a new versioned `HypothesisSpec` with lineage. Evidence that suggested it is exploratory and cannot confirm it; use a fresh sealed experiment or disjoint confirmatory set.

### 8. Metacognitive check and bounded improvement

Before execution, record the design’s expected outcome and confidence. After execution, compare expectation with observation. Do not infer human achievement from the scientific disposition. Only eligible binary experiment outcomes enter their declared forecast cohort. Periodically inspect calibration across experiments and revise the design method when systematic error appears; never count `design_void`, `measurement_void` or `inconclusive` as refutations.

Apply the conclusions-update gate HT-1 after each completed measurement batch:

```lisp
(cond ((or (string= trigger "contradiction") (string= trigger "new_category") (string= trigger "flip") (string= trigger "refinement")) "update") (t "keep"))
```

Simple reconfirmation keeps the prior conclusion. At most three rejected updates per cycle; then preserve the prior conclusion and disclose the skipped update.

## Registry Templates

| Template | Purpose |
|---|---|
| `registry-adapter.j2` | Classify the evidence-bearing semantic preconditions for mapping a complete `DesignSpec` onto the current experimentation contract, for both the binary and scientific headroom arms; it emits HT-R inputs, never a compatibility verdict or declaration args. |
| `registry-declaration.j2` | After HT-R returned a compatible arm, accept that exact token as input and map the versioned scientific artifacts, partitions, report schema and selected headroom attestation into proposed registry arguments for HT-RM validation. |
| `report.j2` | Produce the hypothesis-testing study report from sealed design, real run reports, FindingsSpec, registry lineage and audit results, with proposed checks separated from executed checks. |
| `conclusions.j2` | Apply HT-1 to update the distilled conclusion only on contradiction, new category, recommendation flip or meaningful refinement; reconfirmation keeps the prior text. |

Copy these complete contexts at render call sites; do not reconstruct them from prose.

`hypothesis-testing/registry-adapter`:

```json
{"hypothesis_spec": {}, "design_spec": {}, "feedback_partition": null, "selection_partition": null, "executor_report_schema": {}, "binary_resolution_mapping": null}
```

`hypothesis-testing/registry-declaration`:

```json
{"hypothesis_spec": {}, "design_spec": {}, "feedback_partition": {}, "selection_partition": {}, "executor_report_schema": {}, "binary_resolution_mapping": null, "compatibility_evidence": {}, "headroom_arm": "compatible_scientific"}
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

Run `bash kask/scripts/check-skill-lisp-forms.sh` for committed behavioral regressions over the actual HT-R/HT-RM/HT-1 forms and the production evaluator, not copied formulas. The `scientific_gate_` tests pin both compatible arms, binary preference when both attestations are available, every common compatibility prerequisite, exact expected/mapped arm-token equality, closed arm/kind vocabularies, rubric-kind derivation, top-level-only versioned spec, positive budget, required partitions and each arm's own preconditions. They also pin all four HT-1 update triggers and the reconfirmation/unknown-trigger keep path. Syntactically valid in-memory fail-open and token-switch mutations must fail the same decision oracles.

These tests do not invoke a registry write, authenticate executor reports, establish real partition disjointness from semantic booleans, or satisfy the functional trial. The remaining registry-seam cases below require the experimentation server tests or an authorized real run:
- Arm registration: every predeclared arm registered before measurement — one baseline, challengers, ablations each naming `parent_variant_id`; a parentless ablation is refused.
- Resolution: the ED-2 outcome passes verbatim into `scientific_outcome`, supplied alone; all six values round-trip unchanged; `rejected` carries non-empty reasons; the four non-calibrating outcomes are excluded from Brier on the typed field, with no literal marker invented.
- Scientific no-headroom: a `baseline_pass_rate` of 1.0 is refused; a scientific design closes through human review as typed `design_void`, with the reason preserved, while artifact experiments retain the literal `no-headroom` marker path.
- Server constraints: pre-seal challenger fitness refused; post-seal missing/wrong seal refused; overlapping final refs refused; both-or-neither seal attestation refused; changed DesignSpec produces a different fingerprint/new experiment.

- End-to-end functional trial: a real operator-supplied hypothesis runs design → honest executor boundary → FindingsSpec → grounding audit → human review. A synthetic case does not satisfy this trial.

## Constraints

- One lifecycle only. `experimental-design` returns artifacts and vetoes; this skill orchestrates.
- No design after `experiment_propose`; no confirmatory-data peeking before preregistration.
- Recorded runs name real reports. Never simulate fitness.
- Nothing auto-adopts; human review owns disposition.
- The study report separates proposed checks from checks actually executed.
- Lesson capture is the memory architecture's automatic thread distillation — reports never call memory tools and propose no memory candidates (corrected 2026-10-24: the earlier human-gated-candidates reading inflated the D5 bar, which forbids automated memory_insert paths in loops, not natural thread capture).
- Semantic templates classify evidence; only `lisp_eval` HT-R/HT-RM authorize compatibility and mapping. Never carry a model-typed compatibility verdict across the registry seam.
- `BudgetExhausted` ends the declaration. A changed mechanism or design is a fresh declaration with lineage.
