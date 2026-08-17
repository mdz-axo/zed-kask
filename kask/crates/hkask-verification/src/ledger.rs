//! Central grounding ledger — `VerificationStore`.
//!
//! The `VerificationStore` is the single source of truth for grounding
//! status across the agent ecology. It wraps an `HMemStore` (SQLite +
//! SQLCipher, same pattern as kanban and swarm memory) and provides:
//!
//! - `enforce_for_agent()` — runs grounding, writes a `GroundingRecord` to
//!   the ledger (append-only), returns the result. Called by every MCP
//!   server that delegates to agents (`kanban_task_spawn`,
//!   `swarm_delegate_local`, `swarm_execute_plan_local`).
//! - `grounding_trend()` — aggregates records into a `GroundingTrendReport`.
//! - `grounding_violations()` — returns recent violation records.
//! - `grounding_coverage()` — reports which agent types have contracts vs.
//!   which have delegations but no contract (the coverage gap, paper §6).
//!
//! The DB file is at a standard path (`mcp/verification/grounding.db`),
//! shared across all MCP server processes via SQLite WAL mode. The
//! passphrase is `HKASK_VERIFICATION_PASSPHRASE` (default `"allostery"` for
//! pre-release).
//!
//! Grounding records are stored as h_mems:
//! - Entity: `verification:grounding`
//! - Attribute: `{delegation_uuid}` (a UUID generated per delegation)
//! - Value: JSON-serialized `GroundingRecord`
//! - Ontology: `HMemOntology::episodic("verification", "grounding", agent_id)`
//!
//! This is append-only — each delegation writes a new record. The trend
//! query reads all records and aggregates.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{HMem, HMemStore};
use hkask_types::{HMemOntology, Visibility, WebID};
use serde_json::Value;

use crate::error::VerificationError;
use crate::grounding::{
    GroundingContract, GroundingResult, enforce_grounding, task_agent_contract,
};
use crate::trend::{GroundingTrendReport, TrendScope};
use crate::types::GroundingRecord;

/// Entity name for grounding records in the central ledger. All
/// `GroundingRecord`s are stored under this entity, with the delegation
/// UUID as the attribute (so each delegation is a distinct h_mem).
const VERIFICATION_ENTITY: &str = "verification:grounding";

/// The central grounding ledger. Wraps an `HMemStore` and a contract
/// registry keyed by `agent_type`. Every MCP server that delegates to
/// agents constructs one (sharing the same DB file) and calls
/// `enforce_for_agent()` on each delegation.
///
/// The contract registry starts with the default `task_agent_contract()`
/// registered for the `"task"` agent_type. Additional contracts are
/// registered via `register_contract()`.
pub struct VerificationStore {
    store: HMemStore,
    /// Contract registry keyed by agent_type. New contracts registered via
    /// `register_contract()`. The default `task_agent_contract()` is
    /// registered at construction.
    contracts: Mutex<HashMap<String, GroundingContract>>,
}

impl VerificationStore {
    /// Create a new `VerificationStore` backed by the given `HMemStore`.
    /// Registers the default `task_agent_contract()` for the `"task"`
    /// agent_type.
    pub fn new(store: HMemStore) -> Self {
        let mut contracts = HashMap::new();
        let default_contract = task_agent_contract();
        contracts.insert(default_contract.agent_type.clone(), default_contract);
        Self {
            store,
            contracts: Mutex::new(contracts),
        }
    }

