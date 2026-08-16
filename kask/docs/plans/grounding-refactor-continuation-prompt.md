# Continuation Prompt: Grounding Refactor — System Capability Extraction

## Context

You are continuing work on the verification ladder for the zed-kask agent ecology. The four-rung ladder (Presence → Truth → Grounding → Binding) is implemented and tested (390+ tests passing). The six-valued provenance vocabulary, card-declared grounding, schema validation, delegation-hop envelope, rollup trust, and LeakRule::Quantity matching are all live.

However, a design review revealed that **grounding is structured as a per-tool feature of the kanban tool, not as a system-level capability**. This continuation prompt is a full refactor to extract grounding into a shared crate with a central ledger, common query interface, and curator integration — making it a global function that checks and anchors every tool's output, queryable by the regulation and evaluation systems (gemba walks, curator feedback loop).

**There is no backward compatibility requirement.** This is strictly an exercise in updating and cleaning the code. Struct shapes can change freely, fields can be removed, tools can be renamed, code can move between crates. No migration paths for existing data are needed.

## The decision and its rationale

### The problem

Grounding enforcement code (`enforce_grounding`, `GroundingContract`, `GroundingResult`, `ProvenanceTag`, `schema_validate`, `envelope`, `card_contract`, `rollup_trust`) lives in `hkask-mcp-kata-kanban/src/` — trapped in the kanban MCP server crate. Grounding is wired only in `spawn_via_local_runtime` (kata-kanban). Grounding data is stored in two scattered, tool-specific locations:

