---
name: self-improvement
description: "General self-improvement skill for FM-based agents. Drives persistent, endogenous adaptation of the operational scaffold (prompts, memory, tools, control logic) via intrinsic demonstrations, evaluative feedback, and extrinsic exploratory experience. Runs GEPA prompt experimentation under a measured rollout harness; files proposals, never commits."
audience: platform
---

# Self-Improvement

General multi-purpose self-improvement skill for FM-based agents. Implements the unified self-induced update operator from Ren et al. (2026, arXiv:2607.13104), restricted to the pathway this system can execute: Scaffolding Improvement (Σ) — prompts, memory, tools, control logic — through intrinsic generative demonstrations, intrinsic evaluative feedback, and extrinsic exploratory experience. It embeds its own PDCA loops at the improvement-cycle level and wraps an outer Improvement Kata loop around all algorithmic approaches, following the Toyota Improvement Kata (direction → current condition → target condition → experiment).

**Scope boundary (operator ruling 2026-10-17):** the paper's Foundation Model Improvement pathway (θ — parameter updates via fine-tuning) is **out of scope for the co-researcher mandate** and has no executor in this system: the training server, the lora-training skill and the adapter lifecycle were removed. A task that genuinely requires a parameter update is reported as out of scope with its measured evidence — never simulated through a scaffold change presented as a fine-tune.

## Goal responsibilities

Follow [GOAL_INTERFACE.md](../../../kask/mcp-servers/hkask-mcp-kata-kanban/GOAL_INTERFACE.md).
Measured rollout improvement supports a proposal, not human acceptance or permission to promote it. Preserve the operator review gate.

## Initial and target condition

- **Initial condition:** the named scaffold capability gap, current θ_t and Σ_t, `metric_before` across performance/cost/safety, a fixed eval set with an executable evaluator, explicit resource authority, and a pre-registered outcome prediction.
- **Target condition:** within at most ten outer Kata experiments and five inner PDCA iterations per step, a measured scaffold candidate clears the deterministic acceptance/regression gates and is filed for independent algedonic judgment, or the baseline is retained with the blocker and remaining measured gap reported. A failed Check rolls back, discards or refines; this skill never adopts its own proposal.

## Theoretical Foundation

The skill formalizes self-improvement as a **self-induced update operator** 𝒰 that maps the agent's current configuration to an updated one:

```
𝒜_{t+1} = 𝒰(𝒜_{1:t}, ℰ(π_{θ_t,Σ_t}; Σ_t, 𝒞_t))
```

where:
- `𝒜_t = (θ_t, Σ_t)` — agent configuration (model params + scaffold)
- `Σ_t = (p_t, m_t, 𝒯_t, g_t)` — scaffold (prompts, memory, tools, control logic)
- `ℰ` — agent-executed procedure producing a learning signal
- `𝒞_t` — task or deployment context
- `𝒰` — the update operator (this skill's core)

Two pathways instantiate 𝒰 in the paper; only the second is executable here:
1. **Foundation Model Improvement** (Section 5): `θ_{t+1} = 𝒰_θ(θ_{1:t}; 𝒮_t)`, `Σ_{t+1} = Σ_t` — **out of scope**, no executor (see the scope boundary above)
2. **Scaffolding Improvement** (Section 6): `Σ_{t+1} = 𝒰_Σ(Σ_{1:t}; 𝒮_t)`, `θ_{t+1} = θ_t` — the pathway this skill runs

Three signal forms drive the scaffolding pathway:
- **Intrinsic Generative Demonstrations** (`𝒮_t ≈ 𝒟_t`): agent synthesizes training instances
- **Intrinsic Evaluative Feedback** (`𝒮_t ≈ e_t`): agent judges candidate behavior
- **Extrinsic Exploratory Experience** (`𝒮_t ≈ τ_t`): agent collects interaction trajectories

## When to Use

- When an agent needs to durably modify its own scaffold (prompts, memory, tools, control logic) based on execution experience
- When you need to select the appropriate improvement signal (intrinsic demonstrations, intrinsic evaluations, or extrinsic experience) based on what the environment provides
- When you need to run a structured PDCA improvement cycle with convergence detection and rollback safety
- When you need to wrap an outer Improvement Kata loop around multiple improvement cycles to drive long-term capability gains
- When you need to evaluate self-improvement claims rigorously (trajectory tracking, transfer testing, regression checks, cost accounting)
- When you need to govern self-modification safely (verifier-gated updates, layered permission systems, critic decoupling)

## When NOT to Use

- One-off corrections — if the fix is known, apply it directly; a PDCA cycle around a known fix is ceremony.
- Skill authoring and maintenance — use `create-skill` / `skill-maintenance` (the scaffolding-improvement pathway delegates there anyway).
- Memory curation — use `therapy`; reorganizing memory is not self-modification of configuration.
- Parameter updates (fine-tuning, LoRA adapters) — out of scope for the co-researcher mandate and unexecutable here; report the need, do not improvise a substitute.

## Architecture: Nested PDCA + Outer Kata

The skill follows a **three-layer architecture**:

```
┌─────────────────────────────────────────────────────────────────┐
│  OUTER LAYER: Improvement Kata (kata-improvement)               │
│  Step 1: Understand Direction (what capability are we building?) │
│  Step 2: Grasp Current Condition (baseline measurement)         │
│  Step 3: Establish Target Condition (measurable, bounded in      │
│          this session's experiments)                           │
│  Step 4: Experiment (PDCA) — delegates to MIDDLE LAYER           │
│  Convergence: before/after measurement against target             │
└─────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────────┐
│  MIDDLE LAYER: Improvement Cycle PDCA (per-iteration)            │
│  Plan:   Select pathway + signal + generate improvement plan     │
│  Do:     Execute the improvement operator 𝒰                      │
│  Check:  Evaluate updated agent on held-out tasks                │
│  Act:    Commit, rollback, or refine based on evaluation         │
│  Convergence: trajectory stability + transfer + regression        │
└─────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────────┐
│  INNER LAYER: Signal-Specific Algorithm (per-signal)             │
│  Scaffolding Improvement:                                         │
│    §6.1 Prompt Optimization (scalar/qualitative/experimentation/gradient)│
│    §6.2 Memory Experimentation (CRUD: Create/Read/Update/Delete)        │
│    §6.3 Tool Governance (routing/refinement/creation)            │
│    §6.4 Full Scaffolding (self-referential code rewrite)          │
│  (FM Improvement §5.1–5.3 — out of scope, no executor)            │
└─────────────────────────────────────────────────────────────────┘
```

### Why nest PDCA inside Kata?

The paper's formalism (`𝒜_{t+1} = 𝒰(𝒜_{1:t}, ℰ(...))`) is inherently iterative: each application of 𝒰 produces a new configuration that becomes the input to the next. But the paper also emphasizes that self-improvement is a **process that unfolds over time** (Section 8.1) and must be evaluated as a trajectory, not a single endpoint. The Improvement Kata provides the outer loop that:

1. **Establishes direction** — what capability are we building? (Step 1)
2. **Measures baseline** — where are we now? (Step 2)
3. **Sets a target** — where do we want to be? (Step 3)
4. **Runs experiments** — each experiment is a PDCA improvement cycle (Step 4)

This separation is critical because the paper identifies a key tension: "self-improvement unfolds over time and naturally exhibits plateaus or regressions" (Section 8.1.1). The Kata outer loop provides the long-horizon direction that prevents the inner PDCA cycles from optimizing locally without global progress. The Kata convergence check measures whether the overall trajectory is converging toward the target condition, while the PDCA convergence check measures whether a single improvement iteration is stable enough to commit.

## D/P labelling

Pathway and signal selection, plan generation, GEPA reflection and mutation
proposals, and the Act step's judgment work (failure-mode naming, `next_step`,
proposal-card content) are P — judgment, critiqued by the
measured Check results and by the operator in the algedonic review (the
executing session never commits its own improvements). The propose/discard
bit itself is D: a pure function of the gate result that the rendered
template cannot override. The gates are D:
the propose-or-discard acceptance predicate, the regression count, the GEPA
dominance and convergence computations, the noise-floor standard error,
and the PDCA convergence form all run in `lisp_eval` over measured
harness reports (the recorded spawn-executor report pairs) — a
candidate that fails the gate is discarded regardless of the model's
assessment. (The `safety_violations` gate term is model-judged input:
the gate is deterministic over its inputs, but that one input is P —
goal-drift indicators are not harness-measurable.) Harness evaluation is D (deterministic
contains/not_contains/regex evaluators); filing a proposal is P, gated by
the D verdict. Agenda generation (candidate design, prioritization, prediction
setting) is P, critiqued by the pre-registered prediction (Brier-scored at
resolution) and the headroom pre-check; signal reads, registry queries, the
pre-check verdict, and Brier computation are D.

