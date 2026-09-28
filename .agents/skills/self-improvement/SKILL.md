---
name: self-improvement
description: "General self-improvement skill for FM-based agents. Drives persistent, endogenous adaptation across Foundation Model Improvement and Scaffolding Improvement via intrinsic demonstrations, evaluative feedback, and extrinsic exploratory experience. Runs the verifier-gated LoRA fine-tuning loop (rollout baseline, verdict-bridged dataset, gated submit, held-out adapter evaluation, feedback retrain) and GEPA prompt evolution; files proposals, never commits."
---

# Self-Improvement

General multi-purpose self-improvement skill for FM-based agents. Implements the unified self-induced update operator from Ren et al. (2026, arXiv:2607.13104). The skill drives persistent, endogenous adaptation across two pathways — Foundation Model Improvement (θ) and Scaffolding Improvement (Σ) — through intrinsic generative demonstrations, intrinsic evaluative feedback, and extrinsic exploratory experience. It embeds its own PDCA loops at the improvement-cycle level and wraps an outer Improvement Kata loop around all algorithmic approaches, following the Toyota Improvement Kata (direction → current condition → target condition → experiment).

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

Two pathways instantiate 𝒰:
1. **Foundation Model Improvement** (Section 5 of the paper): `θ_{t+1} = 𝒰_θ(θ_{1:t}; 𝒮_t)`, `Σ_{t+1} = Σ_t`
2. **Scaffolding Improvement** (Section 6 of the paper): `Σ_{t+1} = 𝒰_Σ(Σ_{1:t}; 𝒮_t)`, `θ_{t+1} = θ_t`

Three signal forms drive both pathways:
- **Intrinsic Generative Demonstrations** (`𝒮_t ≈ 𝒟_t`): agent synthesizes training instances
- **Intrinsic Evaluative Feedback** (`𝒮_t ≈ e_t`): agent judges candidate behavior
- **Extrinsic Exploratory Experience** (`𝒮_t ≈ τ_t`): agent collects interaction trajectories

## When to Use

- When an agent needs to durably modify its own configuration (prompts, memory, tools, control logic, or model parameters) based on execution experience
- When you need to select the appropriate self-improvement pathway (FM improvement vs. scaffolding improvement) based on update target, signal availability, and resource constraints
- When you need to select the appropriate improvement signal (intrinsic demonstrations, intrinsic evaluations, or extrinsic experience) based on what the environment provides
- When you need to run a structured PDCA improvement cycle with convergence detection and rollback safety
- When you need to wrap an outer Improvement Kata loop around multiple improvement cycles to drive long-term capability gains
- When you need to evaluate self-improvement claims rigorously (trajectory tracking, transfer testing, regression checks, cost accounting)
- When you need to govern self-modification safely (verifier-gated updates, layered permission systems, critic decoupling)
- When an agent's measured pass rate is poor and the operator wants a fine-tuned adapter, verdict-labeled rollouts should become training data, or a trained adapter needs held-out evaluation or a feedback retrain (the Fine-tuning run section)

## When NOT to Use

- One-off corrections — if the fix is known, apply it directly; a PDCA cycle around a known fix is ceremony.
- Skill authoring and maintenance — use `create-skill` / `skill-maintenance` (the scaffolding-improvement pathway delegates there anyway).
- Memory curation — use `therapy`; reorganizing memory is not self-modification of configuration.
- Choosing the PEFT configuration — use `lora-training`; the Fine-tuning run section below executes only an operator-accepted config.

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
│  INNER LAYER: Signal-Specific Algorithm (per-pathway)            │
│  FM Improvement:                                                  │
│    §5.1 Intrinsic Generative Demos (generate → filter → fine-tune)│
│    §5.2 Intrinsic Evaluative Feedback (sample → judge → optimize) │
│    §5.3 Extrinsic Exploratory Experience (interact → reward → RL) │
│  Scaffolding Improvement:                                         │
│    §6.1 Prompt Optimization (scalar/qualitative/evolution/gradient)│
│    §6.2 Memory Evolution (CRUD: Create/Read/Update/Delete)        │
│    §6.3 Tool Governance (routing/refinement/creation)            │
│    §6.4 Full Scaffolding (self-referential code rewrite)          │
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
proposals, and the Act-gate decision are P — judgment, critiqued by the
measured Check results and by the operator in the algedonic review (the
executing session never commits its own improvements). The gates are D:
the propose-or-discard acceptance predicate, the regression count, the GEPA
dominance and convergence computations, the noise-floor standard error,
and the PDCA convergence form all run in `lisp_eval` over measured
harness reports (`swarm_eval_agent_local`, `training_evaluate`) — a
candidate that fails the gate is discarded regardless of the model's
assessment. Harness evaluation is D (deterministic
contains/not_contains/regex evaluators); filing a proposal is P, gated by
the D verdict.

