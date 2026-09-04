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
    pub curator_webid: WebID,
    pub tokio_handle: &'a tokio::runtime::Handle,
    /// Self-healing curator consolidation service — rebuilt here after a
    /// curator-store heal so the timer promotes freshly-ingested curator h_mems.
    /// Behind an `Arc` shared with the timer, which re-reads it on each tick.
    pub curator_consolidation: &'a Arc<RwLock<Option<Arc<MemoryConsolidator>>>>,
    pub consolidation_cadence_secs: u64,
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
/// `Ok(())` on success. Curator-side, embedding, and tagging failures are
/// non-fatal — they warn and continue (the failure-signal rule: the operator
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
) -> Result<(), MemoryError> {
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
    // The goal store is ephemeral (operator ruling 2026-08-29: zed-agent
    // goals are ephemeral; the curator's memory is the durable vehicle).
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
        return Ok(());
    }

    let entity = format!("curator:thread:{thread_id}");
    let chunks = hkask_memory::chunk_text(
        &cleaned,
        &entity,
        MIN_CHUNK_WORDS,
        MAX_CHUNK_WORDS,
        SENTENCE_BOUNDARY,
    );
    let chunk_texts: Vec<String> = chunks.into_iter().map(|(_, text)| text).collect();
    if chunk_texts.is_empty() {
        return Ok(());
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
            tracing::debug!(
                target: "reg.memory",
                thread_id = %thread_id,
                "No embedding port — chunks written without embeddings (semantic recall degraded to keyword-only)"
            );
            None
        }
    };

    // ── 5. Write one h_mem per chunk + its embedding ──────────────────
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

        if let Some(ref curator_store) = curator_store {
            if let Err(e) = curator_store.store(chunk_h_mem) {
                tracing::warn!(
                    target: "reg.memory",
                    thread_id = %thread_id,
                    chunk_index = index,
                    error = %e,
                    "Failed to store chunk h_mem"
                );
            }
            if let Some(vector) = vectors.as_ref().and_then(|v| v.get(index)) {
                if let Err(e) = curator_store.store_embedding(
                    &entity,
                    vector,
                    ctx.embedding_model,
                    Some(chunk_text),
                ) {
                    tracing::warn!(
                        target: "reg.memory",
                        thread_id = %thread_id,
                        chunk_index = index,
                        error = %e,
                        "Failed to store chunk embedding"
                    );
                }
            }
        }
    }

    tracing::info!(
        target: "reg.memory",
        thread_id = %thread_id,
        model = %model,
        is_curator_turn,
        chunks = chunk_texts.len(),
        tagged = content_tags.map(|tags| tags.len()).unwrap_or(0),
        embedded = vectors.map(|v| v.len()).unwrap_or(0),
        "Turn ingested into curator memory as tagged chunks"
    );

    Ok(())
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
        ontology_tags: std::collections::HashMap::new(),
    }
}

/// Content tags for one chunk, as parsed from the classifier model's JSON.
#[derive(Debug, Default, PartialEq)]
struct ChunkContentTags {
    dimensions: Vec<String>,
    dc_subject: Vec<String>,
    ontology_tags: std::collections::HashMap<String, Vec<String>>,
    expertise_level: String,
}

/// The content dimensions the LLM may add. The structural four (who/when/
/// where/how) are already applied; `what` and `why` are the content-derived
/// pair. Anything else the model emits is dropped.
const CONTENT_DIMENSION_ALLOWLIST: [&str; 2] = ["what", "why"];

/// The expertise levels the corpus tagging schema defines; anything else
/// falls back to the middle rung.
const EXPERTISE_ALLOWLIST: [&str; 3] = ["practitioner", "analyst", "researcher"];

const TAGGING_SYSTEM_PROMPT: &str = "You are an ontology tagger for a memory system. For each numbered passage, return one JSON object with these fields:\n\
- \"dimensions\": array, subset of [\"what\", \"why\"] — what the passage is about, why it matters\n\
- \"dc_subject\": 1-5 short subject keywords\n\
- \"ontology_tags\": object mapping namespace keys (\"fibo\", \"golem\", \"pko\", \"schema\", \"other\") to arrays of 1-5 domain concepts\n\
- \"expertise_level\": one of \"practitioner\", \"analyst\", \"researcher\"\n\
Respond with ONLY a JSON array containing exactly one object per passage, in passage order. No prose, no code fences.";

