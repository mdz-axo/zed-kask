//! Memory embedding backfill sweeps (`missing` and `model_mismatch` modes).

use hkask_mcp_server::server::{McpToolError, map_memory_store_error};
use hkask_types::regulation::RegulationSpan;
use serde_json::json;

use crate::{curator_embedding_model, distillation, embed_for_semantic_recall, thread_turns};

/// The sweep's two modes: restore missing passages, or re-embed rows the
/// search model gate excludes.
pub(crate) enum BackfillMode {
    Missing,
    ModelMismatch,
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

/// Paginate `h_mems_by_prefix_since` from `0001-01-01T00:00:00Z` so scans never
/// silently truncate at the 100,000-row per-page cap.
pub(crate) fn scan_hmems_by_prefix_uncapped(
    memory: &hkask_memory::MemoryStore,
    prefix: &str,
) -> Result<Vec<hkask_storage::HMem>, hkask_memory::MemoryStoreError> {
    const PAGE_CAP: usize = 100_000;
    let mut cursor = chrono::DateTime::from_timestamp(-62_135_596_800, 0).unwrap_or_default();
    let mut seen_ids = std::collections::HashSet::new();
    let mut all = Vec::new();
    loop {
        let page = memory.h_mems_by_prefix_since(prefix, cursor)?;
        let page_len = page.len();
        let mut max_ts = cursor;
        let mut added = 0usize;
        for row in page {
            if row.observed_at > max_ts {
                max_ts = row.observed_at;
            }
            if seen_ids.insert(row.id) {
                all.push(row);
                added += 1;
            }
        }
        if page_len < PAGE_CAP {
            break;
        }
        cursor = if added == 0 {
            max_ts + chrono::Duration::nanoseconds(1)
        } else {
            max_ts
        };
    }
    Ok(all)
}

/// Missing-passage backfill: h_mems whose exact canonical passage has no
/// embedding row.
pub(crate) async fn backfill_missing_passages(
    inference_port: &dyn hkask_types::InferencePort,
    memory: &hkask_memory::MemoryStore,
    dry_run: bool,
) -> Result<serde_json::Value, McpToolError> {
    let mut active = scan_hmems_by_prefix_uncapped(memory, "")
        .map_err(|e| map_memory_store_error(e, "Failed to scan active h_mems"))?;
    active.sort_by_key(|row| std::cmp::Reverse(row.observed_at));

    let is_excluded = |entity: &str| {
        entity.starts_with(thread_turns::SHARED_TURN_PREFIX)
            || entity.starts_with(distillation::WATERMARK_PREFIX)
            || entity.starts_with("curator:goal:")
    };

    let mut candidates: Vec<(&hkask_storage::HMem, String)> = Vec::new();
    let mut excluded_private = 0usize;
    for h_mem in active.iter().filter(|h_mem| !is_excluded(&h_mem.entity)) {
        if !crate::call_admits(h_mem) {
            excluded_private += 1;
            continue;
        }
        let Some(passage) = hkask_memory::semantic_passage_for_h_mem(h_mem) else {
            continue;
        };
        let already_embedded = memory
            .has_embedding_for_passage(&h_mem.entity, &passage)
            .map_err(|error| {
                map_memory_store_error(error, "Failed to inspect passage-level embedding coverage")
            })?;
        if !already_embedded {
            candidates.push((h_mem, passage));
        }
    }

    let store_name = "curator";

    if dry_run {
        let mut out = json!({
            "dry_run": true,
            "mode": "missing",
            "store": store_name,
            "candidate_count": candidates.len(),
            "candidates": candidates.iter().map(|(h_mem, _passage)| json!({
                "h_mem_id": h_mem.id.to_string(),
                "entity": h_mem.entity,
                "attribute": h_mem.attribute,
            })).collect::<Vec<_>>(),
            "guidance": "Dry run — nothing changed. Re-run without dry_run to backfill."
        });
        if excluded_private > 0 {
            out["excluded_private_rows"] = json!(excluded_private);
        }
        return Ok(out);
    }

    let candidate_count = candidates.len();
    let mut results = Vec::with_capacity(candidate_count);
    let mut embedded_count = 0usize;
    let mut failed_count = 0usize;
    for (h_mem, embed_text) in candidates {
        let embedded = embed_for_semantic_recall(
            inference_port,
            memory,
            &h_mem.entity,
            &embed_text,
            h_mem.inference_provenance,
        )
        .await;
        if embedded {
            embedded_count += 1;
        } else {
            failed_count += 1;
        }
        results.push(json!({
            "h_mem_id": h_mem.id.to_string(),
            "entity": h_mem.entity,
            "attribute": h_mem.attribute,
            "embedded": embedded,
        }));
    }

    RegulationSpan::Curation.emit("memory_embeddings_backfilled");

    let mut out = json!({
        "mode": "missing",
        "store": store_name,
        "candidate_count": candidate_count,
        "backfilled": embedded_count,
        "failed": failed_count,
        "results": results,
        "guidance": "Embeddings are backfilled per exact canonical passage. Goal rows are excluded. Failed candidates remain candidates on re-run."
    });
    if excluded_private > 0 {
        out["excluded_private_rows"] = json!(excluded_private);
    }
    Ok(out)
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
    // The probe is a billed embedding call (dry runs included): its usage
    // rides the tool's envelope, and a failed probe is named in the result
    // — selection then compares against the requested form only, which can
    // list rows stored under the provider's own name for the same model.
    let (actual_model, identity_probe_error) = match probe {
        Ok(batch) => {
            hkask_mcp_server::server::record_tool_usage(batch.usage);
            (batch.actual_model, None)
        }
        // A real sweep deletes each stale row before storing its
        // replacement. With the embedder not answering, every store would
        // fail after its delete, so the sweep stops here with nothing
        // deleted.
        Err(error) if !dry_run => {
            return Err(crate::embed_error(
                &error,
                "the model-mismatch sweep did not start (nothing was deleted): the embedding \
                 model did not answer the identity probe",
            ));
        }
        Err(error) => {
            tracing::warn!(
                target: "hkask.mcp.curator",
                %error,
                "identity probe failed — model-mismatch selection compares \
                 against the requested form only"
            );
            (None, Some(error.to_string()))
        }
    };

    let rows = memory
        .all_embeddings_with_text()
        .map_err(|e| map_memory_store_error(e, "Failed to scan stored embeddings"))?;

    // zed-kask: D100 Phase 2 + S041-F01 — private-marked rows are NOT this
    // sweep's constituency: their target model is the private leg's
    // (HKASK_PRIVATE_EMBEDDING_MODEL), and re-embedding them here would cross
    // private passages to the configured cloud embedder. Look up each
    // candidate entity's h_mems directly via `h_mems_by_entity` (uncapped SQL
    // index lookup — never `h_mems_by_entity_prefix("")` which truncates at
    // 100,000 rows), and also skip any vector recorded under
    // `HKASK_PRIVATE_EMBEDDING_MODEL` or attached to an entity carrying legacy
    // passage-less `PrivateInference` rows.
    let private_embedding_model = hkask_inference::model_constants::private_embedding_model();
    let mut entity_cache: std::collections::HashMap<String, Vec<hkask_storage::HMem>> =
        std::collections::HashMap::new();

    let mut candidates: Vec<(String, String, String)> = Vec::new();
    let mut unsupported: Vec<serde_json::Value> = Vec::new();
    let mut skipped_private = 0usize;
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
        if private_embedding_model.as_deref() == Some(stored_model.as_str())
            && stored_model != requested_model
        {
            skipped_private += 1;
            continue;
        }
        let entity_rows = match entity_cache.entry(entity.clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(vacant) => {
                let fetched = memory.h_mems_by_entity(&entity).map_err(|e| {
                    map_memory_store_error(e, "Failed to inspect entity provenance for backfill")
                })?;
                vacant.insert(fetched)
            }
        };
        let has_matching_public = entity_rows.iter().any(|h_mem| {
            h_mem.inference_provenance != hkask_types::InferenceProvenance::PrivateInference
                && hkask_memory::semantic_passage_for_h_mem(h_mem).as_deref()
                    == Some(passage.as_str())
        });
        let blocked_by_private = entity_rows.iter().any(|h_mem| {
            if h_mem.inference_provenance != hkask_types::InferenceProvenance::PrivateInference {
                return false;
            }
            match hkask_memory::semantic_passage_for_h_mem(h_mem) {
                Some(priv_passage) => priv_passage == passage || !has_matching_public,
                None => !has_matching_public,
            }
        });
        if blocked_by_private {
            skipped_private += 1;
            continue;
        }
        candidates.push((entity, passage, stored_model));
    }

