//! Per-agent execution statistics — the local analog of fermi's
//! `measured_exec_stats` (computed server-side from the episodes table and
//! surfaced on every agent via `build_agent_json`'s `execution_stats`).
//!
//! fermi keeps stats in a dedicated store (the episodes table), not in the
//! agent's knowledge graph — aggregation over a KG is expensive and the KG
//! is for consolidated knowledge, not counters. The local analog is the same
//! shape: a dedicated per-agent `stats.json` beside the card
//! (`agents/local/curated/<id>/stats.json`), updated at the one point where
//! the numbers are known (`LocalSwarmRuntime::debit_and_build` — the
//! sequential debit path, so updates are single-writer by construction),
//! and surfaced on `swarm_get_local_agent` / `swarm_list_local_agents`.
//!
//! Honesty rules (the `.rules` broken-feedback-loop trap):
//! - A missing stats file means the agent NEVER RAN — zeros are real
//!   measurements, and `stats_json` labels them `source: "local_stats_file"`
//!   so a consumer can tell "measured zero" from "not measured" the way
//!   fermi's `source: "episodes" | "agents_row"` does.
//! - A failed stats flush is `tracing::warn!`-ed, never silently dropped —
//!   but never fails the delegation (stats are an enhancement, not a
//!   dependency — same contract as the stigmergy writes).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::sanitize::sanitize_agent_id;

/// The persisted per-agent counters. `total_latency_ms` accumulates so the
/// average is derivable without storing per-execution rows (fermi derives
/// `avg_execution_time_ms` from the episodes table the same way).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentExecutionStats {
    pub total_executions: u64,
    pub successful_executions: u64,
    pub failed_executions: u64,
    /// Total credits recorded (the capped ledger cost — the same figure the
    /// delegation result carries as `cost`).
    pub total_cost_credits: i64,
    pub total_tokens_used: i64,
    /// Sum of end-to-end delegation latencies, for the average.
    pub total_latency_ms: u64,
    /// ISO-8601 timestamp of the last recorded execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_executed_at: Option<String>,
}

impl AgentExecutionStats {
    /// fermi's `execution_stats` response shape (`build_agent_json`):
    /// counters, spend, average latency, and the `source` label that lets a
    /// consumer distinguish measured zeros from absent measurement.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total_executions": self.total_executions,
            "successful_executions": self.successful_executions,
            "failed_executions": self.failed_executions,
            "total_cost_credits": self.total_cost_credits,
            "tokens_used": self.total_tokens_used,
            "avg_execution_time_ms": self
                .total_latency_ms
                .checked_div(self.total_executions)
                .unwrap_or(0),
            "last_executed_at": self.last_executed_at,
            "source": "local_stats_file",
        })
    }
}

/// File-backed per-agent stats. One in-memory map, flushed to
/// `<agents_dir>/<safe_id>/stats.json` after every update. The update points
/// (`record_success` in the sequential debit path, `record_failure` on the
/// error paths) are single-writer by construction, so the Mutex is held only
/// briefly for map mutation + flush.
pub struct AgentStatsStore {
    dir: String,
    inner: Mutex<HashMap<String, AgentExecutionStats>>,
}

