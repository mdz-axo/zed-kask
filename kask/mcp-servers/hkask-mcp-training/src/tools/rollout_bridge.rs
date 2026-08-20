//! Trajectory → dataset bridge (event-substrate phase 5).
//!
//! Reads verdict-labeled rollouts from the swarm event store and emits
//! training data in the canonical formats `dataset.rs` already consumes:
//!
//! - **SFT (ChatML JSONL)** — one `{"messages": [...]}` line per passed
//!   rollout: the task as the user turn, the response as the assistant
//!   turn. Only passed rollouts: SFT teaches imitation, and imitating
//!   failures teaches failures.
//! - **Preference pairs (DPO JSONL)** — a passed and a failed rollout on
//!   the same task are a `{"prompt", "chosen", "rejected"}` pair. The
//!   harness is a preference-pair generator grounded in real harness
//!   behavior (Agent Lightning's core thesis).
//!
//! The bridge reads the store; it never writes it. The store path defaults
//! to the same D28 layout the swarm server writes (`mcp/swarm/events.db`),
//! operator-configurable via `HKASK_SWARM_EVENTS_PATH`.

use crate::TrainingServer;
use hkask_mcp_server::server::{McpToolError, contain_for_write, execute_tool_semantic};
use hkask_storage::database::sqlite::SqliteDriver;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

/// Request for `training_bridge_rollouts`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct BridgeRolloutsRequest {
    /// Output dataset path (JSONL). Written under the contained write root,
    /// same rule as every other dataset-writing tool.
    pub output_path: String,
    /// What to emit: "sft" (ChatML from passed rollouts), "preference"
    /// (DPO pairs from passed+failed rollouts on the same task), or "both".
    pub mode: Option<String>,
    /// Only bridge rollouts for this agent (the rollout id prefix before
    /// the first `-`). Absent = all agents.
    pub agent_name: Option<String>,
    /// Maximum rollouts to read from the store. Default 1000.
    pub limit: Option<usize>,
}

#[tool_router(router = rollout_bridge_router, vis = "pub")]
impl TrainingServer {
    #[tool(
        description = "Survey verdict-labeled rollouts in the swarm event store and emit a bridge MANIFEST (counts of SFT candidates from passed rollouts and DPO preference-pair candidates from passed+failed on the same task), written to a JSON file. Does NOT emit training examples yet — the event store retains request shape, not bodies; body retention is the next store capability. Use this to size a future dataset before assembling it."
    )]
    pub async fn training_bridge_rollouts(
        &self,
        parameters: Parameters<BridgeRolloutsRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "training_bridge_rollouts",
            Self::ontology_anchor("training_bridge_rollouts"),
            async {
                let req = parameters.0;
                let mode = req.mode.as_deref().unwrap_or("both");
                if !matches!(mode, "sft" | "preference" | "both") {
                    return Err(McpToolError::invalid_argument(format!(
                        "mode must be 'sft', 'preference', or 'both'; got '{mode}'"
                    )));
                }
                let events_path = std::env::var("HKASK_SWARM_EVENTS_PATH")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| {
                        hkask_types::agent_paths::resolve_under_data_dir(
                            &hkask_types::agent_paths::mcp_server_db("swarm", "events"),
                        )
                        .to_string_lossy()
                        .to_string()
                    });
                if !std::path::Path::new(&events_path).exists() {
                    return Err(McpToolError::invalid_argument(format!(
                        "event store not found at '{events_path}' — run the rollout harness \
                         (swarm_eval_agent_local) first"
                    )));
                }
                let manager = r2d2_sqlite::SqliteConnectionManager::file(&events_path)
                    .with_init(|conn| conn.execute_batch(hkask_storage::WAL_PRAGMA_BATCH));
                let pool = r2d2::Pool::builder()
                    .max_size(2)
                    .build(manager)
                    .map_err(|e| {
                        McpToolError::internal(format!("failed to open event store: {e}"))
                    })?;
                let driver: std::sync::Arc<dyn hkask_storage::DatabaseDriver> =
                    std::sync::Arc::new(SqliteDriver::new(pool));
                let store = hkask_event_store::EventStore::from_driver(driver).map_err(|e| {
                    McpToolError::internal(format!("failed to init event store: {e}"))
                })?;

                // Read verdicts (the labels) and the rollout grouping.
                let limit = req.limit.unwrap_or(1000);
                let verdicts = store
                    .query(&hkask_event_store::EventFilter {
                        kind: Some("verdict".to_string()),
                        limit: Some(limit),
                        ..hkask_event_store::EventFilter::default()
                    })
                    .map_err(|e| {
                        McpToolError::internal(format!("event store query failed: {e}"))
                    })?;

                // Group verdicts by task (harness runs stamp task_index on the
                // verdict payload). A rollout is usable when its verdict names
                // a harness task — the bridge pairs by (harness_run_id, task_index).
                #[derive(Default)]
                struct TaskRollouts {
                    passed: Vec<String>,
                    failed: Vec<String>,
                }
                let mut by_task: std::collections::BTreeMap<(String, i64), TaskRollouts> =
                    std::collections::BTreeMap::new();
                for event in &verdicts {
                    let Some(harness_run_id) =
                        event.payload.get("harness_run_id").and_then(|v| v.as_str())
                    else {
                        continue;
                    };
                    let Some(task_index) = event.payload.get("task_index").and_then(|v| v.as_i64())
                    else {
                        continue;
                    };
                    if let Some(agent) = &req.agent_name
                        && !event
                            .rollout_id
                            .starts_with(&format!("delegation-{agent}-"))
                    {
                        continue;
                    }
                    let passed = event
                        .payload
                        .get("pass")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let entry = by_task
                        .entry((harness_run_id.to_string(), task_index))
                        .or_default();
                    if passed {
                        entry.passed.push(event.rollout_id.clone());
                    } else {
                        entry.failed.push(event.rollout_id.clone());
                    }
                }

                // The event store deliberately does not retain full request
                // and response bodies (retention risk 4 in the proposal:
                // request shape, not body). A dataset needs the bodies. The
                // bridge therefore reports what it found and emits a manifest
                // the operator can act on — it does NOT fabricate training
                // examples from ids alone (never fabricate: the dataset would
                // look complete while being empty of content).
                let sft_candidates: usize = by_task.values().map(|t| t.passed.len()).sum();
                let preference_candidates: usize = by_task
                    .values()
                    .map(|t| t.passed.len().min(t.failed.len()))
                    .sum();
                let output = contain_for_write(&req.output_path)?;
                let manifest = serde_json::json!({
                    "mode": mode,
                    "events_path": events_path,
                    "verdicts_read": verdicts.len(),
                    "tasks_with_verdicts": by_task.len(),
                    "sft_candidates": sft_candidates,
                    "preference_candidates": preference_candidates,
                    "output_path": output.display().to_string(),
                    "note": "the event store retains request shape, not bodies — pair this \
                             manifest with the harness report to assemble the dataset; \
                             body retention is the next store capability",
                });
                std::fs::write(
                    &output,
                    serde_json::to_string_pretty(&manifest).unwrap_or_default(),
                )
                .map_err(|e| {
                    hkask_mcp_server::map_io_error(
                        e,
                        &format!("Failed to write bridge manifest '{}'", output.display()),
                    )
                })?;
                Ok(manifest)
            },
        )
        .await
    }
}