## Instructions

> **Executor status (2026-10-29):** the measurement executor is the live
> spawn path — each candidate (prompt + task) runs as a one-shot
> `spawn_agent` sub-agent whose returned `output` is the response; the
> orchestrating session scores responses with the task's evaluator
> (`lisp_eval`) and records the report pairs (the GEPA executor binding
> below). `kanban_task_spawn` is the companion path when the improvement
> work is tracked as a delegated kanban task. The fail-safe stands: no
> executor or evaluator → `evaluation_method: "none_available"`, the gate
> discards, never fabricates. Redesign landed per algedonic review card
> b96cede1 (operator-accepted 2026-10-29).

### Outer Kata Steps 1–3 (rendered from `kata-improvement`)

The outer Kata uses the `kata-improvement` step templates directly; this skill supplies the self-improvement framing in `context`, and never restates the procedure.

1. Render `kata-improvement/improvement-step1-direction`, `kata-improvement/improvement-step2-current`, and `kata-improvement/improvement-step3-target` in order, passing each step's output in `previous_steps`.
2. In `context`, state the self-improvement framing (Ren et al., 2026, arXiv:2607.13104): the agent configuration 𝒜_t = (θ_t, Σ_t), where θ_t is the foundation model and Σ_t = (p_t, m_t, 𝒯_t, g_t) is the scaffold (prompts, memory, tools, control logic); the challenge is the capability gap the update operator 𝒰 should close.
3. Step 2 (current condition) must describe the current θ_t and Σ_t and include cost metrics (compute, tokens, wall-clock time, human input) and safety metrics (regression rate, safety violations, goal-drift indicators) alongside performance metrics; record `metric_before`.
4. Step 3 (target) counts its horizon in this session's bounded PDCA experiments (at most 5 per Kata step), not calendar time (operator ruling 2026-09-24); record `metrics_target`.

### si-select-pathway (PDCA Plan)

1. Confirm the update target is Scaffolding (Σ). Foundation Model (θ) is out of
   scope for the co-researcher mandate and has no executor — a gap that genuinely
   requires parametric consolidation is reported as out of scope with its measured
   evidence, never routed to a scaffold change dressed as a fine-tune.
   - The paper notes: "Modifying the operational scaffold (Σ) drives a fast adaptation loop... parameter updates (θ) are much slower" (Section 9.1).
