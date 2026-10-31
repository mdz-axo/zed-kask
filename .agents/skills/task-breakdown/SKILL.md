---
name: task-breakdown
core: true
description: "Decompose work into small, verifiable, vertically-sliced tasks with explicit acceptance criteria and checkpoints. Convergent PDCA: gather context and dependency graph, decompose, evaluate against criteria, iterate until stable, then finalize plan."
audience: user
---

# Task Breakdown

Decompose work into verifiable vertical slices with acceptance criteria and checkpoints. Follow the Improvement Kata's current-condition → target-condition → experiment discipline; the seven weighted planning criteria below are a local rubric, not a claim about the published Kata. Gather read-only context and dependencies, produce a plan, check it against the target, and re-slice on measured gaps before finalizing `plan.md` and `todo.md` under `~/Documents/zk-data/skills/task-breakdown/`. Distinct from kanban-task-management (board population) and tdd (execution of slices).

## Initial and target condition

- **Initial condition:** the user's spec/intent and target outcome, observed project structure and constraints, dependency graph, and any explicit unknowns. An empty or unconfirmed target does not become a plan.
- **Target condition:** every slice advances the target outcome, has a testable acceptance criterion and verification seam, respects dependencies, and the independently checked seven-criterion gate passes without compensation masking. A failed or unmeasured gate is not a finished plan.

## D/P labelling

Decomposition, dependency mapping, base criterion scores (`base_gate_scores`) and useful directive text are P — judgment from the independent critic. TB-1 short-circuiting, TB-COUNT task-count facts, and TB-2 adjustment/clamping/weighted grade/directive-reference coverage/final `gate_result` are D (`lisp_eval`). The critic never adds a count penalty or types the final grade. Missing/invalid dimensions and incomplete directive coverage stop acceptance; mechanical presence does not establish semantic quality.

## When to Use

- Decompose work into small, verifiable, vertically-sliced tasks with explicit acceptance criteria and checkpoints before any implementation begins.
- When you need a convergent PDCA loop: gather read-only context and dependency graph, decompose by slicing and writing tasks in one producer, evaluate against weighted criteria, iterate until the plan is stable, then finalize.
- When implementation order must follow a dependency graph built bottom-up (foundations first) rather than ad-hoc task ordering.
- When a plan needs an independent quality gate to detect compensation masking and emit task-addressable refinement directives.
- When the deliverable is `plan.md` + `todo.md` with PKO process-axis anchors (Procedure, Step, StepVerification, etc.) and DC+BIBO document metadata, plus a Refinement History section making the PDCA loop visible.
- When the installed `skill_catalog` is available and each task should carry a `skill_match_query` for skill-discovery (route) consumption.

## When NOT to Use

- Work that is already one verifiable task — decomposition overhead buys nothing; execute directly.
- Requirements gathering — the spec is an input; the product-manager owns intake and spec authority.
- Executing the tasks — this skill produces the plan; tdd consumes it one vertical slice at a time, kanban-task-management populates the board.

## Instructions

### task-breakdown-plan

1. Validate the spec first: if `target_condition` is empty, whitespace-only, or shorter than 10 characters, emit `context_summary: "ERROR: empty or trivial target condition — cannot decompose"`, empty `dependency_graph`, a high-impact "empty target condition" risk, and an `open_questions` entry asking what should be decomposed. Do NOT produce a dependency graph or attempt decomposition — this prevents silent convergence on an empty plan.
2. Read the spec and relevant codebase sections in read-only mode — do NOT write or propose code.
3. Identify existing patterns and conventions by reading the project before planning.
4. Map dependencies between components to build the dependency graph; implementation order follows bottom-up (build foundations first).
5. Identify the deepest crate with no internal dependencies (usually the foundation types crate) and start there.
6. Note risks and unknowns; surface every assumption as an open question rather than silently resolving it.
7. Schedule high-risk areas early so they can be addressed first (fail fast).
8. Produce a JSON object with `context_summary`, `dependency_graph` (node, depends_on, depth, notes), `risks` (risk, impact, mitigation), and `open_questions`.

### task-breakdown-decompose

0. **Trust intake (seam 3, structural):** Before decomposing, extract every explicit user instruction from the conversation verbatim into a numbered `intake_contract` array. Each entry is the instruction text as written, not paraphrased. Every task's `acceptance_criteria` must be consistent with the full `intake_contract`. A task that would violate an explicit instruction is infeasible (`ΔT_trust < 0`): the deliverable's value is `T_reliance · B` and an instruction-violating deliverable is rationally discounted to zero (Akerlof 1970). If any instruction is ambiguous, surface it as an `open_question`, not a silent resolution.

