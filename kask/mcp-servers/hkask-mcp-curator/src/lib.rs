#![forbid(unsafe_code)]
//! hkask-mcp-curator — Curator MCP server library.
//!
//! Exposes the Curator's regulatory surface as MCP tools:
//! system health, escalation management, Regulation observability,
//! cross-pod semantic search, memory recall, spec drift detection,
//! and algedonic event history.

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

pub mod types;

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_mcp_server::daemon::DaemonResponse;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_services_context::governance;
use hkask_storage::database::sqlite::SqliteDriver;

use hkask_types::WebID;
use hkask_types::event::RegulationSink;
use hkask_types::regulation::RegulationSpan;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use serde_json::json;
use std::sync::Arc;

use types::*;

const SERVER_NAME: &str = "hkask-mcp-curator";

hkask_mcp_server::mcp_server!(
    pub struct CuratorServer {
        escalation_queue: Option<Arc<hkask_storage::EscalationQueue>>,
        regulation_store: Option<Arc<hkask_storage::RegulationArchive>>,
        episodic: Option<hkask_memory::EpisodicMemory>,
        semantic: Option<Arc<hkask_memory::SemanticMemory>>,
        token_registry: Option<Arc<dyn hkask_capability::TokenRegistry>>,
    }
);

#[tool_router(server_handler)]
impl CuratorServer {
    // ── Liveness ───────────────────────────────────────────────────────

    #[tool(description = "Liveness check")]
    pub async fn curator_ping(&self, Parameters(_req): Parameters<PingRequest>) -> String {
        execute_tool(self, "curator_ping", async {
            Ok(json!({
                "status": "ok",
                "server": SERVER_NAME,
                "curator_webid": self.webid.to_string(),
                "userpod": self.userpod,
                "daemon_connected": self.daemon.is_some(),
                "stores": {
                    "escalation_queue": self.escalation_queue.is_some(),
                    "regulation_store": self.regulation_store.is_some(),
                    "episodic": self.episodic.is_some(),
                    "semantic": self.semantic.is_some(),
                }
            }))
        })
        .await
    }

    // ── Escalation Management ──────────────────────────────────────────

    #[tool(description = "List all pending escalations requiring review")]
    pub async fn curator_escalations(&self, Parameters(_req): Parameters<PingRequest>) -> String {
        execute_tool(self, "curator_escalations", async {
            let Some(ref queue) = self.escalation_queue else {
                return Err(McpToolError::permission_denied(
                    "EscalationQueue not available",
                ));
            };
            match governance::list_escalations_direct(queue.as_ref()) {
                Ok(entries) => {
                    let serialized: Vec<serde_json::Value> = entries
                        .iter()
                        .map(|e| {
                            json!({
                                "id": e.id,
                                "template_id": e.template_id,
                                "bot_id": e.bot_id,
                                "output": e.output,
                                "confidence": e.confidence,
                                "retry_count": e.retry_count,
                                "error_context": e.error_context,
                                "created_at": e.created_at,
                                "status": e.status,
                                "resolved_at": e.resolved_at,
                                "resolved_by": e.resolved_by,
                            })
                        })
                        .collect();
                    Ok(json!({"count": serialized.len(), "escalations": serialized}))
                }
                Err(e) => Err(McpToolError::internal(format!("{e}"))),
            }
        })
        .await
    }

    #[tool(description = "Resolve an escalation by ID")]
    pub async fn curator_escalation_resolve(
        &self,
        Parameters(req): Parameters<EscalationResolveRequest>,
    ) -> String {
        execute_tool(self, "curator_escalation_resolve", async {
            let Some(ref queue) = self.escalation_queue else {
                return Err(McpToolError::permission_denied(
                    "EscalationQueue not available",
                ));
            };
            let Some(ref events_store) = self.regulation_store else {
                return Err(McpToolError::permission_denied(
                    "RegulationArchive not available",
                ));
            };
            let events: Arc<dyn RegulationSink> =
                Arc::clone(events_store) as Arc<dyn RegulationSink>;
            match governance::resolve_direct(queue.as_ref(), &events, &req.id, &self.userpod) {
                Ok(()) => Ok(json!({"resolved": true, "id": req.id})),
                Err(e) => Err(McpToolError::internal(format!("{e}"))),
            }
        })
        .await
    }

    #[tool(description = "Dismiss an escalation as not actionable")]
    pub async fn curator_escalation_dismiss(
        &self,
        Parameters(req): Parameters<EscalationDismissRequest>,
    ) -> String {
        execute_tool(self, "curator_escalation_dismiss", async {
            let Some(ref queue) = self.escalation_queue else {
                return Err(McpToolError::permission_denied(
                    "EscalationQueue not available",
                ));
            };
            let Some(ref events_store) = self.regulation_store else {
                return Err(McpToolError::permission_denied(
                    "RegulationArchive not available",
                ));
            };
            let events: Arc<dyn RegulationSink> =
                Arc::clone(events_store) as Arc<dyn RegulationSink>;
            match governance::dismiss_direct(queue.as_ref(), &events, &req.id, &self.userpod) {
                Ok(()) => Ok(json!({"dismissed": true, "id": req.id})),
                Err(e) => Err(McpToolError::internal(format!("{e}"))),
            }
        })
        .await
    }

