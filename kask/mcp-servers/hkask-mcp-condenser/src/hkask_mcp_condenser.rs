#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
// `tokio` is in [dependencies] for the bin target's `#[tokio::main]`; the lib
// itself does not use it, so the unused_crate_dependencies lint fires on the
// lib target. This is the legitimate bin-needs-dep case.
#![allow(unused_crate_dependencies)]
//! hKask MCP Condenser — Context condensation for tool outputs
//!
//! Loop: Episodic (Loop 2) — Confirmed. Context condensation operates on the active
//! conversation window, which is episodic in nature. The condenser compresses and persists
//! tool outputs within the episodic memory boundary.
//!
//! Provides compression algorithms (rtk_style, word_rank, flashrank) for reducing
//! tool output size while preserving essential information. `word_rank` uses
//! TF-IDF bag-of-words compression with ontology anchoring.
//! CPU-only algorithms with no LLM dependency. Phase 2 adds LLM-assisted
//! thread summarization via the centralized hKask inference router.
//!
//! When `HKASK_DB_PATH` + `HKASK_DB_PASSPHRASE` environment variables are set,
//! the condenser can persist compressed outputs to episodic memory via the
//! `condenser:persist` tool. Without them, the server operates in memory-only
//! mode (the default — no persistence backend required).
//!
//! The `condenser_thread_summary` tool uses the centralized `InferencePort`
//! (hkask-inference router) for LLM-powered summarization. No standalone
//! HTTP client or inference URL configuration is needed — the router handles
//! provider dispatch (DeepInfra, OpenRouter) automatically.

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_condenser::engine::CondenserEngine;
use hkask_condenser::inference;
use hkask_condenser::inference::SUMMARY_SYSTEM_PROMPT;
use hkask_condenser::saliency;
use hkask_condenser::types::*;

use hkask_mcp_server::server::{
    CapabilityTier, McpToolError, execute_tool_semantic, resolve_db_passphrase,
};
use hkask_memory::{MemoryStore, MemoryStoreError};
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{Database, EmbeddingStore, HMem, HMemError};
use hkask_types::template::LLMParameters;
use hkask_types::time::now_rfc3339;
use hkask_types::{HMemOntology, Visibility};
use hkask_types::{InferenceError, InferencePort};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use serde::Deserialize;
use std::sync::{Arc, Mutex};

hkask_mcp_server::mcp_server!(
    pub struct CondenserServer {
        pub engine: Mutex<CondenserEngine>,
        pub store: Option<Arc<MemoryStore>>,
        pub inference_port: Arc<dyn InferencePort>,
        pub default_model: String,
        pub persona_keywords: Vec<String>,
        pub capability_tier: CapabilityTier,
    }
);

/// Classify an `InferenceError` from the inference router into the MCP
/// wire-level `McpToolError` kind: connection/circuit-breaker failures are
/// availability issues (`unavailable`), a bad model or unsupported vision
/// request is a user-input problem (`invalid_argument`), a missing-credential /
/// missing-provider configuration failure is an authorization problem
/// (`permission_denied`), and generation/JSON failures remain `internal`.
fn map_inference_error(e: InferenceError) -> McpToolError {
    let message = e.to_string();
    match e {
        InferenceError::Connection(_) | InferenceError::CircuitOpen(_) => {
            McpToolError::unavailable(message)
        }
        InferenceError::Model(_) | InferenceError::VisionUnsupported(_) => {
            McpToolError::invalid_argument(message)
        }
        InferenceError::NotConfigured(_) => McpToolError::permission_denied(message),
        InferenceError::Generation(_) | InferenceError::Json(_) => McpToolError::internal(message), // rr0044-ok: mapper-internal-arm
    }
}

/// Classify a `MemoryStoreError` from `MemoryStore::store` into the MCP
/// wire-level `McpToolError` kind: a missing h_mem or embedding is
/// `not_found`, a centroid with no embeddings is a failed precondition, and
/// infrastructure failures remain `internal`.
fn map_memory_error(e: MemoryStoreError) -> McpToolError {
    let message = e.to_string();
    match e {
        MemoryStoreError::HMem(HMemError::NotFound(_)) => McpToolError::not_found(message),
        MemoryStoreError::HMem(HMemError::Infra(_)) => McpToolError::internal(message), // rr0044-ok: mapper-internal-arm
        MemoryStoreError::Embedding(hkask_storage::EmbeddingError::NotFound(_)) => {
            McpToolError::not_found(message)
        }
        MemoryStoreError::Embedding(_) => McpToolError::internal(message), // rr0044-ok: mapper-internal-arm
        MemoryStoreError::NoEmbeddingsForCentroid(_) => McpToolError::failed_precondition(message),
    }
}

