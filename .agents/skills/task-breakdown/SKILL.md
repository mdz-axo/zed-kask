---
name: task-breakdown
core: true
description: "Decompose work into small, verifiable, vertically-sliced tasks with explicit acceptance criteria and checkpoints. Convergent PDCA: gather context and dependency graph, decompose, evaluate against criteria, iterate until stable, then finalize plan."
---

# Task Breakdown

Decompose work into small, verifiable, vertically-sliced tasks with explicit acceptance criteria and checkpoints. Convergent PDCA: gather read-only context and dependency graph, decompose (slice + write tasks in one producer), evaluate against sizing/red-flag/checkpoint criteria, iterate until the plan is stable, then finalize tasks/plan.md + tasks/todo.md with PKO process-axis anchors. v0.31.0: empty-spec validation, context_summary to evaluators (Good Regulator), skill_catalog wired, algedonic escalation for catastrophic plans, mechanical materiality guard, refinement history in plan.md. Distinct from kanban-task-management (single-pass board populate) and tdd (consumes the plan one vertical slice at a time).

## When to Use

- Decompose work into small, verifiable, vertically-sliced tasks with explicit acceptance criteria and checkpoints before any implementation begins.
- When you need a convergent PDCA loop: gather read-only context and dependency graph, decompose by slicing and writing tasks in one producer, evaluate against weighted criteria, iterate until the plan is stable, then finalize.
- When implementation order must follow a dependency graph built bottom-up (foundations first) rather than ad-hoc task ordering.
- When a plan needs an independent quality gate to detect self-assessment bias and compensation masking distinct from the producer-coupled evaluation step.
- When the deliverable is `tasks/plan.md` + `tasks/todo.md` with PKO process-axis anchors (Procedure, Step, StepVerification, etc.) and DC+BIBO document metadata, plus a Refinement History section making the PDCA loop visible.
- When prior outcome evidence (`prior_outcome`) or operator feedback (`prior_operator_feedback`) should calibrate the plan — the self-improvement feedback loop.
- When the installed `skill_catalog` is available and each task should carry a `skill_match_query` for skill-router consumption.
- When you need to distinguish this skill from kanban-task-management (single-pass board populate) or tdd (consumes the plan one vertical slice at a time).

## Instructions

### task-breakdown-plan

1. Validate the spec first (v0.31.0): if `spec_or_intent` is empty, whitespace-only, or shorter than 10 characters, emit `context_summary: "ERROR: empty or trivial spec — cannot decompose"`, empty `dependency_graph`, a high-impact "empty spec" risk, and an `open_questions` entry asking what should be decomposed. Do NOT produce a dependency graph or attempt decomposition — this prevents silent convergence on an empty plan.
2. Read the spec and relevant codebase sections in read-only mode — do NOT write or propose code.
3. Identify existing patterns and conventions by reading the project before planning.
4. Map dependencies between components to build the dependency graph; implementation order follows bottom-up (build foundations first).
5. Identify the deepest crate with no internal dependencies (usually the foundation types crate) and start there.
6. Note risks and unknowns; surface every assumption as an open question rather than silently resolving it.
7. Schedule high-risk areas early so they can be addressed first (fail fast).
8. When `prior_outcome` is present (v0.31.0, τ_t extrinsic exploratory experience): use completion/rework/blocked rates and `plan_followed` to calibrate granularity, AC specificity, and dependency thoroughness. Do not fabricate outcome patterns.
9. When `prior_operator_feedback` is present (v0.31.0, e_t intrinsic evaluative feedback): calibrate toward the operator's accepted style; note overridden tasks, rejection reasons, and `corrected_fields` direction. Do not let operator preference override evidence-based decomposition principles — note conflicts rather than complying.
10. Produce a JSON object with `context_summary`, `dependency_graph` (node, depends_on, depth, notes), `risks` (risk, impact, mitigation), and `open_questions`.

### task-breakdown-decompose

