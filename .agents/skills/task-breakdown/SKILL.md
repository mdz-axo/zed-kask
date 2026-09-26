---
name: task-breakdown
core: true
description: "Decompose work into small, verifiable, vertically-sliced tasks with explicit acceptance criteria and checkpoints. Convergent PDCA: gather context and dependency graph, decompose, evaluate against criteria, iterate until stable, then finalize plan."
---

# Task Breakdown

Decompose work into verifiable vertical slices with acceptance criteria and checkpoints. Follow the Improvement Kata's current-condition → target-condition → experiment discipline; the seven weighted planning criteria below are a local rubric, not a claim about the published Kata. Gather read-only context and dependencies, produce a plan, check it against the target, and re-slice on measured gaps before finalizing `plan.md` and `todo.md` under `~/Documents/zk-data/skills/task-breakdown/`. Distinct from kanban-task-management (board population) and tdd (execution of slices).

## Initial and target condition

- **Initial condition:** the user's spec/intent and target outcome, observed project structure and constraints, dependency graph, and any explicit unknowns. An empty or unconfirmed target does not become a plan.
- **Target condition:** every slice advances the target outcome, has a testable acceptance criterion and verification seam, respects dependencies, and the independently checked seven-criterion gate passes without compensation masking. A failed or unmeasured gate is not a finished plan.

## When to Use

- Decompose work into small, verifiable, vertically-sliced tasks with explicit acceptance criteria and checkpoints before any implementation begins.
- When you need a convergent PDCA loop: gather read-only context and dependency graph, decompose by slicing and writing tasks in one producer, evaluate against weighted criteria, iterate until the plan is stable, then finalize.
- When implementation order must follow a dependency graph built bottom-up (foundations first) rather than ad-hoc task ordering.
- When a plan needs an independent quality gate to detect self-assessment bias and compensation masking distinct from the producer-coupled evaluation step.
- When the deliverable is `plan.md` + `todo.md` with PKO process-axis anchors (Procedure, Step, StepVerification, etc.) and DC+BIBO document metadata, plus a Refinement History section making the PDCA loop visible.
- When the installed `skill_catalog` is available and each task should carry a `skill_match_query` for skill-discovery (route) consumption.
- When you need to distinguish this skill from kanban-task-management (single-pass board populate) or tdd (consumes the plan one vertical slice at a time).

## When NOT to Use

- Work that is already one verifiable task — decomposition overhead buys nothing; execute directly.
- Requirements gathering — the spec is an input; the product-manager owns intake and spec authority.
- Executing the tasks — this skill produces the plan; tdd consumes it one vertical slice at a time, kanban-task-management populates the board.

## Instructions

### task-breakdown-plan

1. Validate the spec first: if `spec_or_intent` is empty, whitespace-only, or shorter than 10 characters, emit `context_summary: "ERROR: empty or trivial spec — cannot decompose"`, empty `dependency_graph`, a high-impact "empty spec" risk, and an `open_questions` entry asking what should be decomposed. Do NOT produce a dependency graph or attempt decomposition — this prevents silent convergence on an empty plan.
2. Read the spec and relevant codebase sections in read-only mode — do NOT write or propose code.
3. Identify existing patterns and conventions by reading the project before planning.
4. Map dependencies between components to build the dependency graph; implementation order follows bottom-up (build foundations first).
5. Identify the deepest crate with no internal dependencies (usually the foundation types crate) and start there.
6. Note risks and unknowns; surface every assumption as an open question rather than silently resolving it.
7. Schedule high-risk areas early so they can be addressed first (fail fast).
8. Produce a JSON object with `context_summary`, `dependency_graph` (node, depends_on, depth, notes), `risks` (risk, impact, mitigation), and `open_questions`.

### task-breakdown-decompose

1. Slice the work vertically AND write each task in ONE step — each vertical slice delivers one complete, testable feature path end-to-end, not a horizontal layer shared across features.
2. Apply refinement directives from the previous evaluation when present; each directive names a criterion that scored above threshold and is addressed to a specific task — re-slice and re-write accordingly. The PDCA loop re-enters here so re-slicing and re-writing happen together.
3. Schedule high-risk slices early (fail fast).
4. Give each task a title (no "and"), slice_id/feature_path, description, acceptance_criteria (specific, testable, ≤3 bullets), verification, dependencies (or "None"), files_likely_touched, and estimated_scope (XS/S/M/L/XL).
5. Break down any task that is L or larger; break down tasks that would take more than one focused session, touch two or more independent subsystems, or whose title contains "and".
6. Arrange tasks so dependencies are satisfied, each task leaves the system in a working state, and verification checkpoints occur after every 2–3 tasks.
7. Group tasks into phases (Foundation, Core Features, Polish) and place checkpoints between phases; a checkpoint verifies all tests pass, the application builds, the core user flow works end-to-end, and the human has reviewed before proceeding.
8. When parallelizing: safely parallelize independent feature slices; keep migrations, shared state changes, and dependency chains sequential; coordinate features that share a trait contract by defining the contract first.
9. When `skill_catalog` is provided: include a `skill_match_query` field per task — a natural-language capability description consumed by skill-discovery (route). Do NOT match skills yourself; just describe the capability need. Omit the field when `skill_catalog` is absent.
10. Algedonic escalation (VSM S1→S5 short-circuit): after producing the tasks array, check for catastrophic conditions and emit a `plan_escalation` entry IN ADDITION to the normal output — `all_tasks_xl`, `no_dependencies_multi_task`, `no_acceptance_criteria`, or `empty_decomposition`. Each entry carries `reason`, `description`, `severity: "critical"`, and `recommended_action`. If none are met, emit `plan_escalation: []`.
11. Produce a JSON object with `slices`, `tasks`, `phases`, `checkpoints`, and `plan_escalation`.