impl CondenserServer {
    /// Fallback persona keywords when no configuration is provided.
    /// These are generic condensation-oriented terms — operators should
    /// override via `HKASK_CONDENSER_PERSONA_KEYWORDS` for domain-specific agents.
    pub fn default_persona_keywords() -> Vec<String> {
        vec![
            "condense".into(),
            "compress".into(),
            "summarize".into(),
            "context".into(),
            "token".into(),
            "budget".into(),
            "saliency".into(),
            "relevance".into(),
            "retention".into(),
            "profile".into(),
            "ontology".into(),
            "category".into(),
            "persist".into(),
        ]
    }

    pub fn has_persistence(&self) -> bool {
        self.store.is_some()
    }

    /// Record a tool call's outcome to memory.
    ///
    /// Persists the experience as a first-person, PKO-anchored `HMem` when
    /// persistence is configured (`HKASK_DB_PATH` + `HKASK_DB_PASSPHRASE`).
    /// Falls back to a debug log when the store is absent so the server still
    /// operates in memory-only mode.
    pub fn record_experience(
        &self,
        tool: &str,
        input_summary: &str,
        outcome: &str,
        detail: serde_json::Value,
    ) {
        let entity = format!("condenser:{tool}");
        let value = serde_json::json!({
            "input": input_summary,
            "outcome": outcome,
            "detail": detail,
            "timestamp": now_rfc3339(),
        });

        if let Some(ref store) = self.store {
            // Process-axis anchoring (P5.4): an experience record is a step
            // execution of the tool that produced it — the tool name is the
            // PKO step, `condense` the procedure.
            let h_mem = HMem::new(&entity, "experience", value, self.webid)
                .with_perspective(self.webid)
                .with_visibility(Visibility::Private)
                .with_confidence(1.0)
                .with_ontology(HMemOntology::episodic("condense", tool, "condenser"));

            if let Err(e) = store.store(h_mem) {
                tracing::warn!(
                    target: "hkask.mcp.condenser.memory",
                    tool = %tool,
                    error = %e,
                    "Failed to persist experience to memory",
                );
            }
        } else {
            tracing::debug!(
                target: "hkask.mcp.condenser.memory",
                tool = %tool,
                input = %input_summary,
                outcome = %outcome,
                detail = ?detail,
                timestamp = %now_rfc3339(),
                "Experience logged (no memory store — memory-only mode)",
            );
        }
    }

    /// Map a tool name to its PKO / Dublin Core ontology concept URI. The
    /// concept tags the `reg.tool` span (via `execute_tool_semantic`) for
    /// type-aware feedback routing. Condensation is a knowledge-production
    /// process, so the condenser tools anchor on PKO (procedure axis); the
    /// liveness ping anchors on Dublin Core (state axis).
    fn ontology_anchor(tool: &str) -> Option<&'static str> {
        use hkask_bridge_ontology::{dc_bibo, pko};
        match tool {
            // Liveness / status — state axis
            "condenser_ping" => Some(dc_bibo::DATASET),
            // Condensation tools — process axis
            "condenser_persist" => Some(pko::PROCEDURE_EXECUTION),
            "condenser_thread_summary" | "condenser_score_saliency" => Some(pko::PROCEDURE),
            _ => Some(pko::PROCEDURE),
        }
    }
}

#[cfg(test)]
mod tool_surface_tests {
    use super::*;

    // Pins the registered tool-surface count end-to-end. Catches silent
    // registration drops — a `#[tool]` impl block without `#[tool_router]`
    // silently registers nothing (`cargo check` passes on an unwired orphan).
    // Mirrors the swarm pin.
    #[test]
    fn tool_surface_is_exactly_4_registered_tools() {
        let n = CondenserServer::tool_router().list_all().len();
        assert_eq!(n, 4, "condenser registered tool surface changed; got {n}");
    }

    // Coverage: every registered tool must have a non-None ontology anchor.
    // Catches the silent-drop failure mode where a new tool is added to the
    // router without a corresponding arm in ontology_anchor. The count pin
    // above catches addition; this test catches anchoring.
    #[test]
    fn ontology_anchor_covers_all_registered_tools() {
        let router = CondenserServer::tool_router();
        for tool in router.list_all() {
            assert!(
                CondenserServer::ontology_anchor(&tool.name).is_some(),
                "ontology_anchor returned None for registered tool '{}'; \
                 add an explicit arm or adjust the fallback",
                tool.name
            );
        }
    }

