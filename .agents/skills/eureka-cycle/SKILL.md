---
name: eureka-cycle
description: "Run one human-chaired discovery cycle over the Open Questions board: intake and routing (research-resolvable vs experiment-requiring vs forecasting), recall-cited candidate generation, the PI funding gate, delegated lifecycles (hypothesis-testing, self-improvement, research runs), sealed pre-registered experiments, resolution at the algedonic review, and a study report. A loop orchestrator and router — it owns no lifecycle."
---

# Eureka Cycle

One invocation runs one full discovery cycle, session-chaired: the operator holds
the question, the funding gate and selection. This skill is the loop layer above
the lifecycle owners — it never frames, designs, executes or audits an experiment
itself; it routes to the owners and carries the cycle's memory.

## Reference models

- **Socher, *The Eureka Machine* (2026)** — the vision of a full-stack scientific
  superintelligence (a living map of human knowledge, a model of physical reality,
  simulations, autonomous labs, an agent swarm of AI scientists on top), and
  **Recursive's automated-research system**: it "automates the full research loop:
  proposing ideas, implementing them, testing them, and using the results to decide
  what to try next. It explores multiple directions in parallel, builds on earlier
  experiments, and combines promising ideas", validating results "for reward hacks
  and variance before treating improved performance as real progress"
  (`recursive.com/updates/first-steps-toward-automated-ai-research`, 2026-06-11;
  `recursive.com/updates/introducing-recursive-flint`, 2026-10-21; both retrieved
  2026-10-23 in research run `12a94d05e473d5d5`, six server-verified sources).
  The name borrows the loop **topology**, not the autonomy claims: this platform's
  pole is human-chaired (the co-research charter,
  `kask/docs/architecture/core/co-research-environment.md`). No parity with
  Recursive's machine is claimed.
- **The P8 experiment protocol**
  (`kask/docs/research/experimentation-reference-model.md`): declare / vary /
  test / select / retain; the pre-registered, Brier-scored prediction as the
  controller; Q1=b budgeted auto-run for deterministic-fitness experiments; Q4=A
  the algedonic review as the single selection surface; the headroom pre-check.
- **The delegated lifecycles**: `hypothesis-testing` (the one end-to-end
  scientific inquiry lifecycle — its SKILL.md pipeline is the protocol this loop
  hands each scientific candidate to) and `self-improvement` (artifact lifecycles
  plus the `si-agenda-generator`, the machine-side intake whose `proposed`
  experiment queue this loop reads at ORIENT and never re-hosts).
- **Tetlock question triage** (`scenario_triage`, `superforecasting` stage 0) —
  the anchored vocabulary for resolvability judgment at the ROUTE step; a
  forecasting question routes onward to `superforecasting` rather than being
  re-triaged here.

## Initial and target condition

- **Initial:** funded question(s) on the Open Questions board (the PI has moved
  them to `ready`), operator authority to run one session-chaired cycle, and the
  recall context assembled at ORIENT.
- **Target:** every funded question of the cycle reaches a resolution record
  (adopt / fossil / answer) or a typed stop, with a grounding-verified study
  report on the question's board card; nothing routed to spend without the
  funding gate; every prediction's confidence recorded pre-seal.

## When to Use

- One or more funded questions are waiting on the Open Questions board and the
  operator asks for a discovery cycle.
- The operator files a new question and asks for intake, routing and candidate
  generation.
- A cycle's resolution records exist and the operator asks for the study report.

## When NOT to Use

- One scientific hypothesis needs its full lifecycle — use `hypothesis-testing`
  directly; this skill routes to it and never replaces it.
- One artifact / self-improvement experiment — use `self-improvement`.
- One bounded research project with a changing task roster — use
  `local-research-swarm` (project-work coordination; its own boundary excludes
  domain verdicts).
- Forecasting one resolvable question — use `superforecasting`.
- Project-scale scenario construction (forces, 2x2 narratives, early-warning
  indicators) — use `scenario-planning` directly; this loop's forecasting route
  estimates probabilities, it does not build scenarios.
