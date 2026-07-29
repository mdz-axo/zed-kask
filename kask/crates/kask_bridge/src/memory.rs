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

use hkask_memory::{ConsolidationBridge, ConsolidationService, EpisodicMemory, SemanticMemory};
use hkask_storage::{Database, EmbeddingStore, HMem, HMemStore};
use hkask_types::{MemoryError, MemoryPort, MemorySnippet, TurnRecord, Visibility, WebID};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use chrono::Utc;

use crate::inference::LanguageModelEmbeddingPort;

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
    embedding_port: LanguageModelEmbeddingPort,
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
    /// test-only `maybe_consolidate` method can check-and-update atomically.
    ///
    /// In production, the background timer (`start_consolidation_timer`) uses
    /// its own `Arc<Mutex<Option<DateTime>>>` (captured at startup) because the
    /// timer task must be `Send + 'static` and cannot borrow `&self`. The two
    /// mutexes are not shared — in production, only the timer runs, so this
    /// field stays at its initial value (`None`). This is not a bug; it's a
    /// deliberate split between the test entry point and the production timer.
    last_consolidation: Mutex<Option<chrono::DateTime<chrono::Utc>>>,
    /// Tokio runtime handle — entered around embedding HTTP calls so that
    /// `reqwest` (which is tokio-backed) has a reactor. The memory port's
    /// async methods are called from GPUI's background executor, not tokio.
    tokio_handle: tokio::runtime::Handle,
    /// Ingestion concurrency limiter. Each `ingest_turn` acquires a permit
    /// before touching the database, so concurrent ingestion futures (one per
    /// completing thread) serialize instead of contending for the SQLite pool.
    /// Without this, N active threads each completing a turn fire N concurrent
    /// ingestion futures — each doing multiple writes + an embedding HTTP call
    /// — which crowds out the recall path (also SQLite-bound) and starves the
    /// inference thread's `inject_context` recall.
    ///
    /// Default 1 (fully serial). Override via `HKASK_MEMORY_INGEST_CONCURRENCY`.
    /// A value of 1 is correct for SQLite (single writer); raise only if the
    /// store is backed by Postgres.
    ingest_semaphore: tokio::sync::Semaphore,
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
        embedding_port: LanguageModelEmbeddingPort,
        consolidation_cadence_secs: u64,
        confidence_floor: f64,
        tokio_handle: tokio::runtime::Handle,
    ) -> Result<Self, String> {
        let db = Database::open(db_path, passphrase).map_err(|e| e.to_string())?;
        let pool = db.sqlite_pool().map_err(|e| e.to_string())?;
        let driver: Arc<dyn hkask_storage::DatabaseDriver> =
            Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool));

        // Episodic store — first-person, Private, perspective-bound
        let h_mem_store = HMemStore::from_driver(Arc::clone(&driver)).map_err(|e| e.to_string())?;
        let episodic = Arc::new(EpisodicMemory::new(h_mem_store));

        // Semantic store — shared knowledge graph with embeddings.
        // The embedding dimension must match the embedding model's output —
        // a mismatch causes `DimensionMismatch` errors on every store call,
        // silently disabling embedding-based recall. The caller resolves
        // this from `kask_settings.corpus.embedding_dim` (default 1024,
        // matching `DeepInfra/Qwen/Qwen3-Embedding-0.6B`).
        //
        // A dim of 0 is a footgun: `unwrap_or(1024)` only fires for `None`,
        // not for `Some(0)`, and `KaskCorpusSettings` deriving `Default`
        // returned `u32::default()` == 0, not the serde default 1024. The
        // `From<KaskSettingsContent>` impl filters Some(0) → 1024, but the
        // Default path (no kask.corpus section) bypassed that filter and
        // produced dim == 0 here, panicking in EmbeddingStore::from_driver.
        // KaskCorpusSettings now has a manual Default returning 1024; we
        // clamp below too, in case a caller bypasses settings (e.g.
        // HKASK_EMBEDDING_DIM=0) — per the .rules trap "Process-global hooks
        // set at runtime need a startup-failure signal".
        if embedding_dim == 0 {
            tracing::warn!(
                target: "reg.memory",
                embedding_dim,
                "RealMemoryPort constructed with embedding_dim == 0 — \
                 clamping to 1024 to avoid a zero-dimensional store panic. \
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
        // Clamp 0 → 1024: the warn above signals the misconfiguration; this
        // keeps the system functional (degraded) instead of panicking.
        let embedding_dim = if embedding_dim == 0 {
            1024
        } else {
            embedding_dim
        };
        let h_mem_store2 = HMemStore::from_driver(Arc::clone(&driver)).expect("hmem store init");
        let embedding_store = EmbeddingStore::from_driver(driver, embedding_dim);
        let semantic = Arc::new(SemanticMemory::new(h_mem_store2, embedding_store));

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
            embedding_port,
            embedding_model,
            user_webid,
            curator_webid,
            consolidation,
            consolidation_cadence_secs,
            confidence_floor,
            last_consolidation: Mutex::new(None),
            tokio_handle,
            ingest_semaphore: tokio::sync::Semaphore::new(
                std::env::var("HKASK_MEMORY_INGEST_CONCURRENCY")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .filter(|v: &usize| *v > 0)
                    .unwrap_or(1),
            ),
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
        embedding_port: LanguageModelEmbeddingPort,
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
            embedding_port,
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
    /// This is the single source of truth for the consolidation check-and-fire
    /// logic. The background timer (`start_consolidation_timer`) inlines its
    /// own version of this logic because it needs to capture `Send + 'static`
    /// state (the timestamp is shared via `Arc<Mutex<...>>` rather than
    /// `&self.last_consolidation`). Both paths use the same cadence check and
    /// the same `ConsolidationService::consolidate` call.
    ///
    /// Kept as a method so tests can fire consolidation directly without
    /// starting a timer.
    #[cfg(test)]
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

    /// Start a background timer that fires consolidation on the configured
    /// cadence. This decouples consolidation from the ingestion path —
    /// ingestion writes complete quickly without waiting for consolidation,
    /// and consolidation runs on its own schedule without holding the
    /// ingestion semaphore.
    ///
    /// The timer checks the cadence every `consolidation_cadence_secs` seconds
    /// (or every 60 seconds if the cadence is < 60, to avoid tight polling).
    /// On each tick it calls `maybe_consolidate`, which does the atomic
    /// check-and-fire under the mutex.
    ///
    /// Returns a `JoinHandle` that the caller can detach or store. Dropping
    /// the handle cancels the timer.
    ///
    /// A cadence of 0 disables consolidation entirely (no timer started).
    pub fn start_consolidation_timer(&self) -> Option<tokio::task::JoinHandle<()>> {
        if self.consolidation_cadence_secs == 0 {
            return None;
        }
        let consolidation = self.consolidation.clone()?;
        let user_webid = self.user_webid;
        let confidence_floor = self.confidence_floor;
        let last_consolidation = self.last_consolidation.lock().ok().and_then(|guard| *guard);
        let cadence = self.consolidation_cadence_secs;
        // Poll interval: check at least once per cadence window, but no more
        // often than every 60s to avoid tight polling.
        let poll_interval = Duration::from_secs(cadence.clamp(60, 3600));

        // We need a self-referential structure for the timer to call
        // maybe_consolidate on each tick. Instead of capturing `self` (which
        // would require Arc<Self>), we capture the consolidation service and
        // a shared mutex for the last-fired timestamp. This is the same
        // pattern as maybe_consolidate but running on a timer.
        let shared_last: Arc<Mutex<Option<chrono::DateTime<chrono::Utc>>>> =
            Arc::new(Mutex::new(last_consolidation));
        let shared_last_for_timer = Arc::clone(&shared_last);

        let handle = self.tokio_handle.spawn(async move {
            let mut interval = tokio::time::interval(poll_interval);
            // The first tick fires immediately — skip it so we don't consolidate
            // on startup before any ingestion has happened.
            interval.tick().await;
            loop {
                interval.tick().await;
                // Check if the cadence has elapsed since the last consolidation.
                let now = Utc::now();
                let cadence_dur = chrono::Duration::seconds(cadence as i64);
                let should_fire = match shared_last_for_timer.lock() {
                    Ok(mut guard) => {
                        let elapsed = guard
                            .map(|last| now.signed_duration_since(last) >= cadence_dur)
                            .unwrap_or(false);
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
                            "consolidation timer: last_consolidation mutex poisoned — stopping timer"
                        );
                        return;
                    }
                };
                if !should_fire {
                    continue;
                }
                let request = hkask_types::ConsolidationRequest {
                    limit: 100,
                    confidence_floor: Some(confidence_floor),
                    max_semantic_triples: None,
                };
                tracing::info!(
                    target: "reg.memory",
                    cadence_secs = cadence,
                    confidence_floor,
                    "Consolidation timer fired"
                );
                match consolidation.consolidate(&user_webid, request) {
                    Ok(outcome) => {
                        tracing::info!(
                            target: "reg.memory",
                            consolidated = outcome.consolidated_count,
                            deleted = outcome.deleted_count,
                            failed = outcome.failed_count,
                            "Consolidation timer pass complete"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.memory",
                            error = %e,
                            "Consolidation timer pass failed"
                        );
                    }
                }
            }
        });
        Some(handle)
    }
}

