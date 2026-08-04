#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
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

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use hkask_condenser::engine::CondenserEngine;
use hkask_condenser::inference;
use hkask_condenser::inference::SUMMARY_SYSTEM_PROMPT;
use hkask_condenser::saliency;
use hkask_condenser::types::*;

use hkask_mcp_server::server::{CapabilityTier, McpToolError, execute_tool};
use hkask_memory::EpisodicMemory;
use hkask_memory::SemanticMemory;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{Database, EmbeddingStore, HMem};
use hkask_types::InferencePort;
use hkask_types::Visibility;
use hkask_types::template::LLMParameters;
use hkask_types::time::now_rfc3339;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use serde::Deserialize;
use std::sync::{Arc, Mutex};

hkask_mcp_server::mcp_server!(
    pub struct CondenserServer {
        pub engine: Mutex<CondenserEngine>,
        pub episodic: Option<Arc<EpisodicMemory>>,
        pub semantic: Option<Arc<SemanticMemory>>,
        pub inference_port: Arc<dyn InferencePort>,
        pub default_model: String,
        pub persona_keywords: Vec<String>,
        pub capability_tier: CapabilityTier,
    }
);

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
        self.episodic.is_some()
    }

    /// Record a tool call's outcome to episodic memory.
    ///
    /// Persists the experience as a first-person `HMem` in the episodic store
    /// when persistence is configured (`HKASK_DB_PATH` + `HKASK_DB_PASSPHRASE`).
    /// Falls back to a debug log when the episodic store is absent so the
    /// server still operates in memory-only mode.
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

        if let Some(ref episodic) = self.episodic {
            let h_mem = HMem::new(&entity, "experience", value, self.webid)
                .with_perspective(self.webid)
                .with_visibility(Visibility::Private)
                .with_confidence(1.0);

            if let Err(e) = episodic.store(h_mem) {
                tracing::warn!(
                    target: "hkask.mcp.condenser.memory",
                    tool = %tool,
                    error = %e,
                    "Failed to persist experience to episodic memory",
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
                "Experience logged (no episodic store — memory-only mode)",
            );
        }
    }
}

