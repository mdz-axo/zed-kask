//! `MemoryPort` adapter — bridges zed's thread completion to hKask memory (D6).
//!
//! Two implementations:
//!
//! - `LoggingMemoryPort` — no-op placeholder. Logs the turn and returns `Ok(())`.
//!   Used when `HKASK_DB_PATH` is not set (graceful degradation).
//!
//! - `RealMemoryPort` — full hKask memory stack. Stores completed turns into
//!   episodic memory (Private, perspective = user WebID) and semantic memory
//!   (Shared, for curator access). Embeds the user prompt for future retrieval.
//!   Used when `HKASK_DB_PATH` + `HKASK_DB_PASSPHRASE` are configured.
//!
//! The port is injected via a global hook (`agent::set_memory_port`) so the
//! `agent` crate doesn't depend on `kask_bridge`.

use hkask_inference::{EmbeddingRouter, InferenceConfig};
use hkask_memory::{ConsolidationBridge, ConsolidationService, EpisodicMemory, SemanticMemory};
use hkask_storage::{Database, EmbeddingStore, HMem, HMemStore};
use hkask_types::{MemoryError, MemoryPort, MemorySnippet, TurnRecord, Visibility, WebID};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;

use chrono::Utc;

// ── Logging no-op (fallback when DB not configured) ────────────────────────

/// Logging no-op `MemoryPort` implementation.
///
/// Logs the turn record at `info` level and returns `Ok(())`.
/// Used when `HKASK_DB_PATH` is not set.
pub struct LoggingMemoryPort;

impl LoggingMemoryPort {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LoggingMemoryPort {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryPort for LoggingMemoryPort {
    fn ingest_turn<'a>(
        &'a self,
        record: TurnRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(
                target: "reg.memory",
                thread_id = %record.thread_id,
                model = %record.model,
                prompt_len = record.user_input.len(),
                response_len = record.agent_response.len(),
                title = ?record.thread_title,
                "Turn ingested into memory (logging no-op — HKASK_DB_PATH not set)"
            );
            Ok(())
        })
    }
}

// ── Real memory port (full hKask memory stack) ─────────────────────────────

/// Real `MemoryPort` implementation backed by hKask's episodic + semantic memory.
///
/// Stores each completed turn as:
/// 1. An episodic h_mem (Private, perspective = user WebID) — the user's
///    first-person experience record.
/// 2. A semantic h_mem (Shared) — a curator-accessible copy for metacognitive
///    reflection.
/// 3. An embedding of the user prompt — for future semantic retrieval and
///    context injection.
///
/// Construction requires a SQLCipher database path and passphrase. When
/// these are not available, use `LoggingMemoryPort` instead.
pub struct RealMemoryPort {
    episodic: Arc<EpisodicMemory>,
    semantic: Arc<SemanticMemory>,
    embedding_router: EmbeddingRouter,
    embedding_model: String,
    user_webid: WebID,
    curator_webid: WebID,
    /// Consolidation service — promotes episodic h_mems to semantic memory.
    /// `None` when consolidation is disabled (`consolidation_cadence_secs == 0`).
    consolidation: Option<Arc<ConsolidationService>>,
    /// Consolidation cadence in seconds. `0` disables the trigger.
    consolidation_cadence_secs: u64,
    /// Confidence floor for semantic cleanup during consolidation.
    confidence_floor: f64,
    /// Timestamp of the last consolidation pass. Guarded by a mutex so the
    /// ingestion path (which is `&self`) can check-and-update atomically.
    last_consolidation: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// Tokio runtime handle — entered around embedding HTTP calls so that
    /// `reqwest` (which is tokio-backed) has a reactor. The memory port's
    /// async methods are called from GPUI's background executor, not tokio.
    tokio_handle: tokio::runtime::Handle,
}