2. Determine which Σ component to update — Prompt (p), Memory (m), Tool (𝒯), or Full Scaffolding (Σ).
   - Prompt: when the bottleneck is task communication, objectives, or constraints.
   - Memory: when the bottleneck is long-horizon recall, cross-context knowledge transfer, or context-window pressure.
   - Tool: when the bottleneck is action execution, capability coverage, or tool reliability.
   - Full Scaffolding: when the bottleneck requires holistic reconfiguration of perception, reasoning, and execution.
3. Determine the improvement signal: Intrinsic Generative Demos (𝒟_t), Intrinsic Evaluative Feedback (e_t), or Extrinsic Exploratory Experience (τ_t).
   - Intrinsic Demos: when the agent can synthesize high-quality exemplars from its own priors.
   - Intrinsic Feedback: when the agent can judge candidate behavior through rubrics, consistency, or critique.
   - Extrinsic Experience: when the environment provides grounded feedback (unit tests, task success, rewards).
4. Generate a concrete improvement plan: what operator 𝒰 will be applied, what signal 𝒮_t will drive it, what experiment scope and explicitly authorized resource constraints apply, and what acceptance criteria will gate the update. Do not invent token quotas; token usage is measured, not budgeted.
5. On PDCA iteration 2+, compute variety repetition and route target deterministically with `lisp_eval` (`SI-1` — sub-pathway routing + variety check):
   - form: `(let ((repeated (if (and (string= pathway prev_pathway) (string= scaffold_component prev_component) (string= primary_signal prev_signal)) 1 0)) (route (cond ((string= pathway "sigma") (cond ((string= scaffold_component "p") "self-improvement/si-exec-scaffold-prompt") ((string= scaffold_component "m") "self-improvement/si-exec-scaffold-memory") ((string= scaffold_component "T") "self-improvement/si-exec-scaffold-tool") ((string= scaffold_component "sigma") "self-improvement/si-exec-scaffold-full") (t "invalid_route"))) (t "invalid_route")))) (list route repeated))`
   - env: `{ "pathway": <"sigma">, "scaffold_component": <"p"|"m"|"T"|"sigma">, "primary_signal": <"D_t"|"e_t"|"tau_t">, "prev_pathway": <prior or "">, "prev_component": <prior or "">, "prev_signal": <prior or ""> }`
   A `pathway` of `"theta"` routes to `invalid_route` by construction — the θ executor was removed, so the form refuses rather than silently falling through to a scaffold template.
6. Render `self-improvement/si-select-pathway` with `challenge`, `current_condition`, `target_condition`, `pathway_preference`, `signal_preference`, `scaffold_component`, `budget`, `acceptance_criteria`, and `context`:
   ```json
   {"template_ref": "self-improvement/si-select-pathway", "context": {
     "challenge": "<the capability gap the update operator should close>",
     "current_condition": "<Kata step 2 object: theta_t, sigma_t, performance/cost/safety metrics, metric_before>",
     "target_condition": "<Kata step 3 object: metrics_target, bounded in this session's experiments>",
     "pathway_preference": "<'sigma'>",
     "signal_preference": "<'D_t', 'e_t', 'tau_t', or ''>",
     "scaffold_component": "<'p', 'm', 'T', 'sigma', or ''>",
     "budget": "<explicitly authorized experiment/compute/spend constraints only>",
     "acceptance_criteria": "<measured criteria that gate propose vs discard>",
     "context": "<the self-improvement framing from Outer Kata step 2>"
   }}
   ```
   Respond with a JSON object containing `pathway` (Σ), `scaffold_component` (p, m, 𝒯, or Σ), `signal_type` (𝒟_t, e_t, or τ_t), `improvement_plan`, `budget`, `acceptance_criteria`, and `variety_check` (on PDCA iteration 2+: the component+signal combinations already attempted and the justification for the current selection when `repeated = 1`).

### si-agenda-generator (Plan-step extension — the experiment-designer loop)

When improvement work has no single named target — the operator asked for self-improvement generally, or regulation signals are accumulating — the Plan step extends into an agenda generator (repair plan §P8.5 Phase 3). It replaces hand-assembling experiments from noticed problems: signals are read mechanically, candidates are designed per the experiment protocol (`experimentation/experiment-protocol`), and the prioritized queue lives in the experimentation registry as `proposed` experiments.

1. **Read signals and compute calibration guard (`SI-3`, D).** Skill-use issues: `curator_semantic_search` over recent issue fragments. Escalations: `curator_algedonic_log`. Variety deficits: `curator_status` with `include_variety: true`. Registry history: `population_query` (all experiments by status) plus `lineage_read` per resolved experiment for its fitness and selection records; compute each non-void resolved experiment's prediction Brier (`(confidence - outcome)^2`) and the demonstrated hit rate with `lisp_eval` (`SI-3` — descriptive agenda hit-rate check, not probability calibration):
   - form: `(let ((n (length outcomes))) (if (= n 0) (list nil 0) (begin (define sum-list (lambda (xs) (if (is_null xs) 0 (+ (car xs) (sum-list (cdr xs)))))) (list (/ (sum-list outcomes) n) n))))`
   - env: `{ "outcomes": <array of 1/0 for non-void resolved claims> }`
   Voids — `no-headroom` (design failure: the eval set cannot discriminate) and `measurement-void` (infrastructure failure: the runs never produced a valid measurement) — never tested their claim; exclude them from `outcomes`.
2. **Design and prioritize (P).** Render `self-improvement/si-agenda-generator` with `signals`, `registry_history` (with computed Briers and `SI-3` hit rate), `budget`, and `context`:
   ```json
   {"template_ref": "self-improvement/si-agenda-generator", "context": {
     "signals": "<skill-use issues, escalations and variety deficits read mechanically this cycle>",
     "registry_history": "<population_query + lineage_read records with computed prediction Briers and the SI-3 hit rate>",
     "budget": "<the cycle's declared spend ceiling>",
     "context": "<the operator's request and the agenda-generator framing>"
   }}
   ```
   The rendered prompt designs candidates — each with hypothesis, layer, genotype refs, eval set, fitness function, pre-registered prediction, and budget — ranked by signal strength × expected information ÷ cost, with confidences calibrated against the `SI-3` hit rate.