#[tool_router(server_handler)]
impl CondenserServer {
    #[tool(description = "Liveness and profile info")]
    pub async fn condenser_ping(&self) -> String {
        execute_tool(self, "condenser_ping", async {
            let engine = self
                .engine
                .lock()
                .map_err(|_| McpToolError::internal("engine lock poisoned"))?;
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
                    "semantic_memory": self.semantic.is_some(),
                    "inference": true,
                    "keystore": self.capability_tier.keystore_available,
                    "reg": self.capability_tier.reg_available(),
                },
                "profile": engine.profile().to_string(),
                "default_model": self.default_model,
            }))
        })
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
        execute_tool(self, "condenser_persist", async {
            let Some(episodic) = &self.episodic else {
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
            .with_confidence(confidence.unwrap_or(1.0));

            match episodic.store(h_mem) {
                Ok(()) => Ok(serde_json::json!({
                    "persisted": true,
                    "entity": entity,
                    "attribute": "content",
                    "perspective": self.webid.to_string(),
                })),
                Err(e) => Err(McpToolError::internal(format!(
                    "Failed to persist to episodic memory: {}",
                    e
                ))),
            }
        })
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
            max_tokens,
            model,
        }): Parameters<ThreadSummaryRequest>,
    ) -> String {
        execute_tool(self, "condenser_thread_summary", async {
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
            let max_tok = max_tokens.unwrap_or_else(|| {
                // Fall back to HKASK_CONDENSE_SALIENCY_WINDOW env var as a
                // default hint. Higher saliency = user wants more context
                // preserved → longer summaries. Clamp to [150, 2000].
                let saliency = std::env::var("HKASK_CONDENSE_SALIENCY_WINDOW")
                    .ok()
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(5);
                (saliency * 100).clamp(150, 2000) as u32
            });

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
                max_tokens: max_tok,
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
                    return Err(McpToolError::internal(format!("Inference failed: {e}")));
                }
            };

            let summary = result.text;
            if summary.trim().is_empty() {
                return Err(McpToolError::internal("Inference engine returned an empty summary"));
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

            Ok(serde_json::to_value(&output).expect("ThreadSummaryOutput serialization is infallible"))
        }).await
    }

    #[tool(
        description = "Score text saliency against persona or memory. Returns 0.0-1.0 where higher = more relevant."
    )]
    pub async fn condenser_score_saliency(
        &self,
        Parameters(req): Parameters<SaliencyRequest>,
    ) -> String {
        execute_tool(self, "condenser_score_saliency", async {
            let (score, method) = match req.against.as_deref() {
                Some("memory") => {
                    // Query memory stores word-by-word, then score via domain crate.
                    let words = saliency::extract_query_words(&req.text);
                    let total_results = if let Some(ref semantic) = self.semantic {
                        words
                            .iter()
                            .filter_map(|w| semantic.query_deduped(w).ok())
                            .map(|m| m.len())
                            .sum::<usize>()
                    } else if let Some(ref episodic) = self.episodic {
                        words
                            .iter()
                            .filter_map(|w| episodic.query_for_deduped(w, self.webid).ok())
                            .map(|m| m.len())
                            .sum::<usize>()
                    } else {
                        // No memory store — neutral score, not an error.
                        return Ok(serde_json::json!({
                            "score": 0.5,
                            "against": "memory",
                            "method": "no_store",
                        }));
                    };
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
        })
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
                let (episodic, semantic) = {
                    let db_path = ctx
                        .credentials
                        .get("HKASK_DB_PATH")
                        .cloned()
                        .or_else(|| std::env::var("HKASK_DB_PATH").ok());
                    match db_path {
                        Some(path) => {
                            let passphrase = ctx
                                .credentials
                                .get("HKASK_DB_PASSPHRASE")
                                .cloned()
                                .or_else(|| std::env::var("HKASK_DB_PASSPHRASE").ok())
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "HKASK_DB_PATH set but HKASK_DB_PASSPHRASE missing"
                                    )
                                })?;
                            let db = Database::open(&path, &passphrase).map_err(|e| {
                                anyhow::anyhow!("Failed to open condenser database: {}", e)
                            })?;
                            let pool =
                                db.sqlite_pool().map_err(|e| anyhow::anyhow!("pool: {e}"))?;
                            let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
                                Arc::new(SqliteDriver::new(pool));

                            // Episodic memory: first-person experience store.
                            let h_mem_store =
                                hkask_storage::HMemStore::from_driver(Arc::clone(&driver))
                                    .map_err(|e| anyhow::anyhow!("hmem store init: {e}"))?;
                            let episodic = hkask_memory::EpisodicMemory::new(h_mem_store);

                            // Semantic memory: shared knowledge graph with embeddings.
                            // Requires a second HMemStore (separate entity namespace) plus
                            // an EmbeddingStore for KNN similarity search. Same driver,
                            // same database — different store handles. Follows the
                            // pattern established by the curator server.
                            let h_mem_store2 =
                                hkask_storage::HMemStore::from_driver(Arc::clone(&driver))
                                    .map_err(|e| {
                                        anyhow::anyhow!("hmem store init (semantic): {e}")
                                    })?;
                            let embedding_store = EmbeddingStore::from_driver(driver, 1024)
                                .map_err(|e| anyhow::anyhow!("embedding store init: {e}"))?;
                            let semantic =
                                hkask_memory::SemanticMemory::new(h_mem_store2, embedding_store);

                            (Some(Arc::new(episodic)), Some(Arc::new(semantic)))
                        }
                        None => (None, None),
                    }
                };

                let default_model = ctx
                    .credentials
                    .get("HKASK_DEFAULT_MODEL")
                    .cloned()
                    .or_else(|| std::env::var("HKASK_DEFAULT_MODEL").ok())
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
                    Mutex::new(CondenserEngine::new()),
                    episodic,
                    semantic,
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
        vec![],
    )
    .await
}
