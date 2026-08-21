//! Turn ingestion write path — episodic + semantic h_mem writes + embedding.
//!
//! Extracted from `RealMemoryPort::ingest_turn` (deep-module split, bridge-audit
//! BD-04 continuation). The port impl now holds only the ingestion semaphore and
//! delegates the actual writes here. This gives the write path a named home so
//! understanding "how a completed turn becomes a curator-accessible semantic
//! h_mem" no longer requires scrolling across the 2700-line `memory.rs`.
//!
//! The write path is a pure transformation of `(store handles, TurnRecord)` into
//! side effects — no new trait, no new ownership. `write_turn` borrows the port's
//! fields via [`WriteContext`].

use std::sync::Arc;

use hkask_memory::{MemoryConsolidator, MemoryStore};
use hkask_storage::HMem;
use hkask_types::{HMemOntology, MemoryError, TurnRecord, Visibility, WebID};
use std::sync::RwLock;

use crate::inference::LanguageModelEmbeddingPort;

use super::curator_stores::{CuratorStore, build_curator_consolidation};

/// Borrowed handles for a single turn write. Constructed by
/// `RealMemoryPort::ingest_turn` from its own fields; tests construct one
/// directly from in-memory stores without going through `RealMemoryPort::new`
/// (no DB open, no passphrase, no consolidation timer).
pub(crate) struct WriteContext<'a> {
    pub store: &'a MemoryStore,
    pub curator_store: &'a CuratorStore,
    pub embedding_port: &'a LanguageModelEmbeddingPort,
    pub embedding_model: &'a str,
    pub user_webid: WebID,
    pub curator_webid: WebID,
    pub tokio_handle: &'a tokio::runtime::Handle,
    /// Self-healing curator consolidation service — rebuilt here after a
    /// curator-store heal so the timer promotes freshly-ingested curator h_mems.
    /// Behind an `Arc` shared with the timer, which re-reads it on each tick.
    pub curator_consolidation: &'a Arc<RwLock<Option<Arc<MemoryConsolidator>>>>,
    pub consolidation_cadence_secs: u64,
}

