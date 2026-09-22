---
title: "Artificial Curiosity: Read-Only Capability-Probe Pilot"
audience: [researchers, architects, agents]
last_updated: 2026-09-22
version: "1.0.0"
status: "Pilot observed — selection comparison blocked"
domain: "Cross-cutting"
mds_categories: [composition, trust]
---

# Artificial Curiosity: Read-Only Capability-Probe Pilot

> **Scope and provenance.** This is an *executed*, read-only pilot following [the open-questions research](artificial-curiosity-open-questions-followup.md). It tests whether installed skill templates accept a task-specific handoff and reject missing required inputs. It does **not** test whether a curiosity score discovers more capabilities. The observable oracle is `render_template` contract validation and the rendered prompt, not a model's self-assessment. No benchmark fixture, skill, or runtime code was added.

## Functional target and preregistered pilot boundary

Target: determine whether the existing installed skills can expose a usable **next-probe handoff** for the fixed question “Can the installed skill templates produce inspectable handoff prompts for a read-only capability probe?” The unit is a template *render*, not a software capability discovery. Positive expectation: the renderer accepts the declared required inputs and carries the supplied task context into its prompt. Negative control: omitting one contract-required field must return a named error. One positive and one negative render per template; no stochastic repetitions. The positive and negative calls used the same four existing templates and `render_template` as the only pass/fail oracle.

The relevant existing registry is [`capability-ontology.yaml`](../../registry/templates/capabilities-reasoner/capability-ontology.yaml): its five composition categories distinguish floors (`determinism_frontier`, `persistence_grounded_learning`, `failure_surfacing`) from maturity gates (`lisp_scaffold`, `co_evolution_loop`). **A successful prompt render does not establish any category's floor.** The templates tested below were read from their current registry paths; a separate `falsifiability` admissibility prompt supplies the testability gate. The domain phrases “capability-probe experiment” and “independent test oracle” resolved only to the `5w1h_core` rung through `onto_anchor`; no ontology definition was added.

## Observed execution

| Handoff and fixed input | Positive tool result | Negative control and observed result | What this establishes |
|---|---|---|---|
| `capabilities-reasoner/capability-register`: target system describing this read-only template-rendering question, `capability_definition=task-performance`, no registry seed | Rendered registry-construction prompt. Its Target System JSON included the supplied task, allowed actions (`read_file`, `render_template`, `lisp_eval`) and renderer oracle. | Omitted `target_system`; `render_template` returned `Template contract validation failed ... missing required input field(s): target_system`. | Task-specific register prompt can be produced and missing target is rejected. It does **not** show that an evaluated capability was elicited. |
| `falsifiability/falsifiability-admit`: fixed claim that installed skills can furnish a next-probe handoff, domain and oracle context | Rendered admissibility prompt including the target claim and context. | Omitted `target`; error named `missing required input field(s): target`. | A falsifiability gate can be prompted; **no admissibility verdict was independently generated** by the renderer. |
| `gradient-hunter/gradient-prior`: per-template task-specific render as target region and render acceptance/context as field | Rendered a prior-building prompt including the target region and field. | Omitted `target_region`; error named `missing required input field(s): target_region`. | Prior-model handoff is available. No measured spatial capability gradient or gradient verdict was produced. |
| `metacognition/meta-grasp-current`: assess whether a curiosity-score comparison is fair, with explicit missing-history list | Rendered a current-condition prompt carrying absent comparable probe history, posterior, held-out labels and cost logs. | Omitted `goal`; error named `missing required input field(s): goal`. | Current-condition handoff is available and required goal is checked. No completed PDCA or empirical score is implied. |

**Count reconciliation:** four of four positive renders returned prompts containing the supplied task context, and four of four single-field omissions returned the named contract error. The negative calls intentionally failed and were logged as expected-absence controls with `curator_report_skill_use_issue`; they do not indicate a newly found renderer defect. The result is a **template-contract probe**, not a four-trial estimate of strategy performance. A deterministic structure check returned true for the four-positive/four-negative ledger, but did not assess usefulness of any selected probe.

**Existing matched test, independently run:** `cargo test -p agent --lib contract_validation_rejects_missing_required_render_inputs -- --nocapture` reported `running 1 test` and `test tools::render_template_tool::tests::contract_validation_rejects_missing_required_render_inputs ... ok` (`1 passed; 924 filtered out`). The first attempt with `--exact` selected **zero** tests and was not counted as verification. The existing test pins missing-field rejection, so the live negative controls confirmed an already tested contract rather than discovering a new behavior; neither the test nor these renders exercises selection outcomes.

## Hypotheses eliminated and not eliminated

- **Eliminated for these four inputs:** “The installed templates cannot be rendered with task-specific data,” and “the renderer silently accepts omission of the tested required fields.” A counterexample to either universal statement was directly observed. This does not establish completeness of their outputs under other input distributions.
- **Not eliminated:** “Existing-skill orchestration identifies the best next capability probe,” “learning-progress selection adds unique value,” and “unexpected spatial gradients predict future discovery.” All predict outcomes over *executed candidate probes*, which this renderer-only pilot never ran.
- **Competing explanations for the positive renders:** correct template interpolation, regardless of whether a later agent would choose a discriminating experiment; a weak oracle that checks prompt form but not task outcomes; and a well-scoped composition contract that still needs substantive execution. This pilot distinguishes none of these.

**Result of the requested A/B/U/C comparison: blocked, not zero.** A systematic sequence (A), human-guided orchestration (B), uncertainty sampling (U), and competence-progress/information-gain selection (C) require a common candidate set, per-probe actual cost, repeated before/after competence results or a calibrated posterior, and an independent held-out task oracle. None is generated by the four prompt renders. In particular, a single render cannot yield a temporal learning-progress estimate, and counting words or prompt novelty is *not* information gain. A reported “C wins” or a numeric gradient here would be fabricated.

## Research consequence and next real experiment

The prior [follow-up protocol](artificial-curiosity-open-questions-followup.md#discriminating-experiment-protocol-not-executed) remains the necessary test of unique value. The smallest next empirical step is to freeze **one authorized, replayable software task family** with independent pass/fail tests, documented probe costs and two functional adjacency definitions. Run an initial shared coverage batch to establish measured histories, then compare B/U/C with the same oracle, budget, frontier archive and eligibility rules; preserve A as a fixed-order baseline. Record all outcomes and failures, not just discovered positives. Hold back tasks from the selector to detect proxy-only progress. A proposed H2 residual-local-contrast term needs a **separate** ablation (C versus C+R) under both adjacency definitions; do not use an unobserved barrier as a measured gradient.

Until a user-authorized test family and its independent oracle are fixed, the open questions remain **empirical** rather than answerable by more inference or by this renderer probe. No distinct curiosity skill is justified by these results. No runtime behavior was changed and no commit was created for this pilot. The earlier [source-grounded mechanism comparison](artificial-curiosity-capability-space.md) and [seven-source follow-up](artificial-curiosity-open-questions-followup.md) remain the literature ground; no new external sources were used in this pilot.