    // Regression: the ontology anchor must not collapse to a single constant.
    // The liveness ping anchors on Dublin Core (state axis); the condensation
    // tools anchor on PKO (process axis). A future stub regression would make
    // these equal.
    #[test]
    fn ontology_anchor_distinguishes_tool_families() {
        use hkask_bridge_ontology::{dc_bibo, pko};
        let ping = CondenserServer::ontology_anchor("condenser_ping");
        let persist = CondenserServer::ontology_anchor("condenser_persist");
        let summary = CondenserServer::ontology_anchor("condenser_thread_summary");
        // State axis vs process axis — distinct ontologies.
        assert_ne!(
            ping, persist,
            "condenser_ping (Dublin Core) and condenser_persist (PKO) must anchor on distinct ontologies"
        );
        // Specific concept pins.
        assert_eq!(
            ping,
            Some(dc_bibo::DATASET),
            "condenser_ping must anchor on Dublin Core Dataset"
        );
        assert_eq!(
            persist,
            Some(pko::PROCEDURE_EXECUTION),
            "condenser_persist must anchor on PKO ProcedureExecution"
        );
        assert_eq!(
            summary,
            Some(pko::PROCEDURE),
            "condenser_thread_summary must anchor on PKO Procedure"
        );
    }
}

#[tool_router(server_handler)]
impl CondenserServer {
    #[tool(description = "Liveness and profile info")]
    pub async fn condenser_ping(&self) -> String {
        execute_tool_semantic(
            self,
            "condenser_ping",
            Self::ontology_anchor("condenser_ping"),
            async {
                let engine = self
                    .engine
                    .lock()
                    .map_err(|_| McpToolError::internal("engine lock poisoned"))?; // rr0044-ok: lock-poisoned
                let mode = if self.capability_tier.embedded {
                    "embedded"
                } else {
                    "standalone"
                };
                Ok(serde_json::json!({
                    "status": "ok",
                    "version": SERVER_VERSION,
                    "mode": mode,
                    "capabilities": {
                        "persistence": self.has_persistence(),
                        "semantic_memory": self.store.is_some(),
                        "inference": true,
                        "keystore": self.capability_tier.keystore_available,
                        "reg": self.capability_tier.reg_available(),
                    },
                    "profile": engine.profile().to_string(),
                    "default_model": self.default_model,
                }))
            },
        )
        .await
    }

