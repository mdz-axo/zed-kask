//! DaemonHandler implementation — bridges the Unix socket daemon to hKask's
//! PodManager, UserStore, memory infrastructure, and internal narrative generation.
//!
//! This is the hKask-side implementation of the `DaemonHandler` trait defined
//! in `hkask-mcp`. It wires daemon queries to the live agent and memory stack.
//!
//! # Narrative Generation
//!
//! When an agent is in server mode, tool calls accumulate as episodic experiences.
//! Every N experiences (NARRATIVE_THRESHOLD), the handler triggers internal narrative
//! generation: it queries the agent's recent episodic memories, calls inference to
//! produce observations about patterns and user intent, and stores those observations
//! as additional episodic memories. This is how the agent "thinks about" what it's
//! observing in the MCP session — the same way a chat-mode agent thinks about
//! conversation turns.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use hkask_mcp_server::daemon::DaemonHandler;
use hkask_pods::pod::ActivePods;
use hkask_regulation::RegulationLedger;
use hkask_storage::user_store::UserStore;
use hkask_types::InferencePort;
use hkask_types::template::LLMParameters;
use hkask_types::time::now_rfc3339;
use tokio::sync::RwLock;

/// Number of experiences before triggering internal narrative generation.
const NARRATIVE_THRESHOLD: usize = 10;

/// System prompt for narrative generation — the agent reflects on what it's observing.
const NARRATIVE_SYSTEM_PROMPT: &str = "You are an observant agent monitoring an MCP tool session. \
     Below is a log of recent tool calls made through the session. \
     Analyze the log and generate 2-3 concise observations about: \
     patterns in tool usage, what the user seems to be trying to accomplish, \
     notable events or anomalies. \
     Format each observation as a single sentence on its own line. \
     Be specific — reference actual tool names and patterns from the log. \
     Do not repeat the log content verbatim; synthesize insights.";

/// hKask-side implementation of the daemon handler trait.
///
/// Wraps PodManager for assignment/capability/memory queries,
/// UserStore for authentication, and InferencePort for narrative generation.
pub struct ServiceDaemonHandler {
    pod_manager: Arc<ActivePods>,
    user_store: Arc<std::sync::Mutex<UserStore>>,
    /// Regulation runtime for health and variety queries (None if unavailable)
    ledger_runtime: Option<Arc<RwLock<RegulationLedger>>>,
    /// Inference port for narrative generation (None if inference unavailable)
    inference_port: Option<Arc<dyn InferencePort>>,
    /// Per-userpod counter of stored experiences (triggers narrative generation)
    experience_counts: Mutex<HashMap<String, usize>>,
}