/// Write a completed turn into episodic + semantic memory.
///
/// Performs, in order:
/// 1. Curator-store self-heal re-open + consolidation rebuild (if healed).
/// 2. User-perspective episodic h_mem (Private) to the user's `memory.db` —
///    every turn, user or curator.
/// 3. If it's a curator turn: curator-perspective episodic h_mem (Private) to
///    the curator's `curator.db`.
/// 4. Shared semantic copy to the curator's `curator.db` — every turn, so
///    `curator_memory_recall` / `curator_semantic_search` see every turn the
///    agent observed.
/// 5. Embed the user prompt and store it to the user's store (always) and the
///    curator's store (curator turns only), for KNN-based semantic recall.
///
/// The embedding's `entity_ref` MUST match the episodic h_mem's `entity`
/// (`chat:thread:{thread_id}`) so the recall path's
/// `query_deduped_untouched(entity_ref)` joins the KNN neighbor back to the
/// h_mem holding the full turn text. See the
/// `recall_context_finds_turn_by_embedding_only` test for the end-to-end pin.
///
/// `Ok(())` on success; the only hard error is a failed user episodic store
/// (`MemoryError::Ingestion`). Curator-side and embedding failures are
/// non-fatal — they warn and continue, since the user's episodic record is the
/// primary store.
pub(crate) async fn write_turn(
    ctx: &WriteContext<'_>,
    record: TurnRecord,
) -> Result<(), MemoryError> {
    let thread_id = record.thread_id.clone();
    let user_input = record.user_input.clone();
    let agent_response = record.agent_response.clone();
    let model = record.model.clone();
    let title = record.thread_title.clone();
    let agent_id = record.agent_id.clone();
    let is_curator_turn = agent_id.as_deref() == Some("Curator");

    let turn_value = serde_json::json!({
        "user_input": user_input,
        "agent_response": agent_response,
        "model": model,
        "title": title,
    });

    // Resolve the curator stores once per ingestion — `get()` re-attempts the
    // open when they're down (self-healing) and signals persistent failure with
    // a warn-once, so the writes below can treat `None` as "already signaled, skip".
    let curator_store = ctx.curator_store.get();
    // Rebuild the curator consolidation service after a heal so the timer
    // promotes freshly-ingested curator h_mems.
    if curator_store.is_some() {
        let needs_rebuild = match ctx.curator_consolidation.read() {
            Ok(guard) => guard.is_none(),
            Err(_) => true,
        };
        if needs_rebuild && ctx.consolidation_cadence_secs > 0 {
            let rebuilt =
                build_curator_consolidation(ctx.consolidation_cadence_secs, &curator_store);
            if let Ok(mut guard) = ctx.curator_consolidation.write()
                && guard.is_none()
            {
                *guard = rebuilt;
            }
        }
    }

    // ── 1. User-perspective episodic h_mem (Private) — EVERY turn ──────────
    //
    // Both user turns and curator turns are conversations the USER participated
    // in, so both land in the user's `memory.db` as the user's first-person
    // record. Pre-dual-write, curator turns were written only to the curator's
    // sovereign DB — the user had no episodic record of their own curator
    // conversations.
    let entity = format!("chat:thread:{thread_id}");
    // Process-axis anchoring (P5.4): a chat turn is a PKO step execution of the
    // `chat` procedure. `dc_source` carries the thread as the session
    // provenance so recall can distinguish turns by conversation without
    // re-parsing the entity string.
    let episodic_ontology = HMemOntology::episodic("chat", "turn", format!("session:{thread_id}"));
    let episodic_h_mem = HMem::new(
        &entity,
        "chatted",
        serde_json::Value::String(turn_value.to_string()),
        ctx.user_webid,
    )
    .with_perspective(ctx.user_webid)
    .with_visibility(Visibility::Private)
    .with_ontology(episodic_ontology);

    if let Err(e) = ctx.store.store(episodic_h_mem) {
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

    // ── 2. Curator-side writes — branch on whose turn it is ───────────────
    if is_curator_turn {
        // Curator-perspective episodic h_mem (Private, `curator_webid`) in
        // `agents/curator/curator.db` — the curator's own first-person record of
        // the same conversation, mirroring the user's record above. Together they
        // give each party a first-person memory of the shared conversation from
        // their own perspective.
        let episodic_h_mem = HMem::new(
            &entity,
            "chatted",
            serde_json::Value::String(turn_value.to_string()),
            ctx.curator_webid,
        )
        .with_perspective(ctx.curator_webid)
        .with_visibility(Visibility::Private)
        .with_ontology(HMemOntology::episodic(
            "chat",
            "turn",
            format!("session:{thread_id}"),
        ));

        if let Some(ref curator_store) = curator_store {
            if let Err(e) = curator_store.store(episodic_h_mem) {
                tracing::warn!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    error = %e,
                    "Failed to store curator episodic h_mem — \
                     curator will not recall this turn as experience"
                );
                // Non-fatal — fall through to semantic copy.
            }
        } else {
            // Store unavailability is already signaled (error at construction,
            // warn-once per heal attempt) — no additional per-turn log here.
            tracing::trace!(
                target: "reg.memory",
                thread_id = %thread_id,
                "Curator store unavailable — skipping curator episodic write"
            );
        }
    }

    // Shared semantic copy in the curator's DB — written for BOTH turn kinds so
    // `curator_memory_recall` / `curator_semantic_search` see every turn the
    // agent has observed, regardless of speaker.
    let curator_entity = format!("curator:thread:{thread_id}");
    // State-axis anchoring (P5.4): the curator copy is a document the curator
    // holds about the conversation, not a step it executed. `bibo:Document` is
    // the BIBO type for a standalone record.
    let curator_ontology =
        HMemOntology::semantic("bibo:Document", vec!["chat_turn".to_string()], "curator");
    let curator_h_mem = HMem::new(
        &curator_entity,
        "turn",
        serde_json::Value::String(turn_value.to_string()),
        ctx.curator_webid,
    )
    .with_visibility(Visibility::Shared)
    .with_ontology(curator_ontology);

    if let Some(ref curator_store) = curator_store {
        if let Err(e) = curator_store.store(curator_h_mem) {
            tracing::warn!(
                target: "reg.memory",
                thread_id = %thread_id,
                error = %e,
                "Failed to store curator semantic h_mem — \
                 curator memory will not include this turn"
            );
            // Non-fatal — the episodic record is the primary store.
        }
    } else {
        tracing::trace!(
            target: "reg.memory",
            thread_id = %thread_id,
            "Curator store unavailable — skipping curator copy"
        );
    }

    // ── 3. Embed the user prompt for future retrieval ─────────────────────
    //
    // The embedding enables semantic search (KNN) for context injection.
    // Written to the user's semantic store always; for curator turns, also
    // written to the curator's semantic store so the curator can recall its own
    // turns by similarity.
    //
    // A separate `embedding:thread:...` namespace was dead code — no h_mem was
    // ever stored under it, so the semantic recall leg always returned zero
    // snippets.
    let embedding_entity = entity.clone();
    // Spawn the embedding HTTP call on the tokio runtime so the GPUI-side
    // channel task (which holds the AsyncApp) can resolve credentials and make
    // the HTTP call. The rest of the write path doesn't need tokio.
    let embedding_model = ctx.embedding_model.to_string();
    let embedding_port = ctx.embedding_port.clone();
    let user_input_owned = user_input.clone();
    let vectors = ctx
        .tokio_handle
        .spawn(async move {
            embedding_port
                .embed(&embedding_model, &[user_input_owned])
                .await
        })
        .await;

    match vectors {
        Ok(Ok(vectors)) => {
            if let Some(vector) = vectors.into_iter().next() {
                if let Err(e) =
                    ctx.store
                        .store_embedding(&embedding_entity, &vector, ctx.embedding_model)
                {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        error = %e,
                        "Failed to store prompt embedding"
                    );
                }
                if is_curator_turn
                    && let Some(ref curator_store) = curator_store
                    && let Err(e) = curator_store.store_embedding(
                        &embedding_entity,
                        &vector,
                        ctx.embedding_model,
                    )
                {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        error = %e,
                        "Failed to store curator prompt embedding"
                    );
                }
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
        is_curator_turn,
        "Turn ingested into episodic + semantic memory"
    );

    // Consolidation is no longer fired from the ingestion path. It runs on a
    // dedicated background timer (see `start_consolidation_timer`) so ingestion
    // completes quickly and consolidation doesn't contend with the recall path
    // or hold the ingestion semaphore.

    Ok(())
}