    #[tool(description = "Persist a compressed output to episodic memory")]
    pub async fn condenser_persist(
        &self,
        Parameters(PersistRequest {
            tool_name,
            compressed_output,
            confidence,
        }): Parameters<PersistRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "condenser_persist",
            Self::ontology_anchor("condenser_persist"),
            async {
                let Some(store) = &self.store else {
                    return Err(McpToolError::permission_denied(
                        "Persistence not available — set HKASK_DB_PATH and HKASK_DB_PASSPHRASE",
                    ));
                };

                if compressed_output.is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "compressed_output must not be empty",
                    ));
                }

                let entity = format!("condenser:{tool_name}");
                let h_mem = HMem::new(
                    &entity,
                    "content",
                    serde_json::Value::String(compressed_output),
                    self.webid,
                )
                .with_perspective(self.webid)
                .with_visibility(Visibility::Private)
                .with_confidence(confidence.unwrap_or(1.0))
                .with_ontology(HMemOntology::episodic(
                    "condense",
                    "persist",
                    format!("tool:{tool_name}"),
                ));

                match store.store(h_mem) {
                    Ok(()) => Ok(serde_json::json!({
                        "persisted": true,
                        "entity": entity,
                        "attribute": "content",
                        "perspective": self.webid.to_string(),
                    })),
                    Err(e) => Err(map_memory_error(e)),
                }
            },
        )
        .await
    }

    #[tool(
        description = "Summarize conversation history using the centralized hKask inference router for context condensation. Call when approaching context window limits to condense older messages."
    )]
    pub async fn condenser_thread_summary(
        &self,
        Parameters(ThreadSummaryRequest {
            messages,
            current_query,
            model,
        }): Parameters<ThreadSummaryRequest>,
    ) -> String {
        execute_tool_semantic(self, "condenser_thread_summary", Self::ontology_anchor("condenser_thread_summary"), async {
            let effective_model = model.as_deref().unwrap_or(&self.default_model);

            let msg_count = messages.len();
            if msg_count == 0 {
                return Err(McpToolError::invalid_argument("messages array is empty"));
            }

            // AnyJsonValue (transparent Value wrapper) → serde_json::Value for
            // the condenser lib API, which is typed against serde_json::Value.
            let messages: Vec<serde_json::Value> =
                messages.into_iter().map(serde_json::Value::from).collect();
            let conversation_text = inference::format_conversation_text(&messages);

            let summarization_prompt =
                inference::build_summarization_prompt(&conversation_text, &current_query);

            // Compose the full prompt: system + user
            let full_prompt = format!(
                "{}\n\nUser: {}",
                SUMMARY_SYSTEM_PROMPT, summarization_prompt
            );

            let params = LLMParameters {
                temperature: 0.3,
                top_p: 0.9,
                top_k: 40,
                min_p: 0.0,
                typical_p: 0.0,
                frequency_penalty: 0.0,
                presence_penalty: 0.0,
                seed: None,
                disable_thinking: true,
                adapter: None,
                system_prompt: None,
            };

            let result = match self
                .inference_port
                .generate_with_model(&full_prompt, &params, Some(effective_model), None)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    return Err(map_inference_error(e));
                }
            };

            let summary = result.text;
            if summary.trim().is_empty() {
                return Err(McpToolError::internal("Inference engine returned an empty summary")); // rr0044-ok: inference-empty-summary
            }
            let summary_len = summary.len();

            let output = inference::build_summary_output(
                summary,
                &conversation_text,
                msg_count,
                effective_model.to_string(),
            );

            self.record_experience(
                "condenser_thread_summary",
                &format!("{} messages", msg_count),
                "success",
                serde_json::json!({"model": effective_model.to_string(), "summary_length": summary_len}),
            );

            serde_json::to_value(&output).map_err(|e| {
                McpToolError::internal(format!("ThreadSummaryOutput serialization failed: {e}")) // rr0044-ok: serialize-own-struct
            })
        }).await
    }

    #[tool(
        description = "Score text saliency against persona or memory. Returns 0.0-1.0 where higher = more relevant."
    )]
    pub async fn condenser_score_saliency(
        &self,
        Parameters(req): Parameters<SaliencyRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "condenser_score_saliency",
            Self::ontology_anchor("condenser_score_saliency"),
            async {
                let (score, method) = match req.against.as_deref() {
                    Some("memory") => {
                        // Query memory stores word-by-word, then score via domain crate.
                        let words = saliency::extract_query_words(&req.text);
                        let Some(ref store) = self.store else {
                            // No memory store — neutral score, not an error.
                            return Ok(serde_json::json!({
                                "score": 0.5,
                                "against": "memory",
                                "method": "no_store",
                            }));
                        };
                        // Propagate infra errors — a SQLCipher outage must NOT
                        // score as "nothing is salient" (the `unwrap_or(0)` / `.ok()`
                        // trap: a DB outage returns 0, the loop reads it as "no
                        // deviation"). Sum successful query counts; surface the
                        // first infra failure as a tool error.
                        let mut total_results = 0usize;
                        for word in &words {
                            match store.query_deduped(word) {
                                Ok(h_mems) => total_results += h_mems.len(),
                                Err(e) => return Err(map_memory_error(e)),
                            }
                        }
                        (
                            saliency::score_memory_results(total_results),
                            "semantic_search",
                        )
                    }
                    _ => {
                        // Score against persona keywords — per-request override if provided,
                        // otherwise use the server's configured keyword set.
                        let keywords: Vec<&str> = if let Some(ref custom) = req.persona_keywords {
                            custom.iter().map(|s| s.as_str()).collect()
                        } else {
                            self.persona_keywords.iter().map(|s| s.as_str()).collect()
                        };
                        (
                            saliency::score_against_persona(&req.text, &keywords),
                            "word_frequency",
                        )
                    }
                };
                Ok(serde_json::json!({
                    "score": score,
                    "against": req.against.as_deref().unwrap_or("persona"),
                    "method": method,
                }))
            },
        )
        .await
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SaliencyRequest {
    pub text: String,
    #[serde(default)]
    pub against: Option<String>, // "persona" or "memory"
    /// Optional per-request override for persona keywords. If omitted,
    /// uses the server's configured keyword set.
    #[serde(default)]
    pub persona_keywords: Option<Vec<String>>,
}