impl RealMemoryPort {
    /// Construct a new `RealMemoryPort` from a database path and passphrase.
    ///
    /// Opens a SQLCipher database, creates episodic and semantic memory stores,
    /// and initializes an embedding router for prompt embedding.
    ///
    /// Returns `Err` if the database cannot be opened.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db_path: &str,
        passphrase: &str,
        user_webid: WebID,
        embedding_model: String,
        embedding_dim: usize,
        consolidation_cadence_secs: u64,
        confidence_floor: f64,
        tokio_handle: tokio::runtime::Handle,
    ) -> Result<Self, String> {
        let db = Database::open(db_path, passphrase).map_err(|e| e.to_string())?;
        let pool = db.sqlite_pool().map_err(|e| e.to_string())?;
        let driver: Arc<dyn hkask_storage::DatabaseDriver> =
            Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool));

        // Episodic store — first-person, Private, perspective-bound
        let h_mem_store = HMemStore::from_driver(Arc::clone(&driver));
        let episodic = Arc::new(EpisodicMemory::new(h_mem_store));

        // Semantic store — shared knowledge graph with embeddings.
        // The embedding dimension must match the embedding model's output —
        // a mismatch causes `DimensionMismatch` errors on every store call,
        // silently disabling embedding-based recall. The caller resolves
        // this from `kask_settings.corpus.embedding_dim` (default 1024,
        // matching `DeepInfra/Qwen/Qwen3-Embedding-0.6B`).
        //
        // A dim of 0 is a footgun: `unwrap_or(1024)` only fires for `None`,
        // not for `Some(0)`, so a user setting `embedding_dim: 0` would
        // construct a store that rejects every vector. `KaskCorpusSettings`
        // already filters 0 → 1024, but warn here too in case a future
        // caller bypasses settings — per the .rules trap "Process-global
        // hooks set at runtime need a startup-failure signal".
        if embedding_dim == 0 {
            tracing::warn!(
                target: "reg.memory",
                embedding_dim,
                "RealMemoryPort constructed with embedding_dim == 0 — \
                 every store_embedding call will fail with DimensionMismatch. \
                 Set kask_settings.corpus.embedding_dim (or HKASK_EMBEDDING_DIM) \
                 to match the embedding model's output (default 1024 for \
                 DeepInfra/Qwen/Qwen3-Embedding-0.6B)."
            );
        } else if embedding_dim != 1024 {
            tracing::info!(
                target: "reg.memory",
                embedding_dim,
                "RealMemoryPort using non-default embedding dimension \
                 (ensure this matches the configured embedding model)"
            );
        }
        let h_mem_store2 = HMemStore::from_driver(Arc::clone(&driver));
        let embedding_store = EmbeddingStore::from_driver(driver, embedding_dim);
        let semantic = Arc::new(SemanticMemory::new(h_mem_store2, embedding_store));

        let inference_config = InferenceConfig::from_env();
        let embedding_router = EmbeddingRouter::new(inference_config);

        let curator_webid = WebID::from_persona(b"Curator");

        // Consolidation service — episodic → semantic promotion.
        // Only constructed when the cadence is non-zero; a zero cadence disables
        // the trigger entirely (the operator can still fire consolidation
        // manually via the curator MCP server).
        let consolidation = if consolidation_cadence_secs > 0 {
            let bridge = Arc::new(ConsolidationBridge::new(
                Arc::clone(&episodic),
                Arc::clone(&semantic),
            ));
            Some(Arc::new(ConsolidationService::new(
                bridge,
                Arc::clone(&semantic),
            )))
        } else {
            None
        };

        Ok(Self {
            episodic,
            semantic,
            embedding_router,
            embedding_model,
            user_webid,
            curator_webid,
            consolidation,
            consolidation_cadence_secs,
            confidence_floor,
            last_consolidation: Mutex::new(None),
            tokio_handle,
        })
    }

    /// Try to construct a `RealMemoryPort` from environment variables.
    ///
    /// Returns `Ok(Some(port))` if `HKASK_DB_PATH` and `HKASK_DB_PASSPHRASE`
    /// are set and the database opens successfully.
    /// Returns `Ok(None)` if `HKASK_DB_PATH` is not set (graceful degradation).
    /// Returns `Err` if the database path is set but cannot be opened.
    ///
    /// `consolidation_cadence_secs` and `confidence_floor` come from
    /// `KaskMemorySettings` — a cadence of `0` disables the consolidation
    /// trigger entirely.
    pub fn from_env(
        user_webid: WebID,
        embedding_model: String,
        embedding_dim: usize,
        consolidation_cadence_secs: u64,
        confidence_floor: f64,
        tokio_handle: tokio::runtime::Handle,
    ) -> Result<Option<Self>, String> {
        let db_path = match std::env::var("HKASK_DB_PATH") {
            Ok(p) if !p.trim().is_empty() => p,
            _ => return Ok(None),
        };

        let passphrase = hkask_keystore::keychain::resolve_db_passphrase_string()
            .map_err(|e| e.to_string())?
            .to_string();

        let port = Self::new(
            &db_path,
            &passphrase,
            user_webid,
            embedding_model,
            embedding_dim,
            consolidation_cadence_secs,
            confidence_floor,
            tokio_handle,
        )?;
        tracing::info!(
            target: "reg.memory",
            db_path = %db_path,
            consolidation_cadence_secs,
            confidence_floor,
            "RealMemoryPort initialized — turns will be stored in episodic + semantic memory"
        );
        Ok(Some(port))
    }

    /// Check whether the consolidation cadence has elapsed and, if so, fire
    /// a consolidation pass (episodic → semantic promotion + semantic cleanup).
    ///
    /// This is called after each successful `ingest_turn`. The cadence check
    /// is atomic: the timestamp is updated under the mutex before consolidation
    /// runs, so concurrent ingestions won't double-fire.
    ///
    /// Consolidation is a synchronous DB operation — it runs inline within
    /// the `ingest_turn` future, which the caller has already detached onto a
    /// background executor (`cx.background_spawn(...).detach()` in the agent
    /// crate). It does not block the UI or the agent thread.
    fn maybe_consolidate(&self) {
        let Some(consolidation) = &self.consolidation else {
            return;
        };
        if self.consolidation_cadence_secs == 0 {
            return;
        }

        // Check-and-update under the mutex so concurrent ingestions don't
        // double-fire consolidation. If the cadence hasn't elapsed, bail.
        let now = Utc::now();
        let cadence = chrono::Duration::seconds(self.consolidation_cadence_secs as i64);
        let should_fire = match self.last_consolidation.lock() {
            Ok(mut guard) => {
                let elapsed = guard
                    .map(|last| now.signed_duration_since(last) >= cadence)
                    .unwrap_or(true);
                if elapsed {
                    *guard = Some(now);
                    true
                } else {
                    false
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.memory",
                    error = %e,
                    "last_consolidation mutex poisoned — skipping consolidation trigger"
                );
                return;
            }
        };

        if !should_fire {
            return;
        }

        tracing::info!(
            target: "reg.memory",
            cadence_secs = self.consolidation_cadence_secs,
            confidence_floor = self.confidence_floor,
            "Consolidation cadence elapsed — firing curator consolidation"
        );

        let request = hkask_types::ConsolidationRequest {
            limit: 100,
            confidence_floor: Some(self.confidence_floor),
            max_semantic_triples: None,
        };

        match consolidation.consolidate(&self.user_webid, request) {
            Ok(outcome) => {
                tracing::info!(
                    target: "reg.memory",
                    consolidated = outcome.consolidated_count,
                    deleted = outcome.deleted_count,
                    failed = outcome.failed_count,
                    "Consolidation pass complete"
                );
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.memory",
                    error = %e,
                    "Consolidation pass failed"
                );
            }
        }
    }
}

