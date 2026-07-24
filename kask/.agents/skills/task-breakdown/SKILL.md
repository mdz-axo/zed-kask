---
name: task-breakdown
visibility: public
description: "Decompose work into small, verifiable, vertically-sliced tasks with explicit acceptance criteria and checkpoints. Convergent PDCA: gather read-only context and dependency graph, decompose (slice + write tasks in one producer), evaluate against sizing/red-flag/checkpoint criteria, iterate until the plan is stable, then finalize tasks/plan.md + tasks/todo.md with PKO process-axis anchors. Distinct from kanban-task-decomposition (single-pass board populate) and tdd (consumes the plan one vertical slice at a time).
"
---

# Task Breakdown

Decompose work into small, verifiable, vertically-sliced tasks with explicit acceptance criteria and checkpoints. Convergent PDCA: gather read-only context and dependency graph, decompose (slice + write tasks in one producer), evaluate against sizing/red-flag/checkpoint criteria, iterate until the plan is stable, then finalize tasks/plan.md + tasks/todo.md with PKO process-axis anchors. Distinct from kanban-task-decomposition (single-pass board populate) and tdd (consumes the plan one vertical slice at a time).


## When to Use

- Decompose work into small, verifiable, vertically-sliced tasks with explicit acceptance criteria and checkpoints before any implementation begins.
- When you need a convergent PDCA loop: gather read-only context and dependency graph, decompose by slicing and writing tasks in one producer, evaluate against weighted criteria, iterate until the plan is stable, then finalize.
- When implementation order must follow a dependency graph built bottom-up (foundations first) rather than ad-hoc task ordering.
- When a plan needs an independent quality gate to detect self-assessment bias and compensation masking distinct from the producer-coupled evaluation step.
- When the deliverable is `tasks/plan.md` + `tasks/todo.md` with PKO process-axis anchors (Procedure, Step, StepVerification, etc.) and DC+BIBO document metadata.
- When you need to distinguish this skill from kanban-task-decomposition (single-pass board populate) or tdd (consumes the plan one vertical slice at a time).

## Instructions

### task-breakdown-plan

1. Read the spec and relevant codebase sections in read-only mode — do NOT write or propose code.
2. Identify existing patterns and conventions by reading the project before planning.
3. Map dependencies between components to build the dependency graph; implementation order follows bottom-up (build foundations first).
4. Identify the deepest crate with no internal dependencies (usually the foundation types crate) and start there.
5. Note risks and unknowns; surface every assumption as an open question rather than silently resolving it.
6. Schedule high-risk areas early so they can be addressed first (fail fast).
7. Produce a JSON object with `context_summary`, `dependency_graph` (node, depends_on, depth, notes), `risks` (risk, impact, mitigation), and `open_questions`.

### task-breakdown-decompose

1. Slice the work vertically AND write each task in ONE step — each vertical slice delivers one complete, testable feature path end-to-end, not a horizontal layer shared across features.
2. Apply refinement directives from the previous evaluation when present; each directive names a criterion that scored above threshold and is addressed to a specific task — re-slice and re-write accordingly.
3. Schedule high-risk slices early (fail fast).
4. Give each task a title (no "and"), slice_id/feature_path, description, acceptance_criteria (specific, testable, ≤3 bullets), verification, dependencies (or "None"), files_likely_touched, and estimated_scope (XS/S/M/L/XL).
5. Break down any task that is L or larger; break down tasks that would take more than one focused session, touch two or more independent subsystems, or whose title contains "and".
6. Arrange tasks so dependencies are satisfied, each task leaves the system in a working state, and verification checkpoints occur after every 2–3 tasks.
7. Group tasks into phases (Foundation, Core Features, Polish) and place checkpoints between phases; a checkpoint verifies all tests pass, the application builds, the core user flow works end-to-end, and the human has reviewed before proceeding.
8. When parallelizing: safely parallelize independent feature slices; keep migrations, shared state changes, and dependency chains sequential; coordinate features that share a trait contract by defining the contract first.
9. Produce a JSON object with `slices`, `tasks`, `phases`, and `checkpoints`.

### task-breakdown-evaluate

1. Score the task breakdown against six weighted criteria: task sizing (0.25), vertical-slice integrity (0.20), acceptance-criteria specificity (0.20), dependency ordering (0.15), checkpoint presence (0.10), red-flag absence (0.10).
2. Score each criterion from 0 (perfect) to 1 (severely deficient); be honest — inflated scores produce worse plans.
3. Check for red flags: implementation begins without a written task list; a task says "implement the feature" without acceptance criteria; no verification steps; all tasks XL-sized; no checkpoints; dependency order not considered; "and" in a task title; a task touches more than ~5 files.
4. Compute the weighted_total as the sum of (score × weight) across all six criteria, in [0,1].
5. For each criterion scored above 0.00, emit a specific, actionable refinement directive that names the criterion, states what is wrong, and describes the expected fix; do not emit directives for criteria scored at 0.00.
6. Produce a JSON object with `scores`, `weighted_total`, `refinement_directives`, and `red_flags`.

### task-breakdown-quality-gate