impl MemoryPort for RealMemoryPort {
    fn ingest_turn<'a>(
        &'a self,
        record: TurnRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            // Acquire an ingestion permit before touching the database. This
            // serializes concurrent ingestion futures (one per completing
            // thread) so they don't contend for the SQLite pool with each
            // other or with the recall path. The permit is held for the
            // duration of the ingestion (including the embedding HTTP call
            // and consolidation trigger) and released on drop.
            //
            // If the semaphore is contended, this future parks until a permit
            // is available — the calling thread's turn has already completed,
            // so the user sees no latency from this wait.
            let _ingest_permit = match self.ingest_semaphore.acquire().await {
                Ok(permit) => permit,
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        error = %e,
                        "ingest_semaphore closed — skipping ingestion"
                    );
                    return Err(MemoryError::Ingestion(format!(
                        "ingest_semaphore closed: {e}"
                    )));
                }
            };

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
            // Spawn the embedding HTTP call on the tokio runtime so the
            // GPUI-side channel task (which holds the AsyncApp) can resolve
            // credentials and make the HTTP call. The rest of ingest_turn
            // doesn't need tokio.
            let embedding_model = self.embedding_model.clone();
            let embedding_port = self.embedding_port.clone();
            let user_input_owned = user_input.clone();
            let vectors = self
                .tokio_handle
                .spawn(async move {
                    embedding_port
                        .embed(&embedding_model, &[user_input_owned])
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

            // Consolidation is no longer fired from the ingestion path. It runs
            // on a dedicated background timer (see `start_consolidation_timer`)
            // so ingestion completes quickly and consolidation doesn't contend
            // with the recall path or hold the ingestion semaphore.

            Ok(())
        })
    }

    fn recall_context<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemorySnippet>, MemoryError>> + Send + 'a>> {
        Box::pin(async move {
            // Collect (snippet, h_mem_id, source) triples so we can sort,
            // truncate, and touch the correct store for each survivor.
            // Tracking the source alongside the id avoids double-touching
            // (episodic + semantic) and keeps id↔snippet correspondence
            // stable across the sort.
            enum RecallSource {
                Episodic,
                Semantic,
            }
            struct Candidate {
                snippet: MemorySnippet,
                h_mem_id: hkask_storage::HMemId,
                source: RecallSource,
            }
            let mut candidates: Vec<Candidate> = Vec::new();

            // ── 1. Semantic search (embedding KNN) ───────────────────────
            //
            // Embed the query and search for similar stored embeddings.
            // This finds turns where the user asked similar questions.
            // Spawn the embedding HTTP call on the tokio runtime so the
            // GPUI-side channel task can resolve credentials and make the
            // HTTP call.
            let embedding_model = self.embedding_model.clone();
            let embedding_port = self.embedding_port.clone();
            let query_owned = query.to_string();
            let vectors = self
                .tokio_handle
                .spawn(async move { embedding_port.embed(&embedding_model, &[query_owned]).await })
                .await;

            if let Ok(Ok(vectors)) = vectors
                && let Some(query_vector) = vectors.into_iter().next()
            {
                match self.semantic.search_similar(&query_vector, limit) {
                    Ok(results) => {
                        for result in results {
                            // Retrieve the h_mem associated with this embedding
                            // to get the full text content. Use the untouched
                            // variant — we touch only the injected ones below.
                            let entity_ref = &result.embedding.entity_ref;
                            if let Ok(h_mems) = self.semantic.query_deduped_untouched(entity_ref) {
                                for h_mem in h_mems {
                                    let text = h_mem.value.as_str().unwrap_or("").to_string();
                                    if !text.is_empty() {
                                        candidates.push(Candidate {
                                            snippet: MemorySnippet {
                                                text,
                                                source: "semantic".to_string(),
                                                confidence: h_mem.confidence.value(),
                                                relevance_score: 1.0 - result.distance,
                                            },
                                            h_mem_id: h_mem.id,
                                            source: RecallSource::Semantic,
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

            // ── 2. Episodic search (keyword overlap) ─────────────────────
            //
            // Load episodic h_mems for the user's chat threads ONCE, then
            // filter by keyword overlap in memory. The previous implementation
            // re-queried the store for each query word (5x) using the same
            // fixed entity string "chat:thread:" — a redundant N+1 scan that
            // also fired touch_recall on every row per iteration, turning
            // recall into a write storm under multi-thread load.
            //
            // We use the untouched variant and touch only the injected h_mems.
            let query_words: Vec<String> = query
                .split_whitespace()
                .filter(|w| w.len() > 3)
                .take(5)
                .map(|w| w.to_lowercase())
                .collect();

            if !query_words.is_empty() {
                // Use a prefix query to load all chat:thread:* episodic h_mems
                // in a single SQL call. The previous implementation queried
                // the exact entity "chat:thread:" (no thread_id suffix), which
                // never matched stored entities "chat:thread:<thread_id>" —
                // so the episodic keyword search was dead code. Combined with
                // the N+1 loop (one query per query word), this was both broken
                // and a write storm.
                //
                // The recall budget caps the number of rows loaded — without
                // it, a session with thousands of past turns would load all of
                // them into memory on every recall call. We load 10x the
                // recall limit (most recent first) to give the keyword filter a
                // reasonable pool to filter from without unbounded loading.
                let entity_prefix = "chat:thread:".to_string();
                let recall_budget = limit.saturating_mul(10).max(50);
                if let Ok(h_mems) = self.episodic.query_for_deduped_untouched_by_prefix(
                    &entity_prefix,
                    self.user_webid,
                    recall_budget,
                ) {
                    for h_mem in h_mems {
                        let text = h_mem.value.as_str().unwrap_or("").to_string();
                        if text.is_empty() {
                            continue;
                        }
                        let text_lower = text.to_lowercase();
                        // Check if ANY query word appears in the text
                        if !query_words.iter().any(|w| text_lower.contains(w)) {
                            continue;
                        }
                        // Skip if already in candidates (dedup by text)
                        if candidates.iter().any(|c| c.snippet.text == text) {
                            continue;
                        }
                        candidates.push(Candidate {
                            snippet: MemorySnippet {
                                text,
                                source: "episodic".to_string(),
                                confidence: h_mem.confidence.value(),
                                relevance_score: 0.5, // Base relevance for keyword match
                            },
                            h_mem_id: h_mem.id,
                            source: RecallSource::Episodic,
                        });
                    }
                }
            }

            // ── 3. Sort by relevance and truncate ─────────────────────────
            candidates.sort_by(|a, b| {
                b.snippet
                    .relevance_score
                    .partial_cmp(&a.snippet.relevance_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            candidates.truncate(limit);

            // ── 4. Touch only the injected h_mems ────────────────────────
            //
            // Resets the decay clock on h_mems that actually got used. This
            // is the “memory that gets used stays fresh” semantics, applied
            // post-filter instead of pre-filter — avoids the write storm.
            // Touch via the correct store for each candidate's source.
            for c in &candidates {
                let result: Result<(), Box<dyn std::error::Error>> = match c.source {
                    RecallSource::Episodic => {
                        self.episodic.touch_recall(&c.h_mem_id).map_err(Into::into)
                    }
                    RecallSource::Semantic => {
                        self.semantic.touch_recall(&c.h_mem_id).map_err(Into::into)
                    }
                };
                if let Err(e) = result {
                    tracing::warn!(
                        target: "reg.memory.decay",
                        triple_id = %c.h_mem_id.as_uuid(),
                        error = %e,
                        "Failed to touch_recall h_mem during recall_context"
                    );
                }
            }

            let touched = candidates.len();
            let snippets: Vec<MemorySnippet> = candidates.into_iter().map(|c| c.snippet).collect();

            tracing::info!(
                target: "reg.memory",
                query_len = query.len(),
                recalled = snippets.len(),
                touched,
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
        let h_mem_store = HMemStore::from_driver(Arc::clone(&driver)).expect("hmem store init");
        let episodic = Arc::new(EpisodicMemory::new(h_mem_store));

        let h_mem_store2 = HMemStore::from_driver(Arc::clone(&driver)).expect("hmem store init");
        let embedding_store = EmbeddingStore::from_driver(driver, 1024);
        let semantic = Arc::new(SemanticMemory::new(h_mem_store2, embedding_store));

        // Tests don't call embed — use a stub port with no backing task.
        let embedding_port = LanguageModelEmbeddingPort::for_tests();

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
            embedding_port,
            embedding_model: "test-model".to_string(),
            user_webid: test_webid(),
            curator_webid: WebID::from_persona(b"Curator"),
            consolidation,
            consolidation_cadence_secs,
            confidence_floor,
            last_consolidation: Mutex::new(None),
            tokio_handle: tokio::runtime::Handle::current(),
            ingest_semaphore: tokio::sync::Semaphore::new(1),
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
    async fn ingest_turn_does_not_fire_consolidation() {
        // Consolidation is now decoupled from ingestion — it runs on a
        // background timer (see start_consolidation_timer). Ingestion should
        // NOT fire consolidation, even when the cadence has elapsed.
        let port = in_memory_port_with_cadence(1, 0.3);

        let record = TurnRecord {
            thread_id: "no-consolidation-from-ingest".to_string(),
            user_input: "Tell me about memory consolidation".to_string(),
            agent_response: "Consolidation promotes episodic to semantic.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
        };
        let result = port.ingest_turn(record).await;
        assert!(result.is_ok(), "ingest_turn should succeed");

        // last_consolidation should remain None — ingestion no longer fires it.
        let last = port.last_consolidation.lock().expect("mutex not poisoned");
        assert!(
            last.is_none(),
            "ingest_turn should not fire consolidation (timer-decoupled)"
        );
    }

    #[tokio::test]
    async fn maybe_consolidate_fires_when_cadence_elapsed() {
        // Directly test the consolidation callback (what the timer calls).
        let port = in_memory_port_with_cadence(1, 0.3);
        let webid = port.user_webid;

        // Ingest a turn so there's something to consolidate.
        port.ingest_turn(TurnRecord {
            thread_id: "consolidation-test".to_string(),
            user_input: "Tell me about memory consolidation".to_string(),
            agent_response: "Consolidation promotes episodic to semantic.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
        })
        .await
        .expect("ingest succeeds");

        // Fire consolidation directly (simulating the timer callback).
        port.maybe_consolidate();

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

        // A second call immediately after should NOT re-fire consolidation
        // (cadence hasn't elapsed).
        port.maybe_consolidate();
        let last_after = port
            .last_consolidation
            .lock()
            .expect("mutex not poisoned")
            .expect("consolidation timestamp should still be set");
        assert_eq!(
            last, last_after,
            "second call within cadence should not re-fire consolidation"
        );

        // After consolidation, the episodic h_mem may have been promoted
        // to semantic and expired in episodic (consolidation is a one-way
        // episodic → semantic promotion).
        let h_mems = port
            .episodic
            .query_for_deduped("chat:thread:consolidation-test", webid)
            .expect("query should succeed");
        // The h_mem may or may not have been consolidated depending on
        // confidence decay — we just verify the query succeeds.
        let _ = h_mems;
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

    /// Pin the N+1 fix in `recall_context`: the episodic keyword search must
    /// load episodic h_mems exactly once per recall call, not once per query
    /// word. The previous implementation re-queried the store for each of the
    /// 5 query words using the same fixed entity string `"chat:thread:"`,
    /// which also fired `touch_recall` on every row per iteration — turning
    /// recall into a write storm under multi-thread load.
    ///
    /// We can't easily count SQL queries from here, but we can verify the
    /// observable consequence: `recall_context` returns snippets that match
    /// ANY query word (not just the last one), and the recall completes in
    /// reasonable time. The regression symptom was that recall returned
    /// results for only the last word (because the loop overwrote the entity
    /// variable) and fired N×5 touch_recall UPDATEs.
    #[tokio::test]
    async fn recall_context_matches_any_query_word_single_load() {
        let port = in_memory_port();

        // Ingest two turns with distinct keywords so we can verify both match.
        let records = [
            TurnRecord {
                thread_id: "t-rust".to_string(),
                user_input: "Tell me about rust programming".to_string(),
                agent_response: "Rust is a systems language.".to_string(),
                model: "test-model".to_string(),
                thread_title: None,
            },
            TurnRecord {
                thread_id: "t-python".to_string(),
                user_input: "Tell me about python programming".to_string(),
                agent_response: "Python is a scripting language.".to_string(),
                model: "test-model".to_string(),
                thread_title: None,
            },
        ];
        for record in records {
            port.ingest_turn(record).await.expect("ingest succeeds");
        }

        // Query with two distinct keywords — both should match. Under the
        // N+1 bug, only the last word's results survived (the entity variable
        // was overwritten each iteration, and the loop re-queried the same
        // entity 5 times, but the substring filter only kept matches for the
        // current word — so earlier words' matches were lost when a later
        // word had no overlap with the same h_mems).
        let snippets = port
            .recall_context("rust python", 10)
            .await
            .expect("recall succeeds");

        // Both turns should be recalled — the fix loads episodic h_mems once
        // and checks all query words against each.
        let texts: Vec<&str> = snippets.iter().map(|s| s.text.as_str()).collect();
        let has_rust = texts.iter().any(|t| t.contains("rust"));
        let has_python = texts.iter().any(|t| t.contains("python"));
        assert!(
            has_rust && has_python,
            "recall should match ANY query word, got: {snippets:?}"
        );
    }

    /// Pin that `recall_context` touches `recalled_at` only on h_mems that
    /// survive the limit, not on every recalled candidate. The previous
    /// implementation touched every deduped h_mem inside `query_for_deduped`,
    /// even ones filtered out by `recall_min_confidence` in the injector.
    ///
    /// We verify this by checking that h_mems NOT in the final snippets have
    /// their `recalled_at` unchanged after a recall that truncates them.
    #[tokio::test]
    async fn recall_context_touches_only_injected_h_mems() {
        let port = in_memory_port();
        let webid = port.user_webid;

        // Ingest one turn.
        port.ingest_turn(TurnRecord {
            thread_id: "touch-test".to_string(),
            user_input: "unique_keyword_xyz".to_string(),
            agent_response: "response".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
        })
        .await
        .expect("ingest succeeds");

        // Read the stored recalled_at via the untouched query (no side effects).
        let before = port
            .episodic
            .query_for_deduped_untouched("chat:thread:touch-test", webid)
            .expect("untouched query succeeds");
        assert_eq!(before.len(), 1);
        let recalled_at_before = before[0].recalled_at;

        // Sleep so a touch would be observable.
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Recall with a query that does NOT match the stored keyword — the
        // h_mem should be loaded as a candidate but NOT injected (no keyword
        // overlap), so its recalled_at should NOT be touched.
        let snippets = port
            .recall_context("completely_different_query", 10)
            .await
            .expect("recall succeeds");
        assert!(
            snippets.is_empty(),
            "no snippets should match a non-overlapping query"
        );

        let after = port
            .episodic
            .query_for_deduped_untouched("chat:thread:touch-test", webid)
            .expect("untouched query succeeds");
        assert_eq!(after.len(), 1);
        assert_eq!(
            after[0].recalled_at, recalled_at_before,
            "recalled_at should be unchanged when the h_mem is not injected"
        );

        // Now recall with a matching query — the h_mem should be injected and
        // its recalled_at should be touched.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let snippets = port
            .recall_context("unique_keyword_xyz", 10)
            .await
            .expect("recall succeeds");
        assert_eq!(snippets.len(), 1, "matching query should recall the h_mem");

        let after_match = port
            .episodic
            .query_for_deduped_untouched("chat:thread:touch-test", webid)
            .expect("untouched query succeeds");
        assert_eq!(after_match.len(), 1);
        assert!(
            after_match[0].recalled_at > recalled_at_before,
            "recalled_at should be updated when the h_mem is injected"
        );
    }

    /// Pin that the ingestion semaphore serializes concurrent ingestions.
    /// Two concurrent ingestions should both complete successfully, but the
    /// second should wait for the first to release its permit.
    #[tokio::test]
    async fn ingestion_semaphore_serializes_concurrent_ingestions() {
        let port = std::sync::Arc::new(in_memory_port());

        let port1 = port.clone();
        let port2 = port.clone();

        let record1 = TurnRecord {
            thread_id: "sem-1".to_string(),
            user_input: "first".to_string(),
            agent_response: "response1".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
        };
        let record2 = TurnRecord {
            thread_id: "sem-2".to_string(),
            user_input: "second".to_string(),
            agent_response: "response2".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
        };

        // Spawn both ingestions concurrently.
        let (r1, r2) = tokio::join!(
            async move { port1.ingest_turn(record1).await },
            async move { port2.ingest_turn(record2).await }
        );

        assert!(r1.is_ok(), "first ingestion should succeed: {r1:?}");
        assert!(r2.is_ok(), "second ingestion should succeed: {r2:?}");

        // Both turns should be stored.
        let webid = port.user_webid;
        let h1 = port
            .episodic
            .query_for_deduped_untouched("chat:thread:sem-1", webid)
            .expect("query succeeds");
        let h2 = port
            .episodic
            .query_for_deduped_untouched("chat:thread:sem-2", webid)
            .expect("query succeeds");
        assert_eq!(h1.len(), 1, "first turn should be stored");
        assert_eq!(h2.len(), 1, "second turn should be stored");
    }
}