1. Slice the work vertically AND write each task in ONE step — each vertical slice delivers one complete, testable feature path end-to-end, not a horizontal layer shared across features.
2. Apply refinement directives from the previous evaluation when present; each directive names a criterion that scored above threshold and is addressed to a specific task — re-slice and re-write accordingly. The PDCA loop re-enters here so re-slicing and re-writing happen together.
3. Schedule high-risk slices early (fail fast).
4. Give each task a title (no "and"), slice_id/feature_path, description, acceptance_criteria (specific, testable, ≤3 bullets), verification, dependencies (or "None"), files_likely_touched, and estimated_scope (XS/S/M/L/XL).
5. Break down any task that is L or larger; break down tasks that would take more than one focused session, touch two or more independent subsystems, or whose title contains "and".
6. Arrange tasks so dependencies are satisfied, each task leaves the system in a working state, and verification checkpoints occur after every 2–3 tasks.
7. Group tasks into phases (Foundation, Core Features, Polish) and place checkpoints between phases; a checkpoint verifies all tests pass, the application builds, the core user flow works end-to-end, and the human has reviewed before proceeding.
8. When parallelizing: safely parallelize independent feature slices; keep migrations, shared state changes, and dependency chains sequential; coordinate features that share a trait contract by defining the contract first.
9. When `skill_catalog` is provided (v0.31.0): include a `skill_match_query` field per task — a natural-language capability description consumed by skill-router. Do NOT match skills yourself; just describe the capability need. Omit the field when `skill_catalog` is absent.
10. Algedonic escalation (v0.31.0, VSM S1→S5 short-circuit): after producing the tasks array, check for catastrophic conditions and emit a `plan_escalation` entry IN ADDITION to the normal output — `all_tasks_xl`, `no_dependencies_multi_task`, `no_acceptance_criteria`, or `empty_decomposition`. Each entry carries `reason`, `description`, `severity: "critical"`, and `recommended_action`. If none are met, emit `plan_escalation: []`.
11. Produce a JSON object with `slices`, `tasks`, `phases`, `checkpoints`, and `plan_escalation`.

### task-breakdown-evaluate

1. Score the task breakdown against six weighted criteria: task sizing (0.25), vertical-slice integrity (0.20), acceptance-criteria specificity (0.20), dependency ordering (0.15), checkpoint presence (0.10), red-flag absence (0.10).
2. Score each criterion from 0 (perfect) to 1 (severely deficient); be honest — inflated scores produce worse plans.
3. Task-count awareness (v0.31.0): in the sizing criterion, add +0.10 if task count > 20 (too granular) or < 3 (too coarse); no adjustment in the 3–20 healthy range. This is in addition to existing XL/L checks.
4. Use the `context_summary` (v0.31.0, Good Regulator) to check project-specific conventions — testing patterns, file-path consistency with module structure, and crate dependency ordering — not just generic criteria.
5. Check for red flags: implementation begins without a written task list; a task says "implement the feature" without acceptance criteria; no verification steps; all tasks XL-sized; no checkpoints; dependency order not considered; "and" in a task title; a task touches more than ~5 files.
6. Compute the weighted_total as the sum of (score × weight) across all six criteria, in [0,1].
7. For each criterion scored above 0.00, emit a specific, actionable, task-addressable refinement directive that names the criterion, states what is wrong, and describes the expected fix; do not emit directives for criteria scored at 0.00.
8. Produce a JSON object with `scores`, `weighted_total`, `refinement_directives`, and `red_flags`.

### task-breakdown-quality-gate

1. Evaluate the plan independently — do NOT trust the producer's self-assessment; `evaluation_result` is provided for bias detection only.
2. Use the `context_summary` (v0.31.0, Good Regulator) to check project-specific conventions independently of the producer's evaluation.
3. Re-derive every score from the plan itself using the same six weighted criteria.
4. Score each criterion 0 (perfect) to 1 (severely deficient), honestly.
5. Flag any dimension where your score diverges from the producer's by more than 0.2 as a `bias_delta` finding.
6. Detect compensation masking: if any single criterion exceeds 0.30, set `gate_pass` to false regardless of the weighted total.
7. Set `gate_pass` to true ONLY if `gate_weighted_total` ≤ 0.15 AND no individual criterion exceeds 0.30.
8. Produce a JSON object with `gate_scores`, `gate_weighted_total`, `gate_pass`, and `gate_findings`.

### task-breakdown-write-plan

1. Create the `tasks/` directory if it does not exist.
2. Write `tasks/plan.md` with: overview, architecture decisions, phased task list with checkpoints, risks table, and open questions.
3. Include a Refinement History section in `plan.md` (v0.31.0 — PDCA loop visibility): when `refinement_directives` were applied across PDCA iterations, document what criterion scored above threshold, what was wrong, and what fix was applied. Omit the section if no refinement was needed.
4. Write `tasks/todo.md` as a flat checklist grouped by phase with checkboxes for each task and its acceptance criteria — scannable, not verbose.
5. Emit `pko_anchors`: map the plan to `pko:Procedure` targeting a `pko:ProcedureTarget`; each task to `pko:Step` with `pko:StepVerification`; phases to `pko:MultiStep`; risks to `pko:IssueOccurrence`; open questions to `pko:UserQuestionOccurrence`; checkpoints to `pko:UserFeedbackOccurrence`.
6. Attach DC+BIBO state metadata (title/creator/date, `bibo:Document`) to the `tasks/plan.md` document itself — PKO grounds the structure, DC+BIBO grounds the document.
7. Do not invent tasks not present in the input `tasks` array.
8. Produce a JSON object with `plan_md`, `todo_md`, `output_paths`, and `pko_anchors`.

### loop (step 8)

