//! Turn ingestion write path — h_mem write + embedding.
//!
//! Extracted from `RealMemoryPort::ingest_turn` (deep-module split, bridge-audit
//! BD-04 continuation). The port impl now holds only the ingestion semaphore and
//! delegates the actual writes here. This gives the write path a named home.
//!
//! The write path is a pure transformation of `(store handles, TurnRecord)` into
//! side effects — no new trait, no new ownership. `write_turn` borrows the port's
//! fields via [`WriteContext`].

use std::sync::Arc;

use hkask_memory::MemoryConsolidator;
use hkask_storage::HMem;
use hkask_types::{HMemOntology, MemoryError, TurnRecord, Visibility, WebID};
use std::sync::RwLock;

use crate::inference_embedding::LanguageModelEmbeddingPort;

use super::curator_stores::{CuratorStore, build_curator_consolidation};

/// Borrowed handles for a single turn write. Constructed by
/// `RealMemoryPort::ingest_turn` from its own fields; tests construct one
/// directly from in-memory stores without going through `RealMemoryPort::new`
/// (no DB open, no passphrase, no consolidation timer).
pub(crate) struct WriteContext<'a> {
    pub curator_store: &'a CuratorStore,
    pub embedding_port: &'a LanguageModelEmbeddingPort,
    pub embedding_model: &'a str,
    pub curator_webid: WebID,
    pub tokio_handle: &'a tokio::runtime::Handle,
    /// Self-healing curator consolidation service — rebuilt here after a
    /// curator-store heal so the timer promotes freshly-ingested curator h_mems.
    /// Behind an `Arc` shared with the timer, which re-reads it on each tick.
    pub curator_consolidation: &'a Arc<RwLock<Option<Arc<MemoryConsolidator>>>>,
    pub consolidation_cadence_secs: u64,
}

/// Write a completed turn into the curator's memory.
///
/// Only curator turns are ingested. User/zed agent turns are NOT ingested —
/// the user is human and has their own memory. The kask memory system only
/// stores the curator's own turns and a shared copy for curator recall.
///
/// Performs, in order:
/// 1. Curator-store self-heal re-open + consolidation rebuild (if healed).
/// 2. If it's a curator turn: curator-perspective h_mem (Private) to the
///    curator's `curator.db`.
/// 3. Shared copy to the curator's `curator.db` — every turn, so
///    `curator_memory_recall` / `curator_semantic_search` see every turn the
///    agent observed.
/// 4. Embed the user prompt and store it to the curator's store (curator
///    turns only), for KNN-based recall.
///
/// `Ok(())` on success. Curator-side and embedding failures are non-fatal —
/// they warn and continue.
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

    // Only ingest curator turns. User/zed agent turns are not stored.
    if !is_curator_turn {
        return Ok(());
    }

    let turn_value = serde_json::json!({
        "user_input": user_input,
        "agent_response": agent_response,
        "model": model,
        "title": title,
    });

    // Resolve the curator stores once per ingestion.
    let curator_store = ctx.curator_store.get();
    // Rebuild the curator consolidation service after a heal.
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

    let entity = format!("chat:thread:{thread_id}");
    let ontology = HMemOntology::process("chat", "turn", format!("session:{thread_id}"));

    // ── 1. Curator-perspective h_mem (Private) ──────────────────────
    let curator_h_mem = HMem::new(
        &entity,
        "chatted",
        serde_json::Value::String(turn_value.to_string()),
        ctx.curator_webid,
    )
    .with_perspective(ctx.curator_webid)
    .with_visibility(Visibility::Private)
    .with_ontology(ontology);

    if let Some(ref curator_store) = curator_store {
        if let Err(e) = curator_store.store(curator_h_mem) {
            tracing::warn!(
                target: "reg.memory",
                thread_id = %thread_id,
                error = %e,
                "Failed to store curator h_mem"
            );
        }
    } else {
        tracing::trace!(
            target: "reg.memory",
            thread_id = %thread_id,
            "Curator store unavailable — skipping curator write"
        );
    }

    // ── 2. Shared copy in the curator's DB ──────────────────────────
    let curator_entity = format!("curator:thread:{thread_id}");
    let curator_ontology =
        HMemOntology::state("bibo:Document", vec!["chat_turn".to_string()], "curator");
    let curator_copy = HMem::new(
        &curator_entity,
        "turn",
        serde_json::Value::String(turn_value.to_string()),
        ctx.curator_webid,
    )
    .with_visibility(Visibility::Shared)
    .with_ontology(curator_ontology);

    if let Some(ref curator_store) = curator_store {
        if let Err(e) = curator_store.store(curator_copy) {
            tracing::warn!(
                target: "reg.memory",
                thread_id = %thread_id,
                error = %e,
                "Failed to store curator copy"
            );
        }
    }

    // ── 3. Embed the user prompt for future retrieval ─────────────────
    let embedding_entity = entity.clone();
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
                if let Some(ref curator_store) = curator_store
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
                "Failed to embed user prompt"
            );
        }
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                thread_id = %thread_id,
                error = %e,
                "Embedding task panicked"
            );
        }
    }

    tracing::info!(
        target: "reg.memory",
        thread_id = %thread_id,
        model = %model,
        "Curator turn ingested into memory"
    );

    Ok(())
}