## Instructions

### Outer Kata Steps 1–3 (rendered from `kata-improvement`)

The outer Kata uses the `kata-improvement` step templates directly; this skill supplies the self-improvement framing in `context`, and never restates the procedure.

1. Render `kata-improvement/improvement-step1-direction`, `kata-improvement/improvement-step2-current`, and `kata-improvement/improvement-step3-target` in order, passing each step's output in `previous_steps`.
2. In `context`, state the self-improvement framing (Ren et al., 2026, arXiv:2607.13104): the agent configuration 𝒜_t = (θ_t, Σ_t), where θ_t is the foundation model and Σ_t = (p_t, m_t, 𝒯_t, g_t) is the scaffold (prompts, memory, tools, control logic); the challenge is the capability gap the update operator 𝒰 should close.
3. Step 2 (current condition) must describe the current θ_t and Σ_t and include cost metrics (compute, tokens, wall-clock time, human input) and safety metrics (regression rate, safety violations, goal-drift indicators) alongside performance metrics; record `metric_before`.
4. Step 3 (target) counts its horizon in this session's bounded PDCA experiments (at most 5 per Kata step), not calendar time (operator ruling 2026-09-24); record `metrics_target`.

### si-select-pathway (PDCA Plan)

1. Determine the update target: Foundation Model (θ) or Scaffolding (Σ).
   - Default to Scaffolding (Σ) unless: (a) FM fine-tuning is explicitly permitted, (b) the capability gap requires parametric consolidation, (c) scaffold-level improvements have plateaued and the gap is in the model's internal representations.
   - The paper notes: "Modifying the operational scaffold (Σ) drives a fast adaptation loop... parameter updates (θ) are much slower" (Section 9.1).
2. If Scaffolding (Σ): determine which component to update — Prompt (p), Memory (m), Tool (𝒯), or Full Scaffolding (Σ).
   - Prompt: when the bottleneck is task communication, objectives, or constraints.
   - Memory: when the bottleneck is long-horizon recall, cross-context knowledge transfer, or context-window pressure.
   - Tool: when the bottleneck is action execution, capability coverage, or tool reliability.
   - Full Scaffolding: when the bottleneck requires holistic reconfiguration of perception, reasoning, and execution.
3. Determine the improvement signal: Intrinsic Generative Demos (𝒟_t), Intrinsic Evaluative Feedback (e_t), or Extrinsic Exploratory Experience (τ_t).
   - Intrinsic Demos: when the agent can synthesize high-quality training instances from its own priors.
   - Intrinsic Feedback: when the agent can judge candidate behavior through rubrics, consistency, or critique.
   - Extrinsic Experience: when the environment provides grounded feedback (unit tests, task success, rewards).
4. Generate a concrete improvement plan: what operator 𝒰 will be applied, what signal 𝒮_t will drive it, what experiment scope and explicitly authorized resource constraints apply, and what acceptance criteria will gate the update. Do not invent token quotas; token usage is measured, not budgeted.
5. Respond with a JSON object containing `pathway` (θ or Σ), `scaffold_component` (if Σ: p, m, 𝒯, or Σ), `signal_type` (𝒟_t, e_t, or τ_t), `improvement_plan`, `budget`, and `acceptance_criteria`.

### si-execute-improvement (PDCA Do)

1. Route to the appropriate sub-pathway template based on the pathway, scaffold component, and signal type selected in the improvement plan.
2. The router (`si-execute-improvement.j2`) delegates to one of 7 sub-pathway templates:
   - **FM Improvement**:
     - `si-exec-fm-demos.j2` (§5.1): Generate training instances, apply quality control, fine-tune via gradient descent, safeguard against model collapse.
     - `si-exec-fm-feedback.j2` (§5.2): Sample candidate outputs, apply intrinsic evaluator, convert to update signal, optimize via RL/DPO/critique-conditioned fine-tuning.
     - `si-exec-fm-experience.j2` (§5.3): Collect interaction trajectories from grounded or simulated environments, update via PPO/DPO.
   - **Scaffolding Improvement**:
     - `si-exec-scaffold-prompt.j2` (§6.1): Apply one of four paradigms (scalar/qualitative/evolution/textual-gradient). For population-based evolution with a Pareto frontier, run the "Prompt evolution (GEPA)" sub-loop below.
     - `si-exec-scaffold-memory.j2` (§6.2): Apply signal-driven CRUD operations (Create/Read/Update/Delete).
     - `si-exec-scaffold-tool.j2` (§6.3): Apply dynamic tool routing, iterative refinement, or autonomous creation.
     - `si-exec-scaffold-full.j2` (§6.4): Treat entire scaffold as mutable program, generate patches, gate through verifier. Delegates to `diagnose` for reproduce→hypothesize→fix loops.
   - All three FM sub-pathways execute through the "Fine-tuning run" section below.