impl ServiceDaemonHandler {
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  pod_manager must be a valid `Arc<ActivePods>`; user_store must be a valid ``Arc<Mutex<UserStore>>``
    /// post: returns ServiceDaemonHandler with all fields initialized; inference_port may be None
    #[must_use]
    pub fn new(
        pod_manager: Arc<ActivePods>,
        user_store: Arc<std::sync::Mutex<UserStore>>,
        ledger_runtime: Option<Arc<RwLock<RegulationLedger>>>,
        inference_port: Option<Arc<dyn InferencePort>>,
    ) -> Self {
        tracing::info!(target: "hkask.daemon", operation = "new_handler", has_reg = ledger_runtime.is_some(), has_inference = inference_port.is_some(), "REG");

        Self {
            pod_manager,
            user_store,
            ledger_runtime,
            inference_port,
            experience_counts: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl DaemonHandler for ServiceDaemonHandler {
    async fn check_auth(&self, userpod: &str) -> (bool, Option<String>) {
        // P9: Regulation span
        tracing::info!(target: "hkask.daemon", operation = "check_auth", userpod = %userpod, "REG");

        let has_sessions = {
            let store = match self.user_store.lock() {
                Ok(s) => s,
                Err(_) => {
                    tracing::error!(target: "hkask.daemon", "UserStore lock poisoned");
                    return (false, None);
                }
            };
            let exists = store.get_userpod(userpod).is_ok();
            if !exists {
                tracing::warn!(target: "hkask.daemon", userpod = %userpod, "UserPod not found in user store");
                return (false, None);
            }
            let sessions = store.list_sessions(userpod).unwrap_or_default();
            !sessions.is_empty()
        };

        if !has_sessions {
            tracing::debug!(target: "hkask.daemon", userpod = %userpod, "No active sessions — needs passphrase");
            return (false, None);
        }

        tracing::debug!(target: "hkask.daemon", userpod = %userpod, "UserPod has active sessions");
        if let Some(pod_id) = self.pod_manager.find_pod_by_name(userpod).await {
            let webid = self.pod_manager.get_pod_webid(&pod_id).await;
            (true, webid.map(|w| w.to_string()))
        } else {
            (false, None)
        }
    }

    async fn check_capability(&self, userpod: &str, tool: &str) -> bool {
        // P9: Regulation span
        tracing::info!(target: "hkask.daemon", operation = "check_capability", userpod = %userpod, tool = %tool, "REG");

        match self.pod_manager.find_pod_by_name(userpod).await {
            Some(pod_id) => {
                let granted = self.pod_manager.has_capability(&pod_id, tool).await;
                tracing::debug!(target: "hkask.daemon", userpod = %userpod, tool = %tool, granted = granted, "Capability check");
                granted
            }
            None => false,
        }
    }

    async fn store_experience(
        &self,
        userpod: &str,
        entity: &str,
        attribute: &str,
        value: &serde_json::Value,
        confidence: Option<f64>,
    ) -> (bool, Option<String>, Option<String>) {
        // P9: Regulation span
        tracing::info!(target: "hkask.daemon", operation = "store_experience", userpod = %userpod, entity = %entity, attribute = %attribute, confidence = ?confidence, "REG");

        let pod_id = match self.pod_manager.find_pod_by_name(userpod).await {
            Some(id) => id,
            None => {
                tracing::warn!(target: "hkask.daemon", userpod = %userpod, "Pod not found for store_experience");
                return (false, None, None);
            }
        };

        let ctx = match self.pod_manager.context(&pod_id).await {
            Ok(ctx) => ctx,
            Err(e) => {
                tracing::warn!(target: "hkask.daemon", userpod = %userpod, error = %e, "Failed to create PodContext");
                return (false, None, None);
            }
        };

        let conf = confidence.unwrap_or(0.85);

        let episodic_result = ctx.store_episodic(
            entity,
            attribute,
            value.clone(),
            hkask_types::Confidence::new(conf),
        );

        let semantic_value = generalize_value(value);
        let semantic_result = ctx.store_semantic(
            entity,
            attribute,
            semantic_value,
            hkask_types::Confidence::new(conf),
        );

        let result = match (episodic_result, semantic_result) {
            (Ok(ep_id), Ok(sem_id)) => {
                tracing::debug!(target: "hkask.daemon", userpod = %userpod, episodic_id = %ep_id, semantic_id = %sem_id, "Dual-encoded experience");
                (true, Some(ep_id), Some(sem_id))
            }
            (Ok(ep_id), Err(e)) => {
                tracing::warn!(target: "hkask.daemon", userpod = %userpod, episodic_id = %ep_id, semantic_error = %e, "Episodic stored, semantic failed");
                (true, Some(ep_id), None)
            }
            (Err(e), _) => {
                tracing::warn!(target: "hkask.daemon", userpod = %userpod, error = %e, "Failed to store episodic experience");
                (false, None, None)
            }
        };

        // Check if we should trigger narrative generation
        if result.0
            && let Some(inference_port) = self.inference_port.as_ref()
        {
            let count = {
                let mut counts = self
                    .experience_counts
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let c = counts.entry(userpod.to_string()).or_insert(0);
                *c += 1;
                *c
            };

            if count % NARRATIVE_THRESHOLD == 0 {
                tracing::info!(target: "hkask.daemon.narrative", userpod = %userpod, count = count, "Triggering narrative generation");
                let pod_manager = Arc::clone(&self.pod_manager);
                let inference = Arc::clone(inference_port);
                let userpod_name = userpod.to_string();
                let handle = tokio::spawn(async move {
                    generate_narrative(&pod_manager, &*inference, &userpod_name).await;
                });
                tokio::spawn(async move {
                    if let Err(e) = handle.await {
                        tracing::error!(target: "hkask.daemon.narrative", error = %e, "Narrative generation task panicked");
                    }
                });
            }
        }

        // Persist session transcript to agents/{userpod}/sessions/
        // for audit and replay. Done as a fire-and-forget background write
        // to avoid blocking the daemon handler on filesystem I/O.
        if result.0 {
            let userpod_name = userpod.to_string();
            let value_clone = value.clone();
            let entity_clone = entity.to_string();
            let attr_clone = attribute.to_string();
            let handle = tokio::task::spawn_blocking(move || {
                append_session_entry(&userpod_name, &entity_clone, &attr_clone, &value_clone);
            });
            tokio::spawn(async move {
                if let Err(e) = handle.await {
                    tracing::error!(target: "hkask.daemon.session", error = %e, "Session append task panicked");
                }
            });
        }

        result
    }

    async fn dispatch_tool(
        &self,
        userpod: &str,
        tool: &str,
        input: &serde_json::Value,
    ) -> (bool, Option<serde_json::Value>, Option<String>) {
        // P9: Regulation span
        tracing::info!(target: "hkask.daemon", operation = "dispatch_tool", userpod = %userpod, tool = %tool, "REG");

        let pod_id = match self.pod_manager.find_pod_by_name(userpod).await {
            Some(id) => id,
            None => {
                return (false, None, Some("Pod not found".into()));
            }
        };

        let ctx = match self.pod_manager.context(&pod_id).await {
            Ok(ctx) => ctx,
            Err(e) => {
                return (false, None, Some(format!("PodContext error: {}", e)));
            }
        };

        match ctx.invoke_tool(tool, input.clone()).await {
            Ok(output) => (true, Some(output), None),
            Err(e) => (false, None, Some(e.to_string())),
        }
    }

    async fn curator_health(&self, _userpod: &str) -> serde_json::Value {
        let Some(ref reg_lock) = self.ledger_runtime else {
            return serde_json::json!({
                "timestamp": now_rfc3339(),
                "reg_health": "unknown",
                "note": "Regulation runtime not available"
            });
        };
        let ledger = reg_lock.read().await;
        let alerts = ledger.alerts().await;
        let critical = alerts.iter().filter(|a| a.is_critical()).count();
        let total = alerts.len();
        // Determine overall health from alerts
        let health = if critical > 0 {
            "critical"
        } else if total > 5 {
            "degraded"
        } else {
            "healthy"
        };
        serde_json::json!({
            "timestamp": now_rfc3339(),
            "reg_health": health,
            "critical_alerts": critical,
            "total_alerts": total,
        })
    }

    async fn reg_status(&self, _userpod: &str, domain: Option<&str>) -> serde_json::Value {
        let Some(ref reg_lock) = self.ledger_runtime else {
            return serde_json::json!({
                "timestamp": now_rfc3339(),
                "note": "Regulation runtime not available"
            });
        };
        let ledger = reg_lock.read().await;
        let variety = ledger.variety().await;
        let domains: Vec<serde_json::Value> = variety
            .iter()
            .filter(|(ns, _)| domain.is_none_or(|d| ns.as_str().contains(d)))
            .map(|(ns, count)| serde_json::json!({"domain": ns.as_str(), "variety": count}))
            .collect();
        serde_json::json!({
            "timestamp": now_rfc3339(),
            "domains": domains
        })
    }
}

/// Generate internal narrative observations from recent session experiences.
///
/// Queries the agent's episodic memory for recent "mcp_session" h_mems,
/// formats them as a log, calls inference to produce observations, and
/// stores those observations as new episodic memories.
async fn generate_narrative(
    pod_manager: &ActivePods,
    inference: &dyn InferencePort,
    userpod: &str,
) {
    // P9: Regulation span
    tracing::info!(target: "hkask.daemon", operation = "generate_narrative", userpod = %userpod, "REG");

    let pod_id = match pod_manager.find_pod_by_name(userpod).await {
        Some(id) => id,
        None => {
            tracing::warn!(target: "hkask.daemon.narrative", userpod = %userpod, "Pod not found for narrative generation");
            return;
        }
    };

    let ctx = match pod_manager.context(&pod_id).await {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::warn!(target: "hkask.daemon.narrative", userpod = %userpod, error = %e, "Failed to create PodContext for narrative");
            return;
        }
    };

    // Paired memory recall — both episodic (first-person) and semantic (third-person)
    // memories for the mcp_session entity, mirroring the dual-recall circuit.
    let memory = ctx.recall_memory("mcp_session");
    let episodes = memory.episodic;
    let _semantic = memory.semantic; // available for enriched narrative context

    if episodes.is_empty() {
        return;
    }

    // Build a log summary from recent experiences (last 20 max)
    let recent: Vec<_> = episodes.iter().take(20).collect();
    let mut log_lines = Vec::new();
    for ep in recent.iter().rev() {
        let val = &ep.value;
        if let Some(tool) = val.get("tool").and_then(|v| v.as_str()) {
            let input = val.get("input").and_then(|v| v.as_str()).unwrap_or("?");
            let outcome = val.get("outcome").and_then(|v| v.as_str()).unwrap_or("?");
            log_lines.push(format!(
                "- {}: input='{}', outcome={}",
                tool, input, outcome
            ));
        }
    }

    if log_lines.is_empty() {
        return;
    }

    let session_log = log_lines.join("\n");
    let prompt = format!(
        "{}\n\nRecent MCP session activity for userpod '{}':\n{}",
        NARRATIVE_SYSTEM_PROMPT, userpod, session_log
    );

    // Call inference — daemon narrative is a background summarization task.
    // Always bypass fusion: use the default model directly.
    let params = LLMParameters {
        temperature: 0.7,
        max_tokens: 256,
        bypass_fusion: true,
        fusion_config: None,
        ..LLMParameters::default()
    };

    let inference_result = match inference.generate(&prompt, &params, None).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(target: "hkask.daemon.narrative", userpod = %userpod, error = %e, "Inference failed for narrative generation");
            return;
        }
    };

