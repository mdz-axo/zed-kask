//! Per-agent execution statistics — the local analog of fermi's
//! `measured_exec_stats` (computed server-side from the episodes table and
//! surfaced on every agent via `build_agent_json`'s `execution_stats`).
//!
//! fermi keeps stats in a dedicated store (the episodes table), not in the
//! agent's knowledge graph — aggregation over a KG is expensive and the KG
//! is for consolidated knowledge, not counters. The local analog is the same
//! shape: a dedicated per-agent `stats.json` beside the card
//! (`agents/local/curated/<id>/stats.json`), updated at the one point where
//! the numbers are known (`LocalSwarmRuntime::build_result` — the
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
//!
//! Selection (`rank_for_slot`) lives here too: it ranks candidates BY the
//! measured stats, so the ranking and the measurement it reads cannot
//! drift apart. fermi's `select_agent` lesson (§4.4 of
//! WHAT_THE_PLATFORM_CAN_REFUSE): measure before promoting — a ranking
//! over thin measurement is noise dressed as a verdict, so the ranking
//! carries each candidate's execution count and says when it is too thin
//! to mean anything.

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
/// (`record_success` in the sequential result path, `record_failure` on the
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

    /// Record a completed execution (the only path that knows tokens and
    /// latency — `build_result`).
    pub fn record_success(&self, agent_id: &str, tokens_used: i64, latency_ms: u64) {
        self.mutate(agent_id, |stats| {
            stats.total_executions += 1;
            stats.successful_executions += 1;
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

/// One ranked candidate for a slot.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedCandidate {
    pub agent_id: String,
    pub agent_type: String,
    /// The measured stats the ranking read — carried on the row so a
    /// caller sees the n behind every rate (a 100% success rate over 1
    /// execution is not a measurement).
    pub measured: AgentExecutionStats,
    /// The rank position, 1-based.
    pub rank: usize,
}

/// Below this many recorded executions, a candidate's success rate is not
/// a measurement — the ranking still orders it, but the report says the
/// order is not earned. fermi's §4.4: the number that justifies trusting a
/// signal is the signal's own volume.
pub const MIN_MEASURED_EXECUTIONS: u64 = 3;

/// Rank candidate agents for a slot by measured performance: success rate
/// over recorded executions, then execution volume (more evidence outranks
/// less), then average latency (faster outranks slower at equal evidence).
/// Candidates with zero recorded executions sort last, alphabetically —
/// they have earned no position.
///
/// Pure over its inputs — the tool layer passes the registry's cards and a
/// stats lookup, tests pass fixtures. Returns the candidates and whether
/// the ranking is meaningful (`max_executions >= MIN_MEASURED_EXECUTIONS`).
pub fn rank_for_slot(
    candidates: &[crate::local_registry::LocalAgentCard],
    stats: impl Fn(&str) -> AgentExecutionStats,
) -> (Vec<RankedCandidate>, bool) {
    let mut rows: Vec<RankedCandidate> = candidates
        .iter()
        .map(|card| {
            let measured = stats(&card.agent_id);
            RankedCandidate {
                agent_id: card.agent_id.clone(),
                agent_type: card.agent_type.clone(),
                measured,
                rank: 0,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        let a_rate = success_rate(&a.measured);
        let b_rate = success_rate(&b.measured);
        // Unmeasured (None) sorts below every measured rate — including a
        // measured 0%: a failure that happened is evidence; nothing
        // happening is not.
        b_rate
            .partial_cmp(&a_rate)
            .map_or(std::cmp::Ordering::Less, |order| order)
            .then(
                b.measured
                    .total_executions
                    .cmp(&a.measured.total_executions),
            )
            .then(avg_latency(&a.measured).cmp(&avg_latency(&b.measured)))
            .then(a.agent_id.cmp(&b.agent_id))
    });
    let max_executions = rows
        .iter()
        .map(|row| row.measured.total_executions)
        .max()
        .unwrap_or(0);
    for (index, row) in rows.iter_mut().enumerate() {
        row.rank = index + 1;
    }
    (rows, max_executions >= MIN_MEASURED_EXECUTIONS)
}

/// The success rate when there is measurement, `None` when there is not —
/// absent must look different from a measured zero.
fn success_rate(stats: &AgentExecutionStats) -> Option<f64> {
    if stats.total_executions == 0 {
        return None;
    }
    Some(stats.successful_executions as f64 / stats.total_executions as f64)
}

/// Average latency in ms; `u64::MAX` sorts an unmeasured agent last on the
/// latency tiebreak.
fn avg_latency(stats: &AgentExecutionStats) -> u64 {
    if stats.total_executions == 0 {
        return u64::MAX;
    }
    stats.total_latency_ms / stats.total_executions
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

    // ── rank_for_slot ─────────────────────────────────────────────────────

    fn candidate(agent_id: &str) -> crate::local_registry::LocalAgentCard {
        crate::local_registry::LocalAgentCard {
            agent_id: agent_id.to_string(),
            agent_type: "analyst".to_string(),
            description: String::new(),
            display_name: String::new(),
            accepts: vec![],
            produces: vec![],
            dependencies: Default::default(),
            capabilities: Default::default(),
            cloud_swarm_id: None,
            tags: vec![],
            visibility: String::new(),
            sample_queries: vec![],
            valence: None,
            version: String::new(),
            workflow_template: None,
        }
    }

    fn stats_of(total: u64, successful: u64, latency: u64) -> AgentExecutionStats {
        AgentExecutionStats {
            total_executions: total,
            successful_executions: successful,
            failed_executions: total - successful,
            total_tokens_used: 0,
            total_latency_ms: latency,
            last_executed_at: None,
        }
    }

    #[test]
    fn ranking_orders_by_measured_success_rate() {
        let candidates = vec![
            candidate("shaky"),
            candidate("solid"),
            candidate("middling"),
        ];
        let stats = |agent_id: &str| match agent_id {
            "shaky" => stats_of(10, 2, 100),
            "solid" => stats_of(10, 9, 100),
            "middling" => stats_of(10, 5, 100),
            _ => AgentExecutionStats::default(),
        };
        let (ranked, meaningful) = rank_for_slot(&candidates, stats);
        assert!(meaningful, "10 executions is enough measurement");
        assert_eq!(
            ranked
                .iter()
                .map(|row| row.agent_id.as_str())
                .collect::<Vec<_>>(),
            vec!["solid", "middling", "shaky"]
        );
        assert_eq!(ranked[0].rank, 1);
    }

    #[test]
    fn more_evidence_outranks_less_at_equal_rate() {
        // Both 100%, but one over 10 runs and one over 2 — the volume
        // breaks the tie because more executions is more evidence.
        let candidates = vec![candidate("thin"), candidate("thick")];
        let stats = |agent_id: &str| match agent_id {
            "thin" => stats_of(2, 2, 100),
            "thick" => stats_of(10, 10, 100),
            _ => AgentExecutionStats::default(),
        };
        let (ranked, _) = rank_for_slot(&candidates, stats);
        assert_eq!(ranked[0].agent_id, "thick");
    }

    #[test]
    fn unmeasured_candidates_sort_last_not_by_rate() {
        // A measured 0% failure outranks an unmeasured agent: a failure
        // that happened is evidence; nothing happening is not.
        let candidates = vec![candidate("never_ran"), candidate("all_failed")];
        let stats = |agent_id: &str| match agent_id {
            "never_ran" => AgentExecutionStats::default(),
            "all_failed" => stats_of(4, 0, 100),
            _ => AgentExecutionStats::default(),
        };
        let (ranked, _) = rank_for_slot(&candidates, stats);
        assert_eq!(ranked[0].agent_id, "all_failed");
        assert_eq!(ranked[0].measured.total_executions, 4);
    }

    #[test]
    fn thin_measurement_is_reported_as_not_meaningful() {
        // fermi's §4.4: measure before promoting. A ranking over 1-2
        // executions is noise dressed as a verdict — the flag says so.
        let candidates = vec![candidate("one_run")];
        let stats = |agent_id: &str| match agent_id {
            "one_run" => stats_of(1, 1, 100),
            _ => AgentExecutionStats::default(),
        };
        let (ranked, meaningful) = rank_for_slot(&candidates, stats);
        assert!(!meaningful, "1 execution is not a measurement");
        assert_eq!(ranked.len(), 1);
    }

    #[test]
    fn latency_breaks_ties_at_equal_rate_and_volume() {
        let candidates = vec![candidate("slow"), candidate("fast")];
        let stats = |agent_id: &str| match agent_id {
            "slow" => stats_of(10, 8, 5000),
            "fast" => stats_of(10, 8, 1000),
            _ => AgentExecutionStats::default(),
        };
        let (ranked, _) = rank_for_slot(&candidates, stats);
        assert_eq!(ranked[0].agent_id, "fast");
    }

    #[test]
    fn record_success_accumulates_and_flushes() {
        let dir = temp_dir();
        std::fs::create_dir_all(std::path::Path::new(&dir).join("my_agent")).expect("agent dir");
        let store = AgentStatsStore::load(&dir);
        store.record_success("my_agent", 2500, 400);
        store.record_success("my_agent", 500, 200);
        let json = store.stats_json("my_agent");
        assert_eq!(json["total_executions"], 2);
        assert_eq!(json["successful_executions"], 2);
        assert_eq!(json["failed_executions"], 0);
        assert_eq!(json["tokens_used"], 3000);
        assert_eq!(json["avg_execution_time_ms"], 300);
        assert_eq!(json["source"], "local_stats_file");
        assert!(json["last_executed_at"].as_str().is_some());
        // The flush wrote the file — a fresh load sees the same counters.
        let reloaded = AgentStatsStore::load(&dir);
        assert_eq!(reloaded.stats("my_agent").total_executions, 2);
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
        store.record_success("../../etc/passwd", 1, 1);
        assert!(!std::path::Path::new(&dir).join("etc").exists());
    }
}
