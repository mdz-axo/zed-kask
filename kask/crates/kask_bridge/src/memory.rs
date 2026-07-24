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
use hkask_memory::EpisodicMemory;
use hkask_memory::SemanticMemory;
use hkask_storage::{Database, EmbeddingStore, HMem, HMemStore};
use hkask_types::{MemoryError, MemoryPort, MemorySnippet, TurnRecord, Visibility, WebID};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

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
    episodic: EpisodicMemory,
    semantic: SemanticMemory,
    embedding_router: EmbeddingRouter,
    embedding_model: String,
    user_webid: WebID,
    curator_webid: WebID,
}

impl RealMemoryPort {
    /// Construct a new `RealMemoryPort` from a database path and passphrase.
    ///
    /// Opens a SQLCipher database, creates episodic and semantic memory stores,
    /// and initializes an embedding router for prompt embedding.
    ///
    /// Returns `Err` if the database cannot be opened.
    pub fn new(
        db_path: &str,
        passphrase: &str,
        user_webid: WebID,
        embedding_model: String,
    ) -> Result<Self, String> {
        let db = Database::open(db_path, passphrase).map_err(|e| e.to_string())?;
        let pool = db.sqlite_pool().map_err(|e| e.to_string())?;
        let driver: Arc<dyn hkask_storage::DatabaseDriver> =
            Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool));

        // Episodic store — first-person, Private, perspective-bound
        let h_mem_store = HMemStore::from_driver(Arc::clone(&driver));
        let episodic = EpisodicMemory::new(h_mem_store);

        // Semantic store — shared knowledge graph with embeddings
        let h_mem_store2 = HMemStore::from_driver(Arc::clone(&driver));
        let embedding_store = EmbeddingStore::from_driver(driver, 1024);
        let semantic = SemanticMemory::new(h_mem_store2, embedding_store);

        let inference_config = InferenceConfig::from_env();
        let embedding_router = EmbeddingRouter::new(inference_config);

        let curator_webid = WebID::from_persona(b"Curator");

        Ok(Self {
            episodic,
            semantic,
            embedding_router,
            embedding_model,
            user_webid,
            curator_webid,
        })
    }

    /// Try to construct a `RealMemoryPort` from environment variables.
    ///
    /// Returns `Ok(Some(port))` if `HKASK_DB_PATH` and `HKASK_DB_PASSPHRASE`
    /// are set and the database opens successfully.
    /// Returns `Ok(None)` if `HKASK_DB_PATH` is not set (graceful degradation).
    /// Returns `Err` if the database path is set but cannot be opened.
    pub fn from_env(user_webid: WebID, embedding_model: String) -> Result<Option<Self>, String> {
        let db_path = match std::env::var("HKASK_DB_PATH") {
            Ok(p) if !p.trim().is_empty() => p,
            _ => return Ok(None),
        };

        let passphrase = hkask_keystore::keychain::resolve_db_passphrase_string()
            .map_err(|e| e.to_string())?
            .to_string();

        let port = Self::new(&db_path, &passphrase, user_webid, embedding_model)?;
        tracing::info!(
            target: "reg.memory",
            db_path = %db_path,
            "RealMemoryPort initialized — turns will be stored in episodic + semantic memory"
        );
        Ok(Some(port))
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
            let vectors = self
                .embedding_router
                .embed_sentences(&self.embedding_model, &[user_input.as_str()])
                .await;

            match vectors {
                Ok(vectors) => {
                    if let Some(vector) = vectors.into_iter().next() {
                        if let Err(e) = self.semantic.store_embedding(
                            &embedding_entity,
                            &vector,
                            &self.embedding_model,
                        ) {
                            tracing::warn!(
                                target: "reg.memory",
                                thread_id = %thread_id,
                                error = %e,
                                "Failed to store prompt embedding"
                            );
                            // Non-fatal — the h_mem records are the primary store.
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        error = %e,
                        "Failed to embed user prompt — embedding-based recall will not work for this turn"
                    );
                    // Non-fatal — entity-based recall still works.
                }
            }

            tracing::info!(
                target: "reg.memory",
                thread_id = %thread_id,
                model = %model,
                "Turn ingested into episodic + semantic memory"
            );

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
            let vectors = self
                .embedding_router
                .embed_sentences(&self.embedding_model, &[query])
                .await;

            if let Ok(vectors) = vectors {
                if let Some(query_vector) = vectors.into_iter().next() {
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
                let entity = format!("chat:thread:");
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
        let driver: Arc<dyn hkask_storage::DatabaseDriver> = SqliteDriver::in_memory_driver();
        let h_mem_store = HMemStore::from_driver(Arc::clone(&driver));
        let episodic = EpisodicMemory::new(h_mem_store);

        let h_mem_store2 = HMemStore::from_driver(Arc::clone(&driver));
        let embedding_store = EmbeddingStore::from_driver(driver, 1024);
        let semantic = SemanticMemory::new(h_mem_store2, embedding_store);

        // EmbeddingRouter needs InferenceConfig, but we won't call embed in tests
        let inference_config = InferenceConfig::from_env();
        let embedding_router = EmbeddingRouter::new(inference_config);

        RealMemoryPort {
            episodic,
            semantic,
            embedding_router,
            embedding_model: "test-model".to_string(),
            user_webid: test_webid(),
            curator_webid: WebID::from_persona(b"Curator"),
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
}
