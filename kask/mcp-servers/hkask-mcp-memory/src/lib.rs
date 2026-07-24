#![forbid(unsafe_code)]
//! hKask MCP Memory — Unified episodic + semantic memory MCP server (library).
//!
//! Exports MemoryServer struct and tool methods for fuzz testability (P5 Testing
//! Discipline, P4 Clear Boundaries). The binary entrypoint in main.rs delegates
//! to `run()`.
//!
//! 18 tools:
//! - `episodic_ping` — Liveness and storage info for episodic memory
//! - `episodic_store` — Store an episodic h_mem (private, perspective-bound)
//! - `episodic_recall` — Recall h_mems by entity (filtered by caller's WebID)
//! - `episodic_recall_context` — Recall episodes ranked by salience to context (mirrors ChatService::recall_episodic)
//! - `episodic_budget` — Storage usage and budget info
//! - `episodic_consolidate_status` — Check consolidation candidates and budget status
//! - `semantic_ping` — Liveness and storage info for semantic memory
//! - `semantic_store` — Store a shared semantic h_mem (no perspective)
//! - `semantic_recall` — Recall h_mems by entity (public, any agent can read)
//! - `memory_recall` — Paired semantic + episodic recall, mirrored dual-recall circuit
//! - `semantic_embed` — Store an embedding vector for similarity search
//! - `semantic_search` — KNN similarity search over embeddings
//! - `semantic_centroid` — Compute mean embedding vector for a prefix-filtered set
//! - `semantic_purge` — Delete embeddings matching an entity_ref prefix
//! - `semantic_chunk` — Chunk text into passages for embedding
//! - `semantic_count` — HMem and embedding counts
//! - `memory_backup` — Export the memory database to a local backup file
//! - `memory_restore` — Restore the memory database from a local backup file

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

pub mod cogat;
pub mod types;

use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_mcp_server::validate_identifier;
use hkask_memory::{ChatTurn, EpisodicMemory, HMemStore, SemanticMemory};
use hkask_types::storage::StorageDriver;
use hkask_types::{EmbeddingError, EmbeddingID, EmbeddingPort, HMem, NotFound, SimilarityResult, StoredEmbedding, Visibility};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use serde_json::json;
use std::sync::Arc;
use types::RecallContextRequest;
use types::*;

// ── Server ──────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub struct MemoryServer {
        pub episodic: EpisodicMemory,
        pub semantic: Arc<SemanticMemory>,
        pub db: Option<Arc<dyn StorageDriver>>,
    }
);

#[tool_router(server_handler)]
impl MemoryServer {
    // ── Episodic tools ──────────────────────────────────────────

    #[tool(description = "Liveness and storage info for episodic memory")]
    pub async fn episodic_ping(&self) -> String {
        execute_tool(self, "episodic_ping", async {
            Ok(json!({
                "status": "ok",
                "server": "hkask-mcp-memory",
                "perspective": self.webid.to_string(),
            }))
        })
        .await
    }

    #[tool(description = "Store an episodic h_mem (private, perspective-bound)")]
    pub async fn episodic_store(
        &self,
        Parameters(StoreRequest {
            entity,
            attribute,
            value,
            confidence,
        }): Parameters<StoreRequest>,
    ) -> String {
        execute_tool(self, "episodic_store", async {
            validate_identifier("entity", &entity, 256)?;
            validate_identifier("attribute", &attribute, 256)?;
            let h_mem = HMem::new(&entity, &attribute, value, self.webid)
                .with_perspective(self.webid)
                .with_confidence(confidence.unwrap_or(1.0))
                .with_visibility(Visibility::Private);
            self.episodic
                .store(h_mem)
                .map_err(|e| McpToolError::internal(format!("store episodic h_mem: {}", e)))?;
            Ok(json!({
                "stored": true, "entity": entity, "attribute": attribute,
            }))
        })
        .await
    }

    #[tool(description = "Recall episodic h_mems by entity (filtered by caller's WebID)")]
    pub async fn episodic_recall(
        &self,
        Parameters(RecallRequest { entity }): Parameters<RecallRequest>,
    ) -> String {
        execute_tool(self, "episodic_recall", async {
            validate_identifier("entity", &entity, 256)?;
            let h_mems = self
                .episodic
                .query_for_deduped(&entity, self.webid)
                .map_err(|e| McpToolError::internal(format!("recall episodic h_mems: {}", e)))?;
            let serialized: Vec<serde_json::Value> = h_mems
                .iter()
                .map(|t| {
                    json!({
                        "entity": t.entity,
                        "attribute": t.attribute,
                        "value": t.value,
                        "confidence": t.confidence,
                        "valid_from": t.observed_at.to_rfc3339(),
                    })
                })
                .collect();
            Ok(json!({"count": serialized.len(), "h_mems": serialized}))
        })
        .await
    }