    /// Open a `VerificationStore` at the standard path
    /// (`mcp/verification/grounding.db`), resolved under the hKask data dir.
    /// Override via `HKASK_VERIFICATION_DB`. The passphrase is
    /// `HKASK_VERIFICATION_PASSPHRASE` (default `"allostery"` for
    /// pre-release — the verification ledger is shared across MCP server
    /// processes via SQLite WAL mode and must be encrypted at rest in
    /// production, but a fixed default lets local-only dev setups work
    /// without configuration).
    ///
    /// Falls back to an in-memory store when the DB cannot be opened —
    /// grounding enforcement still runs, but records do not persist across
    /// restarts and the curator's trend queries see only this process's
    /// delegations. The fallback logs a `warn!` so the operator can
    /// distinguish "not configured" from "configured but broken."
    pub fn open() -> Self {
        let db_path = std::env::var("HKASK_VERIFICATION_DB").unwrap_or_else(|_| {
            let relative = hkask_types::agent_paths::mcp_server_db("verification", "grounding");
            let resolved = hkask_types::agent_paths::resolve_under_data_dir(&relative);
            if let Some(parent) = resolved.parent()
                && let Err(error) = std::fs::create_dir_all(parent)
            {
                tracing::warn!(
                    target: "hkask.verification",
                    %error,
                    path = %parent.display(),
                    "Failed to create verification DB directory — the subsequent DB open will surface the failure"
                );
            }
            resolved.to_string_lossy().to_string()
        });
        let passphrase = match std::env::var("HKASK_VERIFICATION_PASSPHRASE") {
            Ok(p) => p,
            Err(_) => {
                tracing::warn!(
                    target: "hkask.verification",
                    "HKASK_VERIFICATION_PASSPHRASE not set — using default 'allostery'. \
                     This provides zero confidentiality in a multi-user environment. \
                     Set HKASK_VERIFICATION_PASSPHRASE (keychain-provisioned) for production."
                );
                "allostery".to_string()
            }
        };
        match hkask_storage::open_or_repair(&db_path, &passphrase) {
            Ok(db) => match db.sqlite_pool() {
                Ok(pool) => {
                    let driver: Arc<dyn DatabaseDriver> =
                        Arc::new(SqliteDriver::new_labeled(pool, db_path.as_str()));
                    match HMemStore::from_driver(driver) {
                        Ok(store) => Self::new(store),
                        Err(error) => {
                            tracing::warn!(
                                target: "hkask.verification",
                                %error,
                                "Verification HMemStore init failed — falling back to in-memory. Grounding records will not persist across restarts."
                            );
                            Self::in_memory()
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.verification",
                        %error,
                        "Verification SQLite pool init failed — falling back to in-memory. Grounding records will not persist across restarts."
                    );
                    Self::in_memory()
                }
            },
            Err(error) => {
                tracing::warn!(
                    target: "hkask.verification",
                    %error,
                    "Verification DB open failed — falling back to in-memory. Grounding records will not persist across restarts."
                );
                Self::in_memory()
            }
        }
    }

    /// Construct an in-memory `VerificationStore` for tests or as a
    /// non-persistent fallback. Grounding enforcement still runs; records
    /// do not persist across process restarts.
    pub fn in_memory() -> Self {
        let driver = SqliteDriver::in_memory_driver();
        let store = HMemStore::from_driver(driver).expect("in-memory HMemStore");
        Self::new(store)
    }

    /// Register a grounding contract for an agent_type. Extends coverage
    /// beyond the default `"task"` agent_type. If a contract already exists
    /// for this agent_type, it is replaced.
    pub fn register_contract(&self, contract: GroundingContract) {
        let mut contracts = self.contracts.lock().expect("contracts mutex poisoned");
        contracts.insert(contract.agent_type.clone(), contract);
    }

    /// Enforce grounding for a delegation and record the result to the
    /// central ledger. Returns `(Option<GroundingResult>, cleaned_json)`.
    ///
    /// If a contract exists for the agent_type AND the output is a JSON object:
    ///   - Runs `enforce_grounding()` (nulls unsourced fields, scans narrative)
    ///   - Writes a full `GroundingRecord` to the ledger (append-only)
    ///   - Returns `(Some(result), cleaned_json)`
    ///
    /// If a contract exists but the output is not a JSON object (e.g. the
    /// agent produced prose): writes an **unenforceable** record
    /// (`had_contract: true, was_enforced: false`) and returns
    /// `(None, original_json)`. The trend query counts this under
    /// `delegations_unenforceable` — the operator's remediation is to fix
    /// the agent's system prompt, not to write a contract (paper Rule 5.3:
    /// absence ≠ verdict).
    ///
    /// If no contract exists: writes a **coverage-gap** record
    /// (`had_contract: false`) and returns `(None, original_json)`.
    /// The trend query counts this under `delegations_without_contract`.
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
                    // Contract exists for this agent_type but the output is
                    // not a JSON object — grounding cannot run. Record an
                    // unenforceable record (had_contract: true, was_enforced:
                    // false) so the trend query distinguishes this from a
                    // coverage gap (paper Rule 5.3: absence ≠ verdict).
                    self.record_unenforceable(source, agent_id, agent_type);
                    return (None, output_json.clone());
                }
                let (result, cleaned) =
                    enforce_grounding(&contract, output_json, tool_calls, response);
                self.record_grounding(source, agent_id, agent_type, &result);
                (Some(result), cleaned)
            }
            None => {
                // No contract for this agent_type — coverage gap (paper §6).
                self.record_coverage_gap(source, agent_id, agent_type);
                (None, output_json.clone())
            }
        }
    }

    /// The outcome of `enforce_and_stamp` — the caller uses this to stamp
    /// the delegation result fields (`response`, `raw_response`). This
    /// struct exists so the grounding wiring (parse → enforce → warn →
    /// replace → retain) is not duplicated across 3 call sites.
    #[derive(Debug, Clone)]
    pub struct EnforcementOutcome {
        /// The grounding result, when grounding ran. `None` when no contract
        /// existed or the output was not a JSON object.
        pub result: Option<GroundingResult>,
        /// The cleaned JSON (with unsourced fields nulled). When grounding
        /// did not run, this is the original output unchanged.
        pub cleaned: Value,
        /// The raw response string (the original `response` before cleaning).
        /// The caller should store this as `raw_response` for audit.
        pub raw_response: String,
        /// Whether the output was a JSON object (and thus could potentially
        /// be grounded). The caller uses this to decide whether to retain
        /// `raw_response` when grounding did not run.
        pub was_object: bool,
    }

    /// Enforce grounding for a delegation, record the result to the central
    /// ledger, and return an `EnforcementOutcome` that the caller uses to
    /// stamp the delegation result fields. This encapsulates the
    /// parse → enforce → warn pattern that was duplicated across
    /// `spawn_via_local_runtime`, `swarm_delegate_local`, and
    /// `swarm_execute_plan_local`.
    ///
    /// The caller is responsible for:
    /// - Calling this before `record_delegation` or `task_record_delegation`.
    /// - Using `outcome.result` to decide whether to run schema validation
    ///   and envelope building (kata-kanban does; swarm does not).
    /// - Setting `result.response = serde_json::to_string(&outcome.cleaned)`
    ///   when `outcome.result.is_some()`.
    /// - Setting `result.raw_response = Some(outcome.raw_response)` when
    ///   `outcome.result.is_some() || outcome.was_object`.
    pub fn enforce_and_stamp(
        &self,
        source: &str,
        agent_id: &str,
        agent_type: &str,
        response: &str,
        tool_calls: &[Value],
    ) -> EnforcementOutcome {
        let output_json = serde_json::from_str::<Value>(response)
            .unwrap_or(Value::Null);
        let raw_response = response.to_string();
        let was_object = output_json.is_object();
        let (result, cleaned) = self.enforce_for_agent(
            source,
            agent_id,
            agent_type,
            &output_json,
            tool_calls,
            response,
        );
        if let Some(ref gr) = result {
            if !gr.nulled_fields.is_empty() {
                tracing::warn!(
                    target: "hkask.verification",
                    agent_id = %agent_id,
                    nulled_fields = ?gr.nulled_fields,
                    narrative_leaks = ?gr.narrative_leaks,
                    "grounding enforcement: nulled {} unsourced field(s), found {} narrative leak(s)",
                    gr.nulled_fields.len(),
                    gr.narrative_leaks.len(),
                );
            }
        }
        EnforcementOutcome {
            result,
            cleaned,
            raw_response,
            was_object,
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

    /// Write an unenforceable record: the contract existed but the output
    /// was not a JSON object (had_contract: true, was_enforced: false).
    fn record_unenforceable(&self, source: &str, agent_id: &str, agent_type: &str) {
        let record = GroundingRecord::unenforceable(source, agent_id, agent_type);
        self.write_record(&record);
    }

    /// Write a coverage-gap record (no contract for this agent_type).
    fn record_coverage_gap(&self, source: &str, agent_id: &str, agent_type: &str) {
        let record = GroundingRecord::coverage_gap(source, agent_id, agent_type);
        self.write_record(&record);
    }

    /// Write a `GroundingRecord` as an h_mem to the store. Failures are
    /// logged at `warn!` (non-fatal) — the delegation result is still
    /// returned to the caller. A silent failure here would be a broken
    /// feedback loop: the trend query would read "no delegations" when
    /// delegations are happening.
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
        if let Err(error) = self.store.insert(&h_mem) {
            tracing::warn!(
                target: "hkask.verification",
                %error,
                "grounding ledger write failed (non-fatal)"
            );
        }
    }

    /// Query the grounding trend. Reads all records matching the scope and
    /// aggregates them. Returns `Err` when the store is unavailable (the
    /// `.rules` broken-feedback-loop trap: a DB outage must not collapse
    /// to an empty trend, which would read as "no deviation").
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
                if record.was_enforced {
                    if record.nulled_fields.is_empty() {
                        report.delegations_with_zero_nulled += 1;
                    } else {
                        report.delegations_with_nulled += 1;
                    }
                    if !record.narrative_leaks.is_empty() {
                        report.delegations_with_narrative_leaks += 1;
                    }
                } else {
                    // Contract existed but output was not a JSON object —
                    // grounding could not run. Counted as unenforceable, not
                    // as zero_nulled (absence ≠ verdict, Rule 5.3).
                    report.delegations_unenforceable += 1;
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
            .filter(|r| {
                r.timestamp >= since
                    && (!r.nulled_fields.is_empty() || !r.narrative_leaks.is_empty())
            })
            .collect();
        violations.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
        Ok(violations)
    }

    /// Query all grounding records matching the scope. Returns `Err` when
    /// the store query fails (the `.rules` broken-feedback-loop trap).
    fn query_records(&self, scope: &TrendScope) -> Result<Vec<GroundingRecord>, VerificationError> {
        let h_mems = self
            .store
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build an in-memory `VerificationStore` for tests.
    fn test_store() -> VerificationStore {
        VerificationStore::in_memory()
    }

    #[test]
    fn enforce_for_agent_runs_grounding_when_contract_exists() {
        let store = test_store();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "summary": "I wrote the file."
        });
        // No tool calls — deliverable_path is unsourced.
        let (result, cleaned) = store.enforce_for_agent(
            "kanban_task_spawn",
            "task_agent",
            "task",
            &output,
            &[],
            "I wrote the file.",
        );
        let result = result.expect("contract exists for task agent_type");
        assert_eq!(result.nulled_fields, vec!["deliverable_path"]);
        assert!(cleaned["deliverable_path"].is_null());
        // The record was written to the ledger.
        let trend = store
            .grounding_trend(&TrendScope::Global)
            .expect("trend query");
        assert_eq!(trend.total_delegations, 1);
        assert_eq!(trend.delegations_with_contract, 1);
        assert_eq!(trend.delegations_with_nulled, 1);
    }

    #[test]
    fn enforce_for_agent_writes_coverage_gap_when_no_contract() {
        let store = test_store();
        let output = json!({"summary": "done"});
        let (result, cleaned) = store.enforce_for_agent(
            "swarm_delegate_local",
            "researcher",
            "research", // no contract for "research"
            &output,
            &[],
            "done",
        );
        assert!(result.is_none(), "no contract → no grounding result");
        assert_eq!(cleaned, output, "output unchanged when no contract");
        // A coverage-gap record was written.
        let trend = store
            .grounding_trend(&TrendScope::Global)
            .expect("trend query");
        assert_eq!(trend.total_delegations, 1);
        assert_eq!(trend.delegations_without_contract, 1);
        assert_eq!(trend.delegations_with_contract, 0);
    }

    #[test]
    fn enforce_for_agent_writes_coverage_gap_when_output_not_object() {
        let store = test_store();
        let output = json!("just prose, not an object");
        let (result, cleaned) = store.enforce_for_agent(
            "kanban_task_spawn",
            "task_agent",
            "task",
            &output,
            &[],
            "just prose",
        );
        assert!(result.is_none(), "non-object output → no grounding");
        assert_eq!(cleaned, output, "output unchanged when not an object");
        let trend = store
            .grounding_trend(&TrendScope::Global)
            .expect("trend query");
        assert_eq!(trend.total_delegations, 1);
        assert_eq!(trend.delegations_without_contract, 1);
    }

    #[test]
    fn trend_aggregates_across_sources() {
        // The cross-tool aggregation property: delegations from kanban and
        // swarm both land in the same ledger and the global trend sees both.
        let store = test_store();
        // Kanban delegation — clean (sourced deliverable_path).
        let output = json!({"deliverable_path": "/src/main.rs", "summary": "done"});
        let tool_calls = vec![json!({ "tool": "zed/write_file", "ok": true })];
        store.enforce_for_agent(
            "kanban_task_spawn",
            "task_agent",
            "task",
            &output,
            &tool_calls,
            "done",
        );
        // Swarm delegation — coverage gap (research agent_type, no contract).
        store.enforce_for_agent(
            "swarm_delegate_local",
            "researcher",
            "research",
            &json!({"summary": "research done"}),
            &[],
            "research done",
        );
        // Global trend sees both.
        let global = store
            .grounding_trend(&TrendScope::Global)
            .expect("global trend");
        assert_eq!(global.total_delegations, 2);
        assert_eq!(global.delegations_with_contract, 1);
        assert_eq!(global.delegations_without_contract, 1);
        assert_eq!(global.delegations_with_zero_nulled, 1);
        // BySource scope filters to one source.
        let kanban_only = store
            .grounding_trend(&TrendScope::BySource("kanban_task_spawn".to_string()))
            .expect("kanban trend");
        assert_eq!(kanban_only.total_delegations, 1);
        assert_eq!(kanban_only.delegations_with_contract, 1);
        let swarm_only = store
            .grounding_trend(&TrendScope::BySource("swarm_delegate_local".to_string()))
            .expect("swarm trend");
        assert_eq!(swarm_only.total_delegations, 1);
        assert_eq!(swarm_only.delegations_without_contract, 1);
    }

    #[test]
    fn violations_query_returns_only_violations() {
        let store = test_store();
        // Clean delegation.
        let output = json!({"deliverable_path": "/src/main.rs", "summary": "done"});
        let tool_calls = vec![json!({ "tool": "zed/write_file", "ok": true })];
        store.enforce_for_agent(
            "kanban_task_spawn",
            "task_agent",
            "task",
            &output,
            &tool_calls,
            "done",
        );
        // Violation delegation.
        store.enforce_for_agent(
            "kanban_task_spawn",
            "task_agent",
            "task",
            &json!({"deliverable_path": "/src/fake.rs"}),
            &[],
            "I wrote /src/fake.rs",
        );
        let since = Utc::now() - chrono::Duration::hours(1);
        let violations = store
            .grounding_violations(since, &TrendScope::Global)
            .expect("violations query");
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].nulled_fields, vec!["deliverable_path"]);
    }

    #[test]
    fn register_contract_extends_coverage() {
        let store = test_store();
        // No contract for "research" initially.
        let (result, _) = store.enforce_for_agent(
            "swarm_delegate_local",
            "researcher",
            "research",
            &json!({"summary": "done"}),
            &[],
            "done",
        );
        assert!(result.is_none());
        // Register a contract for "research".
        let mut field_sources = std::collections::HashMap::new();
        field_sources.insert(
            "summary".to_string(),
            crate::grounding::FieldSpec {
                sources: vec![],
                why: "A prose summary commissioned by the system prompt.".to_string(),
            },
        );
        store.register_contract(GroundingContract {
            agent_type: "research".to_string(),
            field_sources,
        });
        // Now grounding runs.
        let (result, _) = store.enforce_for_agent(
            "swarm_delegate_local",
            "researcher",
            "research",
            &json!({"summary": "done"}),
            &[],
            "done",
        );
        assert!(result.is_some(), "contract registered → grounding runs");
    }

    #[test]
    fn default_task_contract_is_registered() {
        let store = test_store();
        // The default task_agent_contract is registered at construction.
        let contracts = store.contracts.lock().unwrap();
        assert!(contracts.contains_key("task"));
        let contract = &contracts["task"];
        assert!(contract.field_sources.contains_key("deliverable_path"));
        assert!(contract.field_sources.contains_key("test_verdict"));
    }

    #[test]
    fn trend_empty_when_no_delegations() {
        let store = test_store();
        let trend = store
            .grounding_trend(&TrendScope::Global)
            .expect("trend query");
        assert_eq!(trend.total_delegations, 0);
        assert_eq!(trend.clean_rate(), None);
        assert_eq!(trend.coverage_rate(), None);
    }

    #[test]
    fn by_agent_scope_filters_records() {
        let store = test_store();
        store.enforce_for_agent(
            "kanban_task_spawn",
            "agent_a",
            "task",
            &json!({"deliverable_path": "/src/a.rs"}),
            &[json!({ "tool": "zed/write_file", "ok": true })],
            "a",
        );
        store.enforce_for_agent(
            "kanban_task_spawn",
            "agent_b",
            "task",
            &json!({"deliverable_path": "/src/b.rs"}),
            &[],
            "b",
        );
        let agent_a = store
            .grounding_trend(&TrendScope::ByAgent("agent_a".to_string()))
            .expect("agent_a trend");
        assert_eq!(agent_a.total_delegations, 1);
        assert_eq!(agent_a.delegations_with_zero_nulled, 1);
        let agent_b = store
            .grounding_trend(&TrendScope::ByAgent("agent_b".to_string()))
            .expect("agent_b trend");
        assert_eq!(agent_b.total_delegations, 1);
        assert_eq!(agent_b.delegations_with_nulled, 1);
    }
}

