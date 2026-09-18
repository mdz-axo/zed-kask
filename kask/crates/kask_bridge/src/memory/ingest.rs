//! Turn ingestion write path — clean → chunk → tag → embed → write.
//!
//! Extracted from `RealMemoryPort::ingest_turn` (deep-module split, bridge-audit
//! BD-04 continuation). The port impl holds only the ingestion semaphore and
//! delegates the actual writes here. This gives the write path a named home.
//!
//! The write path is a pure transformation of `(store handles, TurnRecord)` into
//! side effects — no new trait, no new ownership. `write_turn` borrows the port's
//! fields via [`WriteContext`].
//!
//! Design (operator-ratified 2026-09-04): threads are chunked and those chunks
//! embedded and ontologically tagged along the way — a process mirroring the
//! corpus pipeline (chunk → embed → tag) with an added cleaning step, and a
//! single shared copy per turn (the former curator-perspective duplicate copy
//! was removed by the same ruling). Each chunk is one bounded h_mem under the
//! thread entity `curator:thread:{thread_id}` with attribute `chunk:{index}`,
//! its own embedding (stored with `passage_text` so KNN results pinpoint the
//! matched chunk), and a content-derived ontology blob. Raw transcript dumps —
//! the 500KB single-value rows the 2026-09-04 therapy scan found — are gone.

use std::sync::Arc;
use std::sync::RwLock;

use hkask_bridge_ontology::term_resolution::{TERM_RESOLUTION_PROTOCOL, canonicalize_terms};
use hkask_inference::passage_tagging::{
    ExpertiseMode, Passage, PassageTag, PassageTaggingRequest, parse_tagging_response,
    render_deployed_tagging_prompt_at,
};
use hkask_memory::MemoryConsolidator;
use hkask_storage::HMem;
use hkask_types::template::LLMParameters;
use hkask_types::{
    Confidence, Dimension, HMemOntology, MemoryError, TurnRecord, Visibility, WebID,
};

use crate::inference_embedding::LanguageModelEmbeddingPort;

use super::curator_stores::{CuratorStore, build_curator_consolidation};

/// Minimum words per chunk. Below this, fragments merge forward into the next
/// passage (the chunker's buffer) — 1-word chunks pollute embeddings.
pub(crate) const MIN_CHUNK_WORDS: usize = 30;

/// Maximum words per chunk. Bounds every h_mem value; a turn that would have
/// been one 500KB dump becomes N bounded passages.
pub(crate) const MAX_CHUNK_WORDS: usize = 400;

/// Sentence-boundary characters for the chunker's long-paragraph splits.
const SENTENCE_BOUNDARY: &str = ".!?";

/// Borrowed handles for a single turn write. Constructed by
/// `RealMemoryPort::ingest_turn` from its own fields; tests construct one
/// directly from in-memory stores without going through `RealMemoryPort::new`
/// (no DB open, no passphrase, no consolidation timer).
pub(crate) struct WriteContext<'a> {
    pub curator_store: &'a CuratorStore,
    pub embedding_port: Option<&'a LanguageModelEmbeddingPort>,
    pub embedding_model: &'a str,
    /// The classifier model used for write-time chunk tagging
    /// (`kask.models.classifier_model`). `None` = not configured — chunks
    /// get structural tags only (surfaced at wiring time, not per turn).
    pub classifier_model: Option<&'a str>,
    /// Host-deployed template registry root the tagging prompt renders from
    /// (`kask.corpus.template_root` override → `{data_dir}/skills/registry`).
    /// Threaded from settings at wiring time — `HKASK_TEMPLATE_ROOT` only
    /// reaches MCP server child processes, never this in-process path.
    pub template_root: &'a std::path::Path,
    pub curator_webid: WebID,
    pub tokio_handle: &'a tokio::runtime::Handle,
    /// Self-healing curator consolidation service — rebuilt here after a
    /// curator-store heal so the timer promotes freshly-ingested curator h_mems.
    /// Behind an `Arc` shared with the timer, which re-reads it on each tick.
    pub curator_consolidation: &'a Arc<RwLock<Option<Arc<MemoryConsolidator>>>>,
    pub consolidation_cadence_secs: u64,
}