3. **Register the queue (D).** Declare each agenda candidate with `experiment_propose` (the registry is the queue: status `proposed`; a stable `experiment_key` converges retried calls). The declaration carries `budget.max_runs` — a positive integer the server enforces at `fitness_record` (§P8.9 step 2: recording past the ceiling is a typed `budget_exhausted` refusal, never a silent overshoot) — and `linked_goal_id`, the kanban goal created for the candidate (Constraints: every improvement cycle registers a falsifiable outcome claim before Do; the goal tracks verification separately from the experiment prediction — the join does not create an independent forecast). The pre-registered prediction is the controller — never edited after measurement begins.
4. **Headroom pre-check (D, budgeted auto-run §P8.7-Q6/Q7).** Before any challenger spend, register the baseline variant and run it alone through the deterministic harness within the declared budget. Baseline `overall_pass_rate` = 1.0 → the eval set has no headroom: record `selection_record` verdict `rejected` with the no-headroom reason, redesign the eval set (harder discriminators — conservation checks, exact labels — never weaker evaluators), and re-declare as a new experiment. The void rejection's first reject reason MUST contain the literal token `no-headroom` — the calibration record excludes voids mechanically on that marker (§P8.9 step 1). Baseline < 1.0 → headroom exists: proceed to Vary and the normal PDCA flow, the registry as the record path (the GEPA sub-loop for prompt artifacts; the five-step protocol otherwise).
5. **File the cycle (P, gated by the D verdict).** Each experiment that reaches a verdict gets one card on **Algedonic review** (the Proposal card handoff): hypothesis, pre-registered prediction, measured fitness, verdict, computed Brier, and — for a selected challenger — the proposed diff. The card also lists every discarded signal with its reason (INV4: a discard is recorded, never vanished), and a signal discarded in two consecutive cycles escalates as its own algedonic card — a recurring discard is a model gap, not noise. The operator's gemba walk is the algedonic decision; nothing auto-adopts. `selection_record` closes the experiment with `algedonic_reference` set to the review card; for a selected challenger the card carries the commit hash once the adoption lands in git, so `lineage_read` traces genotype → verdict → card → commit → deployed artifact (§P8.9 step 4). An infrastructure-voided measurement (rollouts errored with no model output — 0 tokens) closes the experiment the same way: verdict `rejected` with the first reject reason carrying the literal token `measurement-void`, so the never-tested claim is excluded from the calibration record on that marker and the fossil states the claim was neither held nor refuted — never a mechanism refutation. Re-dispatching past the declared `max_runs` ceiling to salvage a voided measurement is a silent overshoot (D-1): the void closes the experiment, and re-testing the mechanism is a fresh declaration (E4 precedent 2026-10-01: 7/9 challenger rollouts died at the provider under concurrent dispatch; 2 valid rollouts both passed).

### si-execute-improvement (PDCA Do)

1. Route deterministically using the `SI-1` `lisp_eval` route result from `si-select-pathway` (step 5) — no LLM router template is called.
2. Render the selected sub-pathway template with `improvement_plan`, `current_config`, and `context`:
   - **Scaffolding Improvement**:
     - `si-exec-scaffold-prompt.j2` (§6.1): Apply one of four paradigms (scalar/qualitative/experimentation/textual-gradient). For population-based experimentation with a Pareto frontier, run the "Prompt experimentation (GEPA)" sub-loop below.
     - `si-exec-scaffold-memory.j2` (§6.2): Apply signal-driven CRUD operations (Create/Read/Update/Delete).
     - `si-exec-scaffold-tool.j2` (§6.3): Apply dynamic tool routing, iterative refinement, or autonomous creation.
     - `si-exec-scaffold-full.j2` (§6.4): Treat entire scaffold as mutable program, generate patches, gate through verifier. Delegates to `diagnose` for reproduce→hypothesize→fix loops.
   
   ```json
   {"template_ref": "<the SI-1 route result, e.g. self-improvement/si-exec-scaffold-prompt>", "context": {
     "improvement_plan": "<the improvement_plan from si-select-pathway step 6>",
     "current_config": "<the configuration being improved>",
     "context": "<the session's framing>"
   }}
   ```
3. Multi-signal support: if the improvement plan specifies multiple signal types, execute them in canonical deterministic order (`D_t` → `e_t` → `tau_t`).
4. Capture the full execution trace: what was generated, what was filtered, what was updated, what was the cost.
5. Respond with a JSON object containing `updated_config`, `execution_trace`, `cost_breakdown`, and `proposed_artifact` (the candidate update before gating).

### si-evaluate-improvement (PDCA Check)

1. Evaluate the updated agent on a held-out evaluation distribution 𝒟_eval that does NOT overlap with the improvement signal. **Fallback**: If no held-out set is available, use cross-validation or temporal split. If neither is available, set `evaluation_method: "none_available"`; the Act gate then discards the candidate.
2. Render `self-improvement/si-evaluate-improvement` with `updated_config`, `improvement_plan`, `current_condition`, and `context`:
   ```json
   {"template_ref": "self-improvement/si-evaluate-improvement", "context": {
     "updated_config": "<the candidate configuration after Do>",
     "improvement_plan": "<the improvement_plan from si-select-pathway step 6>",
     "current_condition": "<the Kata step 2 baseline object>",
     "context": "<the session's framing>"
   }}
   ```
   Report the full performance trajectory (m_t) across update iterations, not just the final peak score.