/// Run the condenser MCP server (used by binary target).
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    let inference_port: Arc<dyn InferencePort> = hkask_inference::resolve_inference_port().await;

    hkask_mcp_server::run_server(
        "hkask-mcp-condenser",
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::ServerContext| {
            (|| -> anyhow::Result<CondenserServer> {
                let store = {
                    // D28 — Standardized Artifact Storage. Default DB path
                    // is `mcp/condenser/condenser.db`. Override via
                    // `HKASK_DB_PATH`. Requires `HKASK_DB_PASSPHRASE`.
                    let db_path = std::env::var("HKASK_DB_PATH")
                        .ok()
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| {
                            hkask_types::agent_paths::resolve_under_data_dir(
                                &hkask_types::agent_paths::mcp_server_db("condenser", "condenser"),
                            )
                        });
                    let db_path_str = db_path.to_string_lossy().to_string();
                    // Resolve passphrase via the canonical 2-tier chain
                    // (ctx.credentials → resolve_credential which does env → keychain).
                    // The helper emits a `warn!` on miss.
                    match resolve_db_passphrase(&ctx.credentials) {
                        Ok(passphrase) => {
                            if let Some(parent) = db_path.parent() {
                                std::fs::create_dir_all(parent).ok();
                            }
                            let db = Database::open(&db_path_str, &passphrase).map_err(|e| {
                                anyhow::anyhow!("Failed to open condenser database: {}", e)
                            })?;
                            let pool =
                                db.sqlite_pool().map_err(|e| anyhow::anyhow!("pool: {e}"))?;
                            let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
                                Arc::new(SqliteDriver::new(pool));

                            // One store for both episodic experience records
                            // and semantic knowledge — the `HMemOntology` blob
                            // on each h_mem distinguishes them (P5.4).
                            let h_mem_store =
                                hkask_storage::HMemStore::from_driver(Arc::clone(&driver))
                                    .map_err(|e| anyhow::anyhow!("hmem store init: {e}"))?;
                            let embedding_store = EmbeddingStore::from_driver(driver, 1024)
                                .map_err(|e| anyhow::anyhow!("embedding store init: {e}"))?;

                            Some(Arc::new(hkask_memory::MemoryStore::new(
                                h_mem_store,
                                embedding_store,
                            )))
                        }
                        Err(error) => {
                            tracing::warn!(
                                target: "hkask.mcp.condenser",
                                %error,
                                "Falling back to in-memory mode. Condenser data will not persist across restarts."
                            );
                            None
                        }
                    }
                };

                let default_model = std::env::var("HKASK_DEFAULT_MODEL")
                    .ok()
                    .unwrap_or_else(|| {
                        hkask_inference::model_constants::DEFAULT_FALLBACK_MODEL.to_string()
                    });

                // Persona keywords: configurable via env var (comma-separated).
                // Falls back to generic condensation terms if not set.
                let persona_keywords = std::env::var("HKASK_CONDENSER_PERSONA_KEYWORDS")
                    .ok()
                    .map(|raw| {
                        raw.split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    })
                    .filter(|v: &Vec<String>| !v.is_empty())
                    .unwrap_or_else(CondenserServer::default_persona_keywords);

                Ok(CondenserServer::new(
                    ctx.webid,
                    Arc::new(hkask_verification::VerificationStore::open()),
                    Mutex::new(CondenserEngine::new()),
                    store,
                    Arc::clone(&inference_port),
                    default_model,
                    persona_keywords,
                    ctx.capability_tier,
                ))
            })()
            .map_err(|e| hkask_mcp_server::McpError::UnexpectedResponse {
                context: "condenser server init".into(),
                detail: e.to_string(),
            })
        },
        vec![hkask_mcp_server::CredentialRequirement::optional(
            "HKASK_DB_PASSPHRASE",
            "SQLCipher encryption passphrase for the condenser database (required when HKASK_DB_PATH is set)",
        )],
    )
    .await
}

// D28 — pins the default DB path.
#[test]
fn default_db_path_follows_standardized_layout() {
    let relative = hkask_types::agent_paths::mcp_server_db("condenser", "condenser");
    assert_eq!(
        relative,
        std::path::PathBuf::from("mcp")
            .join("condenser")
            .join("condenser.db"),
        "condenser default DB path must follow mcp/condenser/condenser.db"
    );
}