1. Slice the work vertically AND write each task in ONE step — each vertical slice delivers one complete, testable feature path end-to-end, not a horizontal layer shared across features.
2. Apply refinement directives from the previous evaluation when present; each directive names a criterion that scored above threshold and is addressed to a specific task — re-slice and re-write accordingly. The PDCA loop re-enters here so re-slicing and re-writing happen together.
3. Schedule high-risk slices early (fail fast).
4. Give each task a title (no "and"), slice_id/feature_path, description, acceptance_criteria (specific, testable, ≤3 bullets), verification, dependencies (or "None"), files_likely_touched, and estimated_scope (XS/S/M/L — no XL; break down any task that would be L or larger).
5. Break down any task that is L or larger; break down tasks that would take more than one focused session, touch two or more independent subsystems, or whose title contains "and". No task may be XL.
6. Arrange tasks so dependencies are satisfied, each task leaves the system in a working state, and verification checkpoints occur after every 2–3 tasks.
7. Group tasks into phases (Foundation, Core Features, Polish) and place checkpoints between phases; a checkpoint verifies all tests pass, the application builds, the core user flow works end-to-end, and the human has reviewed before proceeding.
8. When parallelizing: safely parallelize independent feature slices; keep migrations, shared state changes, and dependency chains sequential; coordinate features that share a trait contract by defining the contract first.
9. When `skill_catalog` is provided: include a `skill_match_query` field per task — a natural-language capability description consumed by skill-discovery (route). Do NOT match skills yourself; just describe the capability need. Omit the field when `skill_catalog` is absent.
10. Produce a JSON object with `slices`, `tasks`, `phases`, and `checkpoints`.
11. **Algedonic escalation (`TB-1`, D — `lisp_eval`, no template):** after `task-breakdown-decompose.j2` returns `tasks`, run `lisp_eval` to check for catastrophic plan conditions (`empty_decomposition`, `all_tasks_xl`, `no_dependencies_multi_task`, `no_acceptance_criteria`) before invoking the quality gate:
    - form: `(begin (define all-xl (lambda (ts) (if (is_null ts) t (and (string= (assoc "estimated_scope" (car ts)) "XL") (all-xl (cdr ts)))))) (define any-deps (lambda (ts) (if (is_null ts) nil (let ((d (assoc "dependencies" (car ts)))) (or (and (listp d) (> (length d) 0) (not (string= (car d) "None"))) (any-deps (cdr ts))))))) (define any-acs (lambda (ts) (if (is_null ts) nil (let ((a (assoc "acceptance_criteria" (car ts)))) (or (and (listp a) (> (length a) 0)) (any-acs (cdr ts))))))) (if (is_null tasks) (list "empty_decomposition") (append (if (all-xl tasks) (list "all_tasks_xl") (quote ())) (append (if (and (> (length tasks) 1) (not (any-deps tasks))) (list "no_dependencies_multi_task") (quote ())) (if (not (any-acs tasks)) (list "no_acceptance_criteria") (quote ()))))))`
    - env: `{ "tasks": <tasks array from decompose> }`. If the returned list is non-empty, halt and surface the catastrophic escalation reasons to the operator before running the quality gate.

### task-breakdown-quality-gate

1. Before the critic, compute the unchanged task-count rule with **TB-COUNT**, D over the actual decomposed `tasks`:
   - form: `(let ((task-count (length tasks))) (list (list "task_count" task-count) (list "task_count_penalty" (if (or (< task-count 3) (> task-count 20)) 0.10 0.0))))`
   - env: `{ "tasks": <actual tasks from DECOMPOSE> }`. Retain this result/receipt; do not hand-type a count or penalty. If tasks change, recompute these facts before judging them.
2. Render the independent quality gate once with this complete decoded context; JSON-encode the outer object for `render_template.context`:
   ```json
   { "tasks": <from DECOMPOSE>, "slices": <from DECOMPOSE>, "dependency_graph": <from PLAN>,
     "context_summary": "<from PLAN>", "target_condition": "<confirmed target>",
     "phases": <from DECOMPOSE>, "checkpoints": <from DECOMPOSE>,
     "task_count": <TB-COUNT task_count>, "task_count_penalty": <TB-COUNT task_count_penalty> }
   ```