1. **Kanban DB** — `grounding_summary` on `LocalDelegateResult`, per-task, per-board, overwritten on re-delegation (not append-only, not cross-board).
2. **Swarm stigmergy trail** — `delegation:grounding_had_contract` h_mems in `local_knowledge.rs`, per-agent, append-only but scoped to the swarm server (doesn't see kanban-grounded delegations).

Trend queries are tool-specific: `KanbanService::grounding_trend(board_id)` and `local_knowledge::grounding_trend(agent_id, limit)`. These have different shapes and scopes. There is no cross-tool, cross-server aggregation.

The curator has no access to grounding data. The regulation system (`hkask-regulation`) has no grounding sense input. The gemba walk skill doesn't surface grounding trends. There is no feedback loop from grounding violations → curator → user → action.

### The target

Grounding as a **system-level capability** with four layers:

1. **Shared enforcement code** (`hkask-verification` crate) — all verification modules in a crate any MCP server can depend on.
2. **Central grounding ledger** (shared store) — append-only, cross-tool, cross-server. Every grounded delegation writes a full `GroundingRecord`.
3. **Common query interface** — a single query surface (`VerificationStore`) that the curator, regulation system, and gemba walk use.
4. **Curator integration** — verification MCP tools on the curator server, feeding the cybernetic feedback loop: enforcement → ledger → curator → user → action → improved contracts → better enforcement.

### The cybernetic feedback loop

```
Every MCP server that delegates to agents
        │
        ▼
  VerificationStore::enforce_for_agent()  (shared code)
        │
        ├──► Returns (GroundingResult, cleaned_json) to caller
        │
        └──► Writes GroundingRecord to central ledger
                    │
                    ├──► Curator queries (curator_grounding_trend, _violations, _coverage)
                    │       └──► Gemba walk surfaces trends to user
                    │              └──► User acts (adjusts contracts, adds tools, retires agents)
                    ├──► Regulation system sense input (future — not in this refactor)
                    └──► Trend queries (cross-tool, cross-server)
```

## Target architecture

### Crate structure

```
kask/crates/hkask-verification/           (NEW — shared verification crate)
  Cargo.toml
  src/
    hkask_verification.rs                  (lib root, re-exports)
    grounding.rs                           (moved from kata-kanban)
    card_contract.rs                       (moved from kata-kanban)
    schema_validate.rs                     (moved from kata-kanban)
    envelope.rs                            (moved from kata-kanban)
    rollup_trust.rs                        (moved from kata-kanban)
    ledger.rs                              (NEW: VerificationStore, GroundingRecord)
    trend.rs                               (NEW: GroundingTrendReport, TrendScope, query functions)
    types.rs                               (NEW: shared types)
    error.rs                               (NEW: VerificationError)
```

### Dependency graph (after refactor)

```
hkask-verification (leaf — no deps on MCP servers)
  ├── hkask-storage (HMemStore)
  ├── hkask-types (WebID, HMemOntology, etc.)
  └── serde, serde_json, chrono, schemars, tracing, thiserror

hkask-mcp-swarm depends on hkask-verification  (enforcement + ledger write)
hkask-mcp-kata-kanban depends on hkask-verification  (enforcement + ledger write)
hkask-mcp-curator depends on hkask-verification  (ledger query — trend, violations, coverage)
```

### Data flow

```
kanban_task_spawn / swarm_delegate_local / swarm_execute_plan_local
        │
        ▼
  VerificationStore::enforce_for_agent(source, agent_id, agent_type, output, tool_calls, response)
        │
        ├── If contract exists for agent_type:
        │     ├── enforce_grounding() → (GroundingResult, cleaned_json)
        │     ├── schema_validate::validate() on cleaned_json
        │     ├── envelope::build() for hop provenance
        │     └── Write GroundingRecord to central ledger (append-only)
        │
        └── If no contract exists for agent_type:
              └── Write coverage-gap record to central ledger (had_contract: false)
```

### Central ledger storage

The `VerificationStore` wraps an `HMemStore` (same storage pattern as kanban and swarm memory). The DB file is at a standard path (`mcp/verification/grounding.db`), shared across all MCP server processes via SQLite WAL mode. The passphrase is `HKASK_VERIFICATION_PASSPHRASE` (default `"allostery"` for pre-release).

Grounding records are stored as h_mems:
- Entity: `verification:grounding`
- Attribute: `{delegation_uuid}` (a UUID generated per delegation)
- Value: JSON-serialized `GroundingRecord`
- Ontology: `HMemOntology::episodic("verification", "grounding", agent_id)`

This is append-only — each delegation writes a new record. The trend query reads all records and aggregates.

## What exists (current state — to be changed)

### Files to MOVE from `hkask-mcp-kata-kanban/src/` to `hkask-verification/src/`

| File | Contents | Notes |
|------|----------|-------|
| `grounding.rs` | `enforce_grounding()`, `GroundingContract`, `GroundingResult`, `ProvenanceTag`, `FieldSpec`, `LeakRule`, `NARRATIVE_LEAK_RULES`, `task_agent_contract()`, all helper functions, all tests | Pure functions, no storage. Update `crate::` references to be internal. |
| `card_contract.rs` | `validate()` for card-declared contracts | Called in `kanban_task_spawn`. |
| `schema_validate.rs` | `validate()` minimal JSON Schema validator (7 keywords) | Called after grounding. |
| `envelope.rs` | `build()` delegation-hop envelope | Built after grounding. |
| `rollup_trust.rs` | `ROLLUP_CONTRACTS` static contracts | Reference documentation. |

### Files to DELETE (dead code after refactor)

| Location | What | Why |
|----------|------|-----|
| `hkask-mcp-kata-kanban/src/kanban/service_impl/service.rs` | `KanbanService::grounding_trend()` method | Replaced by `VerificationStore::grounding_trend()` |
| `hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs` | `kanban_grounding_trend` MCP tool | Replaced by curator-level tools |
| `hkask-mcp-kata-kanban/src/types.rs` | `GroundingTrendRequest` struct | Tool-specific request, removed |
| `hkask-mcp-swarm/src/local_knowledge.rs` | `GroundingAnnotation` struct | Replaced by `GroundingRecord` in central ledger |
| `hkask-mcp-swarm/src/local_knowledge.rs` | `GroundingTrend` struct | Replaced by `GroundingTrendReport` in verification crate |
| `hkask-mcp-swarm/src/local_knowledge.rs` | `grounding_trend()` function | Replaced by `VerificationStore::grounding_trend()` |
| `hkask-mcp-swarm/src/local_knowledge.rs` | `record_delegation()` `grounding` parameter | Stigmergy trail no longer stores grounding data |
| `hkask-mcp-swarm/src/local_knowledge.rs` | Stigmergy grounding writes (had_contract, nulled, leaks h_mems) | Replaced by central ledger |
| `hkask-mcp-swarm/src/knowledge_tools.rs` | `swarm_grounding_trend` MCP tool | Replaced by curator-level tools |
| `hkask-mcp-swarm/src/request_types.rs` | `GroundingTrendRequest` struct | Tool-specific request, removed |
| `hkask-mcp-swarm/src/local_runtime.rs` | `GroundingSummary` struct | Replaced by `GroundingRecord` in central ledger |
| `hkask-mcp-swarm/src/local_runtime.rs` | `LocalDelegateResult::grounding_summary` field | Source of truth moves to central ledger |
| `hkask-mcp-swarm/src/hkask_mcp_swarm.rs` | `GroundingSummary` re-export | Struct removed |
| All `LocalDelegateResult` construction sites | `grounding_summary: None` field initialization | Field removed |
| `hkask-mcp-kata-kanban/src/kanban/service_impl/tests.rs` | `grounding_trend` tests | Moved to verification crate |
| `hkask-mcp-kata-kanban/src/grounding.rs` | `GroundingTrendReport` struct and methods | Moved to `hkask-verification/src/trend.rs` |

## Detailed implementation steps

### Step 1: Create the `hkask-verification` crate

Create `kask/crates/hkask-verification/` with:

**`Cargo.toml`:**
```toml
[package]
name = "hkask-verification"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Verification ladder for agent ecologies — grounding, schema validation, envelope, rollup trust, and central grounding ledger"

[lib]
name = "hkask_verification"
path = "src/hkask_verification.rs"

[dependencies]
hkask-storage.workspace = true
hkask-types.workspace = true
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
schemars = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }

[lints]
workspace = true

[dev-dependencies]
proptest = { workspace = true }
```

Add to the workspace `Cargo.toml`:
- Add `kask/crates/hkask-verification` to `[workspace.members]` (or ensure the glob `kask/crates/*` covers it)
- Add `hkask-verification = { path = "kask/crates/hkask-verification" }` to `[workspace.dependencies]`

**`src/hkask_verification.rs` (lib root):**
```rust
//! Verification ladder for agent ecologies.
//!
//! Implements the four-rung verification ladder (Presence, Truth, Grounding,
//! Binding) from the ABW team's paper "Verification for Agent Ecologies."
//! This crate is the single source of truth for verification logic — any
//! MCP server that delegates to agents depends on it for grounding enforcement
//! and the central grounding ledger.

pub mod card_contract;
pub mod envelope;
pub mod grounding;
pub mod ledger;
pub mod rollup_trust;
pub mod schema_validate;
pub mod trend;
pub mod types;
pub mod error;

// Re-export the primary API.
pub use grounding::{enforce_grounding, GroundingContract, GroundingResult, ProvenanceTag, FieldSpec, LeakRule, task_agent_contract};
pub use ledger::VerificationStore;
pub use trend::{GroundingTrendReport, TrendScope};
pub use types::GroundingRecord;
pub use error::VerificationError;
```

**`src/error.rs`:**
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("verification store initialization failed: {0}")]
    Init(String),
    #[error("verification store query failed: {0}")]
    Query(String),
    #[error("verification store write failed: {0}")]
    Write(String),
}
```

### Step 2: Move verification modules

Move these files from `hkask-mcp-kata-kanban/src/` to `hkask-verification/src/`:

- `grounding.rs` → `hkask-verification/src/grounding.rs`
- `card_contract.rs` → `hkask-verification/src/card_contract.rs`
- `schema_validate.rs` → `hkask-verification/src/schema_validate.rs`
- `envelope.rs` → `hkask-verification/src/envelope.rs`
- `rollup_trust.rs` → `hkask-verification/src/rollup_trust.rs`

**In each moved file:**
- Update `use crate::` references to point to the new crate's modules (e.g., `use crate::grounding::GroundingResult` stays as-is since it's now in the same crate).
- Remove any `use crate::` references to kata-kanban-specific modules.
- Move all tests with the files (they're in `#[cfg(test)] mod tests` blocks).
- Remove the `GroundingTrendReport` struct from `grounding.rs` — it moves to `trend.rs`.