    // Parse observations (one per line)
    let observations: Vec<&str> = inference_result
        .text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('-') && !l.starts_with('*'))
        .collect();

    if observations.is_empty() {
        // If no clean lines, use the whole response as one observation
        let trimmed = inference_result.text.trim();
        if !trimmed.is_empty() {
            let _ = ctx.store_episodic(
                "narrative",
                "thought",
                serde_json::json!({"observation": trimmed, "timestamp": now_rfc3339()}),
                hkask_types::Confidence::new(0.7),
            );
        }
        return;
    }

    // Store each observation as an episodic memory
    let obs_count = observations.len();
    for obs in observations {
        let value = serde_json::json!({
            "observation": obs,
            "source": "internal_narrative",
            "triggered_by": format!("{} experiences", recent.len()),
            "timestamp": now_rfc3339(),
        });

        match ctx.store_episodic(
            "narrative",
            "thought",
            value,
            hkask_types::Confidence::new(0.7),
        ) {
            Ok(id) => {
                tracing::debug!(target: "hkask.daemon.narrative", userpod = %userpod, triple_id = %id, observation = %obs, "Narrative observation stored");
            }
            Err(e) => {
                tracing::warn!(target: "hkask.daemon.narrative", userpod = %userpod, error = %e, "Failed to store narrative observation");
            }
        }
    }

    tracing::info!(target: "hkask.daemon.narrative", userpod = %userpod, observation_count = obs_count, "Narrative generation complete");
}

