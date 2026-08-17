# Task List: Skill Bundler Improvement + Grounding Refactor Follow-ups

Created: 2026-08-16
Status: Open

## 1. Skill Bundler — Execution Failure & Performance Issues

### 1.1 BUG: JSON parse failure on multi-skill composition (P0)

**Symptom:** `skill_bundle` with 4 skills (`refactor-architecture`, `essentialist`, `grill-me`, `metacognition`) failed with:
```
Manifest execution failed: Manifest error: Step 3: Failed to parse JSON response:
expected value at line 1 column 1
```

**Root cause (inferred):** The skill-bundler manifest's templates use `tool_choice: Auto` (or no explicit `tool_choice`), so the LLM returns prose instead of calling the structured-output tool. This is the same bug pattern fixed in `kask/crates/kask_bridge/src/inference.rs:480` (`Auto` → `Any`) for KnowAct templates. The bundler's composition templates need the same fix.

**Action:** Audit the `skill-bundler` manifest (`kask/registry/manifests/skill-bundler.yaml`) and its `.j2` templates for `contract.output` frontmatter. Any template with structured output must use `tool_choice: Any` (maps to `Required` in OpenAI, `Any` in Anthropic).

### 1.2 PERF: Skill bundler does not use concurrency (P1)

**Problem:** The bundler composes peer-level skills sequentially via a cascade. When 3-4 skills are independent (e.g., `essentialist`, `grill-me`, `metacognition` all review the same code), they should run concurrently via `action: parallel` with `join: allSettled`.

**Action:** Update the `skill-bundler` manifest to use `parallel` steps for independent skill branches. The bundler already knows which skills are peers (that's the bundler's purpose) — it should emit a `parallel` step in the composed `BundleManifest` rather than a sequential cascade.

### 1.3 PERF: Skill bundler uses LLM for deterministic steps (P1)

**Problem:** Steps like "filter findings for soundness," "score composition quality," "resolve conflicts between skills" are deterministic operations that the bundler delegates to the LLM. These should use `lisp.eval` compute steps.

**Action:** Replace LLM-based filtering/scoring steps in the `skill-bundler` manifest with `lisp.eval` compute steps. For example:
- Composition score: `(let ((n_skills (length skills)) (n_conflicts (length conflicts))) (- (+ n_skills (* 0.5 n_conflicts)) n_conflicts))`
- Conflict detection: `(filter (lambda (s) (eq (assoc "severity" s) "blocker")) findings)`

### 1.4 TOKEN: Skill bundler is token-inefficient (P2)

**Problem:** The bundler re-renders the full task context into each skill's template, producing large intermediate results that are then re-processed by downstream steps. For a 4-skill bundle, the task context is rendered 4+ times.

**Action:**
- Use `input_mapping` to pass references (`{{ step_N_result }}`) instead of re-rendering the full context.
- Compact intermediate results before passing to downstream steps (e.g., extract only the findings, not the full skill output).
- Consider a `compact` compute step that uses `lisp.eval` to extract only the load-bearing fields from a large intermediate result.

### 1.5 FUNC: Skill bundler lacks error recovery (P2)

**Problem:** When one skill in a bundle fails (like the JSON parse above), the entire bundle fails. There is no `on_failure: report` or `join: allSettled` semantics — a single skill failure aborts the composition.

**Action:** The composed `BundleManifest` should use `join: allSettled` for parallel branches and `on_failure: report` for sequential steps, so a single skill failure degrades gracefully rather than aborting the bundle.

---

## 2. Grounding Refactor — Adversarial Review Findings

### 2.1 BUG: `enforce_for_agent` miscounts non-object outputs as coverage gaps (P0)

**File:** `kask/crates/hkask-verification/src/ledger.rs:203-209`

**Bug:** When a contract EXISTS for the agent_type but the output is not a JSON object, `enforce_for_agent` calls `record_coverage_gap()` which writes `had_contract: false`. But the doc comment (lines 180-184) says "The trend query counts this as 'with contract' (the contract matched) but not 'zero nulled'."