1. If convergence is not met (metric > 0.15) and refinement directives exist, loop back to DECOMPOSE (step 2) with directives as focused, task-addressable improvement targets.
2. v0.31.0: refinement_directives are explicitly routed back to decompose (was implicit, depended on the manifest executor preserving cross-iteration step results — now mechanical and documented).
3. Carry `prior_metric` forward so you can detect a stable-but-unconverged plan. Each iteration narrows the gap.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `task-breakdown-plan.j2` | KnowAct | PLAN phase — read-only mode. Grasp the current condition relative to the target condition: identify what exists now (patterns, conventions, existing modules), build the dependency graph, and note risks/unknowns. v0.35.0: anchored on target_condition (mapped from {{ task }}). v0.31.0: validates empty target_condition to prevent silent convergence on an empty plan. No code is written. Produces context summary, dependency graph, and risk register. |
| `task-breakdown-decompose.j2` | KnowAct | DO phase — single producer: decompose the target condition into component target conditions (sub-tasks) AND write each task in one step. v0.35.0: each task is a sub-target with acceptance criteria framed as "what must be true for this sub-target to be achieved." v0.31.0: emits plan_escalation for catastrophic plans (all XL, no deps in multi-task plan, no ACs, empty decomposition) as algedonic short-circuit. Each task carries slice_id/feature_path, acceptance criteria, verification, dependencies, files, scope (XS/S/M/L/XL), and skill_match_query (a natural-language capability description consumed by skill-router when the skill_catalog input is provided). The PDCA loop re-enters here so refinement directives are task-addressable and re-slicing + re-writing happen together. |
| `task-breakdown-evaluate.j2` | KnowAct | CHECK phase — score the plan against seven weighted criteria: target condition coverage (0.20, v0.35.0 — do the tasks collectively achieve the target?), task sizing (0.20, v0.31.0: now includes task-count awareness — >20 or <3 tasks penalized), vertical-slice integrity (0.15), acceptance-criteria specificity (0.15), dependency ordering (0.10), checkpoint presence (0.10), red-flag absence (0.10). v0.31.0: receives context_summary for project-specific convention checking (Good Regulator). Emits specific refinement_directives for criteria above threshold — directives are task-addressable (consumed by decompose). |
| `task-breakdown-quality-gate.j2` | KnowAct | Independent quality gate — evaluates the plan WITHOUT self-assessment bias, distinct from the producer-coupled evaluate step. v0.35.0: scores seven criteria including target_condition_coverage. v0.31.0: receives context_summary for independent project-specific convention checking. Scores the seven criteria independently, flags compensation masking, and detects bias deltas vs the producer's self-assessment. |
| `task-breakdown-write-plan.j2` | KnowAct | ACT phase — finalize the plan into tasks/plan.md (target condition, overview, architecture decisions, phased task list with checkpoints, risks, open questions) and tasks/todo.md (checklist-style task list), with a pko_anchors map giving each element a PKO process-axis identity. v0.35.0: includes the target condition at the top of plan.md so the plan is always anchored to what it's achieving. v0.31.0: includes Refinement History section in plan.md documenting what was refined across PDCA iterations, making the loop visible in the artifact. |

## Constraints

- `task-breakdown-plan.j2`: Public. Read-only mode — no code proposals, file edits, or implementation sketches. Empty-spec validation is mandatory before producing any output. `prior_outcome` and `prior_operator_feedback` calibrate but do not override evidence-based decomposition principles.
- `task-breakdown-decompose.j2`: Public. Every task is a vertical feature path, not a horizontal layer. No task may be XL. No "and" in a task title. The "~5 files" limit is advisory for cross-crate Rust features (legitimate 5–7 file touches allowed with justification). Every task must have acceptance criteria AND a verification step AND declared dependencies (or "None"). Dependency order must be respected. `skill_match_query` is required per task when `skill_catalog` is provided, omitted otherwise. `plan_escalation` is emitted for catastrophic conditions (algedonic short-circuit).
- `task-breakdown-evaluate.j2`: Public. Score each criterion independently 0–1; do not inflate. Weighted_total must lie in [0,1]. Only emit refinement directives for criteria scored above 0.00. Task-count awareness (>20 or <3) applies to the sizing criterion only.
- `task-breakdown-quality-gate.j2`: Public. Independent evaluation — do not inherit the producer's scores. Compensation masking: any single criterion > 0.30 forces `gate_pass: false`. Report `bias_delta` only where |your_score − producer_score| > 0.2 for that criterion.
- `task-breakdown-write-plan.j2`: Public. Both files must be complete, self-contained markdown. `plan.md` must include overview, architecture decisions, phased task list with checkpoints, risks table, open questions, and (when refinement occurred) Refinement History. `todo.md` must be a checklist grouped by phase. Do not invent tasks not present in the input `tasks` array.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
