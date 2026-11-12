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
//! thread entity `curator:thread:{thread_id}` with attribute `chunk:{turn_ms}:{index}` (turn-scoped:
//! the chunk index alone repeats every turn), its own embedding (stored with `passage_text` so KNN results pinpoint the
//! matched chunk), and a content-derived ontology blob. Raw transcript dumps —
//! the 500KB single-value rows the 2026-09-04 therapy scan found — are gone.

use std::sync::Arc;
use std::sync::RwLock;

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
    /// zed-kask: D100/FR-4 (operator ruling 2026-11-05) — the private
    /// sibling store: a turn from a privacy-marked thread routes its
    /// writes here (the record's `InferenceProvenance` is the routing
    /// signal — the same mark the zed side read from the thread).
    pub private_curator_store: &'a CuratorStore,
    pub embedding_port: Option<&'a LanguageModelEmbeddingPort>,
    pub embedding_model: &'a str,
    /// zed-kask: D100 item 3 (operator 2026-11-05) — the private leg's
    /// LOCAL embedding port and model. Private turns embed through THIS
    /// pair only; the configured cloud pair above never sees private
    /// content. Absent/empty keeps the keyword-only degrade, surfaced.
    pub private_embedding_port: Option<&'a LanguageModelEmbeddingPort>,
    pub private_embedding_model: &'a str,
    /// The classifier model used for write-time chunk tagging
    /// (`kask.models.classifier_model`). `None` = not configured — chunks
    /// get structural tags only (surfaced at wiring time, not per turn).
    pub classifier_model: Option<&'a str>,
    /// zed-kask: D100 classifier lift (operator ruling 2026-11-06) — the
    /// private leg's LOCAL classifier model
    /// (`kask.private_inference.classifier_model`, default
    /// `ollama/qwen3.5:4b`); `None` = the opt-out or a refused
    /// cloud-prefixed value — private turns keep structural tags only,
    /// surfaced per turn; the configured cloud classifier never sees
    /// private content.
    pub private_classifier_model: Option<&'a str>,
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
    /// zed-kask: D100 (operator ruling 2026-11-06, the review's item 4) —
    /// the private store's consolidation TWIN, same rebuild discipline
    /// (rebuilt after the private store's heal) and the same one-tick fire.
    pub private_curator_consolidation: &'a Arc<RwLock<Option<Arc<MemoryConsolidator>>>>,
    pub consolidation_cadence_secs: u64,
}

/// Durable per-chunk outcomes from one turn ingestion attempt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    pub(crate) fn degraded(&self) -> bool {
        self.failed > 0 || self.degraded > 0
    }
}