**After moving, delete the originals from kata-kanban.**

### Step 3: Create `types.rs` (shared types)

**`src/types.rs`:**
```rust
use std::collections::HashMap;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

use crate::grounding::{GroundingResult, ProvenanceTag};

/// A full grounding record stored in the central ledger. Append-only —
/// each delegation writes a new record. This is the source of truth for
/// grounding status, trend analysis, and the curator's feedback loop.
///
/// The `source` field identifies which MCP server/tool produced this
/// delegation ("kanban_task_spawn", "swarm_delegate_local", etc.), enabling
/// cross-tool aggregation and per-tool trend analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundingRecord {
    /// UUID identifying this specific delegation (for linking with
    /// LocalDelegateResult if needed in the future).
    pub delegation_id: String,
    /// Which MCP server/tool produced this delegation.
    pub source: String,
    /// The agent that was delegated to.
    pub agent_id: String,
    /// The agent's type (determines which grounding contract applies).
    pub agent_type: String,
    /// When the delegation was grounded.
    pub timestamp: DateTime<Utc>,
    /// Whether a grounding contract existed for this agent_type.
    /// `false` = coverage gap (paper §6: coverage is itself a metric).
    pub had_contract: bool,
    /// Fields nulled as Unsourced (empty if clean or no contract).
    pub nulled_fields: Vec<String>,
    /// Narrative leaks detected (empty if clean or no contract).
    pub narrative_leaks: Vec<(String, String)>,
    /// Per-field provenance tags (empty if no contract).
    pub provenance: HashMap<String, ProvenanceTag>,
}

impl GroundingRecord {
    /// Construct from a `GroundingResult` after enforcement.
    pub fn from_result(
        source: &str,
        agent_id: &str,
        agent_type: &str,
        result: &GroundingResult,
    ) -> Self {
        let delegation_id = uuid::Uuid::new_v4().to_string();
        Self {
            delegation_id,
            source: source.to_string(),
            agent_id: agent_id.to_string(),
            agent_type: agent_type.to_string(),
            timestamp: Utc::now(),
            had_contract: true,
            nulled_fields: result.nulled_fields.clone(),
            narrative_leaks: result.narrative_leaks.clone(),
            provenance: result.provenance.clone(),
        }
    }

    /// Construct a coverage-gap record (no contract for this agent_type).
    pub fn coverage_gap(source: &str, agent_id: &str, agent_type: &str) -> Self {
        let delegation_id = uuid::Uuid::new_v4().to_string();
        Self {
            delegation_id,
            source: source.to_string(),
            agent_id: agent_id.to_string(),
            agent_type: agent_type.to_string(),
            timestamp: Utc::now(),
            had_contract: false,
            nulled_fields: Vec::new(),
            narrative_leaks: Vec::new(),
            provenance: HashMap::new(),
        }
    }

    /// True if this delegation was clean (contract ran, zero violations).
    pub fn is_clean(&self) -> bool {
        self.had_contract && self.nulled_fields.is_empty() && self.narrative_leaks.is_empty()
    }
}
```

