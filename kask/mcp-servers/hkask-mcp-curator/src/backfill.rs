//! Memory embedding backfill sweeps (`missing` and `model_mismatch` modes).

use std::sync::Arc;

use hkask_mcp_server::server::{McpToolError, map_memory_store_error, resolve_db_passphrase};
use hkask_types::regulation::RegulationSpan;
use serde_json::json;

use crate::{curator_embedding_model, distillation, embed_for_semantic_recall, thread_turns};

/// The sweep's two modes: restore missing passages, or re-embed rows the
/// search model gate excludes.
pub(crate) enum BackfillMode {
    Missing,
    ModelMismatch,
}

/// The sweep's two store targets: the curator's own memory DB, or the
/// shared swarm memory DB.
pub(crate) enum BackfillStore {
    Curator,
    Swarm,
}

/// The swarm server's sole embed path stores delegation response chunks
/// (`hkask-mcp-swarm/src/local_knowledge.rs`); the swarm-store missing-mode
/// backfill mirrors that eligibility exactly rather than guessing a
/// knowledge-layer shape the swarm store does not have.
const SWARM_RESPONSE_CHUNK_ATTRIBUTE: &str = "delegation:response_chunk";

/// Resolve the shared swarm memory DB path. Mirrors
/// `kask_bridge::identity::resolve_swarm_memory_db_path`: an absolute
/// `HKASK_SWARM_MEMORY_DB` wins; anything else resolves under the shared
/// kask data dir (the swarm server's own default, `mcp/swarm/memory.db`).
fn swarm_memory_db_path() -> std::path::PathBuf {
    match std::env::var("HKASK_SWARM_MEMORY_DB")
        .ok()
        .filter(|raw| !raw.trim().is_empty())
    {
        Some(raw) if std::path::Path::new(&raw).is_absolute() => raw.into(),
        Some(raw) => hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(&raw)),
        None => hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
            "mcp/swarm/memory.db",
        )),
    }
}

/// Open the shared swarm memory DB for the sweep. The passphrase resolves
/// through the canonical env chain — a missing or empty key is
/// `permission_denied` naming the env var, never a silent empty-key open.
pub(crate) fn open_swarm_memory_store(
    cached_passphrase: Option<&str>,
) -> Result<Arc<hkask_memory::MemoryStore>, McpToolError> {
    let passphrase = match cached_passphrase.filter(|p| !p.is_empty()) {
        Some(passphrase) => passphrase.to_owned(),
        None => resolve_db_passphrase(&std::collections::HashMap::new()).map_err(|error| {
            tracing::warn!(
                target: "hkask.mcp.curator",
                %error,
                "HKASK_DB_PASSPHRASE resolution failed — the swarm store sweep is permission_denied"
            );
            McpToolError::permission_denied(
                "HKASK_DB_PASSPHRASE is not resolvable — the swarm store sweep is unavailable \
                 (relaunch with HKASK_DB_PASSPHRASE in the server env)",
            )
        })?,
    };
    let path = swarm_memory_db_path();
    let store = hkask_memory::MemoryStore::open_existing(
        &path.to_string_lossy(),
        &passphrase,
        hkask_storage::embedding_dim(),
    )
    .map_err(|error| {
        hkask_mcp_server::map_database_error(
            error,
            &format!("Cannot open swarm memory DB at {}", path.display()),
        )
    })?;
    let removed = store.delete_orphaned_embeddings().map_err(|error| {
        McpToolError::internal(format!(
            "orphaned-embedding sweep failed on swarm memory DB {}: {error}",
            path.display()
        ))
    })?;
    if removed > 0 {
        tracing::warn!(
            target: "hkask.mcp.curator",
            removed,
            "swarm memory DB carried orphaned embedding rows — swept at open"
        );
    }
    Ok(Arc::new(store))
}

/// The gate's two-form predicate: a stored row is mismatched when its
/// recorded model matches neither the requested form nor the provider-
/// confirmed actual form — the same comparison `EmbeddingStore::search`
/// excludes on. A row with no recorded model can never match either form.
fn model_is_mismatched(stored: Option<&str>, requested: &str, actual: Option<&str>) -> bool {
    match stored {
        None => true,
        Some(stored) => stored != requested && actual != Some(stored),
    }
}