3. Test transfer beyond the improvement signal: does the improvement generalize to held-out tasks?
4. Track regressions: did the update break previously solved tasks?
5. Account for resource efficiency: compute cost, API tokens, wall-clock time, human input.
6. Track safety: any safety policy violations, goal drift, or reward hacking indicators?
7. If using a judge-based evaluator (Φ_judge), ensure evaluator independence: use a distinct judge configuration for final reporting.
8. Respond with a JSON object containing `performance_trajectory`, `baseline_pass_rate`, `pass_rate`, `regressions`, `transfer_score`, `cost_summary`, `safety_violations`, and `evaluation_method` (metric-based, judge-based, or none_available).

### Proposal card handoff (all improvement pathways)

Use only **Algedonic review** for proposals. `kanban_board_list` must succeed before treating the board as absent: use the unique matching board ID, create it with `kanban_board_create` (name `Algedonic review`, default columns, shared `idempotency_key: "algedonic-review-board"`) if none matches, or stop on ambiguous matches. For `kanban_task_create`, supply that board ID, a title identifying the target and change, a description with the full proposed diff (or the complete prompt change), evidence and its limits, and verification criteria; set `criteria` to observable checks, `advances: []` unless a known goal criterion is cited, and reuse the same `idempotency_key` on retries. Keep the returned card ID and attach existing artifacts with `kanban_task_add_deliverable` when relevant. A failed lookup or write blocks filing; never create a file proposal or dated review note.

### si-propose-or-discard (PDCA Act)