### Step 4: Create `trend.rs` (trend report and scope)

**`src/trend.rs`:**
```rust
use serde::{Serialize, Deserialize};

/// The scope for a grounding trend query.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendScope {
    /// All delegations across all tools and agents.
    Global,
    /// Delegations for a specific agent.
    ByAgent(String),
    /// Delegations from a specific source tool.
    BySource(String),
}

impl Default for TrendScope {
    fn default() -> Self {
        Self::Global
    }
}

/// A grounding trend report aggregated across delegations. Answers the
/// paper's §4.1 question: "is this getting better?"
///
/// The lead metric is `delegations_with_zero_nulled` — deletion-resistant
/// (paper Rule 5.4: a scoreboard that counts nulled fields falling can be
/// gamed by recording fewer delegations; counting delegations with zero
/// nulled fields cannot).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundingTrendReport {
    /// Total delegations recorded.
    pub total_delegations: usize,
    /// Delegations for which a grounding contract existed and ran.
    pub delegations_with_contract: usize,
    /// Delegations for which no grounding contract existed (coverage gap).
    pub delegations_without_contract: usize,
    /// Delegations where grounding ran and zero fields were nulled.
    /// The deletion-resistant scoreboard metric (paper Rule 5.4).
    pub delegations_with_zero_nulled: usize,
    /// Delegations where grounding ran and at least one field was nulled.
    pub delegations_with_nulled: usize,
    /// Delegations where grounding ran and at least one narrative leak
    /// was detected.
    pub delegations_with_narrative_leaks: usize,
}

impl GroundingTrendReport {
    /// Fraction of grounded delegations with zero nulled fields.
    /// `None` when no grounded delegations exist (absence ≠ 0, paper Rule 5.3).
    pub fn clean_rate(&self) -> Option<f64> {
        let measured = self.delegations_with_zero_nulled + self.delegations_with_nulled;
        if measured == 0 {
            return None;
        }
        Some(self.delegations_with_zero_nulled as f64 / measured as f64)
    }

    /// Fraction of delegations that had a grounding contract.
    /// `None` when no delegations exist.
    pub fn coverage_rate(&self) -> Option<f64> {
        if self.total_delegations == 0 {
            return None;
        }
        Some(self.delegations_with_contract as f64 / self.total_delegations as f64)
    }
}
```

### Step 5: Create `ledger.rs` (the central store)

**`src/ledger.rs`:**

This is the core of the refactor. The `VerificationStore`:
- Wraps an `HMemStore` (SQLite + SQLCipher, same pattern as kanban and swarm memory)
- Provides `enforce_for_agent()` — runs grounding, writes to ledger, returns result
- Provides `grounding_trend()` — queries the ledger and aggregates
- Provides `grounding_violations()` — queries recent violations
- Provides `grounding_coverage()` — reports which agent types lack contracts
- Holds a contract registry keyed by `agent_type`