/// Durable per-chunk outcomes from one turn ingestion attempt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct IngestionReport {
    /// Chunks produced by the validated chunk contract.
    pub(crate) attempted: usize,
    /// Chunks whose h_mem row was stored successfully.
    pub(crate) stored: usize,
    /// Chunks whose embedding row and passage text were stored successfully.
    pub(crate) embedded: usize,
    /// Chunks missing any expected configured durable output. A partially
    /// stored chunk is counted here as well as in `stored` or `embedded`.
    pub(crate) failed: usize,
    /// Chunks stored without semantic embeddings because the embedding
    /// capability itself was unavailable.
    pub(crate) degraded: usize,
}

impl IngestionReport {
    pub(crate) fn degraded(self) -> bool {
        self.failed > 0 || self.degraded > 0
    }
}

/// Write a completed turn into the curator's memory as cleaned, embedded,
/// ontologically tagged chunks — one shared copy per turn.
///
/// Performs, in order:
/// 1. Curator-store self-heal re-open + consolidation rebuild (if healed).
/// 2. Goal events — one shared h_mem per event under `curator:goal:{goal_id}`
///    (the former curator-perspective `goal:{id}` duplicate was removed by the
///    2026-09-04 single-copy ruling).
/// 3. Clean the turn text (role prefixes, base64-noise stripping) and chunk it
///    into word-bounded passages.
/// 4. Tag: structural dimensions (who/when/where/how — derivable from the
///    record without an LLM) always; content dimensions (what/why, subjects,
///    domain concepts, expertise) via one batched classifier-model call when
///    the inference port is wired and the model configured. Tagging failure
///    degrades to structural-only with a warn — never blocks the write.
/// 5. Embed every chunk in one batched call; each vector is stored under the
///    thread entity with its `passage_text` so KNN pinpoints the matched chunk.
/// 6. Write one h_mem per chunk at the 0.5 confidence floor.
///
/// Returns durable attempted/stored/embedded/failed counts. Curator-side,
/// embedding, and tagging failures are non-fatal — they warn and continue
/// (the failure-signal rule: the operator
/// must be able to distinguish "not configured" from "configured but broken",
/// so every degradation path logs).
///
/// Every h_mem written here — chunks and goal events alike — enters at the
/// 0.5 confidence floor, the same floor `memory_insert` starts distilled
/// memories at. `HMem::new`'s default of 1.0 starves the two consumers of
/// confidence: recall ranking cannot tell a stale turn from a fresh one,
/// and the cleanup-only consolidator's confidence floor never deletes
/// anything because nothing ever decays below it.
pub(crate) async fn write_turn(
    ctx: &WriteContext<'_>,
    record: TurnRecord,
) -> Result<IngestionReport, MemoryError> {
    let thread_id = record.thread_id.clone();
    let model = record.model.clone();
    let is_curator_turn = record.agent_id.as_deref() == Some("Curator");

    // Turn identity for chunk provenance/ordering. TurnRecord carries no
    // ordinal or timestamp, so write time stands in for turn-completion time.
    let turn_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();

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

    // ── 1. Goal events — first-class goal memory, single shared copy ──
    // Resolved kanban goals remain durable outbox entries until a score event
    // is stored here and the thread path acknowledges the handoff. Score writes
    // therefore fail the ingestion and deduplicate retries; other goal events
    // retain their existing best-effort behavior.
    // Each `kanban_goal_*` tool result becomes one structured goal h_mem so
    // therapy / algedonic-review find goal entities (text, criteria,
    // verdicts, Brier scores), not prose archaeology. One key convention:
    // `curator:goal:{goal_id}` (the 2026-09-04 single-copy ruling retired the
    // curator-perspective `goal:{id}` duplicate and the legacy `*:list` keys).
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
            dimensions: vec![Dimension::Why.as_str().to_string()],
            // `pplan:Step` (P-Plan, soft-reused by PKO) — the same term the
            // kanban goal store and the goal responses use, so the retained
            // outbox record and curator-memory record agree. Operator decision
            // 2026-08-30: goals anchor on the PKO family — one linked
            // dataset. (The former `pko:Goal` was fabricated; PKO publishes
            // no Goal class; the interim IAO:0000005 anchor was rejected as
            // opaque.)
            dc_type: hkask_bridge_ontology::pko::STEP.to_string(),
            dc_source: "kanban".to_string(),
            ..Default::default()
        };

        let goal_entity = format!("curator:goal:{goal_id}");
        let is_score = event.tool_name == "kanban_goal_score";
        let Some(ref curator_store) = curator_store else {
            if is_score {
                return Err(MemoryError::Ingestion(format!(
                    "curator store unavailable for resolved goal {goal_id}"
                )));
            }
            continue;
        };
        let already_stored = if is_score {
            curator_store
                .h_mems_by_entity_prefix(&goal_entity)
                .map_err(|e| {
                    MemoryError::Ingestion(format!(
                        "failed to query resolved goal {goal_id} before ingest: {e}"
                    ))
                })?
                .into_iter()
                .any(|h_mem| h_mem.attribute == event.tool_name && h_mem.value == event.output)
        } else {
            false
        };
        if !already_stored {
            let shared_goal = HMem::new(
                &goal_entity,
                event.tool_name.as_str(),
                event.output.clone(),
                ctx.curator_webid,
            )
            .with_visibility(Visibility::Shared)
            .with_ontology(goal_ontology)
            .with_confidence(Confidence::new(0.5));
            if let Err(e) = curator_store.store(shared_goal) {
                if is_score {
                    return Err(MemoryError::Ingestion(format!(
                        "failed to store resolved goal {goal_id}: {e}"
                    )));
                }
                tracing::warn!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    error = %e,
                    "Failed to store shared goal h_mem"
                );
            }
        }

        // ── Brier loop → memory confidence (spec §11 item 4) ──────────
        // The goal score is the one outcome the memory system observes
        // automatically: its Brier calibrates the confidence of the goal's
        // prediction record (the `kanban_goal_create` h_mem) via Bayesian
        // combination — never a raw confidence write. Mapping: a binary
        // no-skill prediction scores Brier 0.25, the neutral point; 0 →
        // 0.95 (strong confirm), 1 → 0.05 (strong disconfirm, which drops
        // the record below the consolidation floor so cleanup deletes it).
        if event.tool_name == "kanban_goal_score" {
            let brier = event
                .output
                .get("brier")
                .or_else(|| event.output.pointer("/content/brier"))
                .and_then(serde_json::Value::as_f64);
            let Some(brier) = brier else {
                // `brier` is null when no intake prediction was recorded —
                // nothing to calibrate, not a failure.
                tracing::debug!(
                    target: "reg.memory",
                    goal_id = %goal_id,
                    "Goal score without a Brier — no prediction to calibrate"
                );
                continue;
            };
            let goal_entity = format!("curator:goal:{goal_id}");
            let create_records = match curator_store.h_mems_by_entity_prefix(&goal_entity) {
                Ok(records) => records
                    .into_iter()
                    .filter(|h_mem| h_mem.attribute == "kanban_goal_create")
                    .collect::<Vec<_>>(),
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        goal_id = %goal_id,
                        error = %e,
                        "Failed to query goal records for Brier calibration"
                    );
                    continue;
                }
            };
            if create_records.is_empty() {
                tracing::warn!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    goal_id = %goal_id,
                    brier,
                    "Goal score carried a Brier but no kanban_goal_create h_mem exists to calibrate"
                );
                continue;
            }
            let signal = (1.0 - 2.0 * brier).clamp(0.05, 0.95);
            let mut calibrated = 0usize;
            for create_record in create_records {
                let combined = hkask_memory::combine_confidences(
                    create_record.confidence,
                    Confidence::new(signal),
                );
                match curator_store.update_confidence(
                    &create_record.id,
                    create_record.value.clone(),
                    combined,
                ) {
                    Ok(()) => calibrated += 1,
                    Err(e) => tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        goal_id = %goal_id,
                        error = %e,
                        "Failed to calibrate goal-create confidence from Brier score"
                    ),
                }
            }
            tracing::info!(
                target: "reg.memory",
                thread_id = %thread_id,
                goal_id = %goal_id,
                brier,
                signal,
                calibrated,
                "Brier score calibrated goal-create memory confidence"
            );
        }
    }

    // ── 2. Clean + chunk the turn content ─────────────────────────────
    let cleaned = clean_turn_text(&record.user_input, &record.agent_response);
    if cleaned.trim().is_empty() {
        // Both sides empty: nothing durable to chunk. Goal events above still
        // landed. The distiller's empty-thread rule covers legacy rows.
        tracing::debug!(
            target: "reg.memory",
            thread_id = %thread_id,
            "Empty turn — no chunk h_mems written"
        );
        return Ok(IngestionReport::default());
    }

    let entity = format!("curator:thread:{thread_id}");
    let chunk_config = hkask_memory::text_chunking::ChunkConfig::new(
        MIN_CHUNK_WORDS,
        MAX_CHUNK_WORDS,
        0,
        SENTENCE_BOUNDARY,
    )
    .map_err(|error| MemoryError::Ingestion(format!("invalid turn chunk contract: {error}")))?;
    let chunking =
        hkask_memory::text_chunking::chunk_text_with_config(&cleaned, &entity, chunk_config);
    let chunking_report = chunking.report;
    let chunk_texts: Vec<String> = chunking
        .chunks
        .into_iter()
        .map(|chunk| chunk.text)
        .collect();
    if chunk_texts.is_empty() {
        return Ok(IngestionReport::default());
    }

    // ── 3. Content tags — one batched classifier-model call per turn ──
    // Structural dimensions (who/when/where/how) are deterministic and always
    // applied below; this pass adds the content-derived dimensions (what/why),
    // subjects, domain concepts, and expertise level. Any failure degrades to
    // structural-only — surfaced, never silent, never blocking the write.
    let content_tags = tag_chunks_with_llm(ctx, &chunk_texts).await;

    // ── 4. Embed every chunk in one batched call ──────────────────────
    // Skipped when no embedding port is available — h_mem writes are pure SQL
    // and don't need embeddings. Semantic recall degrades to keyword-only,
    // but the curator still has episodic memory of the turn.
    //
    // The embedding is stored under the SAME entity as the chunk h_mems
    // (the entity_ref invariant): KNN results join back to the h_mems, and
    // the stored passage_text lets the recall path pinpoint the matched
    // chunk instead of injecting the whole thread.
    let vectors: Option<Vec<Vec<f32>>> = match ctx.embedding_port.cloned() {
        Some(embedding_port) => {
            let embedding_model = ctx.embedding_model.to_string();
            let texts = chunk_texts.clone();
            let vectors = ctx
                .tokio_handle
                .spawn(async move { embedding_port.embed(&embedding_model, &texts).await })
                .await;
            match vectors {
                Ok(Ok(vectors)) if vectors.len() == chunk_texts.len() => Some(vectors),
                Ok(Ok(vectors)) => {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        expected = chunk_texts.len(),
                        got = vectors.len(),
                        "Embedding count mismatch — chunks written without embeddings"
                    );
                    None
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        error = %e,
                        "Failed to embed turn chunks — semantic recall degraded to keyword-only"
                    );
                    None
                }
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        error = %e,
                        "Embedding task panicked — chunks written without embeddings"
                    );
                    None
                }
            }
        }
        None => {
            tracing::warn!(
                target: "reg.memory",
                thread_id = %thread_id,
                "No embedding port — chunks written without embeddings (semantic recall degraded to keyword-only)"
            );
            None
        }
    };

    // ── 5. Write one h_mem per chunk + its embedding ──────────────────
    let mut report = IngestionReport {
        attempted: chunk_texts.len(),
        ..Default::default()
    };
    let embedding_expected = ctx.embedding_port.is_some();
    if !embedding_expected {
        report.degraded = report.attempted;
    }
    for (index, chunk_text) in chunk_texts.iter().enumerate() {
        let mut ontology = structural_ontology(&thread_id, turn_ms, index);
        if let Some(tags) = content_tags.as_ref().and_then(|tags| tags.get(index)) {
            ontology = merge_content_tags(ontology, tags);
        }

        let chunk_h_mem = HMem::new(
            &entity,
            &format!("chunk:{index}"),
            serde_json::Value::String(chunk_text.clone()),
            ctx.curator_webid,
        )
        .with_visibility(Visibility::Shared)
        .with_ontology(ontology)
        .with_confidence(Confidence::new(0.5));

        let mut chunk_stored = false;
        let mut chunk_embedded = !embedding_expected;
        if let Some(ref curator_store) = curator_store {
            match curator_store.store(chunk_h_mem) {
                Ok(()) => {
                    report.stored += 1;
                    chunk_stored = true;
                }
                Err(e) => tracing::warn!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    chunk_index = index,
                    error = %e,
                    "Failed to store chunk h_mem"
                ),
            }
            if let Some(vector) = vectors.as_ref().and_then(|v| v.get(index)) {
                match curator_store.store_embedding(
                    &entity,
                    vector,
                    ctx.embedding_model,
                    Some(chunk_text),
                ) {
                    Ok(_embedding_id) => {
                        report.embedded += 1;
                        chunk_embedded = true;
                    }
                    Err(e) => tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        chunk_index = index,
                        error = %e,
                        "Failed to store chunk embedding"
                    ),
                }
            }
        }
        if !chunk_stored || !chunk_embedded {
            report.failed += 1;
        }
    }

    let tagged = content_tags.as_ref().map_or(0, Vec::len);
    if report.degraded() {
        tracing::warn!(
            target: "reg.memory",
            thread_id = %thread_id,
            model = %model,
            is_curator_turn,
            attempted = report.attempted,
            stored = report.stored,
            embedded = report.embedded,
            failed = report.failed,
            degraded = report.degraded,
            tagged,
            source_words = chunking_report.source_words,
            emitted_words = chunking_report.emitted_words,
            overlap_words = chunking_report.overlap_words,
            "Turn memory ingestion completed with degradation"
        );
    } else {
        tracing::info!(
            target: "reg.memory",
            thread_id = %thread_id,
            model = %model,
            is_curator_turn,
            attempted = report.attempted,
            stored = report.stored,
            embedded = report.embedded,
            failed = report.failed,
            degraded = report.degraded,
            tagged,
            source_words = chunking_report.source_words,
            emitted_words = chunking_report.emitted_words,
            overlap_words = chunking_report.overlap_words,
            "Turn ingested into curator memory as tagged chunks"
        );
    }

    Ok(report)
}