The executing session never commits a durable change to a skill, prompt, memory, tool configuration or model — that would make the session the judge of its own work (Goodhart's law; operator ruling 2026-09-24). It decides only whether its candidate is worth the operator's review.

1. Call `lisp_eval` on the measured Check result: form `(and (not (member evaluation_method (list "none_available"))) (numberp pass_rate) (numberp baseline_pass_rate) (> pass_rate baseline_pass_rate) (= regressions 0) (= (length safety_violations) 0))` (`member` is the string-equality primitive). `baseline_pass_rate`/`pass_rate` are the `overall_pass_rate` values the session computes with `lisp_eval` from the recorded before/after spawn-executor report pairs on the same task set; `regressions` counts tasks whose after `tasks[].pass_rate` fell below baseline, computed with `lisp_eval` from the two reports' per-task pass rates written as ordered number lists in the form: `(begin (define regs (lambda (b a) (if (is_null b) 0 (+ (if (< (car a) (car b)) 1 0) (regs (cdr b) (cdr a)))))) (regs (list b1 b2 ...) (list a1 a2 ...)))`. The invoking agent substitutes this computed count for the Check result's `regressions: null` before running the gate form — the gate env's `regressions` is always the computed count, never the template's null. A missing report means null values and a false gate. A baseline `overall_pass_rate` of 1 is saturated: no candidate can pass, so the discard carries no information — record `failure_mode: "saturated baseline"` and choose a harder held-out task set before re-planning (observed 2026-09-26, `local_extractor` harness runs `bac52690`/`b950572f`).
2. Render `self-improvement/si-propose-or-discard` with `evaluation_result`, `gate_result` (the `lisp_eval` boolean), `improvement_plan` and `proposed_artifact`:
   ```json
   {"template_ref": "self-improvement/si-propose-or-discard", "context": {
     "evaluation_result": "<the Check result, with the lisp_eval-computed regressions count substituted for its null>",
     "gate_result": "<the lisp_eval boolean from step 1>",
     "improvement_plan": "<the improvement_plan from si-select-pathway step 6>",
     "proposed_artifact": "<the candidate update from Do, not applied>"
   }}
   ```
3. `gate_result` true → **propose**: follow the Proposal card handoff above with a title naming the skill/component and change; include the full diff, measurements, evaluation method, goal ID, evidence and verification criteria in its description. If the board or card cannot be created, report filing blocked; do not write a proposal file or use another queue. Nothing is applied; the gemba walk decides it with the operator.
4. `gate_result` false → **discard**: keep the configuration unchanged, record the failure mode (noisy signal, misaligned operator, missing harness) in the Kata obstacle parking lot, and re-plan.
5. Judge the registered goal (`kanban_goal_judge`) with the measured results; native human closure and independent assessment remain separate.
6. Respond with `decision` ("propose", "discard", or "blocked"), `proposal_card_id` (only after card creation succeeds), `failure_mode` (if discarded or blocked), and `next_step` ("re-enter", "exit", or "refine").

### Prompt experimentation (GEPA) — a scaffolding sub-loop (formerly gpa-experimentation)

For a prompt artifact with a runnable eval set, when natural-language reflection on real trajectories should drive the change (Agrawal et al., GEPA, arXiv:2507.19457; NSGA-II non-dominated sorting, Deb et al. 2002 — `onto_anchor` → derived `reflective_prompt_evolution`). Prompts only (v1). Reflection and mutation are P, critiqued by recorded eval-set scores; trajectory assembly, variant score aggregation, dominance, frontier membership and the convergence form are D.

Executor binding (local prompt artifacts): the eval set is a list of `{task, evaluator: {evaluator, spec}}` entries (contains/not_contains/regex); the measured objective is `pass_rate` (maximize). Each candidate prompt runs as a one-shot sub-agent (`spawn_agent` — `label` the variant id; `message` = the candidate prompt plus the task; the returned `output` is the response); the orchestrating session scores each response with the task's evaluator via `lisp_eval` (`string-contains` needle-first for contains, negated for not_contains, the regex spec for regex) and records the `{source, input, output, scores: {pass_rate: 1|0}}` pairs. A spawn error output scores 0 for that task and is recorded as such. Attribute each response to its task by running the per-task spawns sequentially, or by recording each call's returned session_id — parallel spawn batches do not guarantee result-order attribution (observed live in the redesign smoke run 2026-10-29). Cost is NOT measured by this executor — `spawn_agent` returns response text only, no token usage or latency — so `total_tokens` and `mean_latency_ms` are recorded as unmeasured: in the frontier form's env every variant's `total_tokens` slot carries one shared constant (named in the iteration log), cost ties under the 10% band and the frontier is pass_rate-ordered; the proposal must disclose cost-unmeasured, and per-variant token differences must never be invented. Never reconfigure the target artifact. The proposal card on Algedonic review is the durable review record.

Split the eval set before iteration 1 into a **feedback set** (Sample and Reflect read only these tasks) and a disjoint **selection set** (Test scores only these), as GEPA separates D_feedback from D_pareto. Fix both — and the frontier cap `frontier_size` (the maximum non-dominated members kept per iteration; at least 2, so pruning never removes both boundary objectives) — for the session; the proposal records them. A task shown to Reflect or used to write a variant never enters the selection set.

Reflect and Propose are P steps executed by an agent: send the rendered template to a sub-agent with `spawn_agent` (label names the step) and use its returned `output` as the JSON. The orchestrating session only copies each returned variant's `content` into a card, verbatim; it never writes, edits or selects variant text.

Registry recording (repair plan §P8 — the single record path, §P8.7-Q5): the experimentation server is the record of record for this sub-loop. Before iteration 1, declare the run with `experiment_propose` (layer `prompt_template`; genotype_refs the target artifact; eval_set the feedback/selection split; fitness_fn the pass_rate and total_tokens objectives; noise_band the 10% cost band; prediction the pre-registered claim with confidence; budget the session's spend ceiling; experiment_key a stable run key), and register every proposed variant with `variant_register` (genotype_config the variant descriptor, parent_variant_id its parent, variant_key the variant id). Per-run log directories are not written.

1. **Sample (D)** — for the target (iteration 1) or each frontier member (2+), call `spawn_agent` per feedback-set task (message = that prompt plus the task) and score the returned `output` with the task's evaluator via `lisp_eval`; assemble the recorded `{source, input, output, tool_calls, scores: {pass_rate: 1|0}}` pairs directly as `trajectories` (no LLM wrapper template; `tool_calls` is recorded as `[]` — `spawn_agent` returns response text only). No executor or evaluator → stop and report `unverified`.
2. **Reflect (P)** — render `self-improvement/gpa-reflect` with `artifact_type`, `trajectories`, and `objectives` to diagnose why each outcome was poor or good and extract transferable rules. Pass its whole output object to Propose as `reflections`:
   ```json
   {"template_ref": "self-improvement/gpa-reflect", "context": {
     "artifact_type": "<'prompt' — prompts only (v1)>",
     "trajectories": "<the recorded {source, input, output, tool_calls, scores: {pass_rate: 1|0}} pairs assembled in Sample>",
     "objectives": "<the executor-binding objectives: pass_rate maximize, total_tokens minimize, mean_latency_ms minimize>"
   }}
   ```
3. **Propose (P)** — render `self-improvement/gpa-propose-mutations` with `artifact_type`, `reflections`, `current_frontier`, and `target_artifact` to generate 3–7 variants: mutation (one lesson, one hypothesis "if I change X, Y improves because Z") and crossover (complementary frontier members); tag parent, operator, hypothesis and rule; carry full content:
   ```json
   {"template_ref": "self-improvement/gpa-propose-mutations", "context": {
     "artifact_type": "<'prompt' — prompts only (v1)>",
     "reflections": "<the whole gpa-reflect output object>",
     "current_frontier": "<the capped non-dominated [id, pass_rate, total_tokens] triples from the last SI-2 update>",
     "target_artifact": "<the prompt being improved>"
   }}
   ```
4. **Test (D)** — run every variant, and the target itself, through `spawn_agent` on the identical selection set and repeats; the session scores each response with its evaluator and computes `overall_pass_rate` and the per-task pass rates with `lisp_eval`; each variant's recorded report is its `{input, output, scores}` rows plus the computed pass rates. Record every variant's measured scores with `fitness_record` (experiment_id and variant_id from the declaration; runs the recorded spawn report references — never simulated; run_count the number of rollouts those reports cover — the server sums run counts against the declared `budget.max_runs` and refuses past the ceiling with a typed `budget_exhausted` error, §P8.9 step 2; scores the per-objective aggregates from those reports only).
5. **Update frontier (`SI-2`, D)** — merge `current_frontier` with the newly tested variants and compute the non-dominated Pareto frontier and `new_members` count deterministically with `lisp_eval` (`SI-2` — GEPA score aggregation + Pareto frontier update; no LLM frontier-updater template). A cost objective (`total_tokens`, `mean_latency_ms`) counts as strictly better only when it is more than 10% lower, otherwise the two are tied on it — paired same-set runs on 2026-09-26 differed by 6–7% in tokens (789/848, 1676/1569), so a smaller gap is noise. Cost comparisons are valid only between runs on the same selection set. The v1 form below covers `(id pass_rate total_tokens)` triples; `mean_latency_ms` is measured and reported per variant but is not a dominance participant in this form:
   - form: `(begin (define better-cost (lambda (a b) (< a (* 0.9 b)))) (define dom (lambda (a b) (and (>= (nth 1 a) (nth 1 b)) (not (better-cost (nth 2 b) (nth 2 a))) (or (> (nth 1 a) (nth 1 b)) (better-cost (nth 2 a) (nth 2 b)))))) (define dominated-by-any (lambda (x pool) (if (is_null pool) nil (if (and (not (string= (car x) (car (car pool)))) (dom (car pool) x)) t (dominated-by-any x (cdr pool)))))) (define filter-nd (lambda (rem full) (if (is_null rem) (list) (if (dominated-by-any (car rem) full) (filter-nd (cdr rem) full) (cons (car (car rem)) (filter-nd (cdr rem) full)))))) (define count-new (lambda (nd prev) (if (is_null nd) 0 (+ (if (member (car nd) prev) 0 1) (count-new (cdr nd) prev))))) (let ((nd (filter-nd pool pool))) (list nd (count-new nd prev_ids))))`
   - env: `{ "pool": <list of [id, pass_rate, total_tokens] triples for current_frontier ∪ tested_variants — the tested triples copied field-for-field from the recorded reports (agent_name, overall_pass_rate, total_tokens)>, "prev_ids": <list of id strings in current_frontier> }`
   If the non-dominated set exceeds `frontier_size`, cap it with a second `lisp_eval` call (`SI-2` crowding cap — deterministic; no LLM pruner). The form sorts the non-dominated chain by `pass_rate` (ties by `total_tokens`), gives both boundary members infinite crowding distance, and repeatedly removes the interior member with the smallest normalized neighbor gap (first index wins ties) until the set fits — the extremes (best `pass_rate`, best cost) always survive. Record which variant dominated which in the iteration log; `new_members` stays the main form's pre-cap count (an arrival that is capped still moved the frontier):
   - form: `(begin (define le (lambda (a b) (if (= (nth 1 a) (nth 1 b)) (<= (nth 2 a) (nth 2 b)) (< (nth 1 a) (nth 1 b))))) (define insert (lambda (x xs) (if (is_null xs) (list x) (if (le x (car xs)) (cons x xs) (cons (car xs) (insert x (cdr xs))))))) (define isort (lambda (xs) (if (is_null xs) (list) (insert (car xs) (isort (cdr xs)))))) (define fmax (lambda (i xs) (if (is_null (cdr xs)) (nth i (car xs)) (max (nth i (car xs)) (fmax i (cdr xs)))))) (define fmin (lambda (i xs) (if (is_null (cdr xs)) (nth i (car xs)) (min (nth i (car xs)) (fmin i (cdr xs)))))) (define rm (lambda (k xs) (if (= k 0) (cdr xs) (cons (car xs) (rm (- k 1) (cdr xs)))))) (define cd (lambda (i s n rpr rtok) (if (or (= i 0) (= i (- n 1))) 1000000000 (+ (if (= rpr 0) 0 (/ (- (nth 1 (nth (+ i 1) s)) (nth 1 (nth (- i 1) s))) rpr)) (if (= rtok 0) 0 (/ (- (nth 2 (nth (+ i 1) s)) (nth 2 (nth (- i 1) s))) rtok)))))) (define fm (lambda (i best s n rpr rtok) (if (>= i (- n 1)) best (if (< (cd i s n rpr rtok) (cd best s n rpr rtok)) (fm (+ i 1) i s n rpr rtok) (fm (+ i 1) best s n rpr rtok))))) (define prune (lambda (s cap) (if (<= (length s) cap) s (let ((n (length s))) (let ((rpr (- (fmax 1 s) (fmin 1 s))) (rtok (- (fmax 2 s) (fmin 2 s)))) (prune (rm (fm 1 1 s n rpr rtok) s) cap)))))) (let ((sorted (isort nd))) (prune sorted frontier_size)))`
   - env: `{ "nd": <the non-dominated [id, pass_rate, total_tokens] triples returned by the form above>, "frontier_size": <the session's fixed cap from the eval-set split, at least 2> }`
6. **Check (D)** — no tool computes hypervolume, so report it `unverified`; call `lisp_eval` with `(and (>= iteration 2) (= new_members 0))` over the measured count of variants that entered the frontier this iteration; converged only when an iteration adds no new non-dominated member (a single arrival means the frontier is still moving). Minimum 2, maximum 5 iterations per session.
7. **Act** — never adopt. Follow the Proposal card handoff above with a title naming the target and a description containing the proposed prompt diff, frontier content, measured scores, cost, lineage, eval-set identity, evidence and verification criteria. Report a blocked filing if the board or card cannot be created; never use a file fallback. The operator chooses on that card in the gemba walk. Record the outcome with `selection_record` when the card verdict is known — verdict `selected` with the proposed variant on acceptance, `rejected` with reasons otherwise, `algedonic_reference` the card ID; the deciding session writes it, and a session that files and exits without a verdict leaves the experiment `running` (its fitness and lineage are already recorded).

## Improvement Measure

PDCA loops in this skill run within one session (operator ruling 2026-09-24). The signal is the measured Check result's `pass_rate` (the same value the step-1 gate reads, from the deterministic harness). After each full iteration compute with `lisp_eval` over the recorded pass rates, oldest first: `(and (>= (length xs) 3) (< (abs (- (nth (- (length xs) 1) xs) (nth (- (length xs) 2) xs))) 0.02) (< (abs (- (nth (- (length xs) 2) xs) (nth (- (length xs) 3) xs))) 0.02))` — converged when each adjacent pair among the last three measured pass rates differs by less than 0.02 (the form checks adjacent differences, not the max-min spread). An iteration with no measured pass rate adds nothing to the list; it cannot move the loop toward convergence. Convergence is checkable once three measured pass rates are recorded — the form requires `(>= (length xs) 3)` and two adjacent-pair checks, so two rates can never converge (verified live against the form, the deep-module DM1 class).

**Max iterations (per session)**: 10 (outer Kata), 5 (inner PDCA per Kata step). A target the session cannot reach within those bounds is reported with its remaining gap, not extended.

## Safety Governance

The skill implements the paper's safety recommendations (Section 9.1):

1. **Verifier-gated updates**: Before any structural update is committed to Σ_{t+1} or θ_{t+1}, the proposed patch must pass verifier-gated checks covering functional correctness, tool permission boundaries, and robustness to random state perturbations.
2. **Critic decoupling**: The critic (evaluator) is decoupled from the generator. If the agent conflates the roles of proposing updates and accepting them, it collapses into a self-confirming loop. In zed-kask the acceptor is the operator in the algedonic review; this skill only proposes.
3. **Layered gating**: A strict permission system for self-modification. Improvement is only permitted within explicitly defined and continuously audited safety boundaries.
4. **Version history for rollback**: The scaffold maintains version history (Σ_{1:t}) to support validation and rollback against harmful modifications.
5. **Fast-to-slow consolidation**: Scaffold-level improvements (fast, reversible) are validated through rigorous execution tests. Parametric consolidation (slow, hard to trace) is out of scope — the paper's slow path has no executor here.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `si-select-pathway.j2` | Select the scaffold component and improvement signal from the current Kata state and available resources. |
| `si-agenda-generator.j2` | Plan-step extension: design and prioritize the experiment agenda from regulation signals and registry history, with pre-registered predictions and headroom-aware eval sets. |
| `si-evaluate-improvement.j2` | Report measured before/after harness pass rates, transfer, cost and safety for the Act gate (`regressions` is emitted null — the invoking agent computes it with `lisp_eval`); no verdict or Brier score. |
| `si-propose-or-discard.j2` | From the measured evaluation and the deterministic gate result, either file a proposal for the algedonic review or discard the candidate; never commit. |
| `si-exec-scaffold-full.j2` | Scaffolding Improvement pathway — update the full scaffold (prompt, memory, tool configuration) based on the improvement evaluation. |
| `si-exec-scaffold-memory.j2` | Scaffolding Improvement pathway — update the agent's memory configuration based on the improvement evaluation. |
| `si-exec-scaffold-prompt.j2` | Scaffolding Improvement pathway — update the agent's system prompt based on the improvement evaluation. |
| `gpa-reflect.j2` | GEPA sub-loop step 2: natural-language diagnosis and transferable rules. |
| `gpa-propose-mutations.j2` | GEPA sub-loop step 3: mutation and crossover variants, one hypothesis each. |
| `si-exec-scaffold-tool.j2` | Scaffolding Improvement pathway — update the agent's tool configuration based on the improvement evaluation. |

To render a template, call the `render_template` tool with the template ref (e.g., `self-improvement/si-select-pathway`) and a context object with the required variables.

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, backfill pass),
over the fitness gate:

- Harness-measured 0.8 over baseline 0.6, zero regressions, zero safety
  violations → `true`.
- Same with one regression → `false` — an improvement that regresses
  anything is not an improvement.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- The executable pathway is Scaffolding Improvement (Σ); Foundation Model fine-tuning is out of scope and has no executor.
- No durable change is committed by the executing session. A candidate that passes the deterministic gate becomes a card on **Algedonic review**; only the operator in the gemba walk accepts it. No proposal files or dated review notes.
- The configuration in use stays unchanged until the operator accepts a proposal, so no rollback of self-applied changes is needed.
- The critic (evaluator) must be decoupled from the generator to prevent self-confirming loops.
- Token budgets are not an implicit improvement gate. A `budget` record may describe explicitly approved experiment/time/compute/spending constraints, never an agent-invented token quota. Observed token use neither authorizes truncation nor a model downgrade.
- Sub-agent execution inherits the session's configured models (Settings → Kask → Models), including cloud models. A spawned executor never authorizes a smaller/local model downgrade. A `spawn_agent` model override requires operator approval (the tool's `model` parameter, native model ids only); when approved routing is unavailable, stop the experiment rather than substitute a model.
- Every improvement cycle registers a falsifiable outcome claim before PDCA Do: `kanban_goal_create` with the acceptance criteria as observable criteria and the actual specification source (omit routine forecast), and the executing task links the goal via `advances`. `si-propose-or-discard` judges the goal (`kanban_goal_judge`) with the measured results — an improvement with no registered claim is a process violation, not an improvement.
- Scaffold-side updates (p, m, 𝒯, Σ) require a before/after measurement through a deterministic harness (the spawn executor: `spawn_agent` runs whose responses the session scores via `lisp_eval` contains/not_contains/regex — not shell/file evaluators) and a `lisp_eval` gate — improved pass rate at zero regressions — before `si-propose-or-discard` may file a proposal. Deterministic response scoring alone is not ground truth, which is why the operator decides. When no suitable deterministic harness exists, `evaluation_method: "none_available"` makes the gate false and the candidate is discarded (si-evaluate-improvement step 1).
- Max iterations: 10 (outer Kata), 5 (inner PDCA per Kata step).
- Evaluate convergence after each full iteration: the iterates have stopped moving. Converged when stable across 3 iterations. Minimum 2 iterations.
- `decision` field must be exactly "propose", "discard", or "blocked" (lowercase). "blocked" is only for a candidate whose gate passed but whose proposal card could not be filed — never for a gate failure, which is "discard".
- `next_step` field must be exactly "re-enter", "exit", or "refine" (lowercase).
- `signal_type` may be a single value or an array for multi-signal support.
- Variety engineering: PDCA iteration 2+ must check for repeated pathway/signal combinations and justify or diversify.
- Delegation: `si-exec-scaffold-prompt.j2` routes population-based experimentation to the Prompt experimentation (GEPA) sub-loop and its `gpa-*` templates. `si-exec-scaffold-full.j2` delegates to `diagnose` for debugging loops.
