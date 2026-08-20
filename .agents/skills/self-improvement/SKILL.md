---
name: self-improvement
description: "General self-improvement skill for FM-based agents. Drives persistent, endogenous adaptation across Foundation Model Improvement and Scaffolding Improvement via intrinsic demonstrations, evaluative feedback, and extrinsic exploratory experience."
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

## Architecture: Nested PDCA + Outer Kata

The skill follows a **three-layer architecture**:

```
┌─────────────────────────────────────────────────────────────────┐
│  OUTER LAYER: Improvement Kata (kata-improvement)               │
│  Step 1: Understand Direction (what capability are we building?) │
│  Step 2: Grasp Current Condition (baseline measurement)         │
│  Step 3: Establish Target Condition (measurable, time-bounded)   │
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

## Instructions

### si-kata-direction (Outer Kata Step 1)

1. Articulate the strategic direction: what capability should the agent build or improve?
2. Answer what the challenge is with specific, measurable statements.
3. Describe what excellent performance looks like in measurable terms.
4. Define how you will know you've improved by stating the metric and measurement plan.
5. Identify the knowledge threshold: what do you NOT know about the target capability?
6. Respond with a JSON object containing `challenge`, `excellent_performance`, `measurement_plan`, and `knowledge_threshold`.

### si-kata-current (Outer Kata Step 2)

1. Go and see to gather the facts; do not assume—measure.
2. Collect real data to describe the actual performance now.
3. List every metric describing the current state with method and source.
4. Observe what patterns exist in the data.
5. Redraw the boundary between known and assumed for your knowledge threshold.
6. Record the baseline measurements you commit to measuring against as `metric_before`.
7. Respond with a JSON object containing `current_performance`, `metrics`, `patterns`, `knowledge_threshold`, and `metric_before`.

### si-kata-target (Outer Kata Step 3)

1. Declare a specific, measurable target condition 1 week to 3 months out, beyond your current knowledge threshold.
2. Identify every obstacle between current and target conditions to create an Obstacles Parking Lot.
3. Select the ONE most consequential obstacle to address first.
4. Define what you do NOT know about the focus obstacle.
5. Respond with a JSON object containing `target_condition`, `obstacles`, `focus_obstacle`, `knowledge_gap`, and `metrics_target`.

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
4. Generate a concrete improvement plan: what operator 𝒰 will be applied, what signal 𝒮_t will drive it, what budget is allocated, and what acceptance criteria will gate the update.
5. Respond with a JSON object containing `pathway` (θ or Σ), `scaffold_component` (if Σ: p, m, 𝒯, or Σ), `signal_type` (𝒟_t, e_t, or τ_t), `improvement_plan`, `budget`, and `acceptance_criteria`.

### si-execute-improvement (PDCA Do)

1. Route to the appropriate sub-pathway template based on the pathway, scaffold component, and signal type selected in the improvement plan.
2. The router (`si-execute-improvement.j2`) delegates to one of 7 sub-pathway templates:
   - **FM Improvement**:
     - `si-exec-fm-demos.j2` (§5.1): Generate training instances, apply quality control, fine-tune via gradient descent, safeguard against model collapse.
     - `si-exec-fm-feedback.j2` (§5.2): Sample candidate outputs, apply intrinsic evaluator, convert to update signal, optimize via RL/DPO/critique-conditioned fine-tuning.
     - `si-exec-fm-experience.j2` (§5.3): Collect interaction trajectories from grounded or simulated environments, update via PPO/DPO.
   - **Scaffolding Improvement**:
     - `si-exec-scaffold-prompt.j2` (§6.1): Apply one of four paradigms (scalar/qualitative/evolution/textual-gradient). Delegates to `gpa-evolution` for population-based evolution with Pareto frontier.
     - `si-exec-scaffold-memory.j2` (§6.2): Apply signal-driven CRUD operations (Create/Read/Update/Delete).
     - `si-exec-scaffold-tool.j2` (§6.3): Apply dynamic tool routing, iterative refinement, or autonomous creation.
     - `si-exec-scaffold-full.j2` (§6.4): Treat entire scaffold as mutable program, generate patches, gate through verifier. Delegates to `diagnose` for reproduce→hypothesize→fix loops.
3. Multi-signal support: if the improvement plan specifies multiple signal types, execute them in sequence (demos → feedback → experience).
4. Capture the full execution trace: what was generated, what was filtered, what was updated, what was the cost.
5. Respond with a JSON object containing `updated_config`, `execution_trace`, `cost_breakdown`, and `proposed_artifact` (the candidate update before gating).

### si-evaluate-improvement (PDCA Check)

1. Evaluate the updated agent on a held-out evaluation distribution 𝒟_eval that does NOT overlap with the improvement signal. **Fallback**: If no held-out set is available, use cross-validation or temporal split. If neither is available, set `evaluation_method: "none_available"` and block commitment.
2. Report the full performance trajectory (m_t) across update iterations, not just the final peak score.
3. Test transfer beyond the improvement signal: does the improvement generalize to held-out tasks?
4. Track regressions: did the update break previously solved tasks?
5. Account for resource efficiency: compute cost, API tokens, wall-clock time, human input.
6. Track safety: any safety policy violations, goal drift, or reward hacking indicators?
7. If using a judge-based evaluator (Φ_judge), ensure evaluator independence: use a distinct judge configuration for final reporting.
8. Respond with a JSON object containing `performance_trajectory`, `transfer_score`, `regression_rate`, `cost_summary`, `safety_violations`, and `evaluation_method` (metric-based, judge-based, or none_available).

### si-commit-or-rollback (PDCA Act)

1. Apply the acceptance criteria from the improvement plan.
2. If the update passes all criteria (performance improved, no regressions, no safety violations, within budget):
   - **Commit** the update to the agent's intrinsic configuration.
   - For scaffolding updates: persist the new Σ_{t+1}.
   - For FM updates: persist the new θ_{t+1} checkpoint.
   - Record the committed version for future rollback.
3. If the update fails any criterion:
   - **Rollback** to the previous configuration 𝒜_t.
   - Diagnose the failure: was the signal noisy? Was the operator misaligned? Was the budget insufficient?
   - Record the failure mode for the Kata obstacle parking lot.
4. Determine the next step.
5. Respond with a JSON object containing `decision` (exactly "commit" or "rollback"), `committed_version`, `failure_mode` (if rolled back), and `next_step` (exactly "re-enter", "exit", or "refine").

## Improvement Measure

Evaluate convergence after each full iteration: the iterates have stopped moving. Converged when stable across 3 iterations. Minimum 2 iterations.

**Max iterations**: 10 (outer Kata), 5 (inner PDCA per Kata step).

## Safety Governance

The skill implements the paper's safety recommendations (Section 9.1):

1. **Verifier-gated updates**: Before any structural update is committed to Σ_{t+1} or θ_{t+1}, the proposed patch must pass verifier-gated checks covering functional correctness, tool permission boundaries, and robustness to random state perturbations.
2. **Critic decoupling**: The critic (evaluator) is decoupled from the generator. If the agent conflates the roles of proposing updates and accepting them, it collapses into a self-confirming loop. Critics can evolve but only under monotone changes (e.g., purely additive test generation) and gated by human audit trails.
3. **Layered gating**: A strict permission system for self-modification. Improvement is only permitted within explicitly defined and continuously audited safety boundaries.
4. **Version history for rollback**: Both pathways maintain version history (θ_{1:t} and Σ_{1:t}) to support validation and rollback against harmful modifications.
5. **Fast-to-slow consolidation**: Scaffold-level improvements (fast, reversible) are validated through rigorous execution tests before parametric consolidation (slow, hard to trace) is considered.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `si-kata-direction.j2` |  | Improvement Kata Step 1 — understand the strategic direction from the level above and articulate the challenge. |
| `si-kata-current.j2` |  | Improvement Kata Step 2 — gather facts and data to establish the baseline state of the agent's current capabilities. |
| `si-kata-target.j2` |  | Improvement Kata Step 3 — set a measurable, time-bounded target beyond the current knowledge threshold. |
| `si-select-pathway.j2` |  | Select between Foundation Model Improvement and Scaffolding Improvement pathways based on the current Kata state and available resources. |
| `si-execute-improvement.j2` |  | Execute the improvement action selected by si-select-pathway — either an FM improvement step or a Scaffolding improvement step. |
| `si-evaluate-improvement.j2` |  | Evaluate the outcome of the executed improvement against the target condition and produce a Brier-scored assessment. |
| `si-commit-or-rollback.j2` |  | Decide whether to commit the improvement (persist the change) or rollback (revert to the prior state) based on the evaluation. |
| `si-exec-fm-demos.j2` |  | Foundation Model Improvement pathway — generate intrinsic demonstrations by sampling execution trajectories and reflecting on them. |
| `si-exec-fm-experience.j2` |  | Foundation Model Improvement pathway — acquire extrinsic exploratory experience by running the agent in novel environments. |
| `si-exec-fm-feedback.j2` |  | Foundation Model Improvement pathway — process intrinsic evaluative feedback from the improvement cycle. |
| `si-exec-scaffold-full.j2` |  | Scaffolding Improvement pathway — update the full scaffold (prompt, memory, tool configuration) based on the improvement evaluation. |
| `si-exec-scaffold-memory.j2` |  | Scaffolding Improvement pathway — update the agent's memory configuration based on the improvement evaluation. |
| `si-exec-scaffold-prompt.j2` |  | Scaffolding Improvement pathway — update the agent's system prompt based on the improvement evaluation. |
| `si-exec-scaffold-tool.j2` |  | Scaffolding Improvement pathway — update the agent's tool configuration based on the improvement evaluation. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- `si-kata-direction.j2`: Public.
- `si-kata-current.j2`: Public.
- `si-kata-target.j2`: Public.
- `si-select-pathway.j2`: Public.
- `si-execute-improvement.j2`: Public (router only — delegates to sub-pathway templates).
- `si-exec-fm-demos.j2`: Public.
- `si-exec-fm-feedback.j2`: Public.
- `si-exec-fm-experience.j2`: Public.
- `si-exec-scaffold-prompt.j2`: Public.
- `si-exec-scaffold-memory.j2`: Public.
- `si-exec-scaffold-tool.j2`: Public.
- `si-exec-scaffold-full.j2`: Public.
- `si-evaluate-improvement.j2`: Public.
- `si-commit-or-rollback.j2`: Public.

- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
- Default pathway is Scaffolding Improvement (Σ) unless FM fine-tuning is explicitly permitted.
- All updates must pass verifier-gated checks before commitment.
- Version history must be maintained for rollback.
- The critic (evaluator) must be decoupled from the generator to prevent self-confirming loops.
- Max iterations: 10 (outer Kata), 5 (inner PDCA per Kata step).
- Evaluate convergence after each full iteration: the iterates have stopped moving. Converged when stable across 3 iterations. Minimum 2 iterations.
- `decision` field must be exactly "commit" or "rollback" (lowercase).
- `next_step` field must be exactly "re-enter", "exit", or "refine" (lowercase).
- `signal_type` may be a single value or an array for multi-signal support.
- Variety engineering: PDCA iteration 2+ must check for repeated pathway/signal combinations and justify or diversify.
- Delegation: `si-exec-scaffold-prompt.j2` delegates to `gpa-evolution` for population-based evolution. `si-exec-scaffold-full.j2` delegates to `diagnose` for debugging loops.