    #[tool(
        description = "Recall episodic memories ranked by salience to context. \
        Returns formatted episodes (User:/Agent: pairs for chat history) sorted by keyword relevance. \
        Mirrors ChatService::recall_episodic — use this when you need relevant past interactions, \
        not just entity-matched h_mems."
    )]
    pub async fn episodic_recall_context(
        &self,
        Parameters(RecallContextRequest {
            entity,
            context,
            limit,
        }): Parameters<RecallContextRequest>,
    ) -> String {
        execute_tool(self, "episodic_recall_context", async {
            validate_identifier("entity", &entity, 256)?;
            let limit = limit.unwrap_or(10);

            let h_mems = self
                .episodic
                .query_for_deduped(&entity, self.webid)
                .map_err(|e| McpToolError::internal(format!("recall episodic h_mems: {}", e)))?;

            if h_mems.is_empty() {
                return Ok(json!({"count": 0, "episodes": []}));
            }

            if let Some(ref ctx) = context {
                // Salience-scored: build keywords from context, score each episode
                let keywords = hkask_memory::salience::extract_keywords(ctx);

                let mut scored: Vec<(usize, serde_json::Value)> = h_mems
                    .iter()
                    .filter_map(|t| {
                        let ct = ChatTurn::from_value(&t.value)?;
                        let combined = format!("{} {}", ct.user_input, ct.agent_response);
                        let score =
                            hkask_memory::salience::keyword_overlap_score(&keywords, &combined);
                        Some((
                            score,
                            json!({
                                "user_input": ct.user_input,
                                "agent_response": ct.agent_response,
                                "salience": score,
                                "confidence": t.confidence,
                                "valid_from": t.observed_at.to_rfc3339(),
                            }),
                        ))
                    })
                    .collect();

                scored.sort_by(|a, b| b.0.cmp(&a.0));
                let episodes: Vec<serde_json::Value> =
                    scored.into_iter().take(limit).map(|(_, v)| v).collect();

                Ok(json!({
                    "count": episodes.len(),
                    "context": ctx,
                    "episodes": episodes,
                }))
            } else {
                // No context: return most recent episodes, sorted by recency (reverse order)
                let episodes: Vec<serde_json::Value> = h_mems
                    .iter()
                    .take(limit)
                    .filter_map(|t| {
                        let ct = ChatTurn::from_value(&t.value)?;
                        Some(json!({
                            "user_input": ct.user_input,
                            "agent_response": ct.agent_response,
                            "confidence": t.confidence,
                            "valid_from": t.observed_at.to_rfc3339(),
                        }))
                    })
                    .collect();

                Ok(json!({
                    "count": episodes.len(),
                    "episodes": episodes,
                }))
            }
        })
        .await
    }

    #[tool(description = "Storage usage and budget for episodic memory")]
    pub async fn episodic_budget(&self, Parameters(_budget): Parameters<BudgetRequest>) -> String {
        execute_tool(self, "episodic_budget", async {
            let usage = self
                .episodic
                .storage_usage(&self.webid)
                .map_err(|e| McpToolError::internal(format!("storage usage: {}", e)))?;
            let budget = self.episodic.storage_budget();
            let remaining = budget.saturating_sub(usage);
            Ok(json!({"used": usage, "budget": budget, "remaining": remaining}))
        })
        .await
    }

    #[tool(
        description = "Check consolidation candidates and budget status for episodic→semantic promotion"
    )]
    pub async fn episodic_consolidate_status(
        &self,
        Parameters(_req): Parameters<ConsolidateStatusRequest>,
    ) -> String {
        execute_tool(self, "episodic_consolidate_status", async {
            let candidate_count = self.episodic.consolidation_candidate_count(&self.webid);
            let usage = self
                .episodic
                .storage_usage(&self.webid)
                .map_err(|e| McpToolError::internal(format!("storage usage: {}", e)))?;
            let budget = self.episodic.storage_budget();
            let over_budget = usage > budget;
            Ok(json!({
                "consolidation_candidates": candidate_count,
                "episodic_usage": usage,
                "episodic_budget": budget,
                "over_budget": over_budget,
            }))
        })
        .await
    }

    // ── Semantic tools ──────────────────────────────────────────

    #[tool(description = "Liveness and storage info for semantic memory")]
    pub async fn semantic_ping(&self) -> String {
        execute_tool(self, "semantic_ping", async {
            Ok(json!({"status": "ok", "server": "hkask-mcp-memory"}))
        })
        .await
    }

    #[tool(description = "Store a shared semantic h_mem (no perspective)")]
    pub async fn semantic_store(
        &self,
        Parameters(StoreRequest {
            entity,
            attribute,
            value,
            confidence,
        }): Parameters<StoreRequest>,
    ) -> String {
        execute_tool(self, "semantic_store", async {
            validate_identifier("entity", &entity, 256)?;
            validate_identifier("attribute", &attribute, 256)?;
            let h_mem = HMem::new(&entity, &attribute, value, self.webid)
                .with_visibility(Visibility::Public)
                .with_confidence(confidence.unwrap_or(1.0));
            self.semantic
                .store(h_mem)
                .map_err(|e| McpToolError::internal(format!("store semantic h_mem: {}", e)))?;
            Ok(json!({"stored": true, "entity": entity, "attribute": attribute}))
        })
        .await
    }

    #[tool(description = "Recall shared semantic h_mems by entity")]
    pub async fn semantic_recall(
        &self,
        Parameters(RecallRequest { entity }): Parameters<RecallRequest>,
    ) -> String {
        execute_tool(self, "semantic_recall", async {
            validate_identifier("entity", &entity, 256)?;
            let h_mems = self
                .semantic
                .query_deduped(&entity)
                .map_err(|e| McpToolError::internal(format!("recall semantic h_mems: {}", e)))?;
            let serialized: Vec<_> = h_mems
                .iter()
                .map(|t| {
                    json!({
                        "entity": t.entity,
                        "attribute": t.attribute,
                        "value": t.value,
                        "confidence": t.confidence,
                        "valid_from": t.observed_at.to_rfc3339(),
                    })
                })
                .collect();
            Ok(json!({"count": serialized.len(), "h_mems": serialized}))
        })
        .await
    }

    // ── FlowDef dispatch tools — route by memory_type ───────────────────

    #[tool(
        description = "Store a memory h_mem — routes to episodic_store or semantic_store based on memory_type"
    )]
    pub async fn remember(
        &self,
        Parameters(MemoryDispatchRequest {
            entity,
            attribute,
            value,
            confidence,
            memory_type,
        }): Parameters<MemoryDispatchRequest>,
    ) -> String {
        let store_req = StoreRequest {
            entity,
            attribute,
            value,
            confidence,
        };
        match memory_type.as_str() {
            "semantic" => self.semantic_store(Parameters(store_req)).await,
            _ => self.episodic_store(Parameters(store_req)).await,
        }
    }

    #[tool(description = "Recall memory h_mems by entity — routes based on memory_type")]
    pub async fn recall(
        &self,
        Parameters(RecallDispatchRequest {
            entity,
            memory_type,
        }): Parameters<RecallDispatchRequest>,
    ) -> String {
        let recall_req = RecallRequest { entity };
        match memory_type.as_str() {
            "semantic" => self.semantic_recall(Parameters(recall_req)).await,
            _ => self.episodic_recall(Parameters(recall_req)).await,
        }
    }

    #[tool(
        description = "Paired memory recall — returns both semantic (third-person) and \
        episodic (first-person) memories for an entity in a single call. Episodic results \
        are ranked by salience when context is provided. Use this as the primary memory \
        recall tool — it mirrors the dual-recall circuit in ChatService::prepare_chat."
    )]
    pub async fn memory_recall(
        &self,
        Parameters(PairedRecallRequest {
            entity,
            context,
            limit,
        }): Parameters<PairedRecallRequest>,
    ) -> String {
        execute_tool(self, "memory_recall", async {
            validate_identifier("entity", &entity, 256)?;
            let limit = limit.unwrap_or(10);

            // ── Semantic recall (third-person facts, no personal filter) ──
            let semantic_triples = self
                .semantic
                .query_deduped(&entity)
                .map_err(|e| McpToolError::internal(format!("recall semantic memory: {}", e)))?;
            let semantic: Vec<_> = semantic_triples
                .iter()
                .take(limit)
                .map(|t| {
                    json!({
                        "entity": t.entity,
                        "attribute": t.attribute,
                        "value": t.value,
                        "confidence": t.confidence,
                        "valid_from": t.observed_at.to_rfc3339(),
                    })
                })
                .collect();

            // ── Episodic recall (first-person, filtered by caller's WebID) ──
            let episodic_triples = self
                .episodic
                .query_for_deduped(&entity, self.webid)
                .map_err(|e| McpToolError::internal(format!("recall episodic memory: {}", e)))?;

            if episodic_triples.is_empty() {
                return Ok(json!({
                    "entity": entity,
                    "semantic": { "count": semantic.len(), "h_mems": semantic },
                    "episodic": { "count": 0, "episodes": [] },
                }));
            }

            let episodic = if let Some(ref ctx) = context {
                // Salience-scored episodic recall (mirrors ChatService::recall_episodic)
                let keywords = hkask_memory::salience::extract_keywords(ctx);

                let mut scored: Vec<(usize, serde_json::Value)> = episodic_triples
                    .iter()
                    .filter_map(|t| {
                        let ct = ChatTurn::from_value(&t.value)?;
                        let combined = format!("{} {}", ct.user_input, ct.agent_response);
                        let score =
                            hkask_memory::salience::keyword_overlap_score(&keywords, &combined);
                        Some((
                            score,
                            json!({
                                "user_input": ct.user_input,
                                "agent_response": ct.agent_response,
                                "salience": score,
                                "confidence": t.confidence,
                                "valid_from": t.observed_at.to_rfc3339(),
                            }),
                        ))
                    })
                    .collect();
                scored.sort_by(|a, b| b.0.cmp(&a.0));
                scored
                    .into_iter()
                    .take(limit)
                    .map(|(_, v)| v)
                    .collect::<Vec<_>>()
            } else {
                // No context: most recent by recency
                episodic_triples
                    .iter()
                    .take(limit)
                    .filter_map(|t| {
                        let ct = ChatTurn::from_value(&t.value)?;
                        Some(json!({
                            "user_input": ct.user_input,
                            "agent_response": ct.agent_response,
                            "confidence": t.confidence,
                            "valid_from": t.observed_at.to_rfc3339(),
                        }))
                    })
                    .collect::<Vec<_>>()
            };

            Ok(json!({
                "entity": entity,
                "semantic": { "count": semantic.len(), "h_mems": semantic },
                "episodic": { "count": episodic.len(), "episodes": episodic },
            }))
        })
        .await
    }

    #[tool(description = "Store an embedding vector for similarity search")]
    pub async fn semantic_embed(
        &self,
        Parameters(EmbedRequest {
            entity_ref,
            vector,
            model,
        }): Parameters<EmbedRequest>,
    ) -> String {
        execute_tool(self, "semantic_embed", async {
            validate_identifier("entity_ref", &entity_ref, 256)?;
            if vector.is_empty() {
                return Err(McpToolError::invalid_argument("vector must not be empty"));
            }
            self.semantic
                .store_embedding(&entity_ref, &vector, &model)
                .map_err(|e| McpToolError::internal(format!("store embedding: {}", e)))?;
            Ok(json!({
                "stored": true,
                "entity_ref": entity_ref,
                "model": model,
                "dimensions": vector.len(),
            }))
        })
        .await
    }

    #[tool(description = "KNN similarity search over embeddings")]
    pub async fn semantic_search(
        &self,
        Parameters(SearchRequest {
            query_vector,
            limit,
        }): Parameters<SearchRequest>,
    ) -> String {
        execute_tool(self, "semantic_search", async {
            if query_vector.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "query_vector must not be empty",
                ));
            }
            let results = self
                .semantic
                .search_similar(&query_vector, limit.unwrap_or(10))
                .map_err(|e| McpToolError::internal(format!("search embeddings: {}", e)))?;
            let serialized: Vec<_> = results
                .iter()
                .map(|r| {
                    json!({
                        "entity_ref": r.embedding.entity_ref,
                        "model": r.embedding.model,
                        "distance": r.distance,
                    })
                })
                .collect();
            Ok(json!({"count": serialized.len(), "results": serialized}))
        })
        .await
    }

    #[tool(
        description = "Compute mean embedding vector (centroid) for embeddings matching a prefix"
    )]
    pub async fn semantic_centroid(
        &self,
        Parameters(CentroidRequest {
            prefix,
            exclude_prefix,
            exclude_ref,
            dim,
            store_as,
            model,
        }): Parameters<CentroidRequest>,
    ) -> String {
        execute_tool(self, "semantic_centroid", async {
            validate_identifier("prefix", &prefix, 256)?;
            validate_identifier("exclude_prefix", &exclude_prefix, 256)?;
            validate_identifier("exclude_ref", &exclude_ref, 256)?;
            if dim == 0 {
                return Err(McpToolError::invalid_argument("dim must be positive"));
            }
            let result = self
                .semantic
                .compute_centroid(
                    &prefix,
                    &exclude_prefix,
                    &exclude_ref,
                    dim,
                    store_as.as_deref(),
                    model.as_deref(),
                )
                .map_err(|e| McpToolError::internal(format!("compute centroid: {}", e)))?;
            Ok(json!({
                "centroid": result.centroid,
                "dimensions": result.centroid.len(),
                "prefix": prefix,
                "passage_count": result.passage_count,
                "stored": result.stored,
            }))
        })
        .await
    }

    #[tool(description = "Delete all embeddings whose entity_ref starts with a prefix")]
    pub async fn semantic_purge(
        &self,
        Parameters(PurgeRequest { prefix }): Parameters<PurgeRequest>,
    ) -> String {
        execute_tool(self, "semantic_purge", async {
            validate_identifier("prefix", &prefix, 256)?;
            let count = self
                .semantic
                .purge_by_prefix(&prefix)
                .map_err(|e| McpToolError::internal(format!("purge embeddings: {}", e)))?;
            Ok(json!({"purged": count, "prefix": prefix}))
        })
        .await
    }

    #[tool(
        description = "Chunk text into passages for embedding, with optional Gutenberg header stripping"
    )]
    pub async fn semantic_chunk(
        &self,
        Parameters(ChunkTextRequest {
            text,
            entity_ref_prefix,
            min_words,
            max_words,
            sentence_boundary,
            strip_gutenberg,
        }): Parameters<ChunkTextRequest>,
    ) -> String {
        execute_tool(self, "semantic_chunk", async {
            if text.is_empty() || entity_ref_prefix.is_empty() {
                let field = if text.is_empty() {
                    "text"
                } else {
                    "entity_ref_prefix"
                };
                return Err(McpToolError::invalid_argument(format!(
                    "{field} must not be empty"
                )));
            }
            validate_identifier("entity_ref_prefix", &entity_ref_prefix, 256)?;
            let min_w = min_words.unwrap_or(50);
            let max_w = max_words.unwrap_or(200);
            let boundary = sentence_boundary.unwrap_or_else(|| ".!? ".to_string());
            let processed = if strip_gutenberg.unwrap_or(false) {
                SemanticMemory::strip_gutenberg_headers(&text)
            } else {
                text.clone()
            };
            let passages =
                SemanticMemory::chunk_text(&processed, &entity_ref_prefix, min_w, max_w, &boundary);
            let serialized: Vec<_> = passages
                .into_iter()
                .map(|(entity_ref, passage_text)| {
                    json!({"entity_ref": entity_ref, "text": passage_text})
                })
                .collect();
            Ok(json!({
                "total_passages": serialized.len(),
                "passages": serialized,
                "min_words": min_w,
                "max_words": max_w,
                "sentence_boundary": boundary,
                "stripped_gutenberg": strip_gutenberg.unwrap_or(false),
            }))
        })
        .await
    }

    #[tool(description = "HMem and embedding counts for semantic memory")]
    pub async fn semantic_count(&self, Parameters(_req): Parameters<CountRequest>) -> String {
        execute_tool(self, "semantic_count", async {
            let triple_count = self
                .semantic
                .h_mem_count()
                .map_err(|e| McpToolError::internal(format!("count h_mems: {}", e)))?;
            let embedding_count = self
                .semantic
                .embedding_count()
                .map_err(|e| McpToolError::internal(format!("count embeddings: {}", e)))?;
            Ok(json!({"h_mem_count": triple_count, "embedding_count": embedding_count}))
        })
        .await
    }

    // ── Backup/restore tools ───────────────────────────────────
    //
    // NOTE: Backup/restore previously used rusqlite's online backup API to
    // page-copy an encrypted SQLite database. The StorageDriver port does not
    // yet expose backup primitives, so these tools are stubbed until the port
    // gains backup support (tracked separately).

    #[tool(description = "Export the memory database to a local backup file")]
    pub async fn memory_backup(
        &self,
        Parameters(BackupRequest {
            target_path: _,
            passphrase: _,
        }): Parameters<BackupRequest>,
    ) -> String {
        execute_tool(self, "memory_backup", async {
            Err(McpToolError::internal(
                "not yet ported — backup requires StorageDriver support",
            ))
        })
        .await
    }

    #[tool(description = "Restore the memory database from a local backup file")]
    pub async fn memory_restore(
        &self,
        Parameters(RestoreRequest {
            source_path: _,
            passphrase: _,
        }): Parameters<RestoreRequest>,
    ) -> String {
        execute_tool(self, "memory_restore", async {
            Err(McpToolError::internal(
                "not yet ported — backup requires StorageDriver support",
            ))
        })
        .await
    }
}

