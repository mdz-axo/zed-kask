# Principle-Constraints Skill — Status

## All tasks complete

### Cascade (end-to-end, tested 2026-08-19)

Both derive and verify modes work end-to-end.

**Manifest** (`kask/registry/manifests/principle-constraints.yaml`):
- 2-step cascade: `select` (LLM inference) → `loop` (convergence)
- Convergence signal: `{{ (step_1_result.summary.enforced | default(0)) + (step_1_result.summary.gaps | default(0)) }}`
- `code_context` input parameter for caller-provided codebase access
- No `max_tokens`, no `gas_cap`, no `output_schema`

**Derive template** (`kask/registry/templates/principle-constraints/principle-derive.j2`):
- Emits JSON, `thinking_budget = "on"`, no `max_tokens`
- Accepts `code_context` for caller-provided codebase access

**Verify template** (`kask/registry/templates/principle-constraints/principle-verify.j2`):
- Emits JSON, `thinking_budget = "on"`, no `max_tokens`
- Accepts `existing_constraints` and `code_context`
- Emits per-constraint drift reports with `previous_status`, `current_status`, `drift` kind

**Tests** (`kask/crates/hkask-lisp/src/hkask_lisp.rs`):
- `test_principle_constraints_form_with_string` — `listp` guard returns 0 for string input
- `test_principle_constraints_form_with_object` — form extracts `summary.enforced + summary.gaps` from JSON object
- Both pass

**CI gates**:
- `check-skill-span-namespace.sh`: 69 manifests conform
- `check-principle-constraints.sh`: passes (empty state — no constraints persisted yet)
- Lisp tests pass

### Persistence infrastructure

**Constraint set file** (`kask/docs/architecture/principle-constraints.yaml`):
- Empty state (`principles: []`) — ready for human-approved constraint sets
- Format: one entry per principle with constraint_set, gaps, summary

**CI hook** (`kask/scripts/check-principle-constraints.sh`):
- Verifies `enforced` constraints: checks `enforced_at` file exists, checks `falsifier` test exists via grep
- Reports `gap` constraints as warnings (not failures)
- Fails (exit 1) if any enforced constraint has drifted
- Passes (exit 0) when no constraints or all enforced constraints intact
- Tested with both valid and drifted fixtures

### Codegraph decision

The `codegraph_query` execute step was intentionally not added. The step's `on_failure` config can only escalate (`report`/`halt`/`escalate` all exit the cascade via `Effect::Exit(Escalated)`), so a codegraph server outage would abort the entire derivation. Instead, callers pass pre-gathered context via the `code_context` input parameter. This is simpler and doesn't introduce a failure dependency on the codegraph MCP server.

### max_tokens cleanup

Removed `max_tokens` from all skill templates:
- `kask/registry/templates/principle-constraints/principle-derive.j2`
- `kask/registry/templates/principle-constraints/principle-verify.j2`
- `kask/registry/templates/algedonic-review/present-triage.j2`
- `kask/registry/templates/algedonic-review/verify-cleared.j2`
- `kask/registry/templates/algedonic-review/execute-decisions.j2`
- `kask/registry/templates/algedonic-review/triage-briefing.j2`
- Updated stale `max_tokens` comments in `kask/registry/manifests/metacognition.yaml`

### End-to-end results

| Principle | Mode | Constraints | Enforced | Gaps |
|-----------|------|-------------|----------|------|
| P1 (conclusions never promoted) | derive | 7 | 5 | 2 |
| P1 (conclusions never promoted) | verify | 6 | 5 | 1 |
| P6 (differential trust tiers) | derive | 7 | 0 | 7 |
| P5 (one discipline applied twice) | derive | 9 | 1 | 8 |
| P7 (forecast parameter impact check) | derive | 10 | 0 | 10 |

## Decisions deferred to human

- Which P6/P5/P7 gap remediations to implement
- Whether to persist any of the derived constraint sets to `kask/docs/architecture/principle-constraints.yaml`
- Whether to add a `compute` step with `lisp.eval` for iterative refinement (currently single-pass)