- The question is not on the board — file it first; the board is the only intake
  this loop reads.

## Step types (D/P labelling)

| # | Step | D/P | Oracle / gate |
| --- | --- | --- | --- |
| 1 | ORIENT — board + registry + lineage + curator reads | D | the kanban and experimentation servers |
| 2 | ROUTE — research-resolvable / experiment-requiring / forecasting | P:propose-verify | the PI's confirmation at the funding gate |
| 3 | GENERATE — recall-cited fanout | P:propose-verify | the triage brief's grounding check |
| 4 | FUNDING GATE — the PI chairs | human chair | the gate for steps 2-3; the board's `ready` state |
| 5 | FRAME (the handoff) — route each funded candidate to its owning lifecycle: scientific → `hypothesis-testing` (its pipeline runs hypothesis-framer → falsifiability → experimental-design), artifact → `self-improvement`, research-resolvable → the research-run path | P:propose-verify | the funded-kind confirmation at the funding gate; the delegated lifecycle's own gates (`falsifiability-admit`, the declare boundary) apply downstream — this loop never runs them |
| 6 | DECLARE + SEAL — the registry's contract path | D | schema validation, SHA-256 fingerprint, headroom |
| 7 | EXECUTE — the delegated lifecycle's bound protocol | P execution, D measurement | sealed rubric evaluators |
| 8 | MEASURE — `fitness_record` under the budget | D | the server's typed `BudgetExhausted` refusal |
| 9 | RESOLVE — verdict at the algedonic review | human chair, D record | the single selection surface |
| 10 | DEPOSIT + BACK-EDGE — study report | P report, D gates | `EUR-1` reconciliation + `EUR-2` Brier receipts; `grounding-verify` |

## Instructions

1. **ORIENT.** Read the Open Questions board (`kanban_task_list` — pull only
   `ready` cards), the agenda queue (`population_query` over `proposed`
   experiments — read, never re-host), prior lineage (`lineage_read`) and curator
   memory (`curator_semantic_search` / `curator_federated_search`,
   `swarm_recall_local`). Dedup: a question with a live experiment on the same
   subject is not funded twice. Move exactly one card to `in_progress` — one cycle
   per card at a time; the board columns are the concurrency lock.
2. **ROUTE.** Classify each funded question: research-resolvable (answerable by
   evidence, no experiment), experiment-requiring, or forecasting. Nothing routes
   to spend without the PI's confirmation at step 4. The classification is a
   proposal, not a verdict. A forecasting-routed question is a **typed stop**
   for this cycle: record a `routed` disposition on the card citing the
   `superforecasting` invocation, hand the question to that pipeline, and close
   the cycle's work when the forecast is declared — the forecast's Brier-scored
   resolution lands as a card comment when it resolves (the forecasting
   lifecycle owns the resolution; this loop never scores it).
3. **GENERATE.** Render `eureka-cycle/eureka-generate` with the copyable
   context below. Every candidate cites at least one prior artifact; when the
   question's lineage is empty, the source signals ride `prior_context` and the
   brief states the empty context explicitly (the citation requirement never
   silently drops all candidates). Include one recombination candidate over
   previously selected lineages. Uncited candidates are dropped.

   `eureka-cycle/eureka-generate`:
   ```json
   {
     "question": "<the funded question verbatim>",
     "prior_context": ["<id + what it established, one entry per recalled artifact — carries the source signals when the lineage is empty>"],
     "personas": ["<persona names>"],
     "aperture": "<the cycle's aperture>",
     "n_candidates": <the candidate count>
   }
   ```
