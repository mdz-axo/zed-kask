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
        description = "Bridge verdict-labeled rollouts from the swarm event store into training datasets. Emits ChatML JSONL for SFT (passed rollouts) and/or DPO preference pairs (passed+failed on the same task), using the request/response bodies retained on model_request events. Rollouts without retained bodies are skipped and counted in skipped_no_bodies. Reads the event store (mcp/swarm/events.db); writes datasets under the contained write root."
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

                // Fetch the bodies: one query per rollout group, keyed by
                // rollout id. The FINAL model_request of a rollout carries
                // the terminal request/response pair — the training example.
                // Rollouts without bodies (captured before body retention
                // landed, or dropped captures) are skipped and COUNTED —
                // never fabricated, never silently omitted.
                let mut rollout_bodies: std::collections::HashMap<String, (String, String)> =
                    std::collections::HashMap::new();
                for event in store
                    .query(&hkask_event_store::EventFilter {
                        kind: Some("model_request".to_string()),
                        limit: Some(limit * 8),
                        ..hkask_event_store::EventFilter::default()
                    })
                    .map_err(|e| McpToolError::internal(format!("event store query failed: {e}")))?
                {
                    let request_body = event
                        .payload
                        .get("request_body")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let response_body = event
                        .payload
                        .get("response_body")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if response_body.is_empty() {
                        continue;
                    }
                    // Later positions overwrite earlier ones — the LAST
                    // model_request of a rollout is the terminal exchange.
                    rollout_bodies.insert(
                        event.rollout_id.clone(),
                        (request_body.to_string(), response_body.to_string()),
                    );
                }

                // Emit the datasets. SFT: one ChatML line per passed rollout
                // with bodies. Preference: one DPO line per (passed, failed)
                // pair on the same task, both with bodies.
                let mut sft_lines: Vec<String> = Vec::new();
                let mut preference_lines: Vec<String> = Vec::new();
                let mut skipped_no_bodies = 0usize;
                for task_rollouts in by_task.values() {
                    let passed_with_bodies: Vec<&(String, String)> = task_rollouts
                        .passed
                        .iter()
                        .filter_map(|id| rollout_bodies.get(id))
                        .collect();
                    skipped_no_bodies += task_rollouts.passed.len() - passed_with_bodies.len();
                    if mode == "sft" || mode == "both" {
                        for (request_body, response_body) in &passed_with_bodies {
                            // The request body is the JSON-serialized message
                            // array the executor captured: [[role, content], ...].
                            // The ChatML example is the user turn (the task)
                            // and the assistant turn (the response).
                            let messages = parse_message_pairs(request_body);
                            if let Some(user_task) = messages
                                .iter()
                                .find(|(role, _)| role == "user")
                                .map(|(_, content)| content.clone())
                            {
                                let example = serde_json::json!({
                                    "messages": [
                                        { "role": "user", "content": user_task },
                                        { "role": "assistant", "content": response_body },
                                    ]
                                });
                                sft_lines.push(example.to_string());
                            }
                        }
                    }
                    if mode == "preference" || mode == "both" {
                        let failed_with_bodies: Vec<&(String, String)> = task_rollouts
                            .failed
                            .iter()
                            .filter_map(|id| rollout_bodies.get(id))
                            .collect();
                        skipped_no_bodies += task_rollouts.failed.len() - failed_with_bodies.len();
                        for (chosen, rejected) in
                            passed_with_bodies.iter().zip(failed_with_bodies.iter())
                        {
                            let chosen_messages = parse_message_pairs(&chosen.0);
                            let rejected_messages = parse_message_pairs(&rejected.0);
                            let prompt = chosen_messages
                                .iter()
                                .find(|(role, _)| role == "user")
                                .map(|(_, content)| content.clone());
                            if let Some(prompt) = prompt {
                                let example = serde_json::json!({
                                    "prompt": prompt,
                                    "chosen": chosen.1,
                                    "rejected": rejected.1,
                                });
                                preference_lines.push(example.to_string());
                            }
                        }
                    }
                }

                let output = contain_for_write(&req.output_path)?;
                let mut written = serde_json::Map::new();
                if mode == "sft" || mode == "both" {
                    let sft_path = output.with_extension("sft.jsonl");
                    std::fs::write(&sft_path, sft_lines.join("\n") + "\n").map_err(|e| {
                        hkask_mcp_server::map_io_error(
                            e,
                            &format!("Failed to write SFT dataset '{}'", sft_path.display()),
                        )
                    })?;
                    written.insert(
                        "sft_path".into(),
                        serde_json::json!(sft_path.display().to_string()),
                    );
                    written.insert("sft_examples".into(), serde_json::json!(sft_lines.len()));
                }
                if mode == "preference" || mode == "both" {
                    let pref_path = output.with_extension("preference.jsonl");
                    std::fs::write(&pref_path, preference_lines.join("\n") + "\n").map_err(
                        |e| {
                            hkask_mcp_server::map_io_error(
                                e,
                                &format!(
                                    "Failed to write preference dataset '{}'",
                                    pref_path.display()
                                ),
                            )
                        },
                    )?;
                    written.insert(
                        "preference_path".into(),
                        serde_json::json!(pref_path.display().to_string()),
                    );
                    written.insert(
                        "preference_examples".into(),
                        serde_json::json!(preference_lines.len()),
                    );
                }
                let mut report = serde_json::json!({
                    "mode": mode,
                    "events_path": events_path,
                    "verdicts_read": verdicts.len(),
                    "tasks_with_verdicts": by_task.len(),
                    "skipped_no_bodies": skipped_no_bodies,
                });
                if let serde_json::Value::Object(map) = &mut report {
                    map.extend(written);
                }
                Ok(report)
            },
        )
        .await
    }
}

/// Parse the executor's captured request body — a JSON array of
/// `[role, content]` pairs — into owned `(String, String)` tuples. Returns
/// an empty vec on a parse failure (the caller skips the example; a
/// malformed capture is not a training example).
fn parse_message_pairs(request_body: &str) -> Vec<(String, String)> {
    serde_json::from_str::<Vec<(String, String)>>(request_body).unwrap_or_default()
}