impl AgentStatsStore {
    /// Load every agent's persisted stats from `<dir>/<safe_id>/stats.json`.
    /// A missing file is the normal never-ran state (no entry); a malformed
    /// file is warned and skipped — one bad file must not cost the whole
    /// store (the same containment rule as the agent-card loader).
    pub fn load(dir: &str) -> Self {
        let mut map = HashMap::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => {
                // A missing agents dir is the normal first-run state — the
                // registry loader emits the startup warning for that case;
                // here an empty map is correct, not an error.
                return Self {
                    dir: dir.to_string(),
                    inner: Mutex::new(map),
                };
            }
        };
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let stats_path = entry.path().join("stats.json");
            if !stats_path.exists() {
                continue;
            }
            let agent_id = match entry.file_name().to_str() {
                Some(name) => name.to_string(),
                None => continue,
            };
            match std::fs::read_to_string(&stats_path)
                .map_err(|e| e.to_string())
                .and_then(|text| {
                    serde_json::from_str::<AgentExecutionStats>(&text).map_err(|e| e.to_string())
                }) {
                Ok(stats) => {
                    map.insert(agent_id, stats);
                }
                Err(error) => tracing::warn!(
                    target: "hkask.mcp.swarm",
                    agent = %agent_id,
                    %error,
                    "malformed stats.json skipped — stats restart from zero for this agent"
                ),
            }
        }
        Self {
            dir: dir.to_string(),
            inner: Mutex::new(map),
        }
    }

    /// Record a completed execution (the only path that knows cost, tokens,
    /// and latency — `debit_and_build`).
    pub fn record_success(
        &self,
        agent_id: &str,
        cost_credits: i64,
        tokens_used: i64,
        latency_ms: u64,
    ) {
        self.mutate(agent_id, |stats| {
            stats.total_executions += 1;
            stats.successful_executions += 1;
            stats.total_cost_credits += cost_credits;
            stats.total_tokens_used += tokens_used;
            stats.total_latency_ms += latency_ms;
        });
    }

    /// Record a failed execution (the agent ran and errored — an inference
    /// failure or a panicked task, not a request rejected before execution).
    pub fn record_failure(&self, agent_id: &str) {
        self.mutate(agent_id, |stats| {
            stats.total_executions += 1;
            stats.failed_executions += 1;
        });
    }

    /// The agent's current stats, or the zeroed never-ran default. Zeros are
    /// real measurements (the agent has never executed), labeled by
    /// `source` so a consumer can tell.
    pub fn stats(&self, agent_id: &str) -> AgentExecutionStats {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(agent_id)
            .cloned()
            .unwrap_or_default()
    }

    /// fermi's response shape for one agent.
    pub fn stats_json(&self, agent_id: &str) -> serde_json::Value {
        self.stats(agent_id).to_json()
    }

    /// Mutate one agent's stats under the lock, stamp the timestamp, and
    /// flush. A flush failure is warned, never propagated — the delegation
    /// already succeeded and must not fail over bookkeeping.
    fn mutate(&self, agent_id: &str, apply: impl FnOnce(&mut AgentExecutionStats)) {
        let mut map = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let stats = map.entry(agent_id.to_string()).or_default();
        apply(stats);
        stats.last_executed_at = Some(chrono::Utc::now().to_rfc3339());
        let snapshot = stats.clone();
        drop(map);
        self.flush(agent_id, &snapshot);
    }

    /// Write one agent's `stats.json` beside its card. The agent id is
    /// sanitized for the filesystem the same way the card loader does
    /// (defense-in-depth — ids come from cards on disk).
    fn flush(&self, agent_id: &str, stats: &AgentExecutionStats) {
        let Some(safe_id) = sanitize_agent_id(agent_id) else {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                agent = %agent_id,
                "stats flush skipped — agent id contains no safe characters"
            );
            return;
        };
        let path = std::path::Path::new(&self.dir)
            .join(&safe_id)
            .join("stats.json");
        let json = match serde_json::to_string_pretty(stats) {
            Ok(json) => json,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    agent = %agent_id,
                    %error,
                    "stats flush skipped — serialization failed"
                );
                return;
            }
        };
        if let Err(error) = std::fs::write(&path, json) {
            tracing::warn!(
                target: "hkask.mcp.swarm",
                agent = %agent_id,
                path = %path.display(),
                %error,
                "stats flush failed — in-memory stats are ahead of disk (non-fatal)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> String {
        let dir =
            std::env::temp_dir().join(format!("hkask-swarm-stats-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir.to_string_lossy().to_string()
    }

    #[test]
    fn record_success_accumulates_and_flushes() {
        let dir = temp_dir();
        std::fs::create_dir_all(std::path::Path::new(&dir).join("my_agent")).expect("agent dir");
        let store = AgentStatsStore::load(&dir);
        store.record_success("my_agent", 3, 2500, 400);
        store.record_success("my_agent", 1, 500, 200);
        let json = store.stats_json("my_agent");
        assert_eq!(json["total_executions"], 2);
        assert_eq!(json["successful_executions"], 2);
        assert_eq!(json["failed_executions"], 0);
        assert_eq!(json["total_cost_credits"], 4);
        assert_eq!(json["tokens_used"], 3000);
        assert_eq!(json["avg_execution_time_ms"], 300);
        assert_eq!(json["source"], "local_stats_file");
        assert!(json["last_executed_at"].as_str().is_some());
        // The flush wrote the file — a fresh load sees the same counters.
        let reloaded = AgentStatsStore::load(&dir);
        assert_eq!(reloaded.stats("my_agent").total_executions, 2);
        assert_eq!(reloaded.stats("my_agent").total_cost_credits, 4);
    }

    #[test]
    fn record_failure_counts_without_spend() {
        let dir = temp_dir();
        let store = AgentStatsStore::load(&dir);
        store.record_failure("flaky_agent");
        let stats = store.stats("flaky_agent");
        assert_eq!(stats.total_executions, 1);
        assert_eq!(stats.failed_executions, 1);
        assert_eq!(stats.successful_executions, 0);
        assert_eq!(stats.total_cost_credits, 0);
    }

    #[test]
    fn never_ran_agent_reports_labeled_zeros() {
        let store = AgentStatsStore::load(&temp_dir());
        let json = store.stats_json("ghost_agent");
        // Zeros are real (never ran), and the source label says where they
        // came from — the fermi `source: "episodes" | "agents_row"` honesty
        // pattern.
        assert_eq!(json["total_executions"], 0);
        assert_eq!(json["source"], "local_stats_file");
    }

    #[test]
    fn malformed_stats_file_is_warned_and_skipped() {
        let dir = temp_dir();
        let agent_dir = std::path::Path::new(&dir).join("broken_agent");
        std::fs::create_dir_all(&agent_dir).expect("agent dir");
        std::fs::write(agent_dir.join("stats.json"), "not json").expect("write");
        let store = AgentStatsStore::load(&dir);
        // The broken file is skipped — the agent reads as never-ran, and the
        // rest of the store still works.
        assert_eq!(store.stats("broken_agent").total_executions, 0);
    }

    #[test]
    fn unsafe_agent_id_never_writes_outside_the_store() {
        let dir = temp_dir();
        let store = AgentStatsStore::load(&dir);
        // A path-traversal id is refused at the flush boundary — no file is
        // written outside the agents dir.
        store.record_success("../../etc/passwd", 1, 1, 1);
        assert!(!std::path::Path::new(&dir).join("etc").exists());
    }
}