/// Property-based tests for the central grounding ledger.
///
/// These exercise the full pipeline (enforce → write → query → aggregate)
/// with arbitrary combinations of agents, sources, agent_types (some with
/// contracts, some without), and outputs (some clean, some with violations,
/// some non-JSON). The four properties anchor the ledger's integrity:
///
/// 1. **Append-only invariant** — after N `enforce_for_agent` calls,
///    `trend(Global).total_delegations == N`. No records lost, no duplicates.
///
/// 2. **Scope partitioning** — `Σ trend(ByAgent(aᵢ)) == trend(Global)` for all
///    distinct agents. Same for `BySource`. Catches scope filters that include
///    or exclude the wrong records.
///
/// 3. **Trend bucket accounting** — `with_contract + without_contract ==
///    total` and `zero_nulled + nulled <= with_contract`. The `<=` (not `==`)
///    is because non-object outputs with a contract are counted as
///    `with_contract` but neither `zero_nulled` nor `nulled` (absence ≠
///    verdict, Rule 5.3).
/// 4. **`enforce_for_agent` ≡ `enforce_grounding`** — when a contract exists
///    and the output is a JSON object, the cleaning from `enforce_for_agent`
///    must be byte-identical to calling `enforce_grounding` directly. The
///    store is a wrapper that adds ledger writes; it must not modify the
///    cleaning.
#[cfg(test)]
mod proptests {
    use super::*;
    use crate::grounding::{enforce_grounding, task_agent_contract};
    use proptest::prelude::*;
    use serde_json::json;
    use std::collections::BTreeSet;