1. Evaluate the plan independently — do NOT trust the producer's self-assessment; `evaluation_result` is provided for bias detection only.
2. Re-derive every score from the plan itself using the same six weighted criteria.
3. Score each criterion 0 (perfect) to 1 (severely deficient), honestly.
4. Flag any dimension where your score diverges from the producer's by more than 0.2 as a `bias_delta` finding.
5. Detect compensation masking: if any single criterion exceeds 0.30, set `gate_pass` to false regardless of the weighted total.
6. Set `gate_pass` to true ONLY if `gate_weighted_total` ≤ 0.15 AND no individual criterion exceeds 0.30.
7. Produce a JSON object with `gate_scores`, `gate_weighted_total`, `gate_pass`, and `gate_findings`.

### task-breakdown-convergence

1. Measure whether the plan is stable and complete using LLM-assessed saturation detection.
2. Score from 0.00 (plan complete: all tasks ≤M, vertical slices, specific ACs, ordered, checkpointed, zero red flags) to 1.00 (no coherent plan, tasks unordered, all XL).
3. Start at 0.0 and add per violation: sizing (+0.25), vertical-slice integrity (+0.20), AC specificity (+0.20), dependency ordering (+0.15), checkpoints (+0.10), red flags (+0.10); clamp to [0,1].
4. Apply the materiality guard: if `prior_metric` is not null and |metric − prior_metric| < 0.02 AND the plan is unchanged from the prior iteration, force `convergence_metric` to 0.0 and set `materiality_guard` to true.
5. Otherwise set `materiality_guard` to false and report the computed metric.
6. Produce a JSON object with `convergence_metric`, `convergence_method`, `materiality_guard`, `metric_decomposition`, `rationale`, and `blockers`.

### task-breakdown-write-plan

1. Create the `tasks/` directory if it does not exist.
2. Write `tasks/plan.md` with: overview, architecture decisions, phased task list with checkpoints, risks table, and open questions.
3. Write `tasks/todo.md` as a flat checklist grouped by phase with checkboxes for each task and its acceptance criteria — scannable, not verbose.
4. Emit `pko_anchors`: map the plan to `pko:Procedure` targeting a `pko:ProcedureTarget`; each task to `pko:Step` with `pko:StepVerification`; phases to `pko:MultiStep`; risks to `pko:IssueOccurrence`; open questions to `pko:UserQuestionOccurrence`; checkpoints to `pko:UserFeedbackOccurrence`.
5. Attach DC+BIBO state metadata (title/creator/date, `bibo:Document`) to the `tasks/plan.md` document itself — PKO grounds the structure, DC+BIBO grounds the document.
6. Do not invent tasks not present in the input `tasks` array.
7. Produce a JSON object with `plan_md`, `todo_md`, `output_paths`, and `pko_anchors`.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `task-breakdown-plan.j2` | KnowAct | PLAN phase — read-only mode. Read the spec and relevant codebase sections, identify existing patterns/conventions, build the dependency graph, and note risks/unknowns. No code is written. Produces context summary, dependency graph, and risk register.  |
| `task-breakdown-decompose.j2` | KnowAct | DO phase — single producer: slice vertically AND write tasks in one step. Each task carries slice_id/feature_path, acceptance criteria, verification, dependencies, files, and scope (XS/S/M/L/XL). The PDCA loop re-enters here so refinement directives are task-addressable and re-slicing + re-writing happen together. Merges the former slice + write-tasks steps to fix the grain-mismatch (mirrors diataxis-diagram's single generate producer).  |
| `task-breakdown-evaluate.j2` | KnowAct | CHECK phase — score the plan against six weighted criteria: task sizing (0.25), vertical-slice integrity (0.20), acceptance-criteria specificity (0.20), dependency ordering (0.15), checkpoint presence (0.10), red-flag absence (0.10). Emits specific refinement_directives for criteria above threshold — directives are task-addressable (consumed by decompose).  |
| `task-breakdown-quality-gate.j2` | KnowAct | Independent quality gate (fusion: false) — evaluates the plan WITHOUT self-assessment bias, distinct from the producer-coupled evaluate step. Scores the six criteria independently, flags compensation masking, and detects bias deltas vs the producer's self-assessment. Mirrors superforecasting's forecast-quality-gate separation of concerns.  |
| `task-breakdown-convergence.j2` | KnowAct | Compute normalized convergence metric from the evaluation scores. Weighted sum across six criteria, normalized to [0,1] where 0 = plan is complete, correctly ordered, properly sized, and free of red flags. Threshold 0.15.  |
| `task-breakdown-write-plan.j2` | KnowAct | ACT phase — finalize the plan into tasks/plan.md (overview, architecture decisions, phased task list with checkpoints, risks, open questions) and tasks/todo.md (checklist-style task list), with a pko_anchors map giving each element a PKO process-axis identity (Procedure, Step, StepVerification, etc.). The plan.md document itself carries DC+BIBO state metadata. Create the tasks/ directory if absent.  |

## Constraints

- `task-breakdown-plan.j2`: Public.
- `task-breakdown-decompose.j2`: Public.
- `task-breakdown-evaluate.j2`: Public.
- `task-breakdown-quality-gate.j2`: Public.
- `task-breakdown-convergence.j2`: Public.
- `task-breakdown-write-plan.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