/// Missing-passage backfill: h_mems whose exact canonical passage has no
/// embedding row.
pub(crate) async fn backfill_missing_passages(
    inference_port: &dyn hkask_types::InferencePort,
    memory: &hkask_memory::MemoryStore,
    store_target: BackfillStore,
    dry_run: bool,
) -> Result<serde_json::Value, McpToolError> {
    let active = memory
        .h_mems_by_entity_prefix("")
        .map_err(|e| map_memory_store_error(e, "Failed to scan active h_mems"))?;

    let is_excluded = |entity: &str| {
        entity.starts_with(thread_turns::SHARED_TURN_PREFIX)
            || entity.starts_with(distillation::WATERMARK_PREFIX)
            || entity.starts_with("curator:goal:")
    };
    let eligible = |h_mem: &hkask_storage::HMem| match store_target {
        BackfillStore::Curator => !is_excluded(&h_mem.entity),
        BackfillStore::Swarm => h_mem.attribute == SWARM_RESPONSE_CHUNK_ATTRIBUTE,
    };

    let mut candidates: Vec<(&hkask_storage::HMem, String, bool)> = Vec::new();
    let mut unsupported: Vec<serde_json::Value> = Vec::new();
    for h_mem in active.iter().filter(|h_mem| eligible(h_mem)) {
        let (passage, repair) = match hkask_memory::semantic_passage_for_h_mem(h_mem) {
            Some(passage) => (passage, false),
            None if matches!(store_target, BackfillStore::Curator) => {
                let write = memory.prepare_semantic_repair(&h_mem.id).map_err(|error| {
                    map_memory_store_error(error, "Failed to prepare legacy passage repair")
                })?;
                (write.passage().to_string(), true)
            }
            None => {
                unsupported.push(json!({
                    "h_mem_id": h_mem.id.to_string(),
                    "entity": h_mem.entity,
                    "attribute": h_mem.attribute,
                    "reason": "value has no string, recall_text or text field",
                }));
                continue;
            }
        };
        let already_embedded = memory
            .has_embedding_for_passage(&h_mem.entity, &passage)
            .map_err(|error| {
                map_memory_store_error(error, "Failed to inspect passage-level embedding coverage")
            })?;
        if repair || !already_embedded {
            candidates.push((h_mem, passage, repair));
        }
    }

    let store_name = match store_target {
        BackfillStore::Curator => "curator",
        BackfillStore::Swarm => "swarm",
    };

    if dry_run {
        return Ok(json!({
            "dry_run": true,
            "mode": "missing",
            "store": store_name,
            "candidate_count": candidates.len(),
            "unsupported_count": unsupported.len(),
            "unsupported": unsupported,
            "candidates": candidates.iter().map(|(h_mem, _passage, repair)| json!({
                "requires_passage_repair": repair,
                "h_mem_id": h_mem.id.to_string(),
                "entity": h_mem.entity,
                "attribute": h_mem.attribute,
            })).collect::<Vec<_>>(),
            "guidance": "Dry run — nothing changed. Re-run without dry_run to backfill; candidates marked requires_passage_repair will also gain derived recall_text via an atomic memory revision."
        }));
    }

    let candidate_count = candidates.len();
    let mut results = Vec::with_capacity(candidate_count);
    let mut embedded_count = 0usize;
    let mut failed_count = 0usize;
    for (h_mem, embed_text, repair) in candidates {
        let mut written_id = h_mem.id;
        let embedded = if repair {
            let write = memory.prepare_semantic_repair(&h_mem.id).map_err(|error| {
                map_memory_store_error(error, "Failed to prepare legacy passage repair")
            })?;
            match crate::publish_semantic_write(inference_port, write).await {
                Ok(id) => {
                    written_id = id;
                    true
                }
                Err(error) => {
                    tracing::warn!(target: "hkask.mcp.curator", %error, h_mem_id = %h_mem.id,
                        "Legacy passage repair failed — prior row retained");
                    false
                }
            }
        } else {
            embed_for_semantic_recall(inference_port, memory, &h_mem.entity, &embed_text).await
        };
        if embedded {
            embedded_count += 1;
        } else {
            failed_count += 1;
        }
        results.push(json!({
            "h_mem_id": written_id.to_string(),
            "previous_h_mem_id": h_mem.id.to_string(),
            "passage_repaired": repair && embedded,
            "entity": h_mem.entity,
            "attribute": h_mem.attribute,
            "embedded": embedded,
        }));
    }

    RegulationSpan::Curation.emit("memory_embeddings_backfilled");

    Ok(json!({
        "mode": "missing",
        "store": store_name,
        "candidate_count": candidate_count,
        "backfilled": embedded_count,
        "failed": failed_count,
        "unsupported_count": unsupported.len(),
        "unsupported": unsupported,
        "results": results,
        "guidance": "Embeddings are backfilled per exact canonical passage. Curator knowledge rows lacking a passage are revised with derived recall_text and an embedding atomically; results report replacement IDs. Goal rows are excluded and never repaired here. Failed candidates remain candidates on re-run."
    }))
}

