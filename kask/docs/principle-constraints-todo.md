# Principle-Constraints Skill — TODO

## Done

- [x] **Manifest created** (`kask/registry/manifests/principle-constraints.yaml`)
  - 2-step cascade: select (LLM inference) → loop (convergence)
  - `code_context` input parameter for caller-provided codebase access
  - `on_not_reached: abort`, `max_iterations: 1`, `cauchy_window: 1`
  - `gas_cap` field removed (dead config — not parsed by `BundleManifestStep`)

- [x] **Template created** (`kask/registry/templates/principle-constraints/principle-derive.j2`)
  - Emits JSON (not YAML — `execute_select` parses LLM output as JSON)
  - `thinking_budget = "on"` (not "off" which caused empty output, not "full" which caused truncation)
  - Accepts `code_context` for caller-provided codebase access
  - No `codegraph_results` variable (codegraph step removed)

- [x] **Convergence signal wired**
  - Loop step's `convergence_signal` extracts `summary.enforced + summary.gaps` from select step's JSON output via Jinja expression
  - Guards against non-object output via `default(0)`
  - No `compute` step (avoids `last_result_step` overwrite problem)

- [x] **Lisp tests added** (`kask/crates/hkask-lisp/src/hkask_lisp.rs`)
  - `test_principle_constraints_form_with_string` — verifies `listp` guard returns 0 for string input
  - `test_principle_constraints_form_with_object` — verifies form extracts `summary.enforced + summary.gaps` from JSON object
  - Both tests pass; comments accurately note the form is kept for future re-use

- [x] **CI gates pass**
  - `check-skill-span-namespace.sh`: 68 manifests conform
  - Lisp tests pass

- [x] **End-to-end validation on P1** (conclusions never promoted to verified fact)
  - Skill produces 7-8 constraints (5 enforced, 2-3 gaps)
  - All 5 enforced constraints match manual pass (correct assertions + falsifier names)
  - Additional gaps found: no implicit promotion path, multi-source weakest-link

- [x] **End-to-end validation on P6** (differential trust tiers)
  - 7 constraints, all gaps — confirms `Sourced` doesn't distinguish irrevocable/revocable

- [x] **End-to-end validation on P5** (one discipline applied twice)
  - 9 constraints (1 enforced, 8 gaps) — engineering-change side not enforced

- [x] **End-to-end validation on P7** (forecast parameter impact check)
  - 10 constraints, all gaps — no `impact_check` or `parameter_gate` located

## TODO

- [ ] **Task 1: Re-add codegraph_query execute step** (after in-process refactor lands)
  - The in-process refactor removing `gas_cap`/`max_tokens` as concepts broke the codegraph step's budget enforcement
  - Once the refactor lands, add a step 1 `execute` with `mcp: codegraph_query` and `on_failure: action: resume` (not `report` — `report` escalates the entire cascade)
  - Pass codegraph results to the select step as `codegraph_results` template variable
  - Re-add `codegraph_results` to the template's contract and discipline sections
  - Test that the codegraph step provides symbol-level context (names, file paths, line numbers) for more precise `enforced_at` citations

- [ ] **Task 2: Persistence with human approval** (Step 5 from original plan)
  - Write derived constraint sets to `kask/docs/architecture/principle-constraints.yaml` (one file, all principles, each with its constraint set and gap list)
  - The write must be gated on human approval — the skill's output is a proposal, not enforcement
  - Options for the approval gate:
    - (a) The skill emits the proposal; a human reviews and runs a separate `principle-verify` invocation to persist
    - (b) The skill has a `persist` mode that writes to disk only when `mode: persist` and `approved: true` are passed
    - (c) A separate CLI script or MCP tool writes the proposal to disk after human review
  - The persisted file should be machine-readable (YAML or JSON) for the CI hook to consume

- [ ] **Task 3: CI hook for constraint verification** (Step 5 from original plan)
  - Add `kask/scripts/check-principle-constraints.sh` that:
    - Reads `kask/docs/architecture/principle-constraints.yaml`
    - For each constraint with `status: enforced`, verifies the `enforced_at` path still exists and the `falsifier` test still passes
    - Fails if any `enforced` constraint has drifted (code moved, test deleted)
    - Reports `gap` constraints as warnings (not failures — gaps are findings, not violations)
  - This is the `principle-verify` template's job when run manually; the CI script is the automated version

- [ ] **Task 4: Wire `principle-verify` template** (maintenance mode)
  - The `principle-verify.j2` template exists but is not tested
  - It should take `existing_constraints` as input and emit drift reports
  - Test it by running verify mode on the P1 constraint set after a deliberate code change

## Decisions deferred to human

- Which of the P6/P5/P7 gap remediations to implement (extend `Sourced` with `trust_tier`, add CI for D-seam tests, add `impact_check` gate)
- Whether to add a `compute` step with `lisp.eval` for a richer convergence signal (requires solving the `last_result_step` overwrite problem — either by making `compute` use `StoredNamed` or by reordering steps)
- Whether the `output_schema` approach (structured-output tool calls) is worth pursuing for more reliable JSON output (it caused truncated output in testing)
