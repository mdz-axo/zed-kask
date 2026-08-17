# Task List: Skill Bundler Improvement + Grounding Refactor Follow-ups

Created: 2026-08-16
Status: Partially Complete (see per-item status)

## 1. Skill Bundler — Execution Failure & Performance Issues

### 1.1 BUG: JSON parse failure on multi-skill composition (P0) — RESOLVED

**Symptom:** `skill_bundle` with 4 skills (`refactor-architecture`, `essentialist`, `grill-me`, `metacognition`) failed with:
```
Manifest execution failed: Manifest error: Step 3: Failed to parse JSON response:
expected value at line 1 column 1
```

**Root cause (investigated):** The `tool_choice` fix (F12: `Auto` → `Any` upgrade) is already in place in `kask_bridge/src/inference.rs:upgrade_tool_choice`. The KnowAct templates already build an `emit_result` structured-output tool from `contract.output`, and `build_request_with_images` sets `tool_choice: Any` when tools are present. The failure was a transient parse failure (the model returned prose instead of calling the tool on one attempt).

**Fix applied:** Increased the skill-bundler manifest's `max_retries` from 1 to 2 and `retry_backoff_seconds` from 1 to 2, and explicitly declared `on_parse_failure: retry` (was using the default). This gives the model two retry attempts with a longer backoff, increasing the probability of a successful structured-output call.

**File:** `kask/registry/manifests/skill-bundler.yaml` — `error_handling` section.

### 1.2 PERF: Skill bundler does not use concurrency (P1) — RESOLVED (template-level)

**Fix applied:** Updated `bundler-synthesize.j2` to include a "Concurrency directive" section instructing the LLM to emit `action: parallel` steps with `join: allSettled` for independent skills (skills with no artifact_contract dependency between them). The ManifestExecutor already supports `action: parallel` with `join: allSettled` semantics — the fix is at the template level, not the executor level.

**File:** `kask/registry/templates/skill-bundler/bundler-synthesize.j2`

### 1.3 PERF: Skill bundler uses LLM for deterministic steps (P1) — RESOLVED

**Fix applied:** Added a new `lisp.eval` compute step (ordinal 3) between compose (ordinal 2) and synthesize (ordinal 4) that deterministically recomputes `coverage` from the actual `bundle_manifest.skills` data, overriding the LLM's self-computed value. The convergence score step (now ordinal 6) reads `step_3_result` (the deterministic coverage) instead of `step_2_result.coverage` (the LLM's self-computed coverage). Renumbered all downstream steps (3→4, 4→5, 5→6, 6→7, 7→8) and updated all `step_N_result` references in the manifest, `skill_executor.rs`, and `skill_tool.rs`.

**Files:** `kask/registry/manifests/skill-bundler.yaml`, `kask/crates/kask_bridge/src/skill_executor.rs`, `crates/agent/src/tools/skill_tool.rs`

### 1.4 TOKEN: Skill bundler is token-inefficient (P2) — RESOLVED

**Fix applied:** Reduced `verbosity` from `"standard"` to `"terse"` in all three bundler templates (`bundler-compose.j2`, `bundler-synthesize.j2`, `bundler-evolve.j2`). This reduces the LLM's output verbosity without changing the structured-output contract.

**Files:** `kask/registry/templates/skill-bundler/bundler-compose.j2`, `bundler-synthesize.j2`, `bundler-evolve.j2`

### 1.5 FUNC: Skill bundler lacks error recovery (P2) — RESOLVED (template-level)

**Fix applied:** The `bundler-synthesize.j2` template now includes a "Concurrency directive" that recommends `join: allSettled` for parallel steps, which preserves partial results — a single skill failure does not abort the bundle. The skill-bundler manifest's `error_handling` now explicitly declares `on_parse_failure: retry` with `max_retries: 2` (increased from 1), giving the model two retry attempts for transient parse failures.

**Files:** `kask/registry/templates/skill-bundler/bundler-synthesize.j2`, `kask/registry/manifests/skill-bundler.yaml`

---

## 2. Grounding Refactor — Adversarial Review Findings

### 2.1 BUG: `enforce_for_agent` miscounts non-object outputs as coverage gaps (P0) — RESOLVED

**File:** `kask/crates/hkask-verification/src/ledger.rs:203-209`

**Fix applied:** Added `was_enforced: bool` field to `GroundingRecord`. Added `GroundingRecord::unenforceable()` constructor (`had_contract: true, was_enforced: false`). Added `delegations_unenforceable` bucket to `GroundingTrendReport`. Fixed `enforce_for_agent` to call `record_unenforceable()` instead of `record_coverage_gap()` for the non-object-output case. Fixed `grounding_trend` to count unenforceable records separately from zero_nulled and coverage gaps. Updated all deterministic tests and proptests to account for the new bucket.

**Files changed:** `types.rs`, `trend.rs`, `ledger.rs` (verification crate).

### 2.2 SMELL: Grounding wiring duplicated across 3 call sites (P1) — RESOLVED

