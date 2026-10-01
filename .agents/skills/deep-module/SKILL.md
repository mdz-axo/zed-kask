---
name: deep-module
core: true
description: "Module design discipline based on Ousterhout's 'A Philosophy of Software Design.' Apply the deletion test to evaluate whether a module deserves to exist. Enforces depth, interface minimalism, and dependency direction."
---


# Deep Module

Module design discipline based on John Ousterhout's *A Philosophy of Software Design*. Apply the deletion test to evaluate whether a module deserves to exist: delete the callers — if complexity reappears, extract. Delete the module — if complexity vanishes, don't create it. Enforces depth (high benefit/cost ratio), interface minimalism (≤7 public functions — a kask operationalization; Ousterhout argues depth qualitatively), and dependency direction.

## When to Use

- You are evaluating whether an existing module is deep (small interface, much behavior) or shallow (large interface, thin behavior)
- You need to decide whether a module deserves to exist — should it be kept, extracted, deepened, merged, or deleted
- You are designing a new module interface and want to maximize depth from the start
- You need to check whether module-depth design recommendations have converged across iteration cycles
- You suspect a module is a pass-through, data bag, or abstraction-for-one
- You want to enforce the ≤7 public function target and unified error/config design

## When NOT to Use

- Evaluating non-code artifacts (skills, docs, processes) — the deletion test and surface count are defined over code modules with public items.
- Designing a module from scratch — the gates interrogate an existing artifact; for greenfield interface design, use the design step with the ≤7 target as input.
- A module already known shallow — skip the assessment and deepen directly.

## D/P labelling