```rust
use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use hkask_storage::{HMem, HMemStore};
use hkask_types::{HMemOntology, Visibility, WebID};
use serde_json::Value;

use crate::error::VerificationError;
use crate::grounding::{enforce_grounding, GroundingContract, GroundingResult, task_agent_contract};
use crate::trend::{GroundingTrendReport, TrendScope};
use crate::types::GroundingRecord;

const VERIFICATION_ENTITY: &str = "verification:grounding";

pub struct VerificationStore {
    store: HMemStore,
    /// Contract registry keyed by agent_type. New contracts registered via
    /// `register_contract()`. The default `task_agent_contract()` is
    /// registered at construction.
    contracts: std::sync::Mutex<HashMap<String, GroundingContract>>,
}

impl VerificationStore {
    /// Create a new VerificationStore backed by the given HMemStore.
    /// Registers the default `task_agent_contract()` for "task" agent_type.
    pub fn new(store: HMemStore) -> Self {
        let mut contracts = HashMap::new();
        let default_contract = task_agent_contract();
        contracts.insert(default_contract.agent_type.clone(), default_contract);
        Self {
            store,
            contracts: std::sync::Mutex::new(contracts),
        }
    }

    /// Register a grounding contract for an agent_type. Extends coverage
    /// beyond the default "task" agent_type.
    pub fn register_contract(&self, contract: GroundingContract) {
        let mut contracts = self.contracts.lock().expect("contracts mutex poisoned");
        contracts.insert(contract.agent_type.clone(), contract);
    }

    /// Enforce grounding for a delegation and record the result to the
    /// central ledger. Returns `(Option<GroundingResult>, cleaned_json)`.
    ///
    /// If a contract exists for the agent_type:
    ///   - Runs `enforce_grounding()` (nulls unsourced fields, scans narrative)
    ///   - Writes a full `GroundingRecord` to the ledger (append-only)
    ///   - Returns `(Some(result), cleaned_json)`
    ///
    /// If no contract exists:
    ///   - Writes a coverage-gap record to the ledger (`had_contract: false`)
    ///   - Returns `(None, original_json)` — unchanged
    ///
    /// `source` identifies the calling tool ("kanban_task_spawn",
    /// "swarm_delegate_local", etc.) for cross-tool trend analysis.
    pub fn enforce_for_agent(
        &self,
        source: &str,
        agent_id: &str,
        agent_type: &str,
        output_json: &Value,
        tool_calls: &[Value],
        response: &str,
    ) -> (Option<GroundingResult>, Value) {
        let contract = {
            let contracts = self.contracts.lock().expect("contracts mutex poisoned");
            contracts.get(agent_type).cloned()
        };
        match contract {
            Some(contract) => {
                if !output_json.is_object() {
                    // Contract exists but output isn't JSON — can't enforce.
                    // Record a coverage-gap (had_contract: true but unenforceable).
                    // The trend query counts this as "with contract" but
                    // not "zero nulled" (absence ≠ verdict, paper Rule 5.3).
                    self.record_coverage_gap(source, agent_id, agent_type);
                    return (None, output_json.clone());
                }
                let (result, cleaned) = enforce_grounding(
                    &contract, output_json, tool_calls, response,
                );
                self.record_grounding(source, agent_id, agent_type, &result);
                (Some(result), cleaned)
            }
            None => {
                // No contract for this agent_type — coverage gap.
                self.record_coverage_gap(source, agent_id, agent_type);
                (None, output_json.clone())
            }
        }
    }

    /// Write a grounding record to the central ledger (append-only).
    fn record_grounding(
        &self,
        source: &str,
        agent_id: &str,
        agent_type: &str,
        result: &GroundingResult,
    ) {
        let record = GroundingRecord::from_result(source, agent_id, agent_type, result);
        self.write_record(&record);
    }

    /// Write a coverage-gap record (no contract for this agent_type).
    fn record_coverage_gap(&self, source: &str, agent_id: &str, agent_type: &str) {
        let record = GroundingRecord::coverage_gap(source, agent_id, agent_type);
        self.write_record(&record);
    }

    /// Write a GroundingRecord as an h_mem to the store.
    fn write_record(&self, record: &GroundingRecord) {
        let owner = WebID::for_agent_name("verification_store");
        let ontology = HMemOntology::episodic("verification", "grounding", &record.agent_id);
        let mut h_mem = HMem::new(
            VERIFICATION_ENTITY,
            &record.delegation_id,
            serde_json::to_value(record).unwrap_or(Value::Null),
            owner,
        )
        .with_ontology(ontology);
        h_mem.access.visibility = Visibility::Shared;
        if let Err(e) = self.store.insert(&h_mem) {
            tracing::warn!(
                target: "hkask.verification",
                error = %e,
                "grounding ledger write failed (non-fatal)"
            );
        }
    }

    /// Query the grounding trend. Reads all records matching the scope and
    /// aggregates them. Returns `Err` when the store is unavailable (the
    /// `.rules` broken-feedback-loop trap: a DB outage must not collapse
    /// to an empty trend).
    pub fn grounding_trend(
        &self,
        scope: &TrendScope,
    ) -> Result<GroundingTrendReport, VerificationError> {
        let records = self.query_records(scope)?;
        let mut report = GroundingTrendReport::default();
        for record in &records {
            report.total_delegations += 1;
            if record.had_contract {
                report.delegations_with_contract += 1;
                if record.nulled_fields.is_empty() {
                    report.delegations_with_zero_nulled += 1;
                } else {
                    report.delegations_with_nulled += 1;
                }
                if !record.narrative_leaks.is_empty() {
                    report.delegations_with_narrative_leaks += 1;
                }
            } else {
                report.delegations_without_contract += 1;
            }
        }
        Ok(report)
    }

    /// Query recent grounding violations (delegations with nulled fields
    /// or narrative leaks). Returns records sorted by timestamp descending.
    pub fn grounding_violations(
        &self,
        since: DateTime<Utc>,
        scope: &TrendScope,
    ) -> Result<Vec<GroundingRecord>, VerificationError> {
        let records = self.query_records(scope)?;
        let mut violations: Vec<GroundingRecord> = records
            .into_iter()
            .filter(|r| r.timestamp >= since && (!r.nulled_fields.is_empty() || !r.narrative_leaks.is_empty()))
            .collect();
        violations.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(violations)
    }

    /// Query all grounding records matching the scope.
    fn query_records(&self, scope: &TrendScope) -> Result<Vec<GroundingRecord>, VerificationError> {
        let h_mems = self.store
            .query_by_entity(VERIFICATION_ENTITY)
            .map_err(|e| VerificationError::Query(format!("ledger query failed: {e}")))?;
        let records: Vec<GroundingRecord> = h_mems
            .into_iter()
            .filter_map(|h| {
                let record: GroundingRecord = serde_json::from_value(h.value).ok()?;
                match scope {
                    TrendScope::Global => Some(record),
                    TrendScope::ByAgent(agent_id) if record.agent_id == *agent_id => Some(record),
                    TrendScope::BySource(source) if record.source == *source => Some(record),
                    _ => None,
                }
            })
            .collect();
        Ok(records)
    }
}
```