/// Model-mismatch sweep — the write side of the search model gate: stored rows
/// whose recorded model matches neither the current requested embedding model
/// nor its provider-confirmed actual form are re-embedded under the current model.
pub(crate) async fn backfill_model_mismatched_rows(
    inference_port: &dyn hkask_types::InferencePort,
    memory: &hkask_memory::MemoryStore,
    dry_run: bool,
) -> Result<serde_json::Value, McpToolError> {
    let Some(requested_model) = curator_embedding_model() else {
        return Err(McpToolError::failed_precondition(
            "no embedding model configured — set kask.models.embedding_model \
             (injected as HKASK_EMBEDDING_MODEL); the model-mismatch sweep \
             cannot run without the model to re-embed under",
        ));
    };

    let probe = inference_port
        .embed_with_dimensions(
            &requested_model,
            std::slice::from_ref(&"model-mismatch sweep identity probe".to_string()),
            Some(hkask_storage::embedding_dim() as u32),
        )
        .await;
    let actual_model = match &probe {
        Ok(batch) => batch.actual_model.clone(),
        Err(error) => {
            tracing::warn!(
                target: "hkask.mcp.curator",
                %error,
                "identity probe failed — model-mismatch selection compares \
                 against the requested form only"
            );
            None
        }
    };

    let rows = memory
        .all_embeddings_with_text()
        .map_err(|e| map_memory_store_error(e, "Failed to scan stored embeddings"))?;

    let mut candidates: Vec<(String, String, String)> = Vec::new();
    let mut unsupported: Vec<serde_json::Value> = Vec::new();
    for (entity, _vector, passage, stored_model) in rows {
        if !model_is_mismatched(
            Some(stored_model.as_str()),
            &requested_model,
            actual_model.as_deref(),
        ) {
            continue;
        }
        let Some(passage) = passage.filter(|p| !p.trim().is_empty()) else {
            unsupported.push(json!({
                "entity": entity,
                "stored_model": stored_model,
                "reason": "no stored passage text — the row cannot be re-embedded \
                           from its source; delete it and re-ingest the source passage",
            }));
            continue;
        };
        candidates.push((entity, passage, stored_model));
    }

    if dry_run {
        return Ok(json!({
            "dry_run": true,
            "mode": "model_mismatch",
            "requested_model": requested_model,
            "actual_model": actual_model,
            "candidate_count": candidates.len(),
            "unsupported_count": unsupported.len(),
            "unsupported": unsupported,
            "candidates": candidates.iter().map(|(entity, _passage, stored_model)| json!({
                "entity": entity,
                "stored_model": stored_model,
            })).collect::<Vec<_>>(),
            "guidance": "Dry run — nothing re-embedded. Re-run without dry_run to sweep."
        }));
    }

    let candidate_count = candidates.len();
    let mut results = Vec::with_capacity(candidate_count);
    let mut re_embedded = 0usize;
    let mut failed = 0usize;
    for (entity, passage, stored_model) in candidates {
        let retired = memory
            .delete_embedding_by_entity_ref_and_passage(&entity, &passage)
            .map_err(|e| map_memory_store_error(e, "Failed to retire the stale-model row"))?;
        let embedded = if retired == 0 {
            false
        } else {
            embed_for_semantic_recall(inference_port, memory, &entity, &passage).await
        };
        if embedded {
            re_embedded += 1;
        } else {
            failed += 1;
        }
        results.push(json!({
            "entity": entity,
            "stored_model": stored_model,
            "re_embedded": embedded,
        }));
    }

    RegulationSpan::Curation.emit("memory_embeddings_backfilled");

    Ok(json!({
        "mode": "model_mismatch",
        "requested_model": requested_model,
        "actual_model": actual_model,
        "candidate_count": candidate_count,
        "re_embedded": re_embedded,
        "failed": failed,
        "unsupported_count": unsupported.len(),
        "unsupported": unsupported,
        "results": results,
        "guidance": "Stale-model rows are re-embedded under the current model \
                     (delete-then-store per exact passage; siblings survive). A row \
                     that fails between delete and store becomes a missing passage \
                     the missing-mode backfill recovers."
    }))
}