### task-breakdown-evaluate

1. Score the task breakdown against the seven criteria in `task-breakdown-evaluate.j2`: target-condition coverage (0.20), task sizing (0.20), vertical-slice integrity (0.15), acceptance-criteria specificity (0.15), dependency ordering (0.10), checkpoint presence (0.10), red-flag absence (0.10). These local weights sum to 1; the published Improvement Kata does not prescribe them.
2. Score each criterion from 0 (perfect) to 1 (severely deficient); be honest — inflated scores produce worse plans.
3. Task-count awareness: in the sizing criterion, add +0.10 if task count > 20 (too granular) or < 3 (too coarse); no adjustment in the 3–20 healthy range. This is in addition to existing XL/L checks.
4. Use the `context_summary` (Good Regulator) to check project-specific conventions — testing patterns, file-path consistency with module structure, and crate dependency ordering — not just generic criteria.
5. Check for red flags: implementation begins without a written task list; a task says "implement the feature" without acceptance criteria; no verification steps; all tasks XL-sized; no checkpoints; dependency order not considered; "and" in a task title; a task touches more than ~5 files.
6. After the template returns all seven raw scores, require each to be numeric in [0,1] and call `lisp_eval` to compute `weighted_total` from those scores, never from the model's stated total: `(+ (* 0.20 target_condition_coverage) (* 0.20 task_sizing) (* 0.15 vertical_slice_integrity) (* 0.15 ac_specificity) (* 0.10 dependency_ordering) (* 0.10 checkpoint_presence) (* 0.10 red_flag_absence))`. Missing or invalid dimensions stop evaluation; compare the template's reported total and surface a mismatch rather than trusting it.
7. For each criterion scored above 0.00, emit a specific, actionable, task-addressable refinement directive that names the criterion, states what is wrong, and describes the expected fix; do not emit directives for criteria scored at 0.00.
8. Produce a JSON object with `scores`, `weighted_total`, `refinement_directives`, and `red_flags`.

### task-breakdown-quality-gate

1. Evaluate the plan independently — do NOT trust the producer's self-assessment; `evaluation_result` is provided for bias detection only.
2. Use the `context_summary` (Good Regulator) to check project-specific conventions independently of the producer's evaluation.
3. Re-derive every score from the plan itself using the same seven weighted criteria. Recompute `gate_weighted_total` via `lisp_eval` with the step-6 form above, binding the *gate's* independently assigned scores; do not reuse the producer's numbers.
4. Score each criterion 0 (perfect) to 1 (severely deficient), honestly.
5. Flag any dimension where your score diverges from the producer's by more than 0.2 as a `bias_delta` finding.
6. Detect compensation masking: if any single criterion exceeds 0.30, set `gate_pass` to false regardless of the weighted total.
7. Recompute `gate_pass` with `lisp_eval` from the gate's raw scores and computed total: `(and (<= gate_weighted_total 0.15) (<= (max target_condition_coverage task_sizing vertical_slice_integrity ac_specificity dependency_ordering checkpoint_presence red_flag_absence) 0.30))`. If any score is missing or invalid, stop instead of treating it as zero; a model-supplied `gate_pass` that disagrees with the deterministic result is a finding, not authority.
8. Produce a JSON object with `gate_scores`, `gate_weighted_total`, `gate_pass`, and `gate_findings`.

### task-breakdown-write-plan

1. Create the run directory `~/Documents/zk-data/skills/task-breakdown/{date}-{run}/` (via `terminal`; built-in file tools cannot write under `~/Documents/zk-data`). Plans are skill artifacts, never repository files.
2. Write `plan.md` there with: overview, architecture decisions, phased task list with checkpoints, risks table, and open questions.
3. Include a Refinement History section in `plan.md` (PDCA loop visibility): when `refinement_directives` were applied across PDCA iterations, document what criterion scored above threshold, what was wrong, and what fix was applied. Omit the section if no refinement was needed.
4. Write `todo.md` there as a flat checklist grouped by phase with checkboxes for each task and its acceptance criteria — scannable, not verbose.
5. Emit `pko_anchors`: map the plan to `pko:Procedure` targeting a `pko:ProcedureTarget`; each task to `pko:Step` with `pko:StepVerification`; phases to `pko:MultiStep`; risks to `pko:IssueOccurrence`; open questions to `pko:UserQuestionOccurrence`; checkpoints to `pko:UserFeedbackOccurrence`.
6. Attach DC+BIBO state metadata (title/creator/date, `bibo:Document`) to the `plan.md` document itself — PKO grounds the structure, DC+BIBO grounds the document.
7. Do not invent tasks not present in the input `tasks` array.
8. Produce a JSON object with `plan_md`, `todo_md`, `output_paths`, and `pko_anchors`.