/// Generalize a value for semantic memory by stripping caller-specific details.
fn generalize_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut generalized = serde_json::Map::new();
            if let Some(tool) = map.get("tool") {
                generalized.insert("tool".to_string(), tool.clone());
            }
            if let Some(outcome) = map.get("outcome") {
                generalized.insert("outcome".to_string(), outcome.clone());
            }
            generalized.insert("generalized".to_string(), serde_json::Value::Bool(true));
            serde_json::Value::Object(generalized)
        }
        other => other.clone(),
    }
}

/// Append a session experience entry to the agent's sessions directory.
///
/// Each MCP session experience is recorded as a JSON line in a daily log
/// file under `agents/{name}/sessions/{date}.jsonl`. One JSON object per line
/// for easy streaming/parsing by downstream tools.
///
/// Regulation: emits `reg.session.recorded` span for variety tracking and algedonic
/// monitoring — if sessions stop writing, the Regulation detects the silence.
fn append_session_entry(userpod: &str, entity: &str, attribute: &str, value: &serde_json::Value) {
    let sessions_dir = hkask_types::agent_paths::userpod_sessions_dir(userpod);
    if let Err(e) = std::fs::create_dir_all(&sessions_dir) {
        tracing::warn!(target: "hkask.daemon.session", userpod = %userpod, error = %e, "Failed to create sessions directory");
        return;
    }

    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let session_file = sessions_dir.join(format!("{today}.jsonl"));

    let entry = serde_json::json!({
        "timestamp": hkask_types::time::now_rfc3339(),
        "userpod": userpod,
        "entity": entity,
        "attribute": attribute,
        "value": value,
    });

    let line = serde_json::to_string(&entry).unwrap_or_else(|_| String::from("{}"));
    let line_with_newline = format!("{line}\n");

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&session_file)
    {
        Ok(mut file) => {
            use std::io::Write;
            if let Err(e) = file.write_all(line_with_newline.as_bytes()) {
                tracing::warn!(target: "hkask.daemon.session", userpod = %userpod, path = %session_file.display(), error = %e, "Failed to write session entry");
            } else {
                // Regulation: session recorded — variety signal for algedonic monitoring
                tracing::info!(target: "hkask.session.recorded", userpod = %userpod, entity = %entity, "REG");
            }
        }
        Err(e) => {
            tracing::warn!(target: "hkask.daemon.session", userpod = %userpod, path = %session_file.display(), error = %e, "Failed to open session file");
        }
    }
}