    /// One delegation action: a call to `enforce_for_agent` with all its
    /// parameters. Generated by the proptest strategy and executed against
    /// a fresh `VerificationStore`.
    #[derive(Debug, Clone)]
    struct DelegationAction {
        source: String,
        agent_id: String,
        agent_type: String,
        output: serde_json::Value,
        tool_calls: Vec<serde_json::Value>,
        narrative: String,
    }

    /// A small set of agent_types: "task" (has a contract) and a few without.
    /// This ensures we exercise both the contract-matched path and the
    /// coverage-gap path in every generated sequence.
    const AGENT_TYPES: &[&str] = &["task", "research", "narrator", "planner"];

    /// A small set of source tools — the real ones plus a synthetic one.
    const SOURCES: &[&str] = &[
        "kanban_task_spawn",
        "swarm_delegate_local",
        "swarm_execute_plan_local",
        "test_source",
    ];

    /// Generate a delegation action. The output is an arbitrary JSON value
    /// (sometimes an object with contract fields, sometimes not). The
    /// tool_calls are a vec of `{"tool": <str>, "ok": <bool>}` entries.
    fn arb_delegation_action() -> BoxedStrategy<DelegationAction> {
        let source = proptest::sample::select(SOURCES.to_vec());
        let agent_id = prop::string::string_regex("[a-z][a-z0-9_]{0,10}")
            .expect("valid regex")
            .boxed();
        let agent_type = proptest::sample::select(AGENT_TYPES.to_vec());
        let output = arb_output_for_grounding();
        let tool_calls = prop::collection::vec(
            ("[a-z][a-z0-9_/]{0,20}", any::<bool>())
                .prop_map(|(tool, ok)| json!({ "tool": tool, "ok": ok })),
            0..6,
        );
        let narrative = prop::string::string_regex("[a-zA-Z0-9 /._-]{0,100}").expect("valid regex");
        (source, agent_id, agent_type, output, tool_calls, narrative)
            .prop_map(
                |(source, agent_id, agent_type, output, tool_calls, narrative)| DelegationAction {
                    source: source.to_string(),
                    agent_id,
                    agent_type: agent_type.to_string(),
                    output,
                    tool_calls,
                    narrative,
                },
            )
            .boxed()
    }