    // ── System Health ──────────────────────────────────────────────────

    #[tool(description = "Run metacognition cycle — requires live daemon for Regulation data")]
    pub async fn curator_health(&self, Parameters(_req): Parameters<PingRequest>) -> String {
        execute_tool(self, "curator_health", async {
            let Some(ref daemon) = self.daemon else {
                return Err(McpToolError::unavailable("Daemon not available"));
            };
            match daemon.curator_health_query(&self.userpod).await {
                Ok(DaemonResponse::CuratorHealthResponse { health }) => Ok(health),
                Ok(other) => Err(McpToolError::internal(format!(
                    "Bad daemon response: {:?}",
                    other
                ))),
                Err(e) => Err(McpToolError::internal(format!("Daemon query failed: {e}"))),
            }
        })
        .await
    }

    #[tool(description = "Live Regulation status — variety per domain")]
    pub async fn curator_reg_status(
        &self,
        Parameters(req): Parameters<RegStatusRequest>,
    ) -> String {
        execute_tool(self, "curator_reg_status", async {
            let Some(ref daemon) = self.daemon else {
                return Err(McpToolError::unavailable("Daemon not available"));
            };
            match daemon
                .reg_status_query(&self.userpod, req.domain.as_deref())
                .await
            {
                Ok(DaemonResponse::RegStatusResponse { status }) => Ok(status),
                Ok(other) => Err(McpToolError::internal(format!(
                    "Bad daemon response: {:?}",
                    other
                ))),
                Err(e) => Err(McpToolError::internal(format!("Daemon query failed: {e}"))),
            }
        })
        .await
    }

    // ── Memory & Learning ──────────────────────────────────────────────

    #[tool(description = "Query the Curator's semantic memory by entity name")]
    pub async fn curator_semantic_search(
        &self,
        Parameters(req): Parameters<SemanticSearchRequest>,
    ) -> String {
        execute_tool(self, "curator_semantic_search", async {
            let Some(ref semantic) = self.semantic else {
                return Err(McpToolError::permission_denied("SemanticMemory not available"));
            };
            match semantic.query_deduped(&req.query) {
                Ok(h_mems) => {
                    let limit = req.limit.unwrap_or(10);
                    let serialized: Vec<serde_json::Value> = h_mems
                        .iter()
                        .take(limit)
                        .map(|t| {
                            json!({
                                "entity": t.entity, "attribute": t.attribute,
                                "value": t.value, "confidence": t.confidence,
                            })
                        })
                        .collect();
                    Ok(json!({"count": serialized.len(), "total": h_mems.len(), "results": serialized}))
                }
                Err(e) => Err(McpToolError::internal(format!("Semantic recall failed: {e}"))),
            }
        })
        .await
    }

    #[tool(description = "Recall the Curator's episodic and semantic memory about an entity")]
    pub async fn curator_memory_recall(
        &self,
        Parameters(req): Parameters<MemoryRecallRequest>,
    ) -> String {
        execute_tool(self, "curator_memory_recall", async {
            let memory_type = req.memory_type.as_deref().unwrap_or("both");
            let mut result = json!({});

            if memory_type == "episodic" || memory_type == "both" {
                if let Some(ref ep) = self.episodic {
                    match ep.query_for_deduped(&req.entity, self.webid) {
                        Ok(h_mems) => {
                            let s: Vec<serde_json::Value> = h_mems
                                .iter()
                                .map(|t| {
                                    json!({
                                        "entity": t.entity, "attribute": t.attribute,
                                        "value": t.value, "confidence": t.confidence,
                                        "valid_from": t.observed_at.to_rfc3339(),
                                    })
                                })
                                .collect();
                            result["episodic"] = json!({"count": s.len(), "h_mems": s});
                        }
                        Err(e) => {
                            result["episodic"] = json!({"error": format!("{e}")});
                        }
                    }
                } else {
                    result["episodic"] = json!({"status": "unavailable"});
                }
            }
            if memory_type == "semantic" || memory_type == "both" {
                if let Some(ref sem) = self.semantic {
                    match sem.query_deduped(&req.entity) {
                        Ok(h_mems) => {
                            let s: Vec<serde_json::Value> = h_mems
                                .iter()
                                .map(|t| {
                                    json!({
                                        "entity": t.entity, "attribute": t.attribute,
                                        "value": t.value, "confidence": t.confidence,
                                    })
                                })
                                .collect();
                            result["semantic"] = json!({"count": s.len(), "h_mems": s});
                        }
                        Err(e) => {
                            result["semantic"] = json!({"error": format!("{e}")});
                        }
                    }
                } else {
                    result["semantic"] = json!({"status": "unavailable"});
                }
            }
            Ok(result)
        })
        .await
    }