3. The critic judges seven `base_gate_scores` from 0 (perfect) to 1 (deficient), including task sizes but **not** the count penalty. It supplies `gate_findings`, `refinement_directives` and `red_flags`. Weights remain 0.20/0.20/0.15/0.15/0.10/0.10/0.10; these and the count rule are local policy, not prescribed by published Kata.
4. Directive text remains the critic's responsibility: each positive base criterion needs a task-addressable directive, and the rendered count mandate also requires `task_sizing` when the precomputed penalty applies, even with base sizing 0. This avoids a second normal critic call just to add arithmetic. The critic uses project context, target, phases and checkpoints to assess the original red-flag rubric.
5. Run **TB-2**, D, over the critic's unadjusted output plus the unchanged TB-COUNT penalty. It validates numbers, applies the penalty once, clamps at 1, computes the original weighted grade/0.30 ceiling and checks required criterion references:
   - form: `(begin (define all-valid (lambda (xs) (if (is_null xs) true (and (numberp (car xs)) (>= (car xs) 0.0) (<= (car xs) 1.0) (all-valid (cdr xs)))))) (define has-directive (lambda (criterion ds) (if (is_null ds) false (or (string= criterion (assoc "criterion" (car ds))) (has-directive criterion (cdr ds)))))) (define missing-directives (lambda (scores ds) (if (is_null scores) (quote ()) (append (if (and (> (nth 1 (car scores)) 0.0) (not (has-directive (nth 0 (car scores)) ds))) (list (nth 0 (car scores))) (quote ())) (missing-directives (cdr scores) ds))))) (let ((target_condition_coverage (assoc! "target_condition_coverage" base_gate_scores)) (task_sizing (assoc! "task_sizing" base_gate_scores)) (vertical_slice_integrity (assoc! "vertical_slice_integrity" base_gate_scores)) (ac_specificity (assoc! "ac_specificity" base_gate_scores)) (dependency_ordering (assoc! "dependency_ordering" base_gate_scores)) (checkpoint_presence (assoc! "checkpoint_presence" base_gate_scores)) (red_flag_absence (assoc! "red_flag_absence" base_gate_scores))) (if (not (all-valid (list target_condition_coverage task_sizing vertical_slice_integrity ac_specificity dependency_ordering checkpoint_presence red_flag_absence))) (list (list "input_valid" false) (list "gate_pass" false) (list "error" "invalid_scores")) (if (not (and (numberp task_count_penalty) (or (= task_count_penalty 0.0) (= task_count_penalty 0.10)))) (list (list "input_valid" false) (list "gate_pass" false) (list "error" "invalid_count_penalty")) (let ((adjusted (min 1.0 (/ (+ (* 10 task_sizing) (* 10 task_count_penalty)) 10)))) (let ((scores (list (list "target_condition_coverage" target_condition_coverage) (list "task_sizing" adjusted) (list "vertical_slice_integrity" vertical_slice_integrity) (list "ac_specificity" ac_specificity) (list "dependency_ordering" dependency_ordering) (list "checkpoint_presence" checkpoint_presence) (list "red_flag_absence" red_flag_absence))) (total (/ (+ (* 20 target_condition_coverage) (* 20 adjusted) (* 15 vertical_slice_integrity) (* 15 ac_specificity) (* 10 dependency_ordering) (* 10 checkpoint_presence) (* 10 red_flag_absence)) 100)) (worst (max target_condition_coverage adjusted vertical_slice_integrity ac_specificity dependency_ordering checkpoint_presence red_flag_absence))) (let ((missing (missing-directives scores refinement_directives))) (list (list "input_valid" true) (list "base_gate_scores" base_gate_scores) (list "gate_scores" scores) (list "task_count_penalty" task_count_penalty) (list "gate_weighted_total" total) (list "gate_pass" (and (<= total 0.15) (<= worst 0.30) (= (length missing) 0))) (list "directives_complete" (= (length missing) 0)) (list "missing_refinement_criteria" missing) (list "gate_findings" gate_findings) (list "refinement_directives" refinement_directives) (list "red_flags" red_flags)))))))))`
   - env: `{ "base_gate_scores": <critic's original seven base scores>, "task_count_penalty": <TB-COUNT penalty>, "gate_findings": <critic findings>, "refinement_directives": <critic directives>, "red_flags": <critic flags> }`.