### loop (step 8)

1. If convergence is not met (metric > 0.15) and refinement directives exist, loop back to DECOMPOSE (step 2) with directives as focused, task-addressable improvement targets.
2. `refinement_directives` are explicitly routed back to decompose (was implicit, depended on cross-iteration step result preservation — now mechanical and documented).
3. Carry `prior_metric` forward so you can detect a stable-but-unconverged plan. Each iteration narrows the gap.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `task-breakdown-plan.j2` | PLAN phase — read-only mode. Grasp the current condition relative to the target condition: identify what exists now (patterns, conventions, existing modules), build the dependency graph, and note risks/unknowns. Anchored on target_condition (mapped from {{ task }}). Validates empty target_condition to prevent silent convergence on an empty plan. No code is written. Produces context summary, dependency graph, and risk register. |
| `task-breakdown-decompose.j2` | DO phase — single producer: decompose the target condition into component target conditions (sub-tasks) AND write each task in one step. Each task is a sub-target with acceptance criteria framed as "what must be true for this sub-target to be achieved." emits plan_escalation for catastrophic plans (all XL, no deps in multi-task plan, no ACs, empty decomposition) as algedonic short-circuit. Each task carries slice_id/feature_path, acceptance criteria, verification, dependencies, files, scope (XS/S/M/L/XL), and skill_match_query (a natural-language capability description consumed by skill-discovery (route) when the skill_catalog input is provided). The PDCA loop re-enters here so refinement directives are task-addressable and re-slicing + re-writing happen together. |
| `task-breakdown-evaluate.j2` | CHECK phase — score the plan against seven weighted criteria: target condition coverage (0.20 — do the tasks collectively achieve the target?), task sizing (0.20, now includes task-count awareness — >20 or <3 tasks penalized), vertical-slice integrity (0.15), acceptance-criteria specificity (0.15), dependency ordering (0.10), checkpoint presence (0.10), red-flag absence (0.10). Receives context_summary for project-specific convention checking (Good Regulator). Emits specific refinement_directives for criteria above threshold — directives are task-addressable (consumed by decompose). |
| `task-breakdown-quality-gate.j2` | Independent quality gate — evaluates the plan WITHOUT self-assessment bias, distinct from the producer-coupled evaluate step. Scores seven criteria including target_condition_coverage. Receives context_summary for independent project-specific convention checking. Scores the seven criteria independently, flags compensation masking, and detects bias deltas vs the producer's self-assessment. |
| `task-breakdown-write-plan.j2` | ACT phase — finalize the plan into plan.md (target condition, overview, architecture decisions, phased task list with checkpoints, risks, open questions) and todo.md (checklist-style task list), with a pko_anchors map giving each element a PKO process-axis identity. Includes the target condition at the top of plan.md so the plan is always anchored to what it's achieving. Includes Refinement History section in plan.md documenting what was refined across PDCA iterations, making the loop visible in the artifact. |

To render a template, call the `render_template` tool with the template ref (e.g., `task-breakdown/task-breakdown-plan`) and a context object with the required variables.

## Constraints

- `task-breakdown-plan.j2`: Public. Read-only mode — no code proposals, file edits, or implementation sketches. Empty-spec validation is mandatory before producing any output.
- `task-breakdown-decompose.j2`: Public. Every task is a vertical feature path, not a horizontal layer. No task may be XL. No "and" in a task title. The "~5 files" limit is advisory for cross-crate Rust features (legitimate 5–7 file touches allowed with justification). Every task must have acceptance criteria AND a verification step AND declared dependencies (or "None"). Dependency order must be respected. `skill_match_query` is required per task when `skill_catalog` is provided, omitted otherwise. `plan_escalation` is emitted for catastrophic conditions (algedonic short-circuit).
- `task-breakdown-evaluate.j2`: Public. Score each criterion independently 0–1; do not inflate. Weighted_total must lie in [0,1]. Only emit refinement directives for criteria scored above 0.00. Task-count awareness (>20 or <3) applies to the sizing criterion only.
- `task-breakdown-quality-gate.j2`: Public. Independent evaluation — do not inherit the producer's scores. Compensation masking: any single criterion > 0.30 forces `gate_pass: false`. Report `bias_delta` only where |your_score − producer_score| > 0.2 for that criterion.
- `task-breakdown-write-plan.j2`: Public. Both files must be complete, self-contained markdown. `plan.md` must include overview, architecture decisions, phased task list with checkpoints, risks table, open questions, and (when refinement occurred) Refinement History. `todo.md` must be a checklist grouped by phase. Do not invent tasks not present in the input `tasks` array.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