impl MemoryPort for RealMemoryPort {
    fn ingest_turn<'a>(
        &'a self,
        record: TurnRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            let thread_id = record.thread_id.clone();
            let user_input = record.user_input.clone();
            let agent_response = record.agent_response.clone();
            let model = record.model.clone();
            let title = record.thread_title.clone();

            // ── 1. Store episodic h_mem (Private, user perspective) ───────
            //
            // The episodic record stores the full turn as a JSON value under
            // the "chatted" attribute. This is the user's first-person
            // experience — only the owning agent can recall it.
            let entity = format!("chat:thread:{thread_id}");
            let turn_value = serde_json::json!({
                "user_input": user_input,
                "agent_response": agent_response,
                "model": model,
                "title": title,
            });

            let episodic_h_mem = HMem::new(
                &entity,
                "chatted",
                serde_json::Value::String(turn_value.to_string()),
                self.user_webid,
            )
            .with_perspective(self.user_webid)
            .with_visibility(Visibility::Private);

            if let Err(e) = self.episodic.store(episodic_h_mem) {
                tracing::warn!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    error = %e,
                    "Failed to store episodic h_mem"
                );
                return Err(MemoryError::Ingestion(format!(
                    "Episodic store failed: {e}"
                )));
            }

            // ── 2. Store semantic h_mem (Shared, curator-accessible) ──────
            //
            // The semantic record is a curator copy — Shared visibility, no
            // perspective. The curator can recall this for metacognitive
            // reflection on the user's conversation patterns.
            let curator_entity = format!("curator:thread:{thread_id}");
            let curator_h_mem = HMem::new(
                &curator_entity,
                "turn",
                serde_json::Value::String(turn_value.to_string()),
                self.curator_webid,
            )
            .with_visibility(Visibility::Shared);

            if let Err(e) = self.semantic.store(curator_h_mem) {
                tracing::warn!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    error = %e,
                    "Failed to store semantic (curator) h_mem"
                );
                // Non-fatal — the episodic record is the primary store.
            }

            // ── 3. Embed the user prompt for future retrieval ─────────────
            //
            // The embedding enables semantic search (KNN) for context
            // injection — when the user asks a similar question later,
            // this turn can be recalled and injected into the prompt.
            let embedding_entity = format!("embedding:thread:{thread_id}:user_input");
            // Spawn the embedding HTTP call on the tokio runtime so reqwest
            // has a reactor. The rest of ingest_turn doesn't need tokio.
            let embedding_model = self.embedding_model.clone();
            let embedding_router = self.embedding_router.clone();
            let user_input_owned = user_input.clone();
            let vectors = self
                .tokio_handle
                .spawn(async move {
                    embedding_router
                        .embed_sentences(&embedding_model, &[user_input_owned.as_str()])
                        .await
                })
                .await;

            match vectors {
                Ok(Ok(vectors)) => {
                    if let Some(vector) = vectors.into_iter().next()
                        && let Err(e) = self.semantic.store_embedding(
                            &embedding_entity,
                            &vector,
                            &self.embedding_model,
                        )
                    {
                        tracing::warn!(
                            target: "reg.memory",
                            thread_id = %thread_id,
                            error = %e,
                            "Failed to store prompt embedding"
                        );
                    }
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        error = %e,
                        "Failed to embed user prompt — embedding-based recall will not work for this turn"
                    );
                    // Non-fatal — entity-based recall still works.
                }
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        error = %e,
                        "Embedding task panicked — embedding-based recall will not work for this turn"
                    );
                }
            }

            tracing::info!(
                target: "reg.memory",
                thread_id = %thread_id,
                model = %model,
                "Turn ingested into episodic + semantic memory"
            );

            // ── 4. Curator consolidation trigger (Task 5.2) ───────────────
            //
            // After each ingestion, check whether the consolidation cadence has
            // elapsed since the last pass. If so, promote episodic h_mems to
            // semantic memory (episodic → semantic, one-way). This runs inline
            // within the ingestion future — the caller already detached this
            // task (`cx.background_spawn(...).detach()` in the agent crate),
            // so consolidation does not block the UI or the agent thread.
            //
            // A cadence of 0 disables the trigger (the operator can still
            // fire consolidation manually via the curator MCP server).
            self.maybe_consolidate();

            Ok(())
        })
    }

    fn recall_context<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemorySnippet>, MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            let mut snippets: Vec<MemorySnippet> = Vec::new();

            // ── 1. Semantic search (embedding KNN) ───────────────────────
            //
            // Embed the query and search for similar stored embeddings.
            // This finds turns where the user asked similar questions.
            // Spawn the embedding HTTP call on the tokio runtime so reqwest
            // has a reactor.
            let embedding_model = self.embedding_model.clone();
            let embedding_router = self.embedding_router.clone();
            let query_owned = query.to_string();
            let vectors = self
                .tokio_handle
                .spawn(async move {
                    embedding_router
                        .embed_sentences(&embedding_model, &[query_owned.as_str()])
                        .await
                })
                .await;

            if let Ok(Ok(vectors)) = vectors
                && let Some(query_vector) = vectors.into_iter().next()
            {
                match self.semantic.search_similar(&query_vector, limit) {
                    Ok(results) => {
                        for result in results {
                            // Retrieve the h_mem associated with this embedding
                            // to get the full text content.
                            let entity_ref = &result.embedding.entity_ref;
                            if let Ok(h_mems) = self.semantic.query_deduped(entity_ref) {
                                for h_mem in h_mems {
                                    let text = h_mem.value.as_str().unwrap_or("").to_string();
                                    if !text.is_empty() {
                                        snippets.push(MemorySnippet {
                                            text,
                                            source: "semantic".to_string(),
                                            confidence: h_mem.confidence.value(),
                                            relevance_score: 1.0 - result.distance,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.memory",
                            error = %e,
                            "Semantic search failed during recall"
                        );
                    }
                }
            }

            // ── 2. Episodic search (entity/keyword overlap) ──────────────
            //
            // Query episodic memory by extracting keywords from the query
            // and searching for h_mems with matching entities.
            let query_words: Vec<&str> = query
                .split_whitespace()
                .filter(|w| w.len() > 3)
                .take(5)
                .collect();

            for word in &query_words {
                let entity = "chat:thread:".to_string();
                if let Ok(h_mems) = self.episodic.query_for_deduped(&entity, self.user_webid) {
                    for h_mem in h_mems {
                        let text = h_mem.value.as_str().unwrap_or("").to_string();
                        if text.is_empty() {
                            continue;
                        }
                        // Check if the query word appears in the text
                        if text.to_lowercase().contains(&word.to_lowercase()) {
                            // Skip if already in snippets (dedup by text)
                            if snippets.iter().any(|s| s.text == text) {
                                continue;
                            }
                            snippets.push(MemorySnippet {
                                text,
                                source: "episodic".to_string(),
                                confidence: h_mem.confidence.value(),
                                relevance_score: 0.5, // Base relevance for keyword match
                            });
                        }
                    }
                }
            }

            // ── 3. Sort by relevance and truncate ─────────────────────────
            snippets.sort_by(|a, b| {
                b.relevance_score
                    .partial_cmp(&a.relevance_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            snippets.truncate(limit);

            tracing::info!(
                target: "reg.memory",
                query_len = query.len(),
                recalled = snippets.len(),
                "Recalled memory snippets for context injection"
            );

            Ok(snippets)
        })
    }
}

// ── BridgeMemoryPort (agent::ThreadMemoryPort adapter) ─────────────────────

/// Adapter that implements the `agent` crate's `ThreadMemoryPort` trait
/// by delegating to an `hkask_types::MemoryPort`.
///
/// This is the bridge between the `agent` crate's local trait (which can't
/// depend on `hkask-types`) and the hKask `MemoryPort` trait.
pub struct BridgeMemoryPort {
    inner: std::sync::Arc<dyn MemoryPort>,
}

impl BridgeMemoryPort {
    pub fn new(inner: std::sync::Arc<dyn MemoryPort>) -> Self {
        Self { inner }
    }
}

impl agent::ThreadMemoryPort for BridgeMemoryPort {
    fn ingest_turn(
        &self,
        record: agent::ThreadTurnRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let inner = self.inner.clone();
        Box::pin(async move {
            inner
                .ingest_turn(TurnRecord {
                    thread_id: record.thread_id,
                    user_input: record.user_input,
                    agent_response: record.agent_response,
                    model: record.model,
                    thread_title: record.thread_title,
                })
                .await
                .map_err(|e| e.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::database::sqlite::SqliteDriver;

    fn test_webid() -> WebID {
        WebID::new()
    }

    fn in_memory_port() -> RealMemoryPort {
        in_memory_port_with_cadence(0, 0.3)
    }

    fn in_memory_port_with_cadence(
        consolidation_cadence_secs: u64,
        confidence_floor: f64,
    ) -> RealMemoryPort {
        let driver: Arc<dyn hkask_storage::DatabaseDriver> = SqliteDriver::in_memory_driver();
        let h_mem_store = HMemStore::from_driver(Arc::clone(&driver));
        let episodic = Arc::new(EpisodicMemory::new(h_mem_store));

        let h_mem_store2 = HMemStore::from_driver(Arc::clone(&driver));
        let embedding_store = EmbeddingStore::from_driver(driver, 1024);
        let semantic = Arc::new(SemanticMemory::new(h_mem_store2, embedding_store));

        // EmbeddingRouter needs InferenceConfig, but we won't call embed in tests
        let inference_config = InferenceConfig::from_env();
        let embedding_router = EmbeddingRouter::new(inference_config);

        let consolidation = if consolidation_cadence_secs > 0 {
            let bridge = Arc::new(ConsolidationBridge::new(
                Arc::clone(&episodic),
                Arc::clone(&semantic),
            ));
            Some(Arc::new(ConsolidationService::new(
                bridge,
                Arc::clone(&semantic),
            )))
        } else {
            None
        };

        RealMemoryPort {
            episodic,
            semantic,
            embedding_router,
            embedding_model: "test-model".to_string(),
            user_webid: test_webid(),
            curator_webid: WebID::from_persona(b"Curator"),
            consolidation,
            consolidation_cadence_secs,
            confidence_floor,
            last_consolidation: Mutex::new(None),
            tokio_handle: tokio::runtime::Handle::current(),
        }
    }

    #[tokio::test]
    async fn ingest_turn_stores_episodic_h_mem() {
        let port = in_memory_port();
        let webid = port.user_webid;
        let record = TurnRecord {
            thread_id: "test-thread".to_string(),
            user_input: "What is Rust?".to_string(),
            agent_response: "Rust is a systems programming language.".to_string(),
            model: "test-model".to_string(),
            thread_title: Some("Rust Discussion".to_string()),
        };

        let result = port.ingest_turn(record).await;
        assert!(result.is_ok(), "ingest_turn should succeed");

        // Verify episodic h_mem was stored
        let h_mems = port
            .episodic
            .query_for_deduped("chat:thread:test-thread", webid)
            .expect("query should succeed");
        assert_eq!(h_mems.len(), 1, "one episodic h_mem should be stored");
        assert_eq!(h_mems[0].attribute, "chatted");
    }

    #[tokio::test]
    async fn ingest_turn_stores_semantic_curator_copy() {
        let port = in_memory_port();
        let record = TurnRecord {
            thread_id: "test-thread-2".to_string(),
            user_input: "Explain async Rust".to_string(),
            agent_response: "Async Rust uses tokio.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
        };

        let result = port.ingest_turn(record).await;
        assert!(result.is_ok());

        // Verify semantic (curator) h_mem was stored
        let h_mems = port
            .semantic
            .query_deduped("curator:thread:test-thread-2")
            .expect("query should succeed");
        assert_eq!(h_mems.len(), 1, "one semantic h_mem should be stored");
        assert_eq!(h_mems[0].attribute, "turn");
    }

    #[tokio::test]
    async fn ingest_turn_handles_empty_prompt_gracefully() {
        let port = in_memory_port();
        let webid = port.user_webid;
        let record = TurnRecord {
            thread_id: "test-empty".to_string(),
            user_input: String::new(),
            agent_response: "Response".to_string(),
            model: "test".to_string(),
            thread_title: None,
        };

        let result = port.ingest_turn(record).await;
        assert!(result.is_ok(), "empty prompt should not fail ingestion");

        let h_mems = port
            .episodic
            .query_for_deduped("chat:thread:test-empty", webid)
            .expect("query should succeed");
        assert_eq!(h_mems.len(), 1, "episodic h_mem should still be stored");
    }

    #[tokio::test]
    async fn ingest_turn_fires_consolidation_when_cadence_elapses() {
        // Cadence of 1 second — any ingestion should fire consolidation.
        let port = in_memory_port_with_cadence(1, 0.3);
        let webid = port.user_webid;

        // Ingest a turn — this should fire consolidation (no prior consolidation).
        let record = TurnRecord {
            thread_id: "consolidation-test".to_string(),
            user_input: "Tell me about memory consolidation".to_string(),
            agent_response: "Consolidation promotes episodic to semantic.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
        };
        let result = port.ingest_turn(record).await;
        assert!(result.is_ok(), "ingest_turn should succeed");

        // The last_consolidation timestamp should now be set.
        let last = port
            .last_consolidation
            .lock()
            .expect("mutex not poisoned")
            .expect("consolidation should have fired");
        assert!(
            Utc::now().signed_duration_since(last).num_seconds() < 5,
            "last_consolidation should be recent"
        );

        // A second ingestion immediately after should NOT fire consolidation
        // (cadence hasn't elapsed). We verify by checking the timestamp is
        // unchanged.
        let record2 = TurnRecord {
            thread_id: "consolidation-test-2".to_string(),
            user_input: "Another question".to_string(),
            agent_response: "Another answer".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
        };
        let _ = port.ingest_turn(record2).await;
        let last_after = port
            .last_consolidation
            .lock()
            .expect("mutex not poisoned")
            .expect("consolidation timestamp should still be set");
        assert_eq!(
            last, last_after,
            "second ingestion within cadence should not re-fire consolidation"
        );

        // After consolidation, the first episodic h_mem may have been promoted
        // to semantic and expired in episodic (consolidation is a one-way
        // episodic → semantic promotion). The second h_mem was ingested after
        // consolidation, so it should still be in episodic.
        let h_mems = port
            .episodic
            .query_for_deduped("chat:thread:consolidation-test-2", webid)
            .expect("query should succeed");
        assert_eq!(
            h_mems.len(),
            1,
            "second episodic h_mem (ingested after consolidation) should be stored"
        );
    }

    #[tokio::test]
    async fn ingest_turn_skips_consolidation_when_cadence_zero() {
        // Cadence of 0 — consolidation is disabled.
        let port = in_memory_port_with_cadence(0, 0.3);
        let record = TurnRecord {
            thread_id: "no-consolidation".to_string(),
            user_input: "Hello".to_string(),
            agent_response: "Hi".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
        };
        let result = port.ingest_turn(record).await;
        assert!(result.is_ok());

        // last_consolidation should remain None (never fired).
        let last = port.last_consolidation.lock().expect("mutex not poisoned");
        assert!(
            last.is_none(),
            "consolidation should not fire when cadence is 0"
        );
    }
}
