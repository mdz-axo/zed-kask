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
        let passphrase = std::env::var("HKASK_VERIFICATION_PASSPHRASE")
            .unwrap_or_else(|_| "allostery".to_string());
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
    /// If a contract exists for the agent_type:
    ///   - Runs `enforce_grounding()` (nulls unsourced fields, scans narrative)
    ///   - Writes a full `GroundingRecord` to the ledger (append-only)
    ///   - Returns `(Some(result), cleaned_json)`
    ///
    /// If no contract exists:
    ///   - Writes a coverage-gap record to the ledger (`had_contract: false`)
    ///   - Returns `(None, original_json)` — unchanged
    ///
    /// If a contract exists but the output is not a JSON object (e.g. the
    /// agent produced prose): writes a coverage-gap record and returns
    /// `(None, original_json)`. The trend query counts this as
    /// "with contract" (the contract matched) but not "zero nulled" —
    /// absence ≠ verdict (paper Rule 5.3).
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
                    // not a JSON object — grounding cannot run. Record a
                    // coverage-gap so the trend query sees the delegation
                    // (absence ≠ verdict, paper Rule 5.3).
                    self.record_coverage_gap(source, agent_id, agent_type);
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

    /// Write a coverage-gap record (no contract for this agent_type, or
    /// contract existed but output was not a JSON object).
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