**Note:** Check the `HMemStore` API — it may use `insert` or `store` for writes, and `query_by_entity` for reads. Use whatever the actual API is (verify by reading `hkask-storage/src/hmem.rs`).

### Step 6: Update `hkask-mcp-kata-kanban`

**`Cargo.toml`:**
- Add `hkask-verification.workspace = true` to `[dependencies]`
- Remove any dependencies that were only used by the moved modules (run `cargo machete` after)

**`src/hkask_mcp_kata_kanban.rs`:**
- Remove `pub mod grounding;`, `pub mod card_contract;`, `pub mod schema_validate;`, `pub mod envelope;`, `pub mod rollup_trust;` (the files are moved)
- Add `use hkask_verification::*;` where needed
- Add `verification_store: Arc<hkask_verification::VerificationStore>` to the `KanbanServer` struct
- Update the server construction in `run()` to create a `VerificationStore` and pass it to `KanbanServer`
- In `spawn_via_local_runtime`:
  - Replace the inline grounding block (`if agent.agent_type == grounding_contract.agent_type { ... }`) with:
    ```rust
    let output_json = serde_json::from_str::<serde_json::Value>(&result.response)
        .unwrap_or(serde_json::Value::Null);
    let (grounding_result, cleaned) = self.verification_store.enforce_for_agent(
        "kanban_task_spawn",
        &result.agent_id,
        &agent.agent_type,
        &output_json,
        &result.tool_calls,
        &result.response,
    );
    if let Some(ref gr) = grounding_result {
        if !gr.nulled_fields.is_empty() {
            tracing::warn!(...);  // keep existing warn
        }
        // Schema validation (from verification crate)
        let validation = hkask_verification::schema_validate::validate(...);
        // Envelope (from verification crate)
        let envelope = hkask_verification::envelope::build(...);
        // Replace response with cleaned JSON
        result.response = serde_json::to_string(&cleaned).unwrap_or_else(|_| result.response.clone());
        result.raw_response = Some(raw_response);
    }
    ```
  - Remove `result.grounding_summary = Some(...)` (field removed)
  - Keep the `raw_response` retention
- Remove the `kanban_grounding_trend` MCP tool entirely
- Update the tool surface count test: 26 → 25

**`src/kanban/service_impl/service.rs`:**
- Remove `KanbanService::grounding_trend()` method entirely

**`src/kanban/service_impl/tests.rs`:**
- Remove `grounding_trend_aggregates_across_tasks` and `grounding_trend_empty_when_no_delegations` tests
- Remove `delegate_result_with_grounding` helper
- Update all `LocalDelegateResult` construction sites: remove `grounding_summary: None` field

**`src/types.rs`:**
- Remove `GroundingTrendRequest` struct

**`src/rollup_trust.rs`:**
- This file is moved to the verification crate. But the `cost_never_exceeds_cost_uncapped` test constructs a `LocalDelegateResult` — update it to remove `grounding_summary: None`.

### Step 7: Update `hkask-mcp-swarm`

**`Cargo.toml`:**
- Add `hkask-verification.workspace = true` to `[dependencies]`

**`src/hkask_mcp_swarm.rs`:**
- Remove `GroundingSummary` from the re-export
- Add `verification_store: Arc<hkask_verification::VerificationStore>` to the `SwarmServer` struct
- Update the server construction in `run()` to create a `VerificationStore` and pass it
- Update the tool surface count test: 54 → 53
- Update the doc comment: 54 → 53

**`src/local_runtime.rs`:**
- Remove the `GroundingSummary` struct entirely
- Remove the `grounding_summary` field from `LocalDelegateResult`
- Update all `LocalDelegateResult` construction sites (in `delegate()`, tests): remove `grounding_summary: None`

**`src/local_knowledge.rs`:**
- Remove `GroundingAnnotation` struct
- Remove `GroundingTrend` struct
- Remove `grounding_trend()` function
- Remove the `grounding` parameter from `record_delegation()` (revert to the 4-parameter signature: `memory, agent_id, latency_ms, task_success_pass`)
- Remove all stigmergy grounding writes (had_contract, nulled, leaks h_mems) from `record_delegation()`
- Remove all grounding-related tests (`record_delegation_writes_grounding_annotation_when_some`, `record_delegation_writes_had_contract_false_when_none`, `grounding_trend_reports_zero_nulled_as_lead_metric`, `grounding_trend_returns_err_when_memory_unavailable`, `grounding_trend_empty_when_no_delegations`, `temp_memory` helper)
- Update existing `record_delegation` tests: remove the 5th argument (`None`)