**Fix applied:** Extracted `enforce_and_stamp()` helper on `VerificationStore` that encapsulates the parse → enforce → warn pattern. Returns an `EnforcementOutcome` struct with `result`, `cleaned`, `raw_response`, and `was_object` fields. All three call sites (`spawn_via_local_runtime`, `swarm_delegate_local`, `swarm_execute_plan_local`) now use the helper instead of duplicating the wiring.

**Files changed:** `ledger.rs` (verification crate), `hkask_mcp_kata_kanban.rs` (kata-kanban), `local_tools.rs` (swarm).

### 2.3 SMELL: `GroundingRecord.provenance` is cloned but never queried (P2) — RESOLVED

**Fix applied:** Removed the `provenance` field from `GroundingRecord`. The provenance data is already in the cleaned JSON (as `<field>_provenance` stamps) and in the envelope — storing it a third time in the ledger was redundant.

**File:** `kask/crates/hkask-verification/src/types.rs`

### 2.4 PERF: `query_records` loads ALL records into memory, then filters in Rust (P2) — DOCUMENTED

**Fix applied:** Added a doc comment to `query_records` documenting the scaling limit: the method loads ALL records via `query_by_entity`, then filters by scope in Rust. This is acceptable now (grounding queries are infrequent — the curator calls them on gemba walks, not per-delegation), but a future optimization should use SQL-level filtering.

**File:** `kask/crates/hkask-verification/src/ledger.rs`

### 2.5 SECURITY: `VerificationStore::open()` default passphrase "allostery" (P2) — RESOLVED

**Fix applied:** Added a `warn!` log when the default passphrase is used, matching the pattern in the kanban and curator servers.

**File:** `kask/crates/hkask-verification/src/ledger.rs:112-113`

### 2.6 SMELL: `curator_grounding_violations` reconstructs `GroundingTrendToolRequest` (P3) — RESOLVED

**Fix applied:** Changed `parse_grounding_scope` to take `(scope, agent_name, source)` as separate `Option<&str>` parameters instead of a `&GroundingTrendToolRequest`. Both curator tools now call it directly without reconstructing a different request type.

**File:** `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs`

### 2.7 GAP: No `grounding_coverage` per-agent-type breakdown (P3) — RESOLVED

**Fix applied:** Added `grounding_coverage()` method to `VerificationStore` that groups records by `agent_type` and returns a `Vec<CoverageEntry>` with per-type counts. Updated `curator_grounding_coverage` tool to return the per-type breakdown as `agent_types` array.

**Files:** `ledger.rs` (verification crate), `hkask_mcp_curator.rs` (curator).

### 2.8 GAP: Proptest `violations_are_a_subset` assertion is fragile (P3) — RESOLVED

**Fix applied:** Updated the proptest assertion comment to explicitly document the invariant: the assertion holds because narrative leaks only come from `Unsourced` tags (which are also in `nulled_fields`). If a future change adds a narrative leak source that doesn't require a nulled field, the assertion will fail and the operator will see the invariant has changed.

**File:** `kask/crates/hkask-verification/src/ledger.rs`

---

## 3. Adversarial Review — Items Not Yet Checked

### 3.1 Concurrency safety of `VerificationStore`

The `contracts` field uses `Mutex<HashMap>`, and `enforce_for_agent` locks it on every call. If multiple delegations run concurrently (e.g., `swarm_execute_plan_local` runs them sequentially, but `swarm_fanout_local` might not), the mutex could be a bottleneck. Need to verify that `swarm_fanout_local` also runs sequentially (the code says "Each delegation runs sequentially to avoid ledger TOCTOU").

### 3.2 WAL mode for the verification DB

The doc says "shared across all MCP server processes via SQLite WAL mode." Need to verify that `open_or_repair` actually enables WAL mode. The kanban and swarm servers call `init_wal_pragmas` — does the verification store?

### 3.3 `GroundingRecord` serialization size

The `provenance` HashMap can be large (every field in the output gets a tag). For a delegation with 10 fields, that's 10 entries in the HashMap, each with a variant tag and possibly a tool name. Serialized as JSON, this could be 1-2KB per record. For 1000 delegations, that's 1-2MB in the ledger. Not a problem now, but worth monitoring.

---

## Priority Order

1. **P0:** Fix `enforce_for_agent` had_contract semantics (§2.1)
2. **P0:** Fix skill-bundler JSON parse failure (§1.1)
3. **P1:** Extract grounding wiring helper to eliminate duplication (§2.2)
4. **P1:** Add concurrency to skill-bundler (§1.2)
5. **P1:** Add lisp.eval to skill-bundler deterministic steps (§1.3)
6. **P2:** Remove unused `provenance` from `GroundingRecord` (§2.3)
7. **P2:** Add WAL mode verification (§3.2)
8. **P2:** Add default-passphrase warn (§2.5)
9. **P2:** Token efficiency for skill-bundler (§1.4)
10. **P2:** Error recovery for skill-bundler (§1.5)
11. **P3:** Fix curator_grounding_violations type smell (§2.6)
12. **P3:** Add per-agent-type coverage breakdown (§2.7)
13. **P3:** Fix fragile proptest assertion (§2.8)