**Contradiction:** The code writes `had_contract: false` → the trend counts it as `delegations_without_contract` (a coverage gap). The doc says it should be counted as `with_contract` but unmeasured. The code is wrong; the doc is right.

**Impact:** The operator sees a coverage gap ("write a contract") when the contract exists but the agent produced prose. The remediation is different: a coverage gap means "write a contract"; a non-object output means "fix the agent's system prompt to produce JSON."

**Fix:** Add a third record type — `record_contract_unenforceable()` — that writes `had_contract: true` with empty `nulled_fields` and `narrative_leaks`. The trend query's `had_contract: true` + empty nulled_fields path counts it as `delegations_with_zero_nulled`, which is also wrong (it's not clean, it's unmeasured). The correct fix is to add a `measured: bool` field to `GroundingRecord` or to use a sentinel value (e.g., `nulled_fields: vec!["__unenforceable__".to_string()]`) — but that's a hack. The clean fix is a `GroundingRecord::unenforceable()` constructor with `had_contract: true` and a new `was_measured: false` field, and the trend query counts unmeasured records separately (like the old `delegations_unmeasured` bucket that was removed).

**Status:** Not yet fixed. This is the highest-priority bug.

### 2.2 SMELL: Grounding wiring duplicated across 3 call sites (P1)

**Files:**
- `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:1318-1405` (`spawn_via_local_runtime`)
- `kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:96-133` (`swarm_delegate_local`)
- `kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:1407-1438` (`swarm_execute_plan_local`)

**Problem:** The grounding enforcement pattern is duplicated 3 times:
1. Parse response as JSON (`serde_json::from_str`)
2. Clone raw response
3. Call `enforce_for_agent()`
4. If `Some(gr)`: warn on nulled fields, replace response with cleaned JSON, set `raw_response`
5. If `None` and output was object: set `raw_response`

The kata-kanban version also does schema validation and envelope building (steps the swarm versions skip). This is a copy-paste hazard — a future change to the grounding wiring must be applied in 3 places.

**Fix:** Extract a helper function on `VerificationStore`:
```rust
pub fn enforce_and_stamp(
    &self,
    source: &str,
    agent_id: &str,
    agent_type: &str,
    result: &mut LocalDelegateResult,
) -> Option<GroundingResult>
```
This helper encapsulates the parse → enforce → warn → replace → retain pattern. The kata-kanban version can then do its additional schema validation + envelope building on the returned result. The swarm versions just call the helper.