/// Assemble the cleaned turn text with role prefixes.
///
/// The role prefixes live inside the text (not in a JSON envelope) so the
/// value is directly injectable on recall, directly usable as embedding
/// `passage_text`, and directly readable by the distiller — one shape, three
/// consumers. Base64-looking lines (media echoes, inline data URIs) are
/// dropped; code and prose survive.
fn clean_turn_text(user_input: &str, agent_response: &str) -> String {
    let mut out = String::new();
    for (role, text) in [("user", user_input), ("assistant", agent_response)] {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(role);
        out.push_str(": ");
        out.push_str(&strip_base64_lines(trimmed));
    }
    out
}

/// Drop lines that look like base64 payload noise: at least 200 chars, at
/// least 95% base64 alphabet. A 200-char line of pure base64 alphabet is
/// never source code or prose — it is an echoed media blob or data URI,
/// the noise that produced the 538KB single-value rows the therapy scan
/// found. Everything else survives verbatim.
fn strip_base64_lines(text: &str) -> String {
    text.lines()
        .filter(|line| !looks_like_base64(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn looks_like_base64(line: &str) -> bool {
    let trimmed = line.trim();
    let char_count = trimmed.chars().count();
    if char_count < 200 {
        return false;
    }
    // Spaces disqualify: prose and code wrap or contain punctuation;
    // base64 payload lines are contiguous alphabet runs. Allowing spaces
    // here made any long prose line match (observed: a 2000-word test
    // response stripped in full).
    let base64_chars = trimmed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        .count();
    base64_chars * 100 >= char_count * 95
}

/// The deterministic half of the ontology: dimensions derivable from the
/// record itself (who produced it, when, in which thread, as part of what
/// process) plus the PKO process anchor and per-turn provenance. The
/// content-derived half (what/why, subjects, domain concepts, expertise)
/// comes from the classifier-model pass and merges on top.
fn structural_ontology(thread_id: &str, turn_ms: u128, chunk_index: usize) -> HMemOntology {
    HMemOntology {
        dimensions: vec![
            Dimension::How.as_str().to_string(),
            Dimension::When.as_str().to_string(),
            Dimension::Who.as_str().to_string(),
            Dimension::Where.as_str().to_string(),
        ],
        dc_type: hkask_bridge_ontology::pko::STEP_EXECUTION.to_string(),
        dc_subject: Vec::new(),
        dc_source: format!("chat:{thread_id}:turn:{turn_ms}"),
        pko_procedure: Some("chat".to_string()),
        pko_step: Some(format!("chunk:{chunk_index}")),
        candidate_terms: Vec::new(),
        ontology_protocol: None,
        expertise_level: None,
        ontology_tags: std::collections::HashMap::new(),
    }
}

/// Tag the turn's chunks with the classifier model via the app-wide inference
/// port. Rendering and correlation are shared; structural ontology,
/// degradation, and persistence remain bridge-owned.
async fn tag_chunks_with_llm(
    ctx: &WriteContext<'_>,
    chunk_texts: &[String],
) -> Option<Vec<PassageTag>> {
    let classifier_model = ctx.classifier_model?;
    let port = crate::inference_chat::global_inference_port()?;
    let passages = chunk_texts
        .iter()
        .enumerate()
        .map(|(index, text)| Passage::new(format!("item-{index}"), text.clone()))
        .collect();
    let request = match PassageTaggingRequest::new(
        passages,
        vec!["what", "why"],
        vec!["who", "when", "where", "how"],
        ExpertiseMode::ModelAssigned,
    ) {
        Ok(request) => request,
        Err(error) => {
            tracing::warn!(target: "reg.memory", error = %error, "Invalid chunk tagging request — structural tags only");
            return None;
        }
    };
    let prompt = match render_deployed_tagging_prompt_at(ctx.template_root, &request) {
        Ok(prompt) => prompt,
        Err(error) => {
            tracing::warn!(target: "reg.memory", error = %error, "Required chunk tagging template unavailable — structural tags only");
            return None;
        }
    };
    let parameters = LLMParameters {
        temperature: 0.1,
        top_p: 0.9,
        top_k: 40,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
        min_p: 0.0,
        typical_p: 0.0,
        seed: None,
        thinking_allowed: false,
        adapter: None,
        system_prompt: None,
    };
    match port
        .generate_with_model(&prompt, &parameters, Some(classifier_model), None)
        .await
    {
        Ok(result) => match parse_tagging_response(&request, &result.text) {
            Ok(tags) => Some(tags),
            Err(error) => {
                tracing::warn!(
                    target: "reg.memory",
                    model = %result.model,
                    expected = chunk_texts.len(),
                    error = %error,
                    "Chunk tagging response rejected — structural tags only"
                );
                None
            }
        },
        Err(error) => {
            tracing::warn!(
                target: "reg.memory",
                error = %error,
                "Chunk tagging call failed — structural tags only"
            );
            None
        }
    }
}

/// Merge classifier judgments onto structural provenance. Candidate terms are
/// preserved, while namespaces and canonical concepts come only from the
/// shared published-ontology resolver.
fn merge_content_tags(mut ontology: HMemOntology, tags: &PassageTag) -> HMemOntology {
    for dimension in &tags.dimensions {
        if !ontology.dimensions.contains(dimension) {
            ontology.dimensions.push(dimension.clone());
        }
    }
    let canonical = canonicalize_terms(&tags.candidate_terms);
    ontology.dc_subject = canonical.candidate_terms.clone();
    ontology.candidate_terms = canonical.candidate_terms;
    ontology.ontology_tags = canonical.ontology_tags;
    ontology.ontology_protocol = Some(TERM_RESOLUTION_PROTOCOL.to_string());
    ontology.expertise_level = tags.expertise;
    ontology
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_turn_text_prefixes_roles_and_skips_empty_sides() {
        let cleaned = clean_turn_text("  hello  ", "");
        assert_eq!(cleaned, "user: hello");

        let cleaned = clean_turn_text("", "world");
        assert_eq!(cleaned, "assistant: world");

        let cleaned = clean_turn_text("hello", "world");
        assert_eq!(cleaned, "user: hello\n\nassistant: world");

        assert_eq!(clean_turn_text("", "  "), "");
    }

    #[test]
    fn clean_turn_text_strips_base64_noise_but_keeps_code() {
        let base64_line = "A".repeat(250);
        let base64_line = format!("{base64_line}+/=");
        let turn = format!(
            "user: embed this\nassistant: here is the code:\n{base64_line}\nfn main() {{}}"
        );
        let cleaned = clean_turn_text(&turn, "");
        assert!(!cleaned.contains(&base64_line), "base64 line dropped");
        assert!(cleaned.contains("fn main() {}"), "code survives");
        assert!(cleaned.contains("here is the code:"));
    }

    #[test]
    fn looks_like_base64_requires_length_and_purity() {
        assert!(!looks_like_base64(&"x".repeat(199)));
        assert!(looks_like_base64(&"A".repeat(250)));
        // Prose with spaces at length is not base64 — spaces disqualify.
        let prose = "word ".repeat(60);
        assert!(!looks_like_base64(prose.trim()));
    }

    #[test]
    fn structural_ontology_carries_structural_dimensions_and_provenance() {
        let ontology = structural_ontology("t1", 123, 2);
        assert_eq!(
            ontology.dimensions,
            vec!["how", "when", "who", "where"],
            "the four deterministic dimensions, no content pair"
        );
        assert_eq!(ontology.pko_step.as_deref(), Some("chunk:2"));
        assert_eq!(ontology.dc_source, "chat:t1:turn:123");
        assert!(ontology.dc_subject.is_empty());
    }

    #[test]
    fn merge_content_tags_uses_shared_resolution_and_separate_expertise() {
        let base = structural_ontology("t1", 1, 0);
        let tags = PassageTag {
            correlation_id: "item-0".to_string(),
            dimensions: vec!["what".to_string(), "why".to_string()],
            candidate_terms: vec![
                "corporation".to_string(),
                "unknown idea".to_string(),
                "memory".to_string(),
            ],
            expertise: Some(hkask_types::corpus::ExpertiseLevel::Researcher),
        };
        let merged = merge_content_tags(base, &tags);
        assert_eq!(
            merged.dimensions,
            vec!["how", "when", "who", "where", "what", "why"]
        );
        assert_eq!(merged.dc_subject, ["corporation", "unknown idea", "memory"]);
        assert_eq!(
            merged.candidate_terms,
            ["corporation", "unknown idea", "memory"]
        );
        assert_eq!(
            merged.ontology_tags["fibo"],
            [hkask_bridge_ontology::fibo::CORPORATION]
        );
        assert_eq!(merged.ontology_tags["core"], ["5w1h_core"]);
        assert_eq!(
            merged.ontology_protocol.as_deref(),
            Some(TERM_RESOLUTION_PROTOCOL)
        );
        assert_eq!(
            merged.expertise_level,
            Some(hkask_types::corpus::ExpertiseLevel::Researcher)
        );
        assert!(!merged.ontology_tags.contains_key("expertise"));
    }

    /// expect: "In-process chunk tagging renders from the settings-threaded
    /// template root, not the process env" — `HKASK_TEMPLATE_ROOT` only
    /// reaches MCP server child processes; before this threading, every
    /// in-process turn ingest degraded to structural-only tags with
    /// `HKASK_TEMPLATE_ROOT is not configured`.
    /// pre: a sentinel template deployed at a temp registry root; the env
    /// var is NOT set; a capturing stub occupies the global inference port.
    /// post: the stub receives exactly one prompt rendered from the
    /// sentinel root — proving the render used `ctx.template_root`.
    #[tokio::test]
    async fn chunk_tagging_renders_from_the_threaded_template_root() {
        use hkask_types::{InferenceError, InferencePort, InferenceResult};

        let registry = tempfile::tempdir().expect("tempdir");
        let template_dir = registry.path().join("templates").join("docproc");
        std::fs::create_dir_all(&template_dir).expect("create template dir");
        std::fs::write(
            template_dir.join("tag-passages-batch.j2"),
            "SENTINEL-ROOT {{ passages[0].text }}",
        )
        .expect("write sentinel template");

        struct CapturingPort {
            prompts: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        }
        impl InferencePort for CapturingPort {
            fn generate(
                &self,
                prompt: &str,
                _parameters: &hkask_types::template::LLMParameters,
                _tools: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<InferenceResult, InferenceError>>
                        + Send
                        + '_,
                >,
            > {
                self.prompts
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(prompt.to_string());
                Box::pin(async { Err(InferenceError::Generation("capture-only stub".to_string())) })
            }
        }

        let prompts = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        crate::inference_chat::set_global_inference_port(std::sync::Arc::new(CapturingPort {
            prompts: prompts.clone(),
        }));

        let curator_store = CuratorStore::for_tests(None);
        let curator_consolidation = std::sync::Arc::new(std::sync::RwLock::new(
            build_curator_consolidation(0, &None),
        ));
        let tokio_handle = tokio::runtime::Handle::current();
        let ctx = WriteContext {
            curator_store: &curator_store,
            embedding_port: None,
            embedding_model: "test-model",
            classifier_model: Some("test-classifier"),
            template_root: registry.path(),
            curator_webid: WebID::from_persona(b"curator"),
            tokio_handle: &tokio_handle,
            curator_consolidation: &curator_consolidation,
            consolidation_cadence_secs: 0,
        };

        let tags = tag_chunks_with_llm(&ctx, &["sentinel passage text one two".to_string()]).await;
        crate::inference_chat::clear_global_inference_port();

        assert!(
            tags.is_none(),
            "the stub port errors, so tags degrade — but only AFTER a successful render"
        );
        let captured = prompts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            captured.len(),
            1,
            "the render must succeed from the threaded root and reach the port — \
             with the old env-based call this is 0 (TemplateRootNotConfigured)"
        );
        assert!(
            captured[0].starts_with("SENTINEL-ROOT"),
            "prompt must come from the sentinel registry, got: {}",
            captured[0]
        );
        assert!(
            captured[0].contains("sentinel passage text"),
            "chunk text reaches the template"
        );
    }
}