4. **FUNDING GATE.** Render `eureka-cycle/eureka-triage-brief` with the copyable
   context below: candidates ranked, each with citations, expected-value sketch,
   and a budget ask **in the unit its lifecycle will enforce** (`max_runs` for
   experiments, estimated call caps for research runs). The PI funds K. The
   funding gate funds board cards only — promoting an agenda-registry candidate
   is the explicit act of filing it on the board with provenance.

   `eureka-cycle/eureka-triage-brief`:
   ```json
   {
     "question": "<the funded question verbatim>",
     "candidates": [{"hypothesis": "...", "citations": ["..."], "expected_value_sketch": "...", "budget_ask": "...", "resolution_path_guess": "..."}],
     "empty_context_note": "<stated when the lineage is empty, else null>"
   }
   ```
5. **FRAME.** For each funded scientific candidate, hand off to the
   `hypothesis-testing` pipeline (hypothesis-framer → falsifiability →
   experimental-design); artifact candidates go to `self-improvement`;
   research-resolvable questions go to the research server path
   (`begin_research_run` → run-scoped `web_search`/`web_extract` →
   `evaluate_evidence` → `annotate_research_run` → `finish_research_run`). Accept
   `hypothesis-testing`'s registry-compatibility classification as binding: a
   non-compatible candidate resolves through its typed stop.
6. **DECLARE + SEAL.** `experiment_propose` (pre-registered prediction with
   confidence, rubrics, `budget.max_runs`), then `variant_register` (baseline),
   then the baseline's **pre-seal measurement** via `fitness_record` with
   `seal_id: null` (the sanctioned path — the seal refuses a baseline with no
   recorded fitness), then `experiment_seal`. For single-arm designs supply the
   `scientific_headroom` rationale, never a fabricated pass rate. A candidate
   refused at any gate (falsifiability-admit, no-headroom, incompatibility)
   returns to the card as a cheap fossil — "refused at gate X, reason Y" — and
   the cycle continues with the survivors.
7. **EXECUTE.** The delegated lifecycle runs the bound protocol. Swarm delegation
   is one executor among several (data pullers, `lean_check`, job execution,
   research runs); the lifecycle binds the executor per its DesignSpec.
8. **MEASURE.** `fitness_record` under the server-enforced budget; a typed
   `BudgetExhausted` refusal is surfaced to the PI, never retried silently.