The deletion-test verdict (the matrix's EXTRACT / DEEPEN / MERGE / DELETE) and the reduction
recommendations are P — judgment, critiqued by the human in advisory mode and
by the re-run gates. The interface inventory, the depth score (computed with
`lisp_eval` over the counted public surface — the same denominator as the
assess template), and the convergence check (the last three recorded
public-interface counts equal) are D; the mechanical counting itself is D in
substance (grep-able, itemized in the inventory — the auditable receipt), with
the behavior-line estimate P when the source is unavailable (the report says
which). Red-flag identification and the behavior lists are P — judgment over
the counted inventory, critiqued by the human. The design projection is the D
form over P-estimated inputs — its report says which inputs were estimated. A depth score the model states but
does not compute from the counted items is not a score.

## Instructions

### 1. Assess Module Depth

1. Count every public item in the module mechanically: public functions (`pub` / `pub(crate)`), public types (struct, enum, type alias), public traits, public constants, and public sub-modules. Do not count impl blocks, tests, or re-exports.
2. Estimate the behavior encapsulated: count non-comment, non-blank implementation lines (including private helpers and impl blocks), list invariants enforced, and list complexity managed on behalf of callers.
3. Compute the depth score and class with `lisp_eval` (D) — the same denominator as `deep-module-assess.j2` (every public item counted in step 1, sub-modules included):
   - form: `(if (= items 0) (list nil "Empty") (let ((s (/ lines items))) (list s (cond ((>= s 100) "Deep") ((>= s 50) "Adequate") ((>= s 20) "Shallow") (t "Very Shallow")))))`
   - env: `{ "lines": <behavior lines>, "items": <total_interface_items> }`
   Count behavior lines mechanically when the source is available; an estimated count makes the score an inference, and the report must say which.
4. Classify the module from that result: Deep (100+), Adequate (50–99), Shallow (20–49), Very Shallow (0–19), or Empty (no public items).
5. Identify red flags: more public functions than private (pass-through suspicion), more public types than functions (data bag), all public functions delegating to a single dependency (pass-through), zero invariants enforced (no encapsulation), or single consumer (inline candidate).
6. Produce recommendations even if the depth score is acceptable — flag all red flags.

### 2. Execute the Deletion Test

1. **Direction 1 — Caller's perspective**: For each caller, imagine inlining the module's logic at the call site. Determine what complexity would reappear: state management, error handling, invariant enforcement, coordination logic. Assess whether the inline replacement is trivial (a few lines) or substantial.
2. **Direction 2 — Module's perspective**: For each public function, imagine replacing it with a direct call to the module's dependency. Determine whether any behavior would be lost, any invariants broken, or any complexity management eliminated.
3. Apply the decision matrix: complexity reappears + behavior lost → **EXTRACT**; complexity reappears + complexity vanishes → **DEEPEN**; trivial replacement + behavior lost → **MERGE**; trivial replacement + complexity vanishes → **DELETE**.
4. Produce a definitive recommendation with a rationale citing concrete complexity examples, not vague claims. If the module has a single consumer, flag it as an inline candidate.

### 3. Design the Deep Module Interface

1. Define the core operation — the one thing this module does. If you cannot describe it in one sentence, the module is too broad.
2. Add public functions only when the operation cannot be accomplished by combining the core operation with something else, serves a different caller need, and would cause significant complexity if callers implemented it themselves. Target ≤7 public functions total; if exceeded, split the module.
3. Design minimal public types: prefer enums over structs with many optional fields; expose only what callers need.
4. Hide information: keep algorithms, data structures, caching, internal state, business rules, validation logic, and the identity of dependencies private.
5. Design one unified error enum per module: map dependency errors to module-level variants (never leak dependency error types), add context to each variant.
6. Design one config struct: passed at construction time, validated on construction (fail early), with defaults for optional values.
7. Project the depth score with the same step-1.3 form over the designed interface's total public items (functions + types + traits + constants + sub-modules), so projected and assessed depth are comparable. Target ≥100 (Deep), minimum ≥50 (Adequate).

### 4. Check Convergence

1. Evaluate whether the deletion test passes (removing the module would cause complexity to reappear).
2. Check interface depth: public surface ≤7 items with justified exceptions.
3. Verify dependency direction: dependencies are acyclic and point toward stability.
4. Assess caller benefit: callers genuinely benefit from the abstraction (not pass-through).
5. Check that depth-improvement recommendations are specific and actionable.
6. The convergence signal is the public-interface item count (the result of step 3's `public_interface | length`). Track this signal across iterations and evaluate convergence with the Constraints' form: converged when the last three recorded counts are equal — checkable once three counts are recorded (two counts can never satisfy the form; verified live). Re-enter at assess after each iteration; stop when the signal has stabilized. When the design step is skipped (DELETE/MERGE recommendation), the result of step 3 is undefined and the signal defaults to 0, which is stable — convergence is reached once three counts are recorded.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `deep-module-assess.j2` | Assess module depth: enumerate public interface items, evaluate behavior complexity, compute depth score, classify as Deep/Adequate/Shallow/ Very Shallow. Identify interface cost drivers and behavior gaps. |
| `deep-module-delete.j2` | Execute the deletion test in both directions: caller's perspective (complexity reappears?) and module's perspective (complexity vanishes?). Produce a definitive keep/extract/don't create recommendation. |
| `deep-module-design.j2` | Design a deep module interface from deletion test results. Minimize public surface, hide information, design for caller mental model, unify config and errors. Produce a module specification with ≤7 public functions. |

To render a template, call the `render_template` tool with the template ref (e.g., `deep-module/deep-module-assess`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `deep-module-assess.j2`: `module_path`, `module_source`, `caller_paths`, `codebase_context`
- `deep-module-delete.j2`: `module_assessment`, `caller_code`, `dependency_code`, `codebase_context`
- `deep-module-design.j2`: `deletion_test_result`, `domain_requirements`, `dependency_interfaces`, `constraint_classification`


## Constraints

- Count public items mechanically — do not guess. Estimate behavior conservatively, erring toward undercounting.
- Apply both directions of the deletion test — never skip either.
- No more than 7 public functions per module. If the design exceeds 7, split the module.
- One error type per module — map, do not leak, dependency errors. One config struct, validated at construction.
- Hide everything that callers do not strictly require.
- Every recommendation carries a `constraint_force` field: `prohibition`, `guardrail`, `guideline`, `evidence`, or `hypothesis`.
- Depth score thresholds and the ≤7 cap are Evidence-tier kask operationalizations of Ousterhout's qualitative criteria (the repo's derived `deep_module` anchor is qualitative), not Ousterhout's own numbers and not Prohibitions.
- If `total_interface_items == 0`, return `classification: "Empty"` with `depth_score: null` — do not divide by zero.
- Design step is gated on `delete.recommendation in ['EXTRACT', 'DEEPEN']` — skipped for DELETE/MERGE.
- Evaluate convergence after each full iteration with `lisp_eval` over the recorded public-interface counts, oldest first: `(and (>= (length xs) 3) (= (nth (- (length xs) 1) xs) (nth (- (length xs) 2) xs)) (= (nth (- (length xs) 2) xs) (nth (- (length xs) 3) xs)))` — converged when the last three counts are equal (checkable once three counts are recorded; two counts can never converge under this form). Maximum 5 iterations, after which the remaining instability is reported, not iterated.

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, batch-10 audit):

- Depth form, Empty: `{"items": 0, "lines": 500}` → `[null, "Empty"]` (no
  division; `depth_score: null` per the Constraints).
- Depth form, Deep: `{"items": 2, "lines": 300}` → `[150.0, "Deep"]` (s = 150
  ≥ 100). The Adequate/Shallow/Very-Shallow classes are the same `cond` chain
  at their thresholds (50/20/below).
- Convergence form, converged: `{"xs": [3, 5, 5, 5]}` → `true` (the last three
  equal — the early instability is correctly ignored).
- Convergence form, unstable: `{"xs": [3, 5, 5, 4]}` → `false`.
- Convergence form, too few counts: `{"xs": [5, 5]}` → `false` — two counts
  can never converge (the pre-repair prose claimed "minimum 2 iterations";
  the form is the D oracle and requires three).

The skill's forms are executed at use time, never anchored in code.