    if dry_run {
        return Ok(json!({
            "dry_run": true,
            "mode": "model_mismatch",
            "requested_model": requested_model,
            "actual_model": actual_model,
            "identity_probe_error": identity_probe_error,
            "candidate_count": candidates.len(),
            "unsupported_count": unsupported.len(),
            "skipped_private": skipped_private,
            "unsupported": unsupported,
            "candidates": candidates.iter().map(|(entity, _passage, stored_model)| json!({
                "entity": entity,
                "stored_model": stored_model,
            })).collect::<Vec<_>>(),
            "guidance": "Dry run — nothing re-embedded (one embedding call was made to learn the provider's name for the model). Re-run without dry_run to sweep."
        }));
    }

    // A real sweep reaches here only with an answered probe.
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
            // Phase 2: the private-marked candidates were skipped above —
            // this sweep's remaining constituency is the public lane.
            embed_for_semantic_recall(
                inference_port,
                memory,
                &entity,
                &passage,
                hkask_types::InferenceProvenance::PublicProvider,
            )
            .await
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
        "skipped_private": skipped_private,
        "unsupported": unsupported,
        "results": results,
        "guidance": "Stale-model rows are re-embedded under the current model \
                     (delete-then-store per exact passage; siblings survive). A row \
                     that fails between delete and store becomes a missing passage \
                     the missing-mode backfill recovers. Private-marked rows are \
                     skipped (their target is the private leg's \
                     kask.private_inference.embedding_model, never the configured \
                     cloud model) — the skipped_private count is the surfaced \
                     boundary."
    }))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use hkask_storage::{EmbeddingStore, HMem, HMemStore, SqliteDriver};
    use hkask_types::{
        ChatToolDefinition, EmbedFuture, EmbedWithIdentityFuture, EmbeddingBatch, InferenceError,
        InferencePort, InferenceProvenance, InferenceResult, InferenceUsage, LLMParameters, WebID,
    };

    use super::*;

    #[derive(Default)]
    struct RecordingPort {
        calls: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl InferencePort for RecordingPort {
        fn generate(
            &self,
            _prompt: &str,
            _params: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            Box::pin(async { Err(InferenceError::Timeout("unused".to_string())) })
        }

        fn embed<'a>(&'a self, model: &str, texts: &[String]) -> EmbedFuture<'a> {
            self.embed_with_identity(model, texts)
        }

        fn embed_with_identity<'a>(
            &'a self,
            model: &str,
            texts: &[String],
        ) -> EmbedWithIdentityFuture<'a> {
            let model_owned = model.to_string();
            let texts_owned = texts.to_vec();
            self.calls
                .lock()
                .expect("lock")
                .push((model_owned.clone(), texts_owned.clone()));
            let dim = hkask_storage::embedding_dim();
            let mut vec = vec![0.0_f32; dim];
            vec[0] = 1.0;
            Box::pin(async move {
                Ok(EmbeddingBatch {
                    vectors: texts_owned.iter().map(|_| vec.clone()).collect(),
                    requested_model: model_owned.clone(),
                    actual_model: Some(model_owned),
                    usage: InferenceUsage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                        reported: false,
                    },
                })
            })
        }

        fn embed_with_dimensions<'a>(
            &'a self,
            model: &str,
            texts: &[String],
            _dimensions: Option<u32>,
        ) -> EmbedWithIdentityFuture<'a> {
            self.embed_with_identity(model, texts)
        }
    }

    #[tokio::test]
    async fn model_mismatch_backfill_queries_exact_entity_provenance_and_skips_all_private_rows() {
        crate::distillation::tests::ensure_embedding_model_env();
        let driver = SqliteDriver::in_memory_driver();
        let memory = hkask_memory::MemoryStore::new(
            HMemStore::from_driver(Arc::clone(&driver)).expect("hmem"),
            EmbeddingStore::from_driver(Arc::clone(&driver), hkask_storage::embedding_dim())
                .expect("embed"),
        );
        let dim = hkask_storage::embedding_dim();
        let mut unit = vec![0.0_f32; dim];
        unit[0] = 1.0;

        // Seed an ancient PrivateInference row (even pre-2000) under a stale model,
        // plus a legacy object-valued PrivateInference row with no extractable passage,
        // plus one genuine stale-model PublicProvider row.
        let mut old_priv = HMem::new(
            "secret:vault",
            "passphrase_hint",
            serde_json::Value::String("ULTRA_PRIVATE_BACKFILL_TEXT".to_string()),
            WebID::new(),
        )
        .with_inference_provenance(InferenceProvenance::PrivateInference);
        old_priv.observed_at = chrono::Utc::now() - chrono::Duration::days(15_000);
        memory.store(old_priv).expect("store old_priv");
        memory
            .store_embedding(
                "secret:vault",
                &unit,
                "stale/local-v0",
                Some("ULTRA_PRIVATE_BACKFILL_TEXT"),
            )
            .expect("store old_priv embedding");

        let legacy_priv = HMem::new(
            "secret:legacy",
            "blob",
            serde_json::json!({"opaque_num": 42}),
            WebID::new(),
        )
        .with_inference_provenance(InferenceProvenance::PrivateInference);
        memory.store(legacy_priv).expect("store legacy_priv");
        memory
            .store_embedding(
                "secret:legacy",
                &unit,
                "stale/local-v0",
                Some("LEGACY_PRIVATE_ORPHANED_PASSAGE"),
            )
            .expect("store legacy_priv embedding");

        let pub_row = HMem::new(
            "public:doc",
            "summary",
            serde_json::Value::String("PUBLIC_MIGRATION_PASSAGE".to_string()),
            WebID::new(),
        );
        memory.store(pub_row).expect("store pub_row");
        memory
            .store_embedding(
                "public:doc",
                &unit,
                "cloud/embed-v1",
                Some("PUBLIC_MIGRATION_PASSAGE"),
            )
            .expect("store pub_row embedding");

        let port = RecordingPort::default();
        let out = backfill_model_mismatched_rows(&port, &memory, false)
            .await
            .expect("sweep ok");
        assert_eq!(out["re_embedded"].as_u64(), Some(1), "{out}");
        assert_eq!(out["skipped_private"].as_u64(), Some(2), "{out}");

        let calls = port.calls.lock().expect("lock").clone();
        let all_texts: Vec<String> = calls.into_iter().flat_map(|(_, t)| t).collect();
        assert!(
            !all_texts
                .iter()
                .any(|t| t.contains("ULTRA_PRIVATE") || t.contains("LEGACY_PRIVATE")),
            "no private passage may ever reach the cloud embedder during backfill: {all_texts:?}"
        );
        assert!(
            all_texts.iter().any(|t| t == "PUBLIC_MIGRATION_PASSAGE"),
            "stale public passage must still be re-embedded: {all_texts:?}"
        );
    }
}