6. Require `input_valid` and `directives_complete` before accepting the result as `gate_result`. Otherwise stop and report the exact error/missing criteria for correction; an incomplete review is not a scored-plan pass or permission to invent directives. Presence checks do not judge directive usefulness. For complete input, pass the **whole TB-2 object unchanged** downstream; do not manually assemble scores, overwrite `base_gate_scores`, or feed adjusted `gate_scores` back as base input. `gate_pass` retains the 0.15 total/0.30 per-criterion ceilings. Tenths/percentage evaluation avoids decimal-boundary addition artifacts without adding rounding or an epsilon.

### task-breakdown-write-plan

1. Render `task-breakdown/task-breakdown-write-plan` with this complete context shape; encode the whole object once as JSON text for `render_template.context`, keeping its arrays and objects structured inside it:
   ```json
   { "context_summary": "<from PLAN>", "target_condition": "<confirmed target>",
     "dependency_graph": <from PLAN>, "tasks": <from DECOMPOSE>, "slices": <from DECOMPOSE>,
     "risks": <from PLAN>, "open_questions": <from PLAN unchanged, or [] if none>,
     "gate_result": <checked quality-gate result>, "refinement_directives": <latest directives>,
     "phases": <from DECOMPOSE>, "checkpoints": <from DECOMPOSE> }
   ```
   Pass `open_questions` unchanged from PLAN; do not reconstruct or answer them from `context_summary`. The field is required even when empty. Stop on an input error, correct the context and render again before writing either document.
2. Create the run directory `~/Documents/zk-data/skills/task-breakdown/{date}-{run}/` (via `terminal`; built-in file tools cannot write under `~/Documents/zk-data`). Plans are skill artifacts, never repository files.
3. Write `plan.md` there with: overview, architecture decisions, phased task list with checkpoints, risks table, and the supplied open questions unchanged (or explicitly none for `[]`).
4. Include a Refinement History section in `plan.md` (PDCA loop visibility): when `refinement_directives` were applied across PDCA iterations, document what criterion scored above threshold, what was wrong, and what fix was applied. Omit the section if no refinement was needed.
5. Write `todo.md` there as a flat checklist grouped by phase with checkboxes for each task and its acceptance criteria — scannable, not verbose.
6. Emit `pko_anchors`: map the plan to `pko:Procedure` with `pko:StepVerification`; each task to `pplan:Step` with `pko:StepVerification`; phases to `pplan:MultiStep`; risks to `pko:IssueOccurrence`; the supplied open questions to `pko:UserQuestionOccurrence`; checkpoints to `pko:UserFeedbackOccurrence`. (The `pplan:` namespace terms resolve at the derived tier; the former `pko:ProcedureTarget`, `pko:Step`, and `pko:MultiStep` did not resolve and are replaced.)
7. Attach DC+BIBO state metadata (title/creator/date, `bibo:Document`) to the `plan.md` document itself — PKO grounds the structure, DC+BIBO grounds the document.
8. Do not invent tasks not present in the input `tasks` array.
9. Produce a JSON object with `plan_md`, `todo_md`, `output_paths`, and `pko_anchors`.

### Convergence loop

After a complete, input-valid TB-2 quality result, before write-plan (protocol errors/missing directives stop in the quality phase):