    // ── Algedonic History ──────────────────────────────────────────────

    #[tool(description = "Read algedonic event log for a time window")]
    pub async fn curator_algedonic_log(
        &self,
        Parameters(req): Parameters<AlgedonicLogRequest>,
    ) -> String {
        execute_tool(self, "curator_algedonic_log", async {
            let Some(ref store) = self.regulation_store else {
                return Err(McpToolError::permission_denied(
                    "RegulationArchive not available",
                ));
            };
            let hours = req.hours.unwrap_or(24);
            let since = chrono::Utc::now() - chrono::Duration::hours(hours as i64);
            match store.query_algedonic(since, 500) {
                Ok(events) => {
                    let s: Vec<serde_json::Value> = events
                        .iter()
                        .map(|e| {
                            json!({
                                "timestamp": e.timestamp.to_rfc3339(),
                                "span": e.span.path,
                                "phase": format!("{:?}", e.phase),
                                "observation": e.observation,
                            })
                        })
                        .collect();
                    Ok(json!({"window_hours": hours, "count": s.len(), "events": s}))
                }
                Err(e) => Err(McpToolError::internal(format!(
                    "Algedonic query failed: {e}"
                ))),
            }
        })
        .await
    }

    // ── Regulation Query (for platform governance transparency) ────────────────

    #[tool(
        description = "Query Regulation regulation records by namespace prefix within a time window. Returns structured event data for governance transparency reporting and consent auditing."
    )]
    pub async fn reg_query(&self, Parameters(req): Parameters<RegQueryRequest>) -> String {
        execute_tool(self, "reg_query", async {
            let Some(ref store) = self.regulation_store else {
                return Err(McpToolError::permission_denied(
                    "RegulationArchive not available",
                ));
            };
            let window_secs = req.window_seconds.unwrap_or(3600);
            let limit = req.limit.unwrap_or(100) as u64;
            let since = chrono::Utc::now() - chrono::Duration::seconds(window_secs as i64);
            let config = hkask_storage::DecayConfig::default();

            let weighted = store
                .replay_weighted(since, limit, &config)
                .map_err(|e| McpToolError::internal(format!("Regulation query failed: {e}")))?;
            let total_count = weighted.len();
            let filtered: Vec<serde_json::Value> = weighted
                .into_iter()
                .filter(|we| {
                    if let Some(ref ns) = req.namespace {
                        we.event.span.namespace.as_str().starts_with(ns)
                    } else {
                        true
                    }
                })
                .take(req.limit.unwrap_or(100))
                .map(|we| {
                    json!({
                        "timestamp": we.event.timestamp.to_rfc3339(),
                        "namespace": we.event.span.namespace.as_str(),
                        "path": we.event.span.path,
                        "phase": format!("{:?}", we.event.phase),
                        "weight": we.weight,
                        "observation": we.event.observation,
                    })
                })
                .collect();

            let namespace_info = req.namespace.as_deref().unwrap_or("all");
            RegulationSpan::Tool {
                subsystem: hkask_types::regulation::ToolSubsystem::Curator,
            }
            .emit("reg_query");

            Ok(json!({
                "namespace": namespace_info,
                "window_seconds": window_secs,
                "total_events": total_count,
                "filtered_count": filtered.len(),
                "events": filtered
            }))
        })
        .await
    }

    // ── Token Registry (for consent auditing) ───────────────────────────

    #[tool(
        description = "List all DelegationTokens within a time window. Supports filtering by issuer or recipient WebID. Returns structured token data for consent auditing and anomaly detection."
    )]
    pub async fn list_tokens(&self, Parameters(req): Parameters<TokenListRequest>) -> String {
        execute_tool(self, "list_tokens", async {
            let Some(ref registry) = self.token_registry else {
                return Err(McpToolError::permission_denied(
                    "TokenRegistry not available",
                ));
            };
            let window_secs = req.window_seconds.unwrap_or(86400);
            let since = chrono::Utc::now() - chrono::Duration::seconds(window_secs as i64);

            let tokens = if let Some(ref issuer) = req.issuer {
                let wid: WebID = issuer.parse().unwrap_or_default();
                registry.query_by_issuer(&wid, since)
            } else if let Some(ref recipient) = req.recipient {
                let wid: WebID = recipient.parse().unwrap_or_default();
                registry.query_by_recipient(&wid, since)
            } else {
                registry.query_all(since)
            }
            .map_err(|e| McpToolError::internal(format!("Token query failed: {e}")))?;

            let serialized: Vec<serde_json::Value> = tokens
                .iter()
                .map(|t| {
                    json!({
                        "id": t.id,
                        "resource": format!("{:?}", t.resource),
                        "resource_id": t.resource_id,
                        "action": format!("{:?}", t.action),
                        "delegated_from": t.delegated_from.to_string(),
                        "delegated_to": t.delegated_to.to_string(),
                        "expires_at": t.expires_at,
                        "attenuation_level": t.attenuation_level,
                    })
                })
                .collect();

            RegulationSpan::Tool {
                subsystem: hkask_types::regulation::ToolSubsystem::Curator,
            }
            .emit("list_tokens");

            Ok(json!({
                "window_seconds": window_secs,
                "count": serialized.len(),
                "tokens": serialized
            }))
        })
        .await
    }
}

