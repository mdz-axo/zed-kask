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
pub(crate) struct BridgeRolloutsRequest {
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

                // Emit the datasets. The line construction is pure (no I/O) so it
                // lives in `build_dataset_lines`, which the tests exercise directly
                // — the pairing invariants are pinned there.
                let (sft_lines, preference_lines, skipped_no_bodies) =
                    build_dataset_lines(&by_task, &rollout_bodies, mode);

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

/// Verdict-labeled rollouts for one harness task: which rollout ids passed
/// and which failed. The bridge pairs a passed and a failed rollout *within*
/// the same task group to form a preference pair, so `chosen` and `rejected`
/// are responses to the same prompt by construction.
#[derive(Default)]
struct TaskRollouts {
    passed: Vec<String>,
    failed: Vec<String>,
}

/// Build SFT and/or preference-pair dataset lines from grouped task rollouts
/// and their retained request/response bodies. Pure: no I/O, no DB.
///
/// **Pairing invariant.** Preference pairs are formed within a single task
/// group (keyed upstream by `(harness_run_id, task_index)`), so `chosen`
/// (passed) and `rejected` (failed) are responses to the same task prompt. The
/// prompt is taken from the chosen rollout only — the rejected rollout's
/// request body is never parsed. `passed.iter().zip(failed.iter())` pairs
/// positionally and truncates to the shorter side: excess unpaired rollouts
/// on either side are dropped (a passed rollout with no failed counterpart is
/// not fabricated into a pair). Rollouts without retained bodies are skipped
/// and counted in the returned `skipped_no_bodies`, never fabricated.
fn build_dataset_lines(
    by_task: &std::collections::BTreeMap<(String, i64), TaskRollouts>,
    rollout_bodies: &std::collections::HashMap<String, (String, String)>,
    mode: &str,
) -> (Vec<String>, Vec<String>, usize) {
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
            for (chosen, rejected) in passed_with_bodies.iter().zip(failed_with_bodies.iter()) {
                let chosen_messages = parse_message_pairs(&chosen.0);
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
    (sft_lines, preference_lines, skipped_no_bodies)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One task group `("run-1", 0)` with the given passed/failed rollout ids.
    fn one_task(
        passed: &[&str],
        failed: &[&str],
    ) -> std::collections::BTreeMap<(String, i64), TaskRollouts> {
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            ("run-1".to_string(), 0),
            TaskRollouts {
                passed: passed.iter().map(|s| s.to_string()).collect(),
                failed: failed.iter().map(|s| s.to_string()).collect(),
            },
        );
        map
    }

    /// A request body: a JSON array of `[role, content]` pairs (one user turn).
    fn request_body(user_msg: &str) -> String {
        serde_json::json!([["user", user_msg]]).to_string()
    }

    /// A `(request_body, response_body)` pair for the bodies map.
    fn body(user_msg: &str, response: &str) -> (String, String) {
        (request_body(user_msg), response.to_string())
    }

    #[test]
    fn sft_emits_one_chatml_line_per_passed_rollout_with_body() {
        let by_task = one_task(&["r1"], &[]);
        let mut bodies = std::collections::HashMap::new();
        bodies.insert("r1".to_string(), body("solve X", "good response"));
        let (sft, pref, skipped) = build_dataset_lines(&by_task, &bodies, "sft");
        assert_eq!(sft.len(), 1);
        assert!(pref.is_empty());
        assert_eq!(skipped, 0);
        let example: serde_json::Value = serde_json::from_str(&sft[0]).unwrap();
        assert_eq!(example["messages"][0]["role"], "user");
        assert_eq!(example["messages"][0]["content"], "solve X");
        assert_eq!(example["messages"][1]["role"], "assistant");
        assert_eq!(example["messages"][1]["content"], "good response");
    }

    #[test]
    fn preference_pair_uses_chosen_prompt_and_same_task_responses() {
        let by_task = one_task(&["pass1"], &["fail1"]);
        let mut bodies = std::collections::HashMap::new();
        bodies.insert("pass1".to_string(), body("solve X", "good"));
        bodies.insert("fail1".to_string(), body("solve X", "bad"));
        let (sft, pref, skipped) = build_dataset_lines(&by_task, &bodies, "preference");
        assert!(sft.is_empty());
        assert_eq!(pref.len(), 1);
        assert_eq!(skipped, 0);
        let pair: serde_json::Value = serde_json::from_str(&pref[0]).unwrap();
        assert_eq!(pair["prompt"], "solve X");
        assert_eq!(pair["chosen"], "good");
        assert_eq!(pair["rejected"], "bad");
    }

    #[test]
    fn preference_pair_never_parses_rejected_request_body() {
        // The rejected rollout's request body is garbage, but the pair must
        // still build correctly from the chosen rollout's prompt. This pins
        // that `rejected.0` is never consulted — the property that makes
        // skipping `parse_message_pairs(&rejected.0)` safe.
        let by_task = one_task(&["pass1"], &["fail1"]);
        let mut bodies = std::collections::HashMap::new();
        bodies.insert("pass1".to_string(), body("solve X", "good"));
        bodies.insert(
            "fail1".to_string(),
            ("not valid json pairs{{{".to_string(), "bad".to_string()),
        );
        let (_sft, pref, _skipped) = build_dataset_lines(&by_task, &bodies, "preference");
        assert_eq!(
            pref.len(),
            1,
            "a garbage rejected body must not abort the pair"
        );
        let pair: serde_json::Value = serde_json::from_str(&pref[0]).unwrap();
        assert_eq!(pair["prompt"], "solve X");
        assert_eq!(pair["chosen"], "good");
        assert_eq!(pair["rejected"], "bad");
    }

    #[test]
    fn preference_zip_truncates_to_the_shorter_side() {
        // 2 passed, 1 failed → one pair; the extra passed rollout is dropped,
        // not paired with a fabricated rejected. A preference pair needs a
        // real failed counterpart.
        let by_task = one_task(&["p1", "p2"], &["f1"]);
        let mut bodies = std::collections::HashMap::new();
        bodies.insert("p1".to_string(), body("solve X", "good1"));
        bodies.insert("p2".to_string(), body("solve X", "good2"));
        bodies.insert("f1".to_string(), body("solve X", "bad"));
        let (_sft, pref, _skipped) = build_dataset_lines(&by_task, &bodies, "preference");
        assert_eq!(
            pref.len(),
            1,
            "zip truncates: only one pair from 2 passed + 1 failed"
        );
    }

    #[test]
    fn rollouts_without_bodies_are_skipped_and_counted_never_fabricated() {
        // One passed WITH a body, one passed WITHOUT — only the one with a
        // body becomes an SFT example; the bodyless one is counted as skipped.
        let by_task = one_task(&["with_body", "no_body"], &[]);
        let mut bodies = std::collections::HashMap::new();
        bodies.insert("with_body".to_string(), body("solve X", "good"));
        let (sft, _pref, skipped) = build_dataset_lines(&by_task, &bodies, "sft");
        assert_eq!(
            sft.len(),
            1,
            "the bodyless rollout is not fabricated into an example"
        );
        assert_eq!(skipped, 1, "the bodyless rollout is counted as skipped");
    }

    #[test]
    fn both_mode_counts_passed_and_failed_without_bodies_separately() {
        // 2 passed (1 bodyless) + 1 failed (bodyless) in "both" mode:
        // skipped = 1 (passed bodyless) + 1 (failed bodyless) = 2.
        let by_task = one_task(&["p_ok", "p_nobody"], &["f_nobody"]);
        let mut bodies = std::collections::HashMap::new();
        bodies.insert("p_ok".to_string(), body("solve X", "good"));
        let (sft, pref, skipped) = build_dataset_lines(&by_task, &bodies, "both");
        assert_eq!(sft.len(), 1);
        assert_eq!(
            pref.len(),
            0,
            "the only failed has no body, so no pair forms"
        );
        assert_eq!(skipped, 2, "1 passed-without-body + 1 failed-without-body");
    }
}