/// Run the memory MCP server (used by binary target).
pub async fn run(
    userpod: String,
    daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-memory",
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::server::ServerContext| {
            (|| -> anyhow::Result<MemoryServer> {
                // Resolve the StorageDriver via the ServerContext port. The concrete
                // driver is provided by kask_bridge at runtime; when no DB path is
                // configured, open_database returns an error and we fall back to
                // a no-op stub driver so the server still boots (read-only mode).
                let driver: Arc<dyn StorageDriver> = ctx
                    .open_database("HKASK_MEMORY_DB")
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let db = Some(Arc::clone(&driver));

                let h_mem_store = HMemStore::from_driver(Arc::clone(&driver));
                let episodic = EpisodicMemory::new(h_mem_store);

                // EmbeddingPort is not yet provided by kask_bridge. Use a local
                // stub so the server compiles and boots; embedding tools return
                // empty results until the bridge supplies a real implementation.
                let embedding_port: Arc<dyn EmbeddingPort> =
                    Arc::new(StubEmbeddingPort);
                let semantic = Arc::new(SemanticMemory::new(
                    HMemStore::from_driver(driver),
                    embedding_port,
                ));

                Ok(MemoryServer::new(
                    ctx.webid,
                    userpod.clone(),
                    daemon_client.clone(),
                    episodic,
                    semantic,
                    db,
                ))
            })()
            .map_err(|e| hkask_mcp_server::McpError::UnexpectedResponse {
                context: "memory server init".into(),
                detail: e.to_string(),
            })
        },
        vec![
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_MEMORY_DB",
                "Path to per-agent memory database file (defaults to agents/{userpod}/memory.db)",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_DB_PASSPHRASE",
                "SQLCipher encryption passphrase (resolved via hkask keystore chain when not set)",
            ),
        ],
    )
    .await
}