/// Tag the turn's chunks with the classifier model via the app-wide
/// inference port. One batched call per turn. Returns `None` on any
/// degradation (port not wired, model not configured, call failed,
/// unparseable or length-mismatched response) — the caller falls back to
/// structural-only tags. A length mismatch distrusts the whole batch:
/// index-mapping ambiguity makes partial tags worse than none (the corpus
/// tag_batch_size trap was silent partial tagging).
async fn tag_chunks_with_llm(
    ctx: &WriteContext<'_>,
    chunk_texts: &[String],
) -> Option<Vec<ChunkContentTags>> {
    let classifier_model = ctx.classifier_model?;
    let port = crate::inference_chat::global_inference_port()?;
    let prompt = build_tag_prompt(chunk_texts);
    let parameters = LLMParameters {
        temperature: 0.1,
        top_p: 0.9,
        top_k: 40,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
        min_p: 0.0,
        typical_p: 0.0,
        seed: None,
        // Tagging needs output tokens, not reasoning tokens.
        thinking_allowed: false,
        adapter: None,
        system_prompt: None,
    };
    let result = port
        .generate_with_model(&prompt, &parameters, Some(classifier_model), None)
        .await;
    match result {
        Ok(result) => {
            let tags = parse_chunk_tags(&result.text, chunk_texts.len());
            if tags.is_none() {
                tracing::warn!(
                    target: "reg.memory",
                    model = %result.model,
                    expected = chunk_texts.len(),
                    "Chunk tagging response unparseable or length-mismatched — structural tags only"
                );
            }
            tags
        }
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                "Chunk tagging call failed — structural tags only"
            );
            None
        }
    }
}

fn build_tag_prompt(chunk_texts: &[String]) -> String {
    let mut prompt = String::from(TAGGING_SYSTEM_PROMPT);
    prompt.push_str("\n\n");
    for (index, text) in chunk_texts.iter().enumerate() {
        prompt.push_str(&format!("--- Passage {} ---\n{}\n\n", index + 1, text));
    }
    prompt.push_str(&format!(
        "Return the JSON array of {} objects now.",
        chunk_texts.len()
    ));
    prompt
}

/// Parse the tagging response. `extract_json_from_response` handles fenced
/// and embedded JSON; a single object (the model returning one object for
/// one passage) is wrapped into an array. Returns `None` when the response
/// is unparseable or the object count doesn't match the passage count.
fn parse_chunk_tags(response: &str, expected: usize) -> Option<Vec<ChunkContentTags>> {
    let raw = hkask_types::json_extract::extract_json_from_response(response);
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let items = match parsed {
        serde_json::Value::Array(items) => items,
        single @ serde_json::Value::Object(_) => vec![single],
        _ => return None,
    };
    if items.len() != expected {
        return None;
    }
    Some(items.iter().map(ChunkContentTags::from_value).collect())
}

impl ChunkContentTags {
    /// Lenient field extraction: string-or-array coercion for list fields
    /// (models emit both shapes), allowlist filtering for dimensions and
    /// expertise, caps on subject and concept counts.
    fn from_value(value: &serde_json::Value) -> Self {
        let dimensions = string_array_field(value, "dimensions")
            .into_iter()
            .filter(|dim| CONTENT_DIMENSION_ALLOWLIST.contains(&dim.as_str()))
            .collect();
        let dc_subject = string_array_field(value, "dc_subject")
            .into_iter()
            .take(5)
            .collect();
        let mut ontology_tags = std::collections::HashMap::new();
        if let Some(map) = value.get("ontology_tags").and_then(|v| v.as_object()) {
            for (namespace, concepts) in map {
                let concepts: Vec<String> = string_or_array(concepts).into_iter().take(5).collect();
                if !concepts.is_empty() {
                    ontology_tags.insert(namespace.trim().to_lowercase(), concepts);
                }
            }
        }
        let expertise_level = value
            .get("expertise_level")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_lowercase())
            .filter(|s| EXPERTISE_ALLOWLIST.contains(&s.as_str()))
            .unwrap_or_else(|| "analyst".to_string());
        Self {
            dimensions,
            dc_subject,
            ontology_tags,
            expertise_level,
        }
    }
}