**`src/local_tools.rs`:**
- In `swarm_delegate_local`: after `runtime.delegate()` and before `record_delegation()`, add grounding enforcement:
  ```rust
  let output_json = serde_json::from_str::<serde_json::Value>(&result.response)
      .unwrap_or(serde_json::Value::Null);
  let (grounding_result, cleaned) = self.verification_store.enforce_for_agent(
      "swarm_delegate_local",
      &req.agent_name,
      &agent.agent_type,
      &output_json,
      &result.tool_calls,
      &result.response,
  );
  if let Some(ref gr) = grounding_result {
      if !gr.nulled_fields.is_empty() {
          tracing::warn!(...);
      }
      result.response = serde_json::to_string(&cleaned).unwrap_or_else(|_| result.response.clone());
      result.raw_response = Some(/* raw response saved before overwrite */);
  }
  ```
- In `swarm_execute_plan_local`: add the same grounding enforcement for each delegation
- Update `record_delegation` calls: remove the 5th argument (`None`)

**`src/knowledge_tools.rs`:**
- Remove the `swarm_grounding_trend` MCP tool entirely

**`src/request_types.rs`:**
- Remove `GroundingTrendRequest` struct

**`build.rs`:**
- Update comment: 54 → 53

### Step 8: Update `hkask-mcp-curator`

**`Cargo.toml`:**
- Add `hkask-verification.workspace = true` to `[dependencies]`

**`src/hkask_mcp_curator.rs`:**
- Add `verification_store: Arc<hkask_verification::VerificationStore>` to the `CuratorServer` struct
- Update server construction in `run()` to create a `VerificationStore`
- Add three MCP tools:

  1. `curator_grounding_trend` — queries the central ledger for the grounding trend. Parameters: `scope` (enum: "global", "by_agent", "by_source"), `agent_name` (optional, for by_agent scope), `source` (optional, for by_source scope). Returns the `GroundingTrendReport` with `clean_rate` and `coverage_rate`.

  2. `curator_grounding_violations` — queries recent grounding violations. Parameters: `since` (ISO 8601 timestamp), `scope` (same as above). Returns a list of `GroundingRecord`s with nulled fields or narrative leaks.

  3. `curator_grounding_coverage` — queries which agent types have grounding contracts vs. which have delegations but no contract. Returns a coverage report. (This can be a simple query: read all records, group by agent_type, report which types have `had_contract: true` vs `had_contract: false`.)

- Update the tool surface count test: 10 → 13

### Step 9: Update `LocalDelegateResult` construction sites

Every place that constructs a `LocalDelegateResult` needs the `grounding_summary` field removed. Grep for `grounding_summary` across the codebase and remove every occurrence. Known sites:

| File | Location |
|------|----------|
| `hkask-mcp-swarm/src/local_runtime.rs` | `delegate()` method return |
| `hkask-mcp-swarm/src/local_runtime.rs` | `unmeasured_balance_serializes_as_null` test |
| `hkask-mcp-swarm/src/local_runtime.rs` | `measured_balance_serializes_as_a_number_including_negative` test |
| `hkask-mcp-kata-kanban/src/kanban/service_impl/tests.rs` | `task_record_delegation_writes_structured_fields` test |
| `hkask-mcp-kata-kanban/src/kanban/service_impl/tests.rs` | `task_record_delegation_rejects_non_owner` test |
| `hkask-mcp-kata-kanban/src/rollup_trust.rs` | `cost_never_exceeds_cost_uncapped` test |

After removing the field, grep again for `grounding_summary` to verify zero hits.

### Step 10: Update the architecture doc

Update `kask/docs/architecture/verification-for-agent-ecologies.md`:

- Update the "What is grounded" section to describe the central ledger
- Update the "invocation lifecycle" section to describe `VerificationStore::enforce_for_agent()`
- Update the "two clocks" table to reflect the verification crate
- Add a section on the curator integration (the feedback loop)
- Update the "What is NOT grounded" section — the coverage gap is now visible via `curator_grounding_coverage`

### Step 11: Update the `swarm_panel` reference

In `crates/swarm_panel/src/swarm_panel.rs`, update the comment referencing the tool surface count test: 53 → 54... wait, it's being reduced. Update: 54 → 53.

## Cleanup checklist

After all steps, verify:

- [ ] `grep -r "grounding_summary" --include="*.rs"` returns zero hits
- [ ] `grep -r "GroundingSummary" --include="*.rs"` returns zero hits
- [ ] `grep -r "GroundingAnnotation" --include="*.rs"` returns zero hits
- [ ] `grep -r "local_knowledge::GroundingTrend" --include="*.rs"` returns zero hits
- [ ] `grep -r "local_knowledge::grounding_trend" --include="*.rs"` returns zero hits
- [ ] `grep -r "KanbanService::grounding_trend" --include="*.rs"` returns zero hits
- [ ] `grep -r "kanban_grounding_trend" --include="*.rs"` returns zero hits (except maybe docs)
- [ ] `grep -r "swarm_grounding_trend" --include="*.rs"` returns zero hits (except maybe docs)
- [ ] `grep -r "crate::grounding" kask/mcp-servers/hkask-mcp-kata-kanban/` returns zero hits (moved to verification crate)
- [ ] `grep -r "crate::card_contract" kask/mcp-servers/hkask-mcp-kata-kanban/` returns zero hits
- [ ] `grep -r "crate::schema_validate" kask/mcp-servers/hkask-mcp-kata-kanban/` returns zero hits
- [ ] `grep -r "crate::envelope" kask/mcp-servers/hkask-mcp-kata-kanban/` returns zero hits
- [ ] `grep -r "crate::rollup_trust" kask/mcp-servers/hkask-mcp-kata-kanban/` returns zero hits
- [ ] The files `grounding.rs`, `card_contract.rs`, `schema_validate.rs`, `envelope.rs`, `rollup_trust.rs` no longer exist in `hkask-mcp-kata-kanban/src/`
- [ ] `cargo machete` reports no unused dependencies in any crate
- [ ] `./script/clippy` passes with zero warnings
- [ ] All tool surface count tests pass with updated counts

## Validation criteria

1. **Build:** `./script/clippy -p hkask-verification -p hkask-mcp-swarm -p hkask-mcp-kata-kanban -p hkask-mcp-curator` passes with zero warnings.

2. **Tests:** `cargo test -p hkask-verification -p hkask-mcp-swarm -p hkask-mcp-kata-kanban -p hkask-mcp-curator --lib` — all tests pass. The grounding tests that were in kata-kanban now run in the verification crate. The tool surface count tests reflect the updated counts.

3. **No dead code:** All the grep checks in the cleanup checklist return zero hits.

4. **Wiring verification:** Write a test that verifies `spawn_via_local_runtime` writes to the verification ledger (not just the kanban DB). Write a test that verifies `swarm_delegate_local` now runs grounding (it didn't before this refactor).

5. **Trend query:** Write a test that creates delegations via different sources (kanban + swarm) and verifies the global trend query aggregates across both.

6. **Curator tools:** Write tests for `curator_grounding_trend`, `curator_grounding_violations`, `curator_grounding_coverage`.

## Key design rules to follow

- **MCP tool failures must not collapse to `None`** — grounding violations are logged at `warn!`, not silently skipped. The `VerificationStore` returns `Err` on DB failures, not an empty trend.
- **Absence is not a verdict** — `had_contract: false` means "no contract," not "compliant." A coverage-gap record is written so the gap is visible.
- **A check that has never been falsified is inert** — every grounding contract clause has a test that breaks it. The moved tests from kata-kanban cover this.
- **The scoreboard must not reward deletion** — lead with `delegations_with_zero_nulled`, not `nulled_fields_count` falling.
- **No `unwrap_or(0)` on regulation signals** — grounding violation counts are `Option<usize>` in the `GroundingSummary` (if retained) or computed from the ledger records directly.
- **Stale diagnostics after bulk edits** — the crate's lib root is authoritative, not individual-file diagnostics.
- **No `mod.rs` files** — use `src/module.rs` instead.
- **New crates: specify `[lib] path = "..."` in `Cargo.toml`** (e.g., `hkask_verification.rs`, not `lib.rs`).
- **Build: use `./script/clippy` instead of `cargo clippy`.**

## Before starting

1. **Read the architecture doc**: `zed-kask/kask/docs/architecture/verification-for-agent-ecologies.md`
2. **Read the grounding module**: `kask/mcp-servers/hkask-mcp-kata-kanban/src/grounding.rs` — the three-pass `enforce_grounding` function, `ProvenanceTag` enum, `FieldSpec` with `why`, `LeakRule` with `Quantity` matching.
3. **Read the wiring**: `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs` — the `spawn_via_local_runtime` grounding wiring.
4. **Read the stigmergy trail**: `kask/mcp-servers/hkask-mcp-swarm/src/local_knowledge.rs` — `record_delegation` and the grounding annotation writes.
5. **Read the `HMemStore` API**: `kask/crates/hkask-storage/src/hmem.rs` — `insert`, `query_by_entity`, `query_by_attribute` methods.
6. **Read the `LocalDelegateResult` struct**: `kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs` — the struct that carries delegation results.
7. **Verify the build**: `./script/clippy -p hkask-mcp-swarm -p hkask-mcp-kata-kanban` and `cargo test -p hkask-mcp-kata-kanban --lib -- grounding` — confirm all tests pass before making changes.

Then begin with Step 1 (create the `hkask-verification` crate), since all subsequent steps depend on it.