1. If `TB-2`'s `gate_weighted_total` exceeds 0.15 (or `gate_pass` is false) and `refinement_directives` exist, loop back to DECOMPOSE with directives as focused, task-addressable improvement targets.
2. `refinement_directives` are explicitly routed back to decompose (mechanical and documented).
3. Carry `prior_gate_weighted_total` forward so you can detect a stable-but-unconverged plan. Each iteration narrows the gap.
4. On convergence (`gate_weighted_total <= 0.15` and `gate_pass` true), proceed to write-plan with `gate_result`.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `task-breakdown-plan.j2` | PLAN phase — read-only mode. Grasp the current condition relative to the target condition: identify what exists now (patterns, conventions, existing modules), build the dependency graph, and note risks/unknowns. Anchored on target_condition (mapped from {{ task }}). Validates empty target_condition to prevent silent convergence on an empty plan. No code is written. Produces context summary, dependency graph, and risk register. |
| `task-breakdown-decompose.j2` | DO phase — single producer: decompose the target condition into component target conditions (sub-tasks) AND write each task in one step. Each task is a sub-target with acceptance criteria framed as "what must be true for this sub-target to be achieved." Each task carries slice_id/feature_path, acceptance criteria, verification, dependencies, files, scope (XS/S/M/L — no XL), and skill_match_query (a natural-language capability description consumed by skill-discovery (route) when the skill_catalog input is provided). Catastrophic plan conditions are checked deterministically by `TB-1` `lisp_eval`. The PDCA loop re-enters here so refinement directives are task-addressable and re-slicing + re-writing happen together. |
| `task-breakdown-quality-gate.j2` | Independent CHECK quality gate — scores the plan against seven weighted criteria: target condition coverage (0.20), task sizing (0.20, including task-count awareness for >20 or <3 tasks), vertical-slice integrity (0.15), acceptance-criteria specificity (0.15), dependency ordering (0.10), checkpoint presence (0.10), red-flag absence (0.10). Receives context_summary for project-specific convention checking (Good Regulator) and emits gate_findings, task-addressable refinement_directives, and red_flags; `TB-2` `lisp_eval` computes `gate_weighted_total` and `gate_pass`. |
| `task-breakdown-write-plan.j2` | ACT phase — finalize the plan into plan.md (target condition, overview, architecture decisions, phased task list with checkpoints, risks, open questions) and todo.md (checklist-style task list), with a pko_anchors map giving each element a PKO process-axis identity. Includes the target condition at the top of plan.md so the plan is always anchored to what it's achieving. Includes Refinement History section in plan.md documenting what was refined across PDCA iterations, making the loop visible in the artifact. |

To render a template, call the `render_template` tool with the template ref (e.g., `task-breakdown/task-breakdown-plan`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `task-breakdown-plan.j2`: `target_condition`, `crate_scope`, `code_context`, `task`
- `task-breakdown-decompose.j2`: `context_summary`, `dependency_graph`, `risks`, `refinement_directives`, `parallelization_hint`, `skill_catalog`
- `task-breakdown-quality-gate.j2`: `tasks`, `slices`, `dependency_graph`, `context_summary`, `target_condition`, `phases`, `checkpoints`, `task_count`, `task_count_penalty` (both unchanged from TB-COUNT)
- `task-breakdown-write-plan.j2`: `context_summary`, `target_condition`, `dependency_graph`, `tasks`, `slices`, `risks`, `open_questions` (unchanged from PLAN), `gate_result`, `refinement_directives`, `phases`, `checkpoints`

## Regression case

Run TB-COUNT at counts 2/3/20/21: penalties are 0.10/0/0/0.10. Feed the actual count result and critic base scores into TB-2: check raw scores stay unchanged, sizing is adjusted once/clamped, totals match hand-computed expectations and the 0.15/0.30 boundaries remain exact. A positive count-adjusted sizing score with no sizing directive must report `directives_complete: false` and cannot pass; a criterion reference does not prove useful semantic text. Invalid scores/penalties stop explicitly. Run TB-1 on an empty task list (`["empty_decomposition"]`) and a healthy multi-task list (`[]`). These are execution cases to run, not claims of future task quality.

## Constraints

- `task-breakdown-plan.j2`: Read-only mode — no code proposals, file edits, or implementation sketches. Empty-spec validation is mandatory before producing any output.
- `task-breakdown-decompose.j2`: Every task is a vertical feature path, not a horizontal layer. No task may be XL. No "and" in a task title. The "~5 files" limit is advisory for cross-crate Rust features (legitimate 5–7 file touches allowed with justification). Every task must have acceptance criteria AND a verification step AND declared dependencies (or "None"). Dependency order must be respected. `skill_match_query` is required per task when `skill_catalog` is provided, omitted otherwise. Catastrophic conditions are gated via `TB-1` `lisp_eval`.
- `task-breakdown-quality-gate.j2`: semantic base scoring/text remains independent. TB-COUNT supplies count facts; the critic never adds the penalty. TB-2 alone emits the final object with original/effective scores, weighted grade and required-directive coverage. Do not accept invalid input or incomplete references, double-apply a penalty, or treat mechanical coverage as semantic correctness.
- `task-breakdown-write-plan.j2`: Both files must be complete, self-contained markdown. `plan.md` must include overview, architecture decisions, phased task list with checkpoints, risks table, open questions, and (when refinement occurred) Refinement History. `todo.md` must be a checklist grouped by phase. Do not invent tasks not present in the input `tasks` array.