// ── Server startup ─────────────────────────────────────────────────────

pub async fn run(
    userpod: String,
    daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        SERVER_NAME,
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::server::ServerContext| {
            let (escalation_queue, regulation_store, episodic, semantic, token_registry) =
                open_curator_stores(&ctx);
            Ok(CuratorServer::new(
                ctx.webid,
                userpod.clone(),
                daemon_client.clone(),
                escalation_queue,
                regulation_store,
                episodic,
                semantic,
                token_registry,
            ))
        },
        vec![
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_CURATOR_DB",
                "Path to the Curator's SQLCipher database",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_DB_PASSPHRASE",
                "SQLCipher encryption passphrase",
            ),
        ],
    )
    .await
}

#[allow(clippy::type_complexity)]
fn open_curator_stores(
    ctx: &hkask_mcp_server::server::ServerContext,
) -> (
    Option<Arc<hkask_storage::EscalationQueue>>,
    Option<Arc<hkask_storage::RegulationArchive>>,
    Option<hkask_memory::EpisodicMemory>,
    Option<Arc<hkask_memory::SemanticMemory>>,
    Option<Arc<dyn hkask_capability::TokenRegistry>>,
) {
    let curator_db_path = ctx
        .credentials
        .get("HKASK_CURATOR_DB")
        .cloned()
        .unwrap_or_else(|| {
            let p = hkask_types::agent_paths::userpod_pod_db("curator");
            let resolved = hkask_types::agent_paths::resolve_under_data_dir(&p);
            if let Some(parent) = resolved.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            resolved.to_string_lossy().to_string()
        });

    let db = match ctx.credentials.get("HKASK_DB_PASSPHRASE") {
        Some(pw) => match hkask_storage::open_or_repair(&curator_db_path, pw) {
            Ok(db) => Some(db),
            Err(e) => {
                tracing::warn!(target: "hkask.mcp.curator", error = %e, "Failed to open curator DB");
                None
            }
        },
        None => {
            tracing::warn!(target: "hkask.mcp.curator", "HKASK_DB_PASSPHRASE not set");
            None
        }
    };
    let Some(db) = db else {
        return (None, None, None, None, None);
    };

    let pool = match db.sqlite_pool() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.curator", error = %e, "Failed to get SQLite pool");
            return (None, None, None, None, None);
        }
    };
    let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
        Arc::new(SqliteDriver::new(pool));
    let h_mem_store = hkask_storage::HMemStore::from_driver(Arc::clone(&driver));
    let h_mem_store2 = hkask_storage::HMemStore::from_driver(Arc::clone(&driver));
    let embedding_store = hkask_storage::EmbeddingStore::from_driver(Arc::clone(&driver), 1024);
    let escalation_queue = match hkask_storage::EscalationQueue::from_driver(Arc::clone(&driver)) {
        Ok(q) => Some(Arc::new(q)),
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.curator", error = %e, "Failed to create EscalationQueue");
            None
        }
    };
    let regulation_store = Some(Arc::new(hkask_storage::RegulationArchive::from_driver(
        Arc::clone(&driver),
    )));
    // RegulationArchive schema initialized by from_driver().
    let episodic = hkask_memory::EpisodicMemory::new(h_mem_store);
    let semantic = Arc::new(hkask_memory::SemanticMemory::new(
        h_mem_store2,
        embedding_store,
    ));

    // Token registry — consent audit trail for DelegationToken lifecycle.
    // Schema is initialized automatically by from_driver().
    let token_registry: Option<Arc<dyn hkask_capability::TokenRegistry>> = {
        let store = hkask_storage::TokenRegistryStore::from_driver(Arc::clone(&driver));
        Some(Arc::new(store) as Arc<dyn hkask_capability::TokenRegistry>)
    };

    (
        escalation_queue,
        regulation_store,
        Some(episodic),
        Some(semantic),
        token_registry,
    )
}