9. **RESOLVE.** `final_evaluation_record` on the selection set; the verdict lands
   at the algedonic review (the human chair — the single selection surface;
   adoption is never recorded from this loop or the board's `Done` column).
   `selection_record` is the record path.
10. **DEPOSIT + BACK-EDGE.** Compute the cycle's deterministic receipts through
    `lisp_eval` first — the engine owns the counts and the arithmetic; the
    report renders around them and never re-types them.

    **`EUR-1` (reconciliation receipt)** — run over the assembled report fields
    and the `population_query` readback before posting the report:
    form: `(and (= funded_count section_count) (= unresolved_ids 0) (= missing_pre_seal_confidences 0))`
    env: `{ "funded_count": <the cycle's funded candidate count>, "section_count": <the report's candidate-section count>, "unresolved_ids": <count of section experiment ids absent from the population_query readback>, "missing_pre_seal_confidences": <count of predictions whose recorded confidence is absent from the sealed declaration> }`
    The study report's reconciliation field states the EUR-1 receipt verbatim
    (its four inputs and the returned boolean) — the model never counts.

    **`EUR-2` (per-prediction Brier)** — for each resolved binary claim
    (outcome 1 = held, 0 = refuted):
    form: `(* (- probability outcome) (- probability outcome))`
    env: `{ "probability": <the prediction's recorded pre-seal confidence>, "outcome": <1 or 0> }`
    The cycle mean Brier is the arithmetic mean of the EUR-2 outputs.

    Render `eureka-cycle/eureka-study-report` with the copyable context below:
    per-candidate sections reference the delegated lifecycle's own report output,
    plus the cross-candidate outcomes table, the EUR-2 Brier readback, and
    evaluator-hardening candidates (filed via `evolve_mcp_tool_schema` when
    gaming was detected — as the search strengthens, the evaluator must
    strengthen too). Lessons from the cycle land in memory through the memory
    architecture's automatic thread distillation — this loop proposes no memory
    candidates and never calls a memory tool (corrected 2026-10-24: the earlier
    candidate-approval design misread the D5 bar, which forbids automated
    memory_insert paths in loops, not natural thread capture). Post the report
    on the question's board card; spawn or file follow-up questions. Empty
    generation files a finding on the card and ends the cycle — no silent retry
    loop.

    `eureka-cycle/eureka-study-report`:
    ```json
    {
      "question": "<the funded question verbatim>",
      "cycle": {"candidates": ["<id, resolution path, registry/experiment id, verdict, prediction (claim + confidence), outcome>"], "question_id": "<the board card id>", "routing": "<the routing summary>"},
      "brier_readback": {"predictions": ["<claim, confidence, experiment, outcome, single_brier (from EUR-2)>"], "cycle_mean_brier": "<mean of the EUR-2 outputs>"},
      "evaluator_hardening": []
    }
    ```


Card conventions: title = the question; description = provenance (filed-by,
signal source, context links) — the description is the readback-verifiable
provenance carrier (`kanban_task_list` does not surface labels; labels are
advisory). One row-mutating kanban call per task per tool batch, and a
`kanban_task_comment` only in a batch carrying no other mutation for that task.
Every step's durable state lands in the board or the registry: an interrupted
cycle resumes at ORIENT or the card returns to `ready`; an experiment abandoned
mid-flight belongs to the regulation loop's stale-experiment sensor.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `eureka-generate.j2` | Recall-cited candidate fanout across personas, with the recombination line |
| `eureka-triage-brief.j2` | The PI's funding brief: ranked candidates, citations, budget asks in enforced units |
| `eureka-study-report.j2` | The cycle-aggregation envelope over the delegated lifecycles' own reports, with Brier readback |

## Regression case

The T0 rehearsal (2026-10-23, live, one pass): board `8a62f1e1` created; card
EUREKA-0 (`5f4c5ff5`) filed with provenance and walked backlog → ready →
in_progress → review → done; paired experiment `exp_6e55581d74964701837dddad62f7bbb8`
declared with prediction (confidence 0.9), baseline variant
`var_30632f415c32492db9c242e0bab5df6d`, pre-seal fitness `fit_d9010287412a4f41aeefa3eb387b9792`
(4/4 branch receipts true via `lisp_eval` string-contains, needle first), seal
`seal_2debfa6efdce459babbe536912f2dd6e` (fingerprint `cdc4f09d…`), resolution
`sel_ac7505f3bf0d4f6e9e9e89341d383509` — `corroborated`. Two seam findings the
rehearsal forced into this text: (1) the seal ordering — the baseline's pre-seal
`fitness_record` (`seal_id: null`) must precede `experiment_seal` (the server's
typed `failed_precondition` taught the order); (2) `kanban_task_list` does not
surface card labels — the description is the provenance carrier.

## Constraints

- **Router-not-owner (Prohibition):** this skill owns no lifecycle; drifting into
  framing, designing, executing or auditing experiments directly is the
  co-scientist failure mode and grounds for retirement.
- **Single selection surface (Prohibition, Q4=A):** adoption verdicts happen only
  at the algedonic review; the board's `Done` means the question is answered,
  never that a variant was adopted.
- **No memory-candidate machinery (Prohibition, corrected 2026-10-24):** lesson
  capture is automatic — the memory architecture's thread distillation parses
  threads into durable lessons on its own; this loop proposes no memory
  candidates and never calls a memory tool. The D5 bar (2026-10-15) forbids
  automated `memory_insert` paths in loops — it never gated natural thread
  capture; the candidate-approval design misread it and was removed.
- **Proposes, never commits:** no git commits, no file mutations outside the
  board card and the report.
- **No per-cycle doc files:** the board card plus the registry are the record; a
  finding worth durable publication goes through `doc-update`.
- **One row-mutating kanban call per task per batch; comments alone.**
- **Budget honesty:** every budget ask names its enforced unit; every typed
  refusal is surfaced, never retried silently.