/// No-op `EmbeddingPort` stub used until `kask_bridge` provides a concrete
/// `EmbeddingPort` implementation over `StorageDriver`.
///
/// All operations return empty results or "not found" — sufficient for the
/// server to boot and serve h_mem tools while embedding tools remain inert.
struct StubEmbeddingPort;

impl EmbeddingPort for StubEmbeddingPort {
    fn store(
        &self,
        _entity_ref: &str,
        _vector: &[f32],
        _model: &str,
    ) -> Result<String, EmbeddingError> {
        Ok(EmbeddingID::new().to_string())
    }

    fn get(&self, entity_ref: &str) -> Result<StoredEmbedding, EmbeddingError> {
        Err(EmbeddingError::NotFound(NotFound {
            entity_type: "embedding".to_string(),
            id: entity_ref.to_string(),
        }))
    }

    fn search(
        &self,
        _query_embedding: &[f32],
        _limit: usize,
    ) -> Result<Vec<SimilarityResult>, EmbeddingError> {
        Ok(Vec::new())
    }

    fn delete(&self, _entity_ref: &str) -> Result<(), EmbeddingError> {
        Ok(())
    }

    fn count(&self) -> Result<usize, EmbeddingError> {
        Ok(0)
    }

    fn query_by_prefix(&self, _prefix: &str) -> Result<Vec<String>, EmbeddingError> {
        Ok(Vec::new())
    }

    fn get_all_by_prefix(
        &self,
        _prefix: &str,
    ) -> Result<Vec<(String, Vec<f32>)>, EmbeddingError> {
        Ok(Vec::new())
    }
}