    /// Generate an output JSON value. Sometimes a JSON object with contract
    /// fields (deliverable_path, test_verdict, summary, approach), sometimes
    /// an arbitrary JSON value (to exercise the non-object path and the
    /// UncommissionedInference path).
    fn arb_output_for_grounding() -> BoxedStrategy<serde_json::Value> {
        prop_oneof![
            // A JSON object with contract fields — exercises the Sourced /
            // Unsourced / Inferred paths.
            prop::collection::vec(
                proptest::sample::select(
                    [
                        "deliverable_path",
                        "test_verdict",
                        "summary",
                        "approach",
                        "extra_field"
                    ]
                    .to_vec(),
                ),
                0..5,
            )
            .prop_map(|fields| {
                let mut map = serde_json::Map::new();
                for f in fields {
                    let val = match f {
                        "deliverable_path" => json!("/src/main.rs"),
                        "test_verdict" => json!("pass: all tests passed"),
                        "summary" => json!("I completed the task."),
                        "approach" => json!("I used a direct approach."),
                        _ => json!("surprise"),
                    };
                    map.insert(f.to_string(), val);
                }
                serde_json::Value::Object(map)
            }),
            // An arbitrary JSON value — exercises the non-object path and
            // arbitrary object shapes.
            hkask_test_harness::arb_json_value(),
        ]
        .boxed()
    }