3. Multi-signal support: if the improvement plan specifies multiple signal types, execute them in sequence (demos → feedback → experience).
4. Capture the full execution trace: what was generated, what was filtered, what was updated, what was the cost.
5. Respond with a JSON object containing `updated_config`, `execution_trace`, `cost_breakdown`, and `proposed_artifact` (the candidate update before gating).

### si-evaluate-improvement (PDCA Check)

1. Evaluate the updated agent on a held-out evaluation distribution 𝒟_eval that does NOT overlap with the improvement signal. **Fallback**: If no held-out set is available, use cross-validation or temporal split. If neither is available, set `evaluation_method: "none_available"`; the Act gate then discards the candidate.
2. Report the full performance trajectory (m_t) across update iterations, not just the final peak score.
3. Test transfer beyond the improvement signal: does the improvement generalize to held-out tasks?
4. Track regressions: did the update break previously solved tasks?
5. Account for resource efficiency: compute cost, API tokens, wall-clock time, human input.
6. Track safety: any safety policy violations, goal drift, or reward hacking indicators?
7. If using a judge-based evaluator (Φ_judge), ensure evaluator independence: use a distinct judge configuration for final reporting.
8. Respond with a JSON object containing `performance_trajectory`, `baseline_pass_rate`, `pass_rate`, `regressions`, `transfer_score`, `cost_summary`, `safety_violations`, and `evaluation_method` (metric-based, judge-based, or none_available).

### Proposal card handoff (all improvement pathways)

Use only **Algedonic review** for proposals. `kanban_board_list` must succeed before treating the board as absent: use the unique matching board ID, create it with `kanban_board_create` (name `Algedonic review`, default columns, shared `idempotency_key: "algedonic-review-board"`) if none matches, or stop on ambiguous matches. For `kanban_task_create`, supply that board ID, a title identifying the target and change, a description with the full proposed diff (or the complete adapter/prompt change), evidence and its limits, and verification criteria; set `criteria` to observable checks, `advances: []` unless a known goal criterion is cited, and reuse the same `idempotency_key` on retries. Keep the returned card ID and attach existing artifacts with `kanban_task_add_deliverable` when relevant. A failed lookup or write blocks filing; never create a file proposal or dated review note.

### si-propose-or-discard (PDCA Act)