/// Embed one batch of chunk texts through `port` at the store's width —
/// the shared request/response ladder for the public (configured cloud)
/// and private (local ollama) ingest legs. A count mismatch and every
/// failure are surfaced as a warned `None` (chunks stored
/// keyword-recallable only, never blocking the write).
async fn embed_chunk_batch(
    port: &LanguageModelEmbeddingPort,
    model: &str,
    texts: &[String],
    dimensions: u32,
    tokio_handle: &tokio::runtime::Handle,
    thread_id: &str,
) -> Option<Vec<Vec<f32>>> {
    let expected = texts.len();
    let texts = texts.to_vec();
    let model = model.to_string();
    let port = port.clone();
    match tokio_handle
        .spawn(async move {
            port.embed_with_dimensions(&model, &texts, Some(dimensions))
                .await
                .map(|batch| batch.vectors)
        })
        .await
    {
        Ok(Ok(vectors)) if vectors.len() == expected => Some(vectors),
        Ok(Ok(vectors)) => {
            tracing::warn!(
                target: "reg.memory",
                thread_id = %thread_id,
                expected = expected,
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

/// Write a completed turn into the curator's memory as cleaned, embedded,
/// ontologically tagged chunks — one shared copy per turn.
///
/// Performs, in order:
/// 1. Curator-store self-heal re-open + consolidation rebuild (if healed).
/// 2. Clean the turn text (role prefixes, base64-noise stripping) and chunk it
///    into word-bounded passages.
/// 3. Tag: structural dimensions (who/when/where/how — derivable from the
///    record without an LLM) always; content dimensions (what/why, subjects,
///    domain concepts, expertise) via one batched classifier-model call when
///    the inference port is wired and the model configured. Tagging failure
///    degrades to structural-only with a warn — never blocks the write.
/// 4. Embed every chunk in one batched call; each vector is stored under the
///    thread entity with its `passage_text` so KNN pinpoints the matched chunk.
/// 5. Write one h_mem per chunk at the 0.5 confidence floor.
///
/// Returns durable attempted/stored/embedded/failed counts. Curator-side,
/// embedding, and tagging failures are non-fatal — they warn and continue
/// (the failure-signal rule: the operator
/// must be able to distinguish "not configured" from "configured but broken",
/// so every degradation path logs).
///
/// Every turn chunk written here enters at the
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
    // zed-kask: D100/FR-4 (operator ruling 2026-11-05) — the write-routing
    // signal: a turn produced on a privacy-marked thread lands in the
    // PRIVATE store, everything else in the public store. The provenance
    // rides the record from the zed side (the producing thread's D100
    // mark) — never caller-set on the write side.
    let private_turn =
        record.inference_provenance == hkask_types::InferenceProvenance::PrivateInference;

    // Turn identity for chunk provenance/ordering. TurnRecord carries no
    // ordinal or timestamp, so write time stands in for turn-completion time.
    let turn_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();

    // Resolve the curator stores once per ingestion. The consolidation
    // rebuilds stay keyed on their OWN store's heal: the public twin on the
    // public store, the private twin on the private sibling (operator
    // ruling 2026-11-06, the review's item 4 — the private store's
    // forgetting mirror; the former v1 stated limit is lifted).
    let public_curator_store = ctx.curator_store.get();
    if public_curator_store.is_some() {
        let needs_rebuild = match ctx.curator_consolidation.read() {
            Ok(guard) => guard.is_none(),
            Err(_) => true,
        };
        if needs_rebuild && ctx.consolidation_cadence_secs > 0 {
            let rebuilt =
                build_curator_consolidation(ctx.consolidation_cadence_secs, &public_curator_store);
            if let Ok(mut guard) = ctx.curator_consolidation.write()
                && guard.is_none()
            {
                *guard = rebuilt;
            }
        }
    }
    let private_store_for_rebuild = ctx.private_curator_store.get();
    if private_store_for_rebuild.is_some() {
        let needs_rebuild = match ctx.private_curator_consolidation.read() {
            Ok(guard) => guard.is_none(),
            Err(_) => true,
        };
        if needs_rebuild && ctx.consolidation_cadence_secs > 0 {
            let rebuilt = build_curator_consolidation(
                ctx.consolidation_cadence_secs,
                &private_store_for_rebuild,
            );
            if let Ok(mut guard) = ctx.private_curator_consolidation.write()
                && guard.is_none()
            {
                *guard = rebuilt;
            }
        }
    }
    // The store this turn writes to: the private sibling for a private
    // turn, the public store otherwise.
    let curator_store = if private_turn {
        ctx.private_curator_store.get()
    } else {
        ctx.curator_store.get()
    };

    // ── 2. Clean + chunk the turn content ─────────────────────────────
    let cleaned = clean_turn_text(&record.user_input, &record.agent_response);
    if cleaned.trim().is_empty() {
        // Both sides empty: nothing durable to chunk.
        tracing::debug!(
            target: "reg.memory",
            thread_id = %thread_id,
            "Empty turn — no chunk h_mems written"
        );
        return Ok(IngestionReport {
            ..Default::default()
        });
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
        return Ok(IngestionReport {
            ..Default::default()
        });
    }

    // ── 3. Content tags — one batched classifier-model call per turn ──
    // Structural dimensions (who/when/where/how) are deterministic and always
    // applied below; this pass adds the content-derived dimensions (what/why),
    // subjects, domain concepts, and expertise level. Any failure degrades to
    // structural-only — surfaced, never silent, never blocking the write.
    //
    // zed-kask: D100 (operator ruling 2026-11-05): a PRIVATE turn never
    // reaches the configured classifier — it is a cloud model, and private
    // content never crosses to cloud models ("any tools assisting on
    // private threads need to be limited to private inference"). Private
    // chunks get the structural-only degradation, surfaced.
    let content_tags = match (private_turn, tagging_model_for_turn(ctx, private_turn)) {
        (_, Some(model)) => tag_chunks_with_model(ctx, &chunk_texts, model).await,
        (true, None) => {
            tracing::warn!(
                target: "reg.memory",
                thread_id = %thread_id,
                "private turn: classifier tagging skipped — no local private classifier model is wired (set kask.private_inference.classifier_model to lift the degrade); chunks get structural tags only — never the configured cloud classifier"
            );
            None
        }
        (false, None) => None,
    };

    // ── 4. Embed every chunk in one batched call ──────────────────────────
    // Skipped when no embedding port is available — h_mem writes are pure SQL
    // and don't need embeddings. Semantic recall degrades to keyword-only,
    // but the curator still has episodic memory of the turn.
    //
    // The embedding is stored under the SAME entity as the chunk h_mems
    // (the entity_ref invariant): KNN results join back to the h_mems, and
    // the stored passage_text lets the recall path pinpoint the matched
    // chunk instead of injecting the whole thread.
    // zed-kask: D100 item 3 (operator 2026-11-05) — a PRIVATE turn embeds
    // through the LOCAL ollama port ONLY (`kask.private_inference.embedding_model`);
    // the configured cloud embedder never sees private content
    // (degrade-not-leak, lifted). An unwired or failed local port keeps
    // the keyword-only degrade, surfaced.
    let vectors: Option<Vec<Vec<f32>>> = if private_turn {
        match (
            ctx.private_embedding_port,
            ctx.private_embedding_model.is_empty(),
        ) {
            (Some(port), false) => {
                embed_chunk_batch(
                    port,
                    ctx.private_embedding_model,
                    &chunk_texts,
                    ctx.private_curator_store.embedding_dim() as u32,
                    ctx.tokio_handle,
                    &thread_id,
                )
                .await
            }
            _ => {
                tracing::warn!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    "private turn: the local embedding port is not wired — chunks stored keyword-recallable only (set kask.private_inference.embedding_model to lift the degrade; private content never widens to the configured cloud embedder)"
                );
                None
            }
        }
    } else {
        match ctx.embedding_port {
            Some(port) => {
                embed_chunk_batch(
                    port,
                    ctx.embedding_model,
                    &chunk_texts,
                    ctx.curator_store.embedding_dim() as u32,
                    ctx.tokio_handle,
                    &thread_id,
                )
                .await
            }
            None => {
                tracing::warn!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    "No embedding port — chunks written without embeddings (semantic recall degraded to keyword-only)"
                );
                None
            }
        }
    };

    // ── 5. Write one h_mem per chunk + its embedding ──────────────────
    let mut report = IngestionReport {
        attempted: chunk_texts.len(),
        ..Default::default()
    };
    // Embedding was expected when the turn's leg had a wired port+model:
    // the configured cloud leg (public turns) or the local ollama leg
    // (private turns — D100 item 3). An expected-but-failed embed counts
    // per-chunk below; an unwired leg reports degraded up front.
    let embedding_expected = if private_turn {
        ctx.private_embedding_port.is_some() && !ctx.private_embedding_model.is_empty()
    } else {
        ctx.embedding_port.is_some()
    };
    if !embedding_expected {
        report.degraded = report.attempted;
    }
    // FR-4 (D-2): hoisted once — the provenance stamps every chunk below.
    let inference_provenance = record.inference_provenance;
    // The model string that produced `vectors` — the LEG's model, not the
    // other leg's. The store's KNN model gate joins on this label: a
    // private chunk embedded locally must carry the LOCAL model string or
    // the private-context query (embedded with the same local model)
    // would exclude it as a mismatch (D100 item 3).
    let leg_embedding_model: &str = if private_turn {
        ctx.private_embedding_model
    } else {
        ctx.embedding_model
    };
    for (index, chunk_text) in chunk_texts.iter().enumerate() {
        let mut ontology = structural_ontology(&thread_id, turn_ms, index);
        if let Some(tags) = content_tags.as_ref().and_then(|tags| tags.get(index)) {
            ontology.merge_tag_judgments(&tags.dimensions, &tags.candidate_terms, tags.expertise);
        }

        let chunk_h_mem = HMem::new(
            &entity,
            // The turn is part of the key: `chunk:{index}` alone repeats in
            // every turn of a thread, so distinct turns looked like duplicates.
            &format!("chunk:{turn_ms}:{index}"),
            serde_json::Value::String(chunk_text.clone()),
            ctx.curator_webid,
        )
        .with_visibility(Visibility::Shared)
        .with_ontology(ontology)
        // FR-4 (D-2): the producing thread's inference provenance rides the
        // record from the zed side (the D100 mark read at turn completion)
        // and stamps every chunk — content produced on a privacy-marked
        // thread never enters the store unmarked. Pinned by
        // `ingest_turn_stamps_private_routed_turn_chunks_private` and
        // `ingest_turn_leaves_public_turn_chunks_public`.
        .with_inference_provenance(inference_provenance)
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
                    leg_embedding_model,
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

/// zed-kask: D100 classifier lift (operator ruling 2026-11-06) — the
/// tagging-model decision: a private turn tags through the private leg's
/// LOCAL classifier model only (never the configured cloud classifier); a
/// public turn keeps the configured model. `None` = structural-only — for a
/// private turn the opt-out or a refused cloud-prefixed value (surfaced per
/// turn by the caller); for a public turn the configured model's absence
/// (surfaced once at wiring time, never per turn).
fn tagging_model_for_turn<'a>(ctx: &WriteContext<'a>, private_turn: bool) -> Option<&'a str> {
    if private_turn {
        ctx.private_classifier_model
    } else {
        ctx.classifier_model
    }
}

/// Tag the turn's chunks with the given classifier model via the app-wide
/// inference port (the model string is the routing signal — the private
/// leg passes its local model, public turns the configured one). Rendering
/// and correlation are shared; structural ontology, degradation, and
/// persistence remain bridge-owned.
async fn tag_chunks_with_model(
    ctx: &WriteContext<'_>,
    chunk_texts: &[String],
    model: &str,
) -> Option<Vec<PassageTag>> {
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
        system_prompt: None,
        timeout_hint_secs: None,
    };
    match port
        .generate_with_model(&prompt, &parameters, Some(model), None)
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
        // D61-style scoped port — the classifier-race fix: this test's port
        // is thread-local, so a parallel test can no longer clear or replace
        // it mid-flight, and the process global stays untouched.
        let _scoped_port =
            crate::inference_chat::set_scoped_inference_port(std::sync::Arc::new(CapturingPort {
                prompts: prompts.clone(),
            }));

        let curator_store = CuratorStore::for_tests(None);
        let curator_consolidation = std::sync::Arc::new(std::sync::RwLock::new(
            build_curator_consolidation(0, &None),
        ));
        let private_curator_consolidation = std::sync::Arc::new(std::sync::RwLock::new(
            build_curator_consolidation(0, &None),
        ));
        let tokio_handle = tokio::runtime::Handle::current();
        let ctx = WriteContext {
            curator_store: &curator_store,
            private_curator_store: &CuratorStore::for_tests(None),
            embedding_port: None,
            embedding_model: "test-model",
            private_embedding_port: None,
            private_embedding_model: "",
            classifier_model: Some("test-classifier"),
            private_classifier_model: None,
            template_root: registry.path(),
            curator_webid: WebID::from_persona(b"curator"),
            tokio_handle: &tokio_handle,
            curator_consolidation: &curator_consolidation,
            private_curator_consolidation: &private_curator_consolidation,
            consolidation_cadence_secs: 0,
        };

        let tags = tag_chunks_with_model(
            &ctx,
            &["sentinel passage text one two".to_string()],
            ctx.classifier_model.expect("configured in this fixture"),
        )
        .await;

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

    /// zed-kask: D100 classifier lift (operator ruling 2026-11-06) — the
    /// tagging-model decision's table: a private turn takes the private
    /// leg's LOCAL classifier model only (never the configured cloud
    /// classifier); a public turn keeps the configured model; the private
    /// absence is its own surfaced structural-only degrade.
    #[tokio::test]
    async fn tagging_model_for_turn_pins_the_provenance_table() {
        let curator_store = CuratorStore::for_tests(None);
        let private_curator_store = CuratorStore::for_tests(None);
        let consolidation = std::sync::Arc::new(std::sync::RwLock::new(
            build_curator_consolidation(0, &None),
        ));
        let tokio_handle = tokio::runtime::Handle::current();
        let context = |private_classifier_model: Option<&'static str>| WriteContext {
            curator_store: &curator_store,
            private_curator_store: &private_curator_store,
            embedding_port: None,
            embedding_model: "",
            private_embedding_port: None,
            private_embedding_model: "",
            classifier_model: Some("cloud/classifier"),
            private_classifier_model,
            template_root: std::path::Path::new("unused-in-this-decision-test"),
            curator_webid: WebID::from_persona(b"curator"),
            tokio_handle: &tokio_handle,
            curator_consolidation: &consolidation,
            private_curator_consolidation: &consolidation,
            consolidation_cadence_secs: 0,
        };

        assert_eq!(
            tagging_model_for_turn(&context(Some("ollama/qwen3.5:4b")), true),
            Some("ollama/qwen3.5:4b"),
            "a private turn tags through the private leg's local model"
        );
        assert_eq!(
            tagging_model_for_turn(&context(Some("ollama/qwen3.5:4b")), false),
            Some("cloud/classifier"),
            "a public turn keeps the configured model unchanged"
        );
        assert_eq!(
            tagging_model_for_turn(&context(None), true),
            None,
            "a private turn with the opt-out degrades to structural-only — never the cloud classifier"
        );
    }

    /// D100 classifier lift (operator ruling 2026-11-06), the end-to-end
    /// routing pin: a private turn's chunk-tagging generate call carries the
    /// PRIVATE classifier model (recorded at the port, after the sentinel
    /// render succeeds); the public turn's carries the configured model,
    /// unchanged. The stub errors after recording — the tag calls degrade,
    /// the routing is the assertion. One port instance spans both legs so
    /// the global-port window stays single.
    #[tokio::test]
    async fn private_turn_tag_generate_carries_the_private_classifier_model() {
        use hkask_types::{InferenceError, InferencePort, InferenceResult};

        let registry = tempfile::tempdir().expect("tempdir");
        let template_dir = registry.path().join("templates").join("docproc");
        std::fs::create_dir_all(&template_dir).expect("create template dir");
        std::fs::write(
            template_dir.join("tag-passages-batch.j2"),
            "SENTINEL-ROOT {{ passages[0].text }}",
        )
        .expect("write sentinel template");

        struct ModelRecordingPort {
            models: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        }
        impl InferencePort for ModelRecordingPort {
            fn generate(
                &self,
                _prompt: &str,
                _parameters: &hkask_types::template::LLMParameters,
                _tools: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<InferenceResult, InferenceError>>
                        + Send
                        + '_,
                >,
            > {
                Box::pin(async { Err(InferenceError::Generation("recording stub".to_string())) })
            }
            fn generate_with_model(
                &self,
                _prompt: &str,
                _parameters: &hkask_types::template::LLMParameters,
                model_override: Option<&str>,
                _tools: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<InferenceResult, InferenceError>>
                        + Send
                        + '_,
                >,
            > {
                self.models
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(model_override.unwrap_or("<unspecified>").to_string());
                Box::pin(async { Err(InferenceError::Generation("recording stub".to_string())) })
            }
        }

        let models: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        // D61-style scoped port (the classifier race): thread-local, so the
        // parallel suite cannot clear or replace it mid-turn, and the
        // process global stays untouched.
        let _scoped_port = crate::inference_chat::set_scoped_inference_port(std::sync::Arc::new(
            ModelRecordingPort {
                models: models.clone(),
            },
        ));

        let curator_store = CuratorStore::for_tests(None);
        let consolidation = std::sync::Arc::new(std::sync::RwLock::new(
            build_curator_consolidation(0, &None),
        ));
        let tokio_handle = tokio::runtime::Handle::current();
        let ctx = WriteContext {
            curator_store: &curator_store,
            private_curator_store: &CuratorStore::for_tests(None),
            embedding_port: None,
            embedding_model: "test-model",
            private_embedding_port: None,
            private_embedding_model: "",
            classifier_model: Some("test-classifier"),
            private_classifier_model: Some("test-private-classifier"),
            template_root: registry.path(),
            curator_webid: WebID::from_persona(b"curator"),
            tokio_handle: &tokio_handle,
            curator_consolidation: &consolidation,
            private_curator_consolidation: &consolidation,
            consolidation_cadence_secs: 0,
        };

        // The private turn: the tag call carries the private model.
        let private_record = TurnRecord {
            thread_id: "private-classifier-thread".to_string(),
            user_input: "a private question that must tag locally".to_string(),
            agent_response: "a private answer".to_string(),
            model: "private-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            inference_provenance: hkask_types::InferenceProvenance::PrivateInference,
        };
        write_turn(&ctx, private_record)
            .await
            .expect("private turn ingest");

        // The public turn, same server: the configured model.
        let public_record = TurnRecord {
            thread_id: "public-classifier-thread".to_string(),
            user_input: "a public question".to_string(),
            agent_response: "a public answer".to_string(),
            model: "public-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
        };
        write_turn(&ctx, public_record)
            .await
            .expect("public turn ingest");

        let recorded = models
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            *recorded,
            vec![
                "test-private-classifier".to_string(),
                "test-classifier".to_string(),
            ],
            "the private turn tags through the private model, the public turn through the configured — got: {recorded:?}"
        );
    }
}