/// Extract a field as a list of trimmed, non-empty strings, accepting either
/// a JSON array of strings or a single string.
fn string_array_field(value: &serde_json::Value, field: &str) -> Vec<String> {
    value
        .get(field)
        .map(string_or_array)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect()
}

fn string_or_array(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(s) => vec![s.trim().to_string()],
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Merge the content-derived tags onto the structural ontology. Content
/// dimensions append (deduped); subjects replace the empty structural list;
/// domain concepts merge per namespace; expertise files under the
/// `expertise` namespace (HMemOntology has no dedicated field — the
/// open-world map is the substrate).
fn merge_content_tags(mut ontology: HMemOntology, tags: &ChunkContentTags) -> HMemOntology {
    for dimension in &tags.dimensions {
        if !ontology.dimensions.contains(dimension) {
            ontology.dimensions.push(dimension.clone());
        }
    }
    ontology.dc_subject = tags.dc_subject.clone();
    for (namespace, concepts) in &tags.ontology_tags {
        ontology
            .ontology_tags
            .entry(namespace.clone())
            .or_default()
            .extend(concepts.iter().cloned());
    }
    if !tags.expertise_level.is_empty() {
        ontology
            .ontology_tags
            .entry("expertise".to_string())
            .or_default()
            .push(tags.expertise_level.clone());
    }
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
    fn parse_chunk_tags_accepts_array_and_single_object() {
        let array = r#"[{"dimensions":["what"],"dc_subject":["memory"],"ontology_tags":{"fibo":["moat"]},"expertise_level":"researcher"}]"#;
        let tags = parse_chunk_tags(array, 1).expect("array parses");
        assert_eq!(tags[0].dc_subject, vec!["memory"]);
        assert_eq!(tags[0].ontology_tags["fibo"], vec!["moat"]);
        assert_eq!(tags[0].expertise_level, "researcher");

        let single = r#"{"dimensions":["what"],"dc_subject":["memory"]}"#;
        assert!(parse_chunk_tags(single, 1).is_some(), "single object wraps");
    }

    #[test]
    fn parse_chunk_tags_rejects_length_mismatch_and_garbage() {
        let short = r#"[{"dimensions":["what"]}]"#;
        assert!(
            parse_chunk_tags(short, 2).is_none(),
            "count mismatch distrusts mapping"
        );
        assert!(parse_chunk_tags("no json here", 1).is_none());
        assert!(
            parse_chunk_tags("[]", 1).is_none(),
            "empty array is a mismatch"
        );
    }

    #[test]
    fn parse_chunk_tags_coerces_string_fields_and_filters_allowlists() {
        // dc_subject as a bare string; dimensions outside the allowlist
        // dropped; expertise outside the allowlist falls back to analyst.
        let raw = r#"[{"dimensions":["what","spurious"],"dc_subject":"memory systems","ontology_tags":{"FIBO":["moat","edge"]},"expertise_level":"wizard"}]"#;
        let tags = parse_chunk_tags(raw, 1).expect("parses");
        assert_eq!(tags[0].dimensions, vec!["what"]);
        assert_eq!(tags[0].dc_subject, vec!["memory systems"]);
        assert_eq!(tags[0].ontology_tags["fibo"], vec!["moat", "edge"]);
        assert_eq!(tags[0].expertise_level, "analyst");
    }

    #[test]
    fn merge_content_tags_appends_dimensions_and_files_expertise() {
        let base = structural_ontology("t1", 1, 0);
        let tags = ChunkContentTags {
            dimensions: vec!["what".to_string(), "why".to_string()],
            dc_subject: vec!["memory".to_string()],
            ontology_tags: std::collections::HashMap::from([(
                "fibo".to_string(),
                vec!["moat".to_string()],
            )]),
            expertise_level: "researcher".to_string(),
        };
        let merged = merge_content_tags(base, &tags);
        assert_eq!(
            merged.dimensions,
            vec!["how", "when", "who", "where", "what", "why"]
        );
        assert_eq!(merged.dc_subject, vec!["memory"]);
        assert_eq!(merged.ontology_tags["fibo"], vec!["moat"]);
        assert_eq!(merged.ontology_tags["expertise"], vec!["researcher"]);
    }
}