**Blocker:** `LocalDelegateResult` lives in `hkask-mcp-swarm`, and `VerificationStore` lives in `hkask-verification`. The helper can't take `&mut LocalDelegateResult` without a circular dependency. Options:
- (a) Move `LocalDelegateResult` to `hkask-verification` (it's a shared type).
- (b) Return a struct `EnforcementOutcome { result: Option<GroundingResult>, cleaned: Value, raw: String }` and let the caller stamp the fields.
- (c) Use a trait `GroundableResult` that both `LocalDelegateResult` and future types implement.

Option (b) is the simplest and doesn't require moving types.

### 2.3 SMELL: `GroundingRecord.provenance` is cloned but never queried (P2)

**File:** `kask/crates/hkask-verification/src/types.rs:42`

**Problem:** `GroundingRecord` carries a full `HashMap<String, ProvenanceTag>` cloned from `GroundingResult`. But neither `grounding_trend()` nor `grounding_violations()` reads the `provenance` field — they only check `nulled_fields` and `narrative_leaks`. The provenance data is written to the ledger and never read.

**Impact:** Every delegation clones a `HashMap` of provenance tags that is serialized to JSON, stored in SQLite, and never queried. This is wasted storage and compute.

**Options:**
- (a) Remove `provenance` from `GroundingRecord` (it's in the envelope already, which is logged at `debug!`).
- (b) Keep it for future use but add a `grounding_provenance()` query that actually reads it.
- (c) Make it `Option<HashMap<...>>` and only populate it when `had_contract: true` (currently always populated for contract records, always empty for coverage gaps — so the `Option` doesn't help).

Recommend (a) — the provenance is already in the cleaned JSON (as `<field>_provenance` stamps) and in the envelope. Storing it a third time in the ledger is redundant.

### 2.4 PERF: `query_records` loads ALL records into memory, then filters in Rust (P2)

**File:** `kask/crates/hkask-verification/src/ledger.rs:318-336`

**Problem:** `query_records` calls `store.query_by_entity(VERIFICATION_ENTITY)` which returns ALL grounding records, then filters by scope in Rust. For a long-running system with thousands of delegations, this loads every record into memory on every trend query.

**Fix:** Use `query_by_entity_attribute` or add a SQL-level filter. The `HMemStore` API supports `query_by_entity` and `query_by_attribute` but not arbitrary SQL filters. The scope filter (`ByAgent`, `BySource`) would need to be done in Rust unless we change the entity/attribute scheme. For now, this is acceptable (grounding queries are infrequent — the curator calls them on gemba walks, not per-delegation). But it should be documented as a known scaling limit.

### 2.5 SECURITY: `VerificationStore::open()` default passphrase "allostery" (P2)

**File:** `kask/crates/hkask-verification/src/ledger.rs:112-113`

**Problem:** The default passphrase `"allostery"` is hardcoded. Any process on the machine can open the verification DB. The `.rules` says "All DBs should be encrypted at rest; using a hardcoded public key provides zero confidentiality."

**Mitigation:** The default is for pre-release dev setups only. In production, `HKASK_VERIFICATION_PASSPHRASE` should be set via the keychain. But the default should log a `warn!` (like the kanban server does when the passphrase is missing) rather than silently using the hardcoded value.

**Fix:** Add a `warn!` when the default passphrase is used:
```rust
let passphrase = match std::env::var("HKASK_VERIFICATION_PASSPHRASE") {
    Ok(p) => p,
    Err(_) => {
        tracing::warn!(
            target: "hkask.verification",
            "HKASK_VERIFICATION_PASSPHRASE not set — using default 'allostery'. \
             This provides zero confidentiality in a multi-user environment."
        );
        "allostery".to_string()
    }
};
```

### 2.6 SMELL: `curator_grounding_violations` reconstructs `GroundingTrendToolRequest` (P3)

**File:** `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:897-901`

**Problem:** The `curator_grounding_violations` tool reconstructs a `GroundingTrendToolRequest` from the `GroundingViolationsToolRequest` fields to pass to `parse_grounding_scope()`. This is a type smell — the scope parsing should work on a trait or shared struct, not require reconstructing a different request type.

**Fix:** Extract the scope fields into a shared `GroundingScopeFields` struct (or just pass the three fields directly to `parse_grounding_scope`).

### 2.7 GAP: No `grounding_coverage` per-agent-type breakdown (P3)

**File:** `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:939-960`

**Problem:** The `curator_grounding_coverage` tool returns aggregate counts (`total_delegations`, `with_contract`, `without_contract`) but doesn't break down the coverage gap by agent_type. The operator sees "5 delegations without a contract" but not which agent_types those are. The doc says "see which agent types need a contract written" but the tool doesn't actually return that information.

**Fix:** The `grounding_coverage` query should group records by `agent_type` and return a per-type breakdown:
```json
{
  "agent_types": [
    {"agent_type": "task", "delegations": 10, "had_contract": true},
    {"agent_type": "research", "delegations": 5, "had_contract": false}
  ]
}
```

### 2.8 GAP: Proptest `violations_are_a_subset` assertion is fragile (P3)

**File:** `kask/crates/hkask-verification/src/ledger.rs:972-978`

**Problem:** The proptest asserts `violations.len() == delegations_with_nulled`. This is currently correct because narrative leaks only come from `Unsourced` tags (which are also in `nulled_fields`). But if a future change adds a narrative leak source that doesn't require a nulled field, this assertion will break silently (the proptest will fail, but the operator may not understand why).

**Fix:** The assertion should be `violations.len() == delegations_with_nulled + delegations_with_leaks_only` where `delegations_with_leaks_only` is the count of delegations that have narrative leaks but no nulled fields. Currently this is always 0, but the assertion should be explicit about the relationship.

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
