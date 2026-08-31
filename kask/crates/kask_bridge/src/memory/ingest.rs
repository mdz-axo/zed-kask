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
use hkask_types::{Confidence, HMemOntology, MemoryError, TurnRecord, Visibility, WebID};
use std::sync::RwLock;

use crate::inference_embedding::LanguageModelEmbeddingPort;

use super::curator_stores::{CuratorStore, build_curator_consolidation};

/// Borrowed handles for a single turn write. Constructed by
/// `RealMemoryPort::ingest_turn` from its own fields; tests construct one
/// directly from in-memory stores without going through `RealMemoryPort::new`
/// (no DB open, no passphrase, no consolidation timer).
pub(crate) struct WriteContext<'a> {
    pub curator_store: &'a CuratorStore,
    pub embedding_port: Option<&'a LanguageModelEmbeddingPort>,
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
/// All agent turns are ingested into the curator's shared store so the
/// curator can recall what happened across all agents. Curator turns
/// additionally get a private perspective-scoped h_mem (the curator's own
/// memory of its own turn).
///
/// Performs, in order:
/// 1. Curator-store self-heal re-open + consolidation rebuild (if healed).
/// 2. If it's a curator turn: curator-perspective h_mem (Private) to the
///    curator's `curator.db`.
/// 3. Shared copy to the curator's `curator.db` — every turn, so
///    `curator_memory_recall` / `curator_semantic_search` see every turn the
///    agent observed.
/// 4. Embed the user prompt and store it to the curator's store, for KNN-based
///    recall.
///
/// `Ok(())` on success. Curator-side and embedding failures are non-fatal —
/// they warn and continue.
///
/// Every h_mem written here — turn dumps and goal events alike — enters at
/// the 0.5 confidence floor, the same floor `memory_insert` starts distilled
/// memories at. `HMem::new`'s default of 1.0 starves the two consumers of
/// confidence: recall ranking cannot tell a stale turn from a fresh one,
/// and the cleanup-only consolidator's confidence floor never deletes
/// anything because nothing ever decays below it.
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
        "agent_id": agent_id,
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

    // ── 1. Curator-perspective h_mem (Private, curator turns only) ──
    // The curator's own memory of its own turn. Non-curators don't get a
    // perspective-scoped h_mem — they get only the shared copy below.
    if is_curator_turn {
        let curator_h_mem = HMem::new(
            &entity,
            "chatted",
            serde_json::Value::String(turn_value.to_string()),
            ctx.curator_webid,
        )
        .with_perspective(ctx.curator_webid)
        .with_visibility(Visibility::Private)
        .with_ontology(ontology)
        .with_confidence(Confidence::new(0.5));

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
    }

    // ── 2. Shared copy in the curator's DB ──────────────────────────
    let curator_entity = format!("curator:thread:{thread_id}");
    let curator_ontology = HMemOntology::state(
        hkask_bridge_ontology::dc_bibo::DOCUMENT,
        vec!["chat_turn".to_string()],
        "curator",
    );
    let curator_copy = HMem::new(
        &curator_entity,
        "turn",
        serde_json::Value::String(turn_value.to_string()),
        ctx.curator_webid,
    )
    .with_visibility(Visibility::Shared)
    .with_ontology(curator_ontology)
    .with_confidence(Confidence::new(0.5));

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

    // ── 3. Goal events — first-class goal memory ─────────────────────
    // The goal store is ephemeral (operator ruling 2026-08-29: zed-agent
    // goals are ephemeral; the curator's memory is the durable vehicle).
    // Each `kanban_goal_*` tool result becomes a structured goal h_mem so
    // therapy / algedonic-review find goal entities (text, criteria,
    // verdicts, Brier scores), not prose archaeology. Routing mirrors the
    // turn writes above: curator turns get a curator-perspective Private
    // h_mem (the curator's own memory of goals it was involved with); every
    // turn gets a shared copy (curator recall sees every goal event even
    // after the ephemeral store has evaporated).
    for event in &record.goal_events {
        // `extract_goal_events` hands us the raw MCP tool result, which the
        // response envelope wraps as `{"content": {...}}` — the goal_id
        // lives one level down. The top-level probe stays for results that
        // bypass the envelope (parsed text contents), and id-less outputs
        // (e.g. `kanban_goal_list`) deliberately land under the `list`
        // entity so list-shaped events still file somewhere stable.
        let goal_id = event
            .output
            .get("goal_id")
            .or_else(|| event.output.pointer("/content/goal_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("list");
        let goal_ontology = HMemOntology {
            dimensions: vec![hkask_types::Dimension::Why.as_str().to_string()],
            // `pplan:Step` (P-Plan, soft-reused by PKO) — the same term the
            // kanban goal store and the goal responses use, so the ephemeral
            // and durable records of the same goal agree. Operator decision
            // 2026-08-30: goals anchor on the PKO family — one linked
            // dataset. (The former `pko:Goal` was fabricated; PKO publishes
            // no Goal class; the interim IAO:0000005 anchor was rejected as
            // opaque.)
            dc_type: hkask_bridge_ontology::pko::STEP.to_string(),
            dc_source: "kanban".to_string(),
            ..Default::default()
        };

        if is_curator_turn {
            let curator_goal = HMem::new(
                &format!("goal:{goal_id}"),
                event.tool_name.as_str(),
                event.output.clone(),
                ctx.curator_webid,
            )
            .with_perspective(ctx.curator_webid)
            .with_visibility(Visibility::Private)
            .with_ontology(goal_ontology.clone())
            .with_confidence(Confidence::new(0.5));
            if let Some(ref curator_store) = curator_store {
                if let Err(e) = curator_store.store(curator_goal) {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        error = %e,
                        "Failed to store curator goal h_mem"
                    );
                }
            }
        }

        let shared_goal = HMem::new(
            &format!("curator:goal:{goal_id}"),
            event.tool_name.as_str(),
            event.output.clone(),
            ctx.curator_webid,
        )
        .with_visibility(Visibility::Shared)
        .with_ontology(goal_ontology)
        .with_confidence(Confidence::new(0.5));
        if let Some(ref curator_store) = curator_store {
            if let Err(e) = curator_store.store(shared_goal) {
                tracing::warn!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    error = %e,
                    "Failed to store shared goal h_mem"
                );
            }
        }
    }

    // ── 3. Embed the user prompt for future retrieval ─────────────────
    // Skipped when no embedding port is available — h_mem writes (steps 1-2)
    // are pure SQL and don't need embeddings. Semantic recall will degrade to
    // keyword-only, but the curator still has episodic memory of the turn.
    //
    // The embedding is stored under the SHARED COPY entity
    // (`curator:thread:{id}`), not `chat:thread:{id}`: the shared copy h_mem
    // is written for EVERY turn (step 2), while the `chat:thread:` h_mem
    // only exists for curator turns (step 1). An embedding under
    // `chat:thread:` for a non-curator turn joined to no h_mem — an orphan
    // the KNN recall path could never resolve, making every non-curator
    // turn invisible to semantic recall.
    let embedding_entity = curator_entity.clone();
    let embedding_model = ctx.embedding_model.to_string();
    let embedding_port = ctx.embedding_port.cloned();
    let user_input_owned = user_input.clone();
    if let Some(embedding_port) = embedding_port {
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
                            None,
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
    } else {
        tracing::debug!(
            target: "reg.memory",
            thread_id = %thread_id,
            "No embedding port — skipping prompt embedding (semantic recall degraded to keyword-only)"
        );
    }

    tracing::info!(
        target: "reg.memory",
        thread_id = %thread_id,
        model = %model,
        is_curator_turn,
        "Turn ingested into curator memory"
    );

    Ok(())
}