The executing session never commits a durable change to a skill, prompt, memory, tool configuration or model — that would make the session the judge of its own work (Goodhart's law; operator ruling 2026-09-24). It decides only whether its candidate is worth the operator's review.

1. Call `lisp_eval` on the measured Check result: form `(and (not (member evaluation_method (list "none_available"))) (numberp pass_rate) (numberp baseline_pass_rate) (> pass_rate baseline_pass_rate) (= regressions 0) (= (length safety_violations) 0))` (`member` is the string-equality primitive). `baseline_pass_rate`/`pass_rate` are the `overall_pass_rate` of the recorded before/after `swarm_eval_agent_local` reports on the same task set; `regressions` counts tasks whose after `tasks[].pass_rate` fell below baseline, computed with `lisp_eval` from the two reports' per-task pass rates written as ordered number lists in the form: `(begin (define regs (lambda (b a) (if (is_null b) 0 (+ (if (< (car a) (car b)) 1 0) (regs (cdr b) (cdr a)))))) (regs (list b1 b2 ...) (list a1 a2 ...)))`. A missing report means null values and a false gate. A baseline `overall_pass_rate` of 1 is saturated: no candidate can pass, so the discard carries no information — record `failure_mode: "saturated baseline"` and choose a harder held-out task set before re-planning (observed 2026-09-26, `local_extractor` harness runs `bac52690`/`b950572f`).
2. Render `self-improvement/si-propose-or-discard` with `evaluation_result`, `gate_result` (the `lisp_eval` boolean), `improvement_plan` and `proposed_artifact`.
3. `gate_result` true → **propose**: follow the Proposal card handoff above with a title naming the skill/component and change; include the full diff, measurements, evaluation method, goal ID, evidence and verification criteria in its description. If the board or card cannot be created, report filing blocked; do not write a proposal file or use another queue. Nothing is applied; the gemba walk decides it with the operator.
4. `gate_result` false → **discard**: keep the configuration unchanged, record the failure mode (noisy signal, misaligned operator, missing harness) in the Kata obstacle parking lot, and re-plan.
5. Judge the registered goal (`kanban_goal_judge`) with the measured results; the operator's score comes later.
6. Respond with `decision` ("propose", "discard", or "blocked"), `proposal_card_id` (only after card creation succeeds), `failure_mode` (if discarded or blocked), and `next_step` ("re-enter", "exit", or "refine").

### Prompt evolution (GEPA) — a scaffolding sub-loop (formerly `gpa-evolution`)

For a prompt artifact with a runnable eval set, when natural-language reflection on real trajectories should drive the change (Agrawal et al., GEPA, arXiv:2507.19457; NSGA-II non-dominated sorting, Deb et al. 2002 — `onto_anchor` → derived `reflective_prompt_evolution`). Prompts only (v1). Reflection and mutation are P, critiqued by recorded eval-set scores; dominance, frontier membership and the convergence form are D.

Executor binding (local prompt artifacts): the eval set is a list of `{task, evaluator: {evaluator, spec}}` entries (contains/not_contains/regex); objectives are `pass_rate` (maximize), `total_tokens` (minimize) and `mean_latency_ms` (minimize). Each candidate prompt runs as a temporary local card whose `agent_id` is the variant id (`swarm_create_local_agent`, copying the target card's other fields); remove every temporary card with `swarm_remove_local` when the session ends and confirm with `swarm_list_local_agents`, and never reconfigure the target card. The proposal card on Algedonic review is the durable review record; remove temporary cards at session end.

Split the eval set before iteration 1 into a **feedback set** (Sample and Reflect read only these tasks) and a disjoint **selection set** (Test scores only these), as GEPA separates D_feedback from D_pareto. Fix both for the session; the proposal records both. A task shown to Reflect or used to write a variant never enters the selection set.

Reflect and Propose are P steps executed by an agent: send the rendered template to a local reasoning agent with `swarm_delegate_local` and use its returned JSON. The orchestrating session only copies each returned variant's `content` into a card, verbatim; it never writes, edits or selects variant text.

1. **Sample** (`self-improvement/gpa-sample-trajectories`) — for the target (iteration 1) or each frontier member (2+), call `swarm_delegate_local` per eval task to get the response text, then `swarm_evaluate_local` with that task's evaluator on the response; each `{source, input, output, tool_calls, scores: {pass_rate: 1|0}}` pair is a recorded run. `swarm_eval_agent_local` reports counts only, no response text, so it cannot feed Reflect. No executor or evaluator → stop and report `unverified`.
2. **Reflect** (`self-improvement/gpa-reflect`) — diagnose why each outcome was poor or good and extract transferable rules. Pass its whole output object to Propose as `reflections`.
3. **Propose** (`self-improvement/gpa-propose-mutations`) — 3–7 variants: mutation (one lesson, one hypothesis "if I change X, Y improves because Z") and crossover (complementary frontier members); tag parent, operator, hypothesis and rule; carry full content.
4. **Test** (`self-improvement/gpa-test-variants`) — run every variant's temporary card, and the target itself, through `swarm_eval_agent_local` on the identical selection set and repeats; each report (`agent_name` = variant id, `overall_pass_rate`, `tasks[].pass_rate`, `total_tokens`, `tasks[].mean_latency_ms`) is its `recorded_runs` entry. Aggregate per-objective scores and cost from those reports only. Logs go under `~/Documents/zk-data/skills/self-improvement/gepa/{date}-{run}/`.
5. **Update frontier** (`self-improvement/gpa-frontier-update`) — pass Test's `tested_variants` array; merge, keep non-dominated members (A dominates B when at least as good on every objective and strictly better on one). Compute dominance with `lisp_eval`; a cost objective (`total_tokens`, `mean_latency_ms`) counts as better only when it is more than 10% lower, otherwise the two are tied on it — paired same-set runs on 2026-09-26 differed by 6–7% in tokens (789/848, 1676/1569), so a smaller gap is noise. Cost comparisons are valid only between runs on the same selection set. Form over `(pass_rate total_tokens)` pairs: `(begin (define better-cost (lambda (a b) (< a (* 0.9 b)))) (define dom (lambda (a b) (and (>= (car a) (car b)) (not (better-cost (nth 1 b) (nth 1 a))) (or (> (car a) (car b)) (better-cost (nth 1 a) (nth 1 b)))))) (dom a b))`; prune by crowding distance past `frontier_size`, record who dominated whom.
6. **Check (D)** — no tool computes hypervolume, so report it `unverified`; call `lisp_eval` with `(and (>= iteration 2) (= new_members 0))` over the measured count of variants that entered the frontier this iteration; converged only when an iteration adds no new non-dominated member (a single arrival means the frontier is still moving). Minimum 2, maximum 5 iterations per session.
7. **Act** — never adopt. Follow the Proposal card handoff above with a title naming the target and a description containing the proposed prompt diff, frontier content, measured scores, cost, lineage, eval-set identity, evidence and verification criteria. Report a blocked filing if the board or card cannot be created; never use a file fallback. The operator chooses on that card in the gemba walk.

### Fine-tuning run — the θ pathway executor (formerly `adapter-lifecycle`)

When `si-select-pathway` selects Foundation Model Improvement (θ), the `si-exec-fm-*` templates plan the signal and this procedure executes it: measure, build a dataset from verdict-labeled rollouts, train under the `lora-training` math-contract gates, evaluate against the baseline, file a proposal. Use it only after a scaffold fix (prompt, skill body, tool schema) has been ruled out, a deterministic evaluator exists, and the operator has accepted a PEFT configuration from `lora-training`.

#### Phase 1 — Measure (the rollout harness)

Model policy: “local swarm” means execution on the hKask substrate, not a local
or smaller LLM. Inherit the platform/curator defaults from Settings → Kask →
Models through the host inference bridge. Do not substitute an unapproved
local/cheaper model to avoid provider cost; if approved routing is unavailable,
surface that blocker. Only an explicit operator choice may override the model.

1. Define the task set: 3-10 representative tasks, each with a
   deterministic response evaluator (contains / not_contains / regex)
   and an explicit experiment scope (tasks/repeats and any approved run
   deadline). Agree with the operator *before training* on the held-out
   acceptance criterion and evaluation method for comparing the baseline
   and candidate on identical inputs; record what outcome counts as
   acceptance rather than inventing a pass-rate threshold afterward.
   Token usage is observed evidence, not a quota. The local harness has
   no credits_authorized or token-budget parameter.
2. Call `swarm_eval_agent_local` (swarm server) with the agent name,
   the task set, and repeats (2-3 for a first measurement). Read the
   per-task pass rates and standard error. This is the BASELINE —
   record it (it is also recorded as model_request + verdict events in
   the event store). Register the acceptance claim in the same step:
   `kanban_goal_create` with the baseline and target pass rates as
   observable criteria and your intake prediction, linking this task
   to the goal via `advances`.

#### Phase 2 — Build the dataset

3. Call `training_bridge_rollouts` (training server) with an `output_path`
   under this run's folder, `~/Documents/zk-data/skills/self-improvement/adapter/{date}-{run}/`,
   and `agent_name`, selecting mode `sft` for passed-rollout ChatML JSONL
   or `preference` for DPO JSONL (`prompt`, `chosen`, `rejected`) when
   passed and failed rollouts from the same harness task have retained
   bodies. `both` emits separate files; choose the dataset compatible
   with the operator-accepted trainer (do not pass `dpo` as a bridge mode).
   Read `sft_path`/`sft_examples` or `preference_path`/`preference_examples`;
   stop if the selected output is empty. Check `skipped_no_bodies` —
   stripped captures cannot be bridged; note the count.
4. Call `training_ingest_dataset` with the selected emitted dataset path
   to normalize and cache it. Read the format detection and sample count.

#### Phase 3 — Validate and submit (the gates)

5. Call `training_validate_config` with the base model, the dataset
   path, and the operator-accepted PEFT params (from the lora-training
   skill's G6 gate). Read EVERY finding — refuse/warn severities must
   be resolved or explicitly accepted by the operator before
   submission. The G-D0 profile (format, sample count, token
   estimates) must match the dataset you built.
6. Call `training_submit` with the dataset path, base model, validated
   params, and confirmed: true (only after the operator confirms the
   spend). Record the job id.

#### Phase 4 — Track and evaluate

7. Poll `training_status` with the job id until completion. It reports
   pod status, GPU, recent logs, and — on completion — registers the
   adapter from the HuggingFace manifest. `ab_comparison` is available
   only for a retrain with a previous adapter for the same skill and
   both losses present; it compares previous vs new training loss, NOT
   base-model vs candidate loss. First-time adapters have no such loss
   comparison: do not synthesize one or block their held-out verdict on it.
   On retrains, record any `loss_improved` as separate evidence; the
   `auto_promoted` field is not evidence of the held-out evaluation gate.
8. Prepare held-out tasks and expected answers that neither model trained
   on. For a pass-rate comparison, run `training_evaluate` twice on the
   SAME `test_dataset_path`, `method`, `max_examples`, and (if semantic)
   `judge_model`: once with `model` set to the deployed baseline model,
   once with `model` set to the deployed candidate adapter's model name.
   Pass the corresponding `adapter_id` to label each report; this field
   does NOT route inference — `model` does. Use exact_match / contains /
   semantic / benchmark only with the matching ChatML or benchmark dataset;
   semantic is LLM-judged and requires an explicit judge_model. Record
   both `accuracy` values and per-example errors; use the Phase 1 harness
   pass rate as diagnostic context, not as the denominator for this
   different evaluator. Verify both model routes actually serve the intended
   weights (returned model strings are reported, not attested). If the
   candidate is not deployed or identity cannot be verified, do not treat
   an evaluation of the base route as candidate evidence: no promotion.

#### Phase 5 — Verdict and retrain

9. Convergence gate — for a FIRST adapter, require verified candidate
   deployment and both matched Phase 4 evaluations. Call `lisp_eval` with
   the operator-approved acceptance predicate over the *measured* baseline
   and candidate results; include deployment in the predicate. For example,
   ONLY if the operator agreed that the candidate must strictly beat the
   baseline on accuracy:
   - form: `(and (= deployed 1) (> pass_rate baseline_pass_rate))`
   - env: `{ "deployed": <1 only if the candidate route serves the adapter>,
            "pass_rate": <candidate Phase 4 accuracy>,
            "baseline_pass_rate": <baseline Phase 4 accuracy> }`
   If the approved criterion differs, encode that exact criterion instead;
   do not use the example as a default policy.
   **Noise floor (always, in addition to the operator's criterion).** A
   difference inside sampling noise is not evidence of improvement,
   whatever margin the operator chose. From each `training_evaluate`
   report's `correct` and example count, compute with `lisp_eval`:
   - form: `(let ((pc (/ kc nc)) (pb (/ kb nb))) (let ((se (sqrt (+ (/ (* pc (- 1 pc)) nc) (/ (* pb (- 1 pb)) nb))))) (cond ((or (< nc 10) (< nb 10)) (list "undetermined" "fewer than 10 held-out examples")) ((= se 0) (list (if (> pc pb) "beyond_noise" "within_noise") 0)) (t (list (if (> (- pc pb) (* 2 se)) "beyond_noise" "within_noise") (- pc pb) (* 2 se))))))`
   - env: `{ "kc": <candidate correct>, "nc": <candidate examples>, "kb": <baseline correct>, "nb": <baseline examples> }`
   Accept only when the operator's criterion holds AND the result is
   `beyond_noise` (the gain exceeds twice the combined standard error of
   the two proportions). `within_noise` is reported as no demonstrated
   improvement, with the difference and the noise band; `undetermined`
   (fewer than 10 examples per side) blocks acceptance and asks for a
   larger held-out set. Example: 42/50 vs 36/50 is a 12-point gain inside
   a 16-point noise band — not demonstrated. The practical margin — how
   large a real gain justifies deployment — remains the operator's. A first adapter can receive
   an acceptance verdict with NO `ab_comparison`: its loss is not an input.
   For a RETRAIN, apply the same deployed, matched held-out gate against
   the agreed baseline (base model or prior deployed adapter, fixed before
   evaluation). Record `ab_comparison` only if present. If the operator's
   pre-agreed retrain criterion also requires improved loss vs the prior
   adapter, include its evidenced `loss_improved` in `lisp_eval`; if that
   comparison is absent, mark that criterion unresolved, not false or
   satisfied. Counterexamples to an unconditional loss gate: a first
   adapter with no `ab_comparison` but verified deployment and a passing
   operator-approved held-out comparison can be accepted; a retrain with
   `loss_improved: true` and `auto_promoted: true` but a failed matched
   held-out criterion cannot. Never treat loss improvement or
   `auto_promoted` alone as a pass-rate verdict. Missing deployment,
   route verification, matched evaluation, or an approved criterion means
   no acceptance/promotion;
   report the gap rather than fabricate inputs or compare Phase 1 rates.
   If a measured gate fails, diagnose the failures, curate the exchanges
   into a feedback file, and re-enter Phase 3 with `training_submit`
   passing feedback_path and skill_name (retrain mode merges feedback,
   deduplicates by question, and increments the adapter version). Obtain
   operator confirmation for each new submission. Bound: max 2 retrain
   cycles per adapter version; a third failure escalates to the operator.
10. File the measurements for review — the session that trained the
    adapter does not record its acceptance (operator ruling 2026-09-24:
    evaluation is separated from execution). Follow the Proposal card handoff
    above with a title naming the adapter and a description containing the
    proposed change/diff, adapter_id, baseline_pass_rate, pass_rate,
    evaluation_method, model_routes, evidence_gaps, verification criteria
    and evidence.
    Use null pass rates when unmeasured. If board or card creation fails,
    report filing blocked, never write a file fallback. The operator accepts
    or rejects the adapter on that card in the gemba walk; promotion follows
    only an accepted proposal.
11. Judge the registered goal — call `kanban_goal_judge` against the
    goal's criteria with a verdict and per-criterion results from the
    measured pass rates; mark unmeasured criteria as unresolved rather
    than guessing. When the operator confirms the outcome,
    `kanban_goal_score` Brier-scores the intake prediction — the
    scored acceptance record for this adapter change.

#### Fine-tuning constraints

- Training methods are what the training server accepts: sft (axolotl or ludwig) and dpo/kto/orpo/grpo (ludwig only); PPO is unavailable. The bridge emits only `sft` and `preference` datasets, and `training_validate_config` has no dataset-format check for grpo (G-D0 reports it undetermined) — for grpo, record the dataset-format gate as unverified in the proposal, never as passed.

- Never submit with unresolved refuse-severity gate findings.
- Do not record acceptance or recommend promotion without verified
  deployment, matched held-out baseline/candidate evaluation, and the
  operator's pre-agreed criterion being met. Do not require retrain-only
  loss comparison for a first adapter or treat it as sufficient for a
  retrain. The skill files measurements as a proposal; it does not record
  a verdict, deploy, or promote.
- Rollouts and training jobs consume provider resources; do not invent a
  spend quota or promote on the basis of an unverified cost estimate.
  Present the available estimate and obtain operator confirmation before
  each Phase 3 submission.
- If any MCP tool call fails, call `curator_report_skill_use_issue`
  with skill_name "self-improvement", the tool name, step ordinal, failure
  origin and error. A failed board or card write blocks proposal filing;
  do not claim success from a planned call.
- Clean up (storage Cleanup rule). A job the run abandons, or whose
  retrain supersedes it, is stopped with `training_cancel` and confirmed
  with `training_status`; never leave a paid pod running. When the
  proposal card is filed, delete this run's dataset files except the exact
  dataset the card cites, and list any kept file with its reason.
  A discarded run leaves no proposal file or review note.

## Improvement Measure

PDCA loops in this skill run within one session (operator ruling 2026-09-24). The signal is the measured Check result's `pass_rate` (the same value the step-1 gate reads, from the deterministic harness). After each full iteration compute with `lisp_eval` over the recorded pass rates, oldest first: `(and (>= (length xs) 3) (< (abs (- (nth (- (length xs) 1) xs) (nth (- (length xs) 2) xs))) 0.02) (< (abs (- (nth (- (length xs) 2) xs) (nth (- (length xs) 3) xs))) 0.02))` — converged when the last three measured pass rates differ by less than 0.02. An iteration with no measured pass rate adds nothing to the list; it cannot move the loop toward convergence. Minimum 2 iterations.

**Max iterations (per session)**: 10 (outer Kata), 5 (inner PDCA per Kata step). A target the session cannot reach within those bounds is reported with its remaining gap, not extended.

## Safety Governance

The skill implements the paper's safety recommendations (Section 9.1):

1. **Verifier-gated updates**: Before any structural update is committed to Σ_{t+1} or θ_{t+1}, the proposed patch must pass verifier-gated checks covering functional correctness, tool permission boundaries, and robustness to random state perturbations.
2. **Critic decoupling**: The critic (evaluator) is decoupled from the generator. If the agent conflates the roles of proposing updates and accepting them, it collapses into a self-confirming loop. In zed-kask the acceptor is the operator in the algedonic review; this skill only proposes.
3. **Layered gating**: A strict permission system for self-modification. Improvement is only permitted within explicitly defined and continuously audited safety boundaries.
4. **Version history for rollback**: Both pathways maintain version history (θ_{1:t} and Σ_{1:t}) to support validation and rollback against harmful modifications.
5. **Fast-to-slow consolidation**: Scaffold-level improvements (fast, reversible) are validated through rigorous execution tests before parametric consolidation (slow, hard to trace) is considered.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `si-select-pathway.j2` | Select between Foundation Model Improvement and Scaffolding Improvement pathways based on the current Kata state and available resources. |
| `si-execute-improvement.j2` | Execute the improvement action selected by si-select-pathway — either an FM improvement step or a Scaffolding improvement step. |
| `si-evaluate-improvement.j2` | Report measured before/after harness pass rates, per-task regressions, transfer, cost and safety for the Act gate; no verdict or Brier score. |
| `si-propose-or-discard.j2` | From the measured evaluation and the deterministic gate result, either file a proposal for the algedonic review or discard the candidate; never commit. |
| `si-exec-fm-demos.j2` | Foundation Model Improvement pathway — generate intrinsic demonstrations by sampling execution trajectories and reflecting on them. |
| `si-exec-fm-experience.j2` | Foundation Model Improvement pathway — acquire extrinsic exploratory experience by running the agent in novel environments. |
| `si-exec-fm-feedback.j2` | Foundation Model Improvement pathway — process intrinsic evaluative feedback from the improvement cycle. |
| `si-exec-scaffold-full.j2` | Scaffolding Improvement pathway — update the full scaffold (prompt, memory, tool configuration) based on the improvement evaluation. |
| `si-exec-scaffold-memory.j2` | Scaffolding Improvement pathway — update the agent's memory configuration based on the improvement evaluation. |
| `si-exec-scaffold-prompt.j2` | Scaffolding Improvement pathway — update the agent's system prompt based on the improvement evaluation. |
| `gpa-sample-trajectories.j2` | GEPA sub-loop step 1: assemble trajectories and evaluator scores from recorded runs; never simulated. |
| `gpa-reflect.j2` | GEPA sub-loop step 2: natural-language diagnosis and transferable rules. |
| `gpa-propose-mutations.j2` | GEPA sub-loop step 3: mutation and crossover variants, one hypothesis each. |
| `gpa-test-variants.j2` | GEPA sub-loop step 4: per-objective scores and cost from recorded runs; unrecorded runs are reported, not estimated. |
| `gpa-frontier-update.j2` | GEPA sub-loop step 5: non-dominated frontier with crowding-distance pruning. |
| `si-exec-scaffold-tool.j2` | Scaffolding Improvement pathway — update the agent's tool configuration based on the improvement evaluation. |

To render a template, call the `render_template` tool with the template ref (e.g., `self-improvement/si-select-pathway`) and a context object with the required variables.

## Constraints

- `si-execute-improvement.j2`: Public (router only — delegates to sub-pathway templates).

- Default pathway is Scaffolding Improvement (Σ) unless FM fine-tuning is explicitly permitted.
- No durable change is committed by the executing session. A candidate that passes the deterministic gate becomes a card on **Algedonic review**; only the operator in the gemba walk accepts it. No proposal files or dated review notes.
- The configuration in use stays unchanged until the operator accepts a proposal, so no rollback of self-applied changes is needed.
- The critic (evaluator) must be decoupled from the generator to prevent self-confirming loops.
- Token budgets are not an implicit improvement gate. A `budget` record may describe explicitly approved experiment/time/compute/spending constraints, never an agent-invented token quota. Observed token use neither authorizes truncation nor a model downgrade.
- Local swarm execution inherits the same configured platform/curator models (Settings → Kask → Models), including cloud models. “Local” never authorizes a smaller/local model downgrade. Use the host inference bridge; a model override requires operator approval. If approved routing is unavailable, stop the experiment rather than substitute a model.
- Every improvement cycle registers a falsifiable outcome claim before PDCA Do: `kanban_goal_create` with the acceptance criteria as observable criteria and an honest intake prediction, and the executing task links the goal via `advances`. `si-propose-or-discard` judges the goal (`kanban_goal_judge`) with the measured results — an improvement with no registered claim is a process violation, not an improvement.
- Scaffold-side updates (p, m, 𝒯, Σ) require a before/after measurement through a deterministic harness (`swarm_eval_agent_local` supports pure contains/not_contains/regex response checks, not shell/file evaluators) and a `lisp_eval` gate — improved pass rate at zero regressions — before `si-propose-or-discard` may file a proposal. Deterministic response scoring alone is not ground truth, which is why the operator decides. When no suitable deterministic harness exists, `evaluation_method: "none_available"` makes the gate false and the candidate is discarded (si-evaluate-improvement step 1).
- Max iterations: 10 (outer Kata), 5 (inner PDCA per Kata step).
- Evaluate convergence after each full iteration: the iterates have stopped moving. Converged when stable across 3 iterations. Minimum 2 iterations.
- `decision` field must be exactly "propose" or "discard" (lowercase).
- `next_step` field must be exactly "re-enter", "exit", or "refine" (lowercase).
- `signal_type` may be a single value or an array for multi-signal support.
- Variety engineering: PDCA iteration 2+ must check for repeated pathway/signal combinations and justify or diversify.
- Delegation: `si-exec-scaffold-prompt.j2` routes population-based evolution to the Prompt evolution (GEPA) sub-loop and its `gpa-*` templates. `si-exec-scaffold-full.j2` delegates to `diagnose` for debugging loops.
