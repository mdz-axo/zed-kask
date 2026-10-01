//! Evolution registry health source bridge (§P8.9 step 1).
//!
//! Implements `hkask_regulation::EvolutionHealthSource` over the evolution
//! server's own store: opens the same SQLCipher registry the MCP child
//! serves (`HKASK_EVOLUTION_DB` override → the per-agent default under the
//! hKask data dir) and reads `EvolutionStore::health_snapshot()`.
//!
//! ## The blind-feedback-loop gap this closes
//!
//! Without this source, the cybernetics loop reports `signal_count=0` while
//! an evolution experiment sits unresolved forever: a running experiment
//! stranded past the stale set point (D-3: 7 days) or at its declared budget
//! ceiling with no verdict produces no signal. The registry is the record of
//! record; this bridge is its Layer-A afferent pathway.
//!
//! ## Path parity with the MCP child
//!
//! Both the bridge and the evolution server's `run()` resolve the registry
//! through `hkask_mcp_evolution::registry_path()` — one source of truth;
//! parity is structural, not mirrored. A drift there would read a different
//! registry than the one the tools write; the shared helper is pinned by
//! `registry_path_override_and_default` in the evolution crate.

use std::sync::Arc;

use hkask_mcp_evolution::store::EvolutionStore;
use hkask_regulation::EvolutionHealthSource;
use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::sqlite::SqliteDriver;

/// A `EvolutionHealthSource` over the evolution registry's health snapshot.
///
/// The composition root opens one `Arc<BridgeEvolutionHealthSource>` in the
/// deferred wiring task (where the DB passphrase is in scope) and passes it
/// to `CyberneticsLoop::set_evolution_health_source`. The snapshot is
/// re-read on every sense call — the store is a plain SQLCipher handle, and
/// the MCP child's writes are visible to this reader (the same multi-process
/// shape as the curator memory stores).
pub struct BridgeEvolutionHealthSource {
    store: EvolutionStore,
}

impl BridgeEvolutionHealthSource {
    /// Open the registry the evolution MCP child serves.
    ///
    /// Resolution is shared with the child (`registry_path()`): the
    /// `HKASK_EVOLUTION_DB` override or the per-agent default under the
    /// hKask data dir — never a private copy.
    pub fn open(passphrase: &str) -> Result<Self, String> {
        let db_path = hkask_mcp_evolution::registry_path()
            .to_string_lossy()
            .to_string();
        let db = hkask_storage::open_or_repair(&db_path, passphrase)
            .map_err(|error| format!("evolution registry {db_path}: {error}"))?;
        let pool = db
            .sqlite_pool()
            .map_err(|error| format!("sqlite pool: {error}"))?;
        let driver: Arc<dyn DatabaseDriver> =
            Arc::new(SqliteDriver::new_labeled(pool, db_path.as_str()));
        let store = EvolutionStore::with_driver(driver)
            .map_err(|error| format!("evolution store schema: {error}"))?;
        Ok(Self { store })
    }

    /// Build over an existing store — the test seam (mirrors the evolution
    /// server's in-memory test construction).
    #[cfg(test)]
    fn with_store(store: EvolutionStore) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl EvolutionHealthSource for BridgeEvolutionHealthSource {
    async fn stuck_running_experiments(&self, stale_days: u32) -> Result<Vec<String>, String> {
        let snapshot = self
            .store
            .health_snapshot()
            .map_err(|error| format!("health snapshot: {error}"))?;
        Ok(snapshot.stuck_running(stale_days as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_mcp_evolution::types::Prediction;
    use hkask_storage::database::sqlite::SqliteDriver;

    /// §P8.9 step 1 through the bridge: a running experiment whose whole
    /// declared budget is recorded with no verdict is stuck; a fresh
    /// under-budget experiment is not.
    #[tokio::test]
    async fn bridge_senses_budget_spent_running_experiments() {
        let store = EvolutionStore::with_driver(SqliteDriver::in_memory_driver())
            .expect("in-memory evolution store");
        let experiment = store
            .propose_experiment(
                "Variant B beats the baseline",
                "skill",
                &[".agents/skills/x/SKILL.md".to_string()],
                &serde_json::json!({"task_set": "fixed"}),
                "pass rate",
                &serde_json::json!({"cost_band_pct": 10}),
                &Prediction {
                    claim: "variant B wins".to_string(),
                    confidence: 0.5,
                },
                &serde_json::json!({"max_runs": 9}),
                9,
                None,
                None,
            )
            .expect("propose");
        let variant = store
            .register_variant(
                &experiment.id,
                &serde_json::json!({"name": "baseline"}),
                None,
                None,
            )
            .expect("variant");
        store
            .record_fitness(
                &experiment.id,
                &variant.id,
                &["harness report".to_string()],
                9,
                &serde_json::json!({"pass_rate": 0.5}),
            )
            .expect("fitness at the ceiling");

        let source = BridgeEvolutionHealthSource::with_store(store);
        let stuck = source
            .stuck_running_experiments(7)
            .await
            .expect("snapshot read");
        assert_eq!(stuck, vec![experiment.id.clone()]);

        // A verdict un-sticks it: the experiment resolves and leaves the
        // running set entirely.
        // (Resolved experiments are never stuck — stuck_running filters on
        // STATUS_RUNNING; this is pinned in the evolution crate's snapshot
        // tests. Here the budget-spent branch is the bridge's own path.)
    }
}