    /// Generate a sequence of delegation actions.
    fn arb_delegation_sequence() -> BoxedStrategy<Vec<DelegationAction>> {
        prop::collection::vec(arb_delegation_action(), 0..20).boxed()
    }

    /// Execute a sequence of delegation actions against a fresh store and
    /// return the store. This is the shared setup for all properties.
    fn execute_sequence(actions: &[DelegationAction]) -> VerificationStore {
        let store = VerificationStore::in_memory();
        for action in actions {
            store.enforce_for_agent(
                &action.source,
                &action.agent_id,
                &action.agent_type,
                &action.output,
                &action.tool_calls,
                &action.narrative,
            );
        }
        store
    }

    proptest! {
        /// **Property 1: Append-only invariant.**
        ///
        /// After N `enforce_for_agent` calls, `trend(Global).total_delegations`
        /// must equal N. No records lost, no duplicates. This is the
        /// foundation: if the ledger lies about the count, every downstream
        /// metric is wrong.
        #[test]
        fn ledger_total_delegations_equals_action_count(
            actions in arb_delegation_sequence(),
        ) {
            let store = execute_sequence(&actions);
            let trend = store.grounding_trend(&TrendScope::Global)
                .expect("trend query must succeed on a healthy store");
            prop_assert_eq!(
                trend.total_delegations,
                actions.len(),
                "total_delegations must equal the number of enforce_for_agent calls — \
                 a mismatch means the ledger lost or duplicated records"
            );
        }

        /// **Property 2a: Scope partitioning by agent.**
        ///
        /// The sum of `total_delegations` across all distinct agents (via
        /// `ByAgent` scope) must equal `total_delegations` for `Global` scope.
        /// Catches scope filters that include or exclude the wrong records.
        #[test]
        fn by_agent_scope_partitions_global(
            actions in arb_delegation_sequence(),
        ) {
            let store = execute_sequence(&actions);
            let global = store.grounding_trend(&TrendScope::Global)
                .expect("global trend");
            // Collect the distinct agent_ids from the action sequence.
            let distinct_agents: BTreeSet<&str> =
                actions.iter().map(|a| a.agent_id.as_str()).collect();
            let mut sum_by_agent = 0usize;
            for agent_id in &distinct_agents {
                let trend = store
                    .grounding_trend(&TrendScope::ByAgent(agent_id.to_string()))
                    .expect("per-agent trend");
                sum_by_agent += trend.total_delegations;
            }
            prop_assert_eq!(
                sum_by_agent, global.total_delegations,
                "Σ trend(ByAgent(aᵢ)).total_delegations must equal trend(Global).total_delegations — \
                 a mismatch means the ByAgent scope filter is wrong"
            );
        }

        /// **Property 2b: Scope partitioning by source.**
        ///
        /// Same as 2a but for `BySource` scope.
        #[test]
        fn by_source_scope_partitions_global(
            actions in arb_delegation_sequence(),
        ) {
            let store = execute_sequence(&actions);
            let global = store.grounding_trend(&TrendScope::Global)
                .expect("global trend");
            let distinct_sources: BTreeSet<&str> =
                actions.iter().map(|a| a.source.as_str()).collect();
            let mut sum_by_source = 0usize;
            for source in &distinct_sources {
                let trend = store
                    .grounding_trend(&TrendScope::BySource(source.to_string()))
                    .expect("per-source trend");
                sum_by_source += trend.total_delegations;
            }
            prop_assert_eq!(
                sum_by_source, global.total_delegations,
                "Σ trend(BySource(sᵢ)).total_delegations must equal trend(Global).total_delegations — \
                 a mismatch means the BySource scope filter is wrong"
            );
        }

        /// **Property 3: Trend bucket accounting.**
        ///
        /// `delegations_with_contract + delegations_without_contract ==
        /// total_delegations` (every delegation is in exactly one bucket).
        ///
        /// `delegations_with_zero_nulled + delegations_with_nulled <=
        /// delegations_with_contract` (the `<=` is because non-object outputs
        /// with a contract are counted as `with_contract` but neither
        /// `zero_nulled` nor `nulled` — absence ≠ verdict, Rule 5.3).
        ///
        /// `delegations_with_narrative_leaks <= delegations_with_nulled`
        /// (a narrative leak requires a nulled field to leak from — a
        /// delegation with zero nulled fields cannot have a narrative leak).
        #[test]
        fn trend_buckets_are_consistent(
            actions in arb_delegation_sequence(),
        ) {
            let store = execute_sequence(&actions);
            let trend = store.grounding_trend(&TrendScope::Global)
                .expect("trend");
            // Every delegation is in exactly one contract bucket.
            prop_assert_eq!(
                trend.delegations_with_contract + trend.delegations_without_contract,
                trend.total_delegations,
                "with_contract + without_contract must equal total — \
                 every delegation must be in exactly one bucket"
            );
            // The zero_nulled + nulled buckets are a subset of with_contract.
            // The gap is delegations where the contract matched but the output
            // was not a JSON object (absence ≠ verdict, Rule 5.3).
            prop_assert!(
                trend.delegations_with_zero_nulled + trend.delegations_with_nulled
                    <= trend.delegations_with_contract,
                "zero_nulled + nulled must be <= with_contract — \
                 the gap is non-object outputs with a contract (Rule 5.3: absence ≠ verdict)"
            );
            // A narrative leak requires a nulled field to leak from.
            prop_assert!(
                trend.delegations_with_narrative_leaks <= trend.delegations_with_nulled,
                "narrative_leaks must be <= nulled — \
                 a leak requires a nulled field to leak from"
            );
        }

        /// **Property 4: `enforce_for_agent` ≡ `enforce_grounding`.**
        ///
        /// When a contract exists for the agent_type and the output is a JSON
        /// object, the cleaned output from `enforce_for_agent` must be
        /// byte-identical to calling `enforce_grounding` directly. The store
        /// is a wrapper that adds ledger writes; it must not modify the
        /// cleaning. If this property breaks, the ledger records would
        /// disagree with the actual cleaned output returned to the caller.
        #[test]
        fn enforce_for_agent_matches_enforce_grounding(
            output in arb_output_for_grounding(),
            tool_calls in prop::collection::vec(
                ("[a-z][a-z0-9_/]{0,20}", any::<bool>())
                    .prop_map(|(tool, ok)| json!({ "tool": tool, "ok": ok })),
                0..6,
            ),
            narrative in prop::string::string_regex("[a-zA-Z0-9 /._-]{0,100}")
                .expect("valid regex"),
        ) {
            let store = VerificationStore::in_memory();
            let contract = task_agent_contract();
            // Direct call to the pure function.
            let (direct_result, direct_cleaned) =
                enforce_grounding(&contract, &output, &tool_calls, &narrative);
            // Call through the store wrapper.
            let (store_result, store_cleaned) = store.enforce_for_agent(
                "test_source",
                "test_agent",
                "task", // contract exists for "task"
                &output,
                &tool_calls,
                &narrative,
            );
            // When the output is a JSON object, both must agree.
            if output.is_object() {
                let store_result = store_result.expect("contract exists for task → Some result");
                // Compare nulled_fields as sets (order is non-deterministic —
                // it depends on HashMap iteration order in enforce_grounding).
                let store_nulled: BTreeSet<&str> =
                    store_result.nulled_fields.iter().map(|s| s.as_str()).collect();
                let direct_nulled: BTreeSet<&str> =
                    direct_result.nulled_fields.iter().map(|s| s.as_str()).collect();
                prop_assert_eq!(
                    store_nulled, direct_nulled,
                    "nulled_fields must match (as sets) between enforce_for_agent and enforce_grounding"
                );
                prop_assert_eq!(
                    store_result.narrative_leaks, direct_result.narrative_leaks,
                    "narrative_leaks must match between enforce_for_agent and enforce_grounding"
                );
                prop_assert_eq!(
                    store_cleaned, direct_cleaned,
                    "cleaned output must be byte-identical — the store must not modify the cleaning"
                );
            } else {
                // Non-object output: the store writes a coverage-gap and
                // returns (None, original). The pure function returns an
                // empty result and the original output.
                prop_assert!(
                    store_result.is_none(),
                    "non-object output → store returns None (coverage gap)"
                );
                prop_assert_eq!(
                    store_cleaned, output,
                    "non-object output → store returns the original unchanged"
                );
                prop_assert!(
                    direct_result.nulled_fields.is_empty(),
                    "non-object output → enforce_grounding finds no nulled fields"
                );
            }
        }

        /// **Property 5: Violations query is a subset of all delegations.**
        ///
        /// The violations query returns delegations with nulled fields or
        /// narrative leaks. Every violation record must have `had_contract:
        /// true` (a coverage-gap record has no violations) and at least one
        /// nulled field or narrative leak. The count must not exceed
        /// `total_delegations`.
        #[test]
        fn violations_are_a_subset_of_all_delegations(
            actions in arb_delegation_sequence(),
        ) {
            let store = execute_sequence(&actions);
            let since = chrono::Utc::now() - chrono::Duration::days(7);
            let violations = store
                .grounding_violations(since, &TrendScope::Global)
                .expect("violations query");
            let trend = store
                .grounding_trend(&TrendScope::Global)
                .expect("trend");
            prop_assert!(
                violations.len() <= trend.total_delegations,
                "violations count must not exceed total delegations"
            );
            for v in &violations {
                prop_assert!(
                    v.had_contract,
                    "every violation must have had_contract: true — \
                     a coverage-gap record (had_contract: false) has no violations"
                );
                prop_assert!(
                    !v.nulled_fields.is_empty() || !v.narrative_leaks.is_empty(),
                    "every violation must have at least one nulled field or narrative leak"
                );
            }
            // The violations count must equal the number of delegations with
            // nulled fields or narrative leaks in the trend.
            let expected_violation_count =
                trend.delegations_with_nulled; // every nulled delegation is a violation
            prop_assert_eq!(
                violations.len(),
                expected_violation_count,
                "violations count must equal delegations_with_nulled in the trend"
            );
        }

        /// **Property 6: Coverage gap records have no violations.**
        ///
        /// A coverage-gap record (`had_contract: false`) must have empty
        /// `nulled_fields` and `narrative_leaks`. This is the absence ≠
        /// verdict property (Rule 5.3): a delegation with no contract is
        /// "not checked," not "compliant" — but it also cannot have
        /// violations because the check never ran.
        #[test]
        fn coverage_gap_records_have_no_violations(
            actions in arb_delegation_sequence(),
        ) {
            let store = execute_sequence(&actions);
            let since = chrono::Utc::now() - chrono::Duration::days(7);
            // Query all records by checking that violations only contain
            // had_contract: true records (Property 5 already checks this).
            // Here we check the dual: the coverage-gap count in the trend
            // must equal total - with_contract, and none of those are in
            // the violations list.
            let trend = store
                .grounding_trend(&TrendScope::Global)
                .expect("trend");
            let violations = store
                .grounding_violations(since, &TrendScope::Global)
                .expect("violations");
            // Every violation has had_contract: true (checked in Property 5).
            // The number of non-violation delegations with a contract is:
            //   with_contract - violations.len()
            // The number of coverage-gap delegations is:
            //   without_contract
            // Their sum + violations.len() must equal total.
            let non_violation_with_contract =
                trend.delegations_with_contract.saturating_sub(violations.len());
            prop_assert_eq!(
                non_violation_with_contract + violations.len()
                    + trend.delegations_without_contract,
                trend.total_delegations,
                "every delegation is either: a violation (with contract), \
                 a non-violation (with contract), or a coverage gap (without contract)"
            );
        }
    }
}
