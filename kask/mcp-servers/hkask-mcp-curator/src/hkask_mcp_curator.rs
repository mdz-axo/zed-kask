#![cfg_attr(not(test), forbid(unsafe_code))]
// Test builds allow `unsafe` for the one-shot `env::set_var` in the
// tool-behavior harness (HKASK_EMBEDDING_MODEL — the semantic paths
// resolve it as an Option since the model_constants refactor). Production
// still forbids unsafe outright. Same pattern as hkask-mcp-media,
// hkask-inference, hkask-keystore.
#![warn(clippy::let_underscore_future)]
// `tokio` is in [dependencies] for the bin target's `#[tokio::main]`; the lib
// itself does not use it, so the unused_crate_dependencies lint fires on the
// lib target. This is the legitimate bin-needs-dep case.
#![allow(unused_crate_dependencies)]
//! hkask-mcp-curator — Curator MCP server library.
//!
//! Exposes Regulation history, memory search, and per-store liveness as MCP tools. Live metacognition health belongs
//! to the single built-in `curator_status` AgentTool; do not mirror it
//! in this child process or reconstruct its reading from event history.

pub(crate) mod backfill;
pub(crate) mod db;
pub(crate) mod distillation;
pub(crate) mod forgetting;
pub(crate) mod ontology;
pub(crate) mod thread_turns;
pub mod types;

pub use db::{CuratorDb, CuratorStores};

use hkask_mcp_server::server::{
    McpToolError, execute_tool, map_infra_error, map_memory_store_error,
};
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use hkask_types::regulation::RegulationSpan;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use serde_json::json;
use std::sync::Arc;

use types::*;

const SERVER_NAME: &str = "hkask-mcp-curator";

/// `curator_memory_recall`'s default and largest result bound. Turn storage
/// is stamped with one process value per chunk, so an axis recall can match
/// every stored turn; the bound keeps one call from returning them all.
const RECALL_DEFAULT_LIMIT: usize = 100;
const RECALL_MAX_LIMIT: usize = 500;

/// The most events one `curator_algedonic_log` call returns.
const ALGEDONIC_LOG_LIMIT: u64 = 500;

/// Cap on fragments one entity may contribute to semantic recall. A thread
/// entity holds one h_mem per turn; without a cap a single chatty thread
/// floods the whole result set and every other entity vanishes from recall.
const MAX_FRAGMENTS_PER_ENTITY: usize = 2;

/// A semantic-recall failure, structured by kind. Callers fall back to
/// exact-entity lookup and surface the Display message in the result's
/// `note` so the operator can tell "semantic recall broken" from "no
/// matching memories" (the `unwrap_or(0)` trap).
#[derive(Debug, thiserror::Error)]
enum SemanticRecallError {
    #[error("curator memory unavailable: {source}")]
    MemoryUnavailable {
        #[source]
        source: McpToolError,
    },
    #[error("embedding the recall query failed: {source}")]
    Embed {
        #[source]
        source: hkask_types::EmbeddingGenerationError,
    },
    #[error("embedding model returned no vector for the recall query")]
    NoVector,
    #[error("semantic search over curator memory failed: {source}")]
    Search {
        #[source]
        source: hkask_memory::MemoryStoreError,
    },
    #[error(
        "no embedding model configured — set kask.models.embedding_model (injected as HKASK_EMBEDDING_MODEL); kask never falls back to a hidden code constant"
    )]
    EmbeddingNotConfigured,
    /// zed-kask: D100's named follow-up (operator-approved 2026-11-06) — the
    /// private-call degrade: no private embedding model is configured, so
    /// the semantic leg is SKIPPED for private call contexts (the exact and
    /// keyword legs serve the call); the query never widens to the
    /// configured cloud embedder.
    #[error(
        "private call context: no private embedding model configured — set kask.private_inference.embedding_model (injected as HKASK_PRIVATE_EMBEDDING_MODEL); the semantic leg is skipped for private contexts and the query never widens to the configured cloud embedder"
    )]
    PrivateEmbeddingNotConfigured,
}

/// Semantic-recall outcome: the resolved `(h_mem, distance)` fragments
/// plus the number of KNN hits that failed to resolve to a matching h_mem —
/// the store errored, or the hit's entity or named passage is orphaned.
/// Zero fragments with failures > 0 means recall degraded, not an empty
/// store; callers surface the failure count so the two cannot be confused.
struct SemanticRecall {
    fragments: Vec<(hkask_storage::HMem, f64)>,
    resolution_failures: usize,
    /// KNN hits excluded because their stored model matches neither the
    /// query's requested nor its provider-confirmed actual model — surfaced
    /// in tool responses so an embedding-model migration window reads as
    /// degradation, never as an empty store.
    excluded_model_mismatch: usize,
    /// M5 (FR-4 read side, falsifier (b)): h_mems excluded because they
    /// carry the private-inference mark and the current call's context
    /// is public — surfaced by every consumer, never silently dropped
    /// (zero fragments with exclusions reads as filtered, not empty).
    excluded_private: usize,
}

/// M5 (FR-4 read side): whether the current call's provenance admits
/// the row — private-marked rows flow ONLY in private call contexts
/// (the `_meta` mark the parent sets; absent reads public, fail-closed
/// for recall). The ONE predicate every fragment-emitting site shares;
/// each site surfaces its own exclusion count.
pub(crate) fn call_admits(h_mem: &hkask_storage::HMem) -> bool {
    hkask_mcp_server::current_call_provenance()
        == hkask_types::InferenceProvenance::PrivateInference
        || h_mem.inference_provenance != hkask_types::InferenceProvenance::PrivateInference
}

fn join_provenance(
    a: hkask_types::InferenceProvenance,
    b: hkask_types::InferenceProvenance,
) -> hkask_types::InferenceProvenance {
    if a == hkask_types::InferenceProvenance::PrivateInference
        || b == hkask_types::InferenceProvenance::PrivateInference
    {
        hkask_types::InferenceProvenance::PrivateInference
    } else {
        hkask_types::InferenceProvenance::PublicProvider
    }
}

fn record_admitted(record: &RegulationRecord) -> bool {
    if hkask_mcp_server::current_call_provenance()
        == hkask_types::InferenceProvenance::PrivateInference
    {
        return true;
    }
    record.visibility != hkask_types::InferenceProvenance::PrivateInference.as_str()
        && record
            .observation
            .get("inference_provenance")
            .and_then(serde_json::Value::as_str)
            != Some(hkask_types::InferenceProvenance::PrivateInference.as_str())
}

/// zed-kask: D100 (operator-approved 2026-11-06) — the provenance-routed
/// model decision, the ONE home every private-leg model selection at the
/// child resolves through: the query-embed sites AND (Phase 2, 2026-11-06)
/// the write-side passage-embed sites. A private provenance takes the
/// private leg's LOCAL model or the surfaced degrade/error; a public
/// provenance takes the configured model or its surfaced error. The model
/// string IS the routing signal (the stored-vector model-string gate's
/// discipline): a private query vector lands in the local model's space,
/// so the KNN gate excludes every cloud-model row and surfaces
/// `model_mismatch_excluded` — the bridge's recorded posture ("the public
/// store's KNN leg is unreachable from private contexts by construction"),
/// made true at the child. The private leg never widens to the configured
/// cloud embedder.
fn routed_model_for_provenance(
    provenance: hkask_types::InferenceProvenance,
    private_model: Option<String>,
    configured_model: Option<String>,
) -> Result<String, SemanticRecallError> {
    match provenance {
        hkask_types::InferenceProvenance::PrivateInference => {
            private_model.ok_or(SemanticRecallError::PrivateEmbeddingNotConfigured)
        }
        _ => configured_model.ok_or(SemanticRecallError::EmbeddingNotConfigured),
    }
}

/// The rows the call's provenance admits, and how many it excluded — the
/// one filter every row-returning read shares, so each surfaces its count.
fn admitted(h_mems: Vec<hkask_storage::HMem>) -> (Vec<hkask_storage::HMem>, usize) {
    let total = h_mems.len();
    let kept: Vec<_> = h_mems.into_iter().filter(call_admits).collect();
    let excluded = total - kept.len();
    (kept, excluded)
}

/// The start of a lookback window, or `invalid_argument` when the amount
/// does not fit a calendar offset. `delta` is `None` when the unit
/// conversion itself overflowed.
fn lookback_start(
    field: &str,
    delta: Option<chrono::Duration>,
) -> Result<chrono::DateTime<chrono::Utc>, McpToolError> {
    delta
        .and_then(|delta| chrono::Utc::now().checked_sub_signed(delta))
        .ok_or_else(|| {
            McpToolError::invalid_argument(format!("{field} is too large to be a lookback window"))
        })
}

/// A confidence argument must be a probability; the store would otherwise
/// clamp an out-of-range value silently.
fn checked_confidence(field: &str, value: f64) -> Result<hkask_types::Confidence, McpToolError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(hkask_types::Confidence::new(value))
    } else {
        Err(McpToolError::invalid_argument(format!(
            "{field} must be between 0.0 and 1.0, got {value}"
        )))
    }
}

/// A free-text argument that must carry text.
fn require_text(field: &str, value: &str) -> Result<(), McpToolError> {
    if value.trim().is_empty() {
        Err(McpToolError::invalid_argument(format!(
            "{field} must not be empty"
        )))
    } else {
        Ok(())
    }
}

/// An unset embedding-model setting is absent state with a named remedy:
/// one kind at every site that needs the model (the write paths, federated
/// search, the model-mismatch sweep).
fn model_not_configured(context: &str, reason: &SemanticRecallError) -> McpToolError {
    McpToolError::failed_precondition(format!("{context}: {reason}"))
}

/// Classify an embedding failure under the canonical error contract: a
/// missing provider or credential is configuration to fix, a dropped
/// connection or a retryable status is transient, the rest is a fault.
pub(crate) fn embed_error(
    error: &hkask_types::EmbeddingGenerationError,
    context: &str,
) -> McpToolError {
    use hkask_types::EmbeddingGenerationError as E;
    let message = format!("{context}: {error}");
    match error {
        E::NotConfigured(_) => McpToolError::permission_denied(message),
        E::Connection(_) | E::Api(408 | 429 | 500..=599, _) => McpToolError::unavailable(message),
        E::InvalidRequest(_)
        | E::Api(..)
        | E::Json(_)
        | E::EmptyResponse
        | E::DimensionMismatch { .. } => McpToolError::internal(message),
    }
}

fn degradation_note(
    resolution_failures: usize,
    excluded_model_mismatch: usize,
    yielded: usize,
) -> Option<String> {
    let mut notes = Vec::new();
    if resolution_failures > 0 {
        notes.push(resolution_failure_note(resolution_failures, yielded));
    }
    if excluded_model_mismatch > 0 {
        notes.push(model_mismatch_note(excluded_model_mismatch));
    }
    (!notes.is_empty()).then(|| notes.join("; "))
}

/// Resolve an existing `HMem` by ID across the provenance-admitted store(s):
/// a `PrivateInference` call checks `curator-private.db` (when configured)
/// before `curator.db`; a `PublicProvider` call checks `curator.db` only and
/// enforces `call_admits` via `fetch_existing_h_mem`.
fn fetch_existing_h_mem_routed(
    db: &CuratorDb,
    field_name: &str,
    raw_id: &str,
    not_found_message: String,
    fetch_error_ctx: &str,
) -> Result<
    (
        Arc<hkask_memory::MemoryStore>,
        hkask_storage::HMemId,
        hkask_storage::HMem,
    ),
    McpToolError,
> {
    let id = raw_id.parse::<hkask_storage::HMemId>().map_err(|e| {
        McpToolError::invalid_argument(format!("Invalid {field_name} '{raw_id}': {e}"))
    })?;
    if hkask_mcp_server::current_call_provenance()
        == hkask_types::InferenceProvenance::PrivateInference
    {
        if let Some(private_memory) = db.private_memory()? {
            if let Some(h_mem) = private_memory
                .get_by_id(&id)
                .map_err(|e| map_memory_store_error(e, fetch_error_ctx))?
            {
                return Ok((private_memory, id, h_mem));
            }
        }
    }
    let public_memory = db.memory()?;
    let (id, h_mem) = fetch_existing_h_mem(
        &public_memory,
        field_name,
        raw_id,
        not_found_message,
        fetch_error_ctx,
    )?;
    Ok((public_memory, id, h_mem))
}

fn read_memories_for_call(
    db: &CuratorDb,
) -> Result<Vec<Arc<hkask_memory::MemoryStore>>, McpToolError> {
    let mut stores = Vec::with_capacity(2);
    if hkask_mcp_server::current_call_provenance()
        == hkask_types::InferenceProvenance::PrivateInference
    {
        if let Some(private_memory) = db.private_memory()? {
            stores.push(private_memory);
        }
    }
    let public_memory = db.memory()?;
    if !stores
        .iter()
        .any(|existing| Arc::ptr_eq(existing, &public_memory))
    {
        stores.push(public_memory);
    }
    Ok(stores)
}
fn fetch_existing_h_mem(
    memory: &hkask_memory::MemoryStore,
    field_name: &str,
    raw_id: &str,
    not_found_message: String,
    fetch_error_ctx: &str,
) -> Result<(hkask_storage::HMemId, hkask_storage::HMem), McpToolError> {
    let id = raw_id.parse::<hkask_storage::HMemId>().map_err(|e| {
        McpToolError::invalid_argument(format!("Invalid {field_name} '{raw_id}': {e}"))
    })?;
    let h_mem = memory
        .get_by_id(&id)
        .map_err(|e| map_memory_store_error(e, fetch_error_ctx))?
        .ok_or_else(|| McpToolError::not_found(not_found_message))?;
    // M5 mutation-surface extension (operator ruling 2026-11-07, dual-axis
    // review Q2): the provenance partition covers the direct-ID surface,
    // not just the recall legs — a public call context naming a
    // private-marked row is refused at the boundary (least privilege: an
    // ID the caller can name is not an access grant), never an existence
    // oracle with read/write/delete power. A private context flows
    // unchanged.
    if !call_admits(&h_mem) {
        return Err(McpToolError::permission_denied(
            "the named h_mem is private-marked and this call context is public — the \
             provenance boundary refuses the direct-ID access (recall legs filter with \
             surfaced excluded_private_rows; the mutation surface refuses typed)"
                .to_string(),
        ));
    }
    Ok((id, h_mem))
}

/// Note naming the KNN hits that failed h_mem resolution, shared by every
/// semantic call site so the wording stays identical. `yielded` is the
/// fragment count that DID resolve; when it is zero the note must say the
/// empty result is a resolution failure, not an empty store.
fn resolution_failure_note(failures: usize, yielded: usize) -> String {
    if yielded == 0 {
        format!("{failures} semantic hits failed h_mem resolution — recall degraded, not empty")
    } else {
        format!("{failures} semantic hits failed h_mem resolution — recall degraded")
    }
}

fn model_mismatch_note(excluded: usize) -> String {
    format!(
        "{excluded} stored embeddings were excluded from KNN because they were embedded \
         under a different model — re-embed pending, not an empty store"
    )
}

/// Serialize one h_mem as its recall wire fragment — the six fields every
/// recall surface shares. Site extras (distance, ontology) are
/// appended by the caller; each is last in its literal, so insertion keeps
/// the field order (serde_json preserve_order).
fn h_mem_fragment(t: &hkask_storage::HMem) -> serde_json::Value {
    json!({
        "id": t.id.to_string(),
        "entity": t.entity, "attribute": t.attribute,
        "value": t.value, "confidence": t.confidence,
        "provenance": t.provenance.as_str(),
    })
}

/// A semantic hit: the h_mem fragment plus its KNN distance.
fn h_mem_with_distance(t: &hkask_storage::HMem, distance: &f64) -> serde_json::Value {
    let mut fragment = h_mem_fragment(t);
    fragment["distance"] = json!(distance);
    fragment
}

/// One exact-entity recall leg: run `query` (an UNTOUCHED store read) and
/// return the leg as one JSON object — hits under count/h_mems, at most
/// `take` of them, with `total` and `limit_reached` saying what the bound
/// cut. Only the rows returned count as recalled: their decay clocks are
/// reset here, after the provenance filter and the bound, so a row the
/// call may not see, or did not receive, is not refreshed by it. A store
/// error is the call's typed error, never a field inside a success.
/// `fallback_reason` names the semantic-recall degradation so the exact
/// fallback reads as degraded, not as matches.
fn exact_recall_leg(
    memories: &[Arc<hkask_memory::MemoryStore>],
    query: impl Fn(
        &hkask_memory::MemoryStore,
    ) -> Result<Vec<hkask_storage::HMem>, hkask_memory::MemoryStoreError>,
    take: usize,
    fallback_reason: Option<&str>,
) -> Result<serde_json::Value, McpToolError> {
    let mut combined: Vec<(usize, hkask_storage::HMem)> = Vec::new();
    let mut excluded_private = 0usize;
    let mut seen_ids = std::collections::HashSet::new();
    for (store_idx, memory) in memories.iter().enumerate() {
        let rows = query(memory).map_err(|e| map_memory_store_error(e, "Entity recall failed"))?;
        let (rows, excluded) = admitted(rows);
        excluded_private += excluded;
        for row in rows {
            if seen_ids.insert(row.id) {
                combined.push((store_idx, row));
            }
        }
    }
    combined.sort_by_key(|(_, row)| std::cmp::Reverse(row.observed_at));
    let total = combined.len();
    let hits = combined.iter().take(take).collect::<Vec<_>>();
    for (store_idx, hit) in &hits {
        if let Err(error) = memories[*store_idx].touch_recall(&hit.id) {
            tracing::warn!(
                target: "hkask.mcp.curator",
                %error,
                h_mem_id = %hit.id,
                "Failed to reset the decay clock of a recalled h_mem"
            );
        }
    }
    let serialized: Vec<serde_json::Value> = hits
        .into_iter()
        .map(|(_, row)| h_mem_fragment(row))
        .collect();
    let mut leg = json!({});
    leg["count"] = json!(serialized.len());
    leg["total"] = json!(total);
    leg["limit_reached"] = json!(total > take);
    if excluded_private > 0 {
        leg["excluded_private_rows"] = json!(excluded_private);
    }
    if let Some(reason) = fallback_reason {
        leg["mode"] = json!("entity_exact");
        leg["note"] = json!(format!(
            "semantic recall unavailable — fell back to exact-entity lookup: {reason}"
        ));
    }
    leg["h_mems"] = json!(serialized);
    Ok(leg)
}

/// Strip the derived keys the insert path adds (`recall_text`, `_note`,
/// `evidence` — the citation the insert path stores with the value, CU-21)
/// before comparing a new value against a stored one — the write-path
/// contradiction check compares payloads, not storage decorations. A value
/// that is not an object and has no text of its own is stored wrapped as
/// `{"value": ...}` (by the store, or by the note path below), so the wrapper
/// compares as the value it holds. A same-value re-assertion on a different
/// citation stays a re-affirmation, not a contradiction: each row keeps
/// its own citation and recall dedupes.
fn comparable_payload(value: &serde_json::Value) -> serde_json::Value {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("recall_text");
        object.remove("_note");
        object.remove("evidence");
        if object.len() == 1 {
            if let Some(inner) = object.remove("value") {
                return inner;
            }
        }
    }
    value
}

/// The editor's consolidation floor — the confidence at or below which the
/// next consolidation pass deletes the row. Reached through
/// `HKASK_MEMORY_CONFIDENCE_FLOOR` (emitted by kask_bridge from
/// `kask.memory.confidence_floor`, allowlisted in `mcp_servers.rs`); unset
/// or malformed, the documented default applies — the editor's own default.
/// CU-31 (b): the write tools say so in the response when a write lands at
/// or below it.
const DEFAULT_CONSOLIDATION_FLOOR: f64 = 0.3;

fn consolidation_floor() -> f64 {
    match std::env::var("HKASK_MEMORY_CONFIDENCE_FLOOR") {
        Ok(raw) => match raw.trim().parse::<f64>() {
            Ok(value) if (0.0..=1.0).contains(&value) => value,
            _ => {
                tracing::warn!(
                    target: "hkask.mcp.curator",
                    env = "HKASK_MEMORY_CONFIDENCE_FLOOR",
                    value = %raw,
                    "Malformed consolidation floor — must be a number in 0.0-1.0; using the default 0.3"
                );
                DEFAULT_CONSOLIDATION_FLOOR
            }
        },
        Err(_) => DEFAULT_CONSOLIDATION_FLOOR,
    }
}

/// CU-25: one durable archive record per deliberate curation action. The
/// Regulation archive is the audit surface `reg_query` reads; the server's
/// curation spans were log lines only, so a reg.curation query returned 0
/// events while the server curated all day. The record is NOT a memory:
/// nothing is written to the memory store about a deletion, so nothing can
/// be recalled, embedded, decayed or re-forgotten (operator ruling
/// 2026-11-14) — the archive answers "what did the server do to the
/// store?", never "what should I recall?". Only deliberate tool actions
/// are recorded; the mechanical nightly forgetting pass stays a log line.
/// Best-effort with a warn, the same contract as the store's own sink
/// emission.
fn persist_curation_record(
    stores: &crate::db::CuratorStores,
    webid: &hkask_types::WebID,
    operation: &str,
    mut observation: serde_json::Value,
    provenance: hkask_types::InferenceProvenance,
) {
    let Some(archive) = &stores.regulation_store else {
        tracing::warn!(
            target: "hkask.mcp.curator",
            operation,
            "the curation record was not persisted: the Regulation archive is not open"
        );
        return;
    };
    let Some(namespace) = SpanNamespace::new("reg.curation") else {
        tracing::warn!(
            target: "hkask.mcp.curator",
            operation,
            "reg.curation namespace is not registered — the curation record was not persisted"
        );
        return;
    };
    if provenance == hkask_types::InferenceProvenance::PrivateInference {
        if let Some(obj) = observation.as_object_mut() {
            obj.insert(
                "inference_provenance".to_string(),
                json!(provenance.as_str()),
            );
        }
    }
    let mut event = RegulationRecord::new(
        *webid,
        Span::new(namespace, operation),
        CyclePhase::Act,
        observation,
        0,
    );
    if provenance == hkask_types::InferenceProvenance::PrivateInference {
        event.visibility = provenance.as_str().to_string();
    }
    if let Err(error) = archive.persist(&event) {
        tracing::warn!(
            target: "hkask.mcp.curator",
            %error,
            operation,
            "Failed to persist the curation record to the Regulation archive"
        );
    }
}

fn normalize_dedup_value(value: &str) -> String {
    let lower = value.to_lowercase();
    let mut result = String::with_capacity(lower.len());
    let mut prev_was_space = false;
    for ch in lower.chars() {
        if ch.is_whitespace() {
            if !prev_was_space && !result.is_empty() {
                result.push(' ');
            }
            prev_was_space = true;
        } else if ch.is_ascii_punctuation() {
            prev_was_space = false;
        } else {
            result.push(ch);
            prev_was_space = false;
        }
    }
    result.trim_end().to_string()
}

fn prune_admitted_by_age(
    memory: &hkask_memory::MemoryStore,
    prefixes: Option<&[&str]>,
    max_age_days: i64,
    spare_recalled_within_days: Option<i64>,
) -> Result<(hkask_memory::memory_store::PruneOutcome, usize), McpToolError> {
    let now = chrono::Utc::now();
    let cutoff = now - chrono::Duration::days(max_age_days);
    let spare_cutoff = spare_recalled_within_days.map(|days| now - chrono::Duration::days(days));

    let raw_rows = if let Some(prefixes) = prefixes {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for prefix in prefixes {
            let batch = backfill::scan_hmems_by_prefix_uncapped(memory, prefix)
                .map_err(|e| map_memory_store_error(e, "Age-based prune failed"))?;
            for row in batch {
                if seen.insert(row.id) {
                    out.push(row);
                }
            }
        }
        out
    } else {
        backfill::scan_hmems_by_prefix_uncapped(memory, "")
            .map_err(|e| map_memory_store_error(e, "Age-based prune failed"))?
    };

    let mut excluded_private = 0usize;
    let mut candidates = 0usize;
    let mut deleted_count = 0usize;
    let mut spared_count = 0usize;
    let mut failed_count = 0usize;

    for h_mem in raw_rows {
        if h_mem.observed_at >= cutoff {
            continue;
        }
        if !call_admits(&h_mem) {
            excluded_private += 1;
            continue;
        }
        candidates += 1;
        if let Some(ref spare_cutoff) = spare_cutoff {
            if h_mem.recalled_at > *spare_cutoff {
                spared_count += 1;
                continue;
            }
        }
        match memory.delete_h_mem(&h_mem.id) {
            Ok(()) => deleted_count += 1,
            Err(error) => {
                failed_count += 1;
                tracing::warn!(
                    target: "hkask.mcp.curator",
                    %error,
                    h_mem_id = %h_mem.id,
                    "Failed to delete aged h_mem during curator_memory_prune"
                );
            }
        }
    }

    Ok((
        hkask_memory::memory_store::PruneOutcome {
            candidates,
            deleted_count,
            spared_count,
            failed_count,
        },
        excluded_private,
    ))
}

fn dedup_admitted_by_normalized_value(
    memory: &hkask_memory::MemoryStore,
    limit: usize,
) -> Result<(hkask_memory::memory_store::DedupOutcome, usize), McpToolError> {
    let mut all_rows = backfill::scan_hmems_by_prefix_uncapped(memory, "")
        .map_err(|e| map_memory_store_error(e, "Normalized-value dedup failed"))?;
    all_rows.sort_by_key(|row| std::cmp::Reverse(row.observed_at));
    let scanned_rows: Vec<hkask_storage::HMem> = all_rows.into_iter().take(limit).collect();
    let scanned = scanned_rows.len();

    let mut groups: std::collections::HashMap<
        (String, String, String, hkask_types::InferenceProvenance),
        Vec<&hkask_storage::HMem>,
    > = std::collections::HashMap::new();
    let mut skipped_non_string = 0usize;
    let mut skipped_turn_storage = 0usize;
    let mut excluded_private = 0usize;

    for h_mem in &scanned_rows {
        if !call_admits(h_mem) {
            excluded_private += 1;
            continue;
        }
        if h_mem.entity.starts_with(thread_turns::SHARED_TURN_PREFIX) {
            skipped_turn_storage += 1;
            continue;
        }
        let Some(value_str) = h_mem.value.as_str() else {
            skipped_non_string += 1;
            continue;
        };
        let normalized = normalize_dedup_value(value_str);
        let key = (
            h_mem.entity.clone(),
            h_mem.attribute.clone(),
            normalized,
            h_mem.inference_provenance,
        );
        groups.entry(key).or_default().push(h_mem);
    }

    let mut deleted_count = 0usize;
    let mut failed_count = 0usize;
    let mut groups_with_dupes = 0usize;

    for group in groups.values() {
        if group.len() < 2 {
            continue;
        }
        groups_with_dupes += 1;
        let mut sorted = group.clone();
        sorted.sort_by(|a, b| {
            b.confidence
                .value()
                .partial_cmp(&a.confidence.value())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for h_mem in &sorted[1..] {
            match memory.delete_h_mem(&h_mem.id) {
                Ok(()) => deleted_count += 1,
                Err(error) => {
                    failed_count += 1;
                    tracing::warn!(
                        target: "hkask.mcp.curator",
                        %error,
                        h_mem_id = %h_mem.id,
                        "Failed to delete duplicate h_mem during curator_memory_dedup"
                    );
                }
            }
        }
    }

    Ok((
        hkask_memory::memory_store::DedupOutcome {
            scanned,
            groups_with_dupes,
            deleted_count,
            failed_count,
            skipped_non_string,
            skipped_turn_storage,
        },
        excluded_private,
    ))
}

/// One human-readable cause line per staleness reason — the reasons are
/// structured in `forgetting` and serialized into the sweep response here.
fn citation_reason_note(reason: &forgetting::CitationStalenessReason) -> String {
    match reason {
        forgetting::CitationStalenessReason::MissingFile { path } => {
            format!("missing file: {path}")
        }
        forgetting::CitationStalenessReason::LineOutOfBounds {
            path,
            cited_line,
            actual_lines,
        } => format!(
            "line out of bounds: {path}:{cited_line} exceeds the live file's {actual_lines} lines"
        ),
        forgetting::CitationStalenessReason::MissingDivergenceSeam { seam_id } => {
            format!("divergence seam {seam_id} absent from DIVERGENCE.md")
        }
    }
}

hkask_mcp_server::mcp_server!(
    pub struct CuratorServer {
        /// Self-healing handle over the curator's sovereign `curator.db`.
        /// Both stores are read through it on every tool call so a
        /// mid-process heal takes effect without a server restart.
        db: Arc<CuratorDb>,
        /// Inference port for semantic memory recall — embeds recall queries
        /// through the zed IPC bridge (`HKASK_INFERENCE_SOCKET`), the same
        /// routing every other kask MCP server uses. Without it, the
        /// "semantic" tools degrade to exact-entity lookup, which never
        /// matches a natural-language question.
        inference_port: Arc<dyn hkask_types::InferencePort>,
    } = crate::ontology::annotate;
);

#[tool_router(server_handler)]
impl CuratorServer {
    // ── Liveness ───────────────────────────────────────────────────────

    #[tool(
        description = "Liveness check: reports whether each of the server's two stores (curator memory, Regulation archive) is open. status is ok when both are, degraded with the reason when one is not."
    )]
    pub async fn curator_ping(
        &self,
        Parameters(_req): Parameters<PingRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_ping", async {
            let stores = self.db.get();
            let healthy = stores.regulation_store.is_some() && stores.memory.is_some();
            let mut body = json!({
                "status": if healthy { "ok" } else { "degraded" },
                "server": SERVER_NAME,
                "curator_webid": self.webid.to_string(),
                "stores": {
                    "regulation_store": stores.regulation_store.is_some(),
                    "memory": stores.memory.is_some(),
                }
            });
            if !healthy {
                body["reason"] = json!(
                    self.db
                        .outage_reason()
                        .unwrap_or_else(|| "a store is not open".to_string())
                );
            }
            Ok(body)
        })
        .await
    }

    // ── Memory & Learning ──────────────────────────────────────────────

    /// Embed a recall query and resolve the nearest stored h_mems by cosine
    /// similarity. Returns the resolved `(h_mem, distance)` pairs (most
    /// similar first) plus the count of KNN hits that failed to resolve to
    /// any matching h_mem — the store errored, or its entity/passage is orphaned.
    /// Named passage vectors resolve only matching canonical passages; entity-level
    /// vectors retain the freshest-row behavior. Each entity contributes at most
    /// `MAX_FRAGMENTS_PER_ENTITY` fragments, with no duplicated h_mem IDs.
    /// `Err(reason)` when the query cannot be embedded (no IPC bridge, no
    /// embedding provider) or the store has no embedding index — callers fall
    /// back to exact-entity lookup and surface the reason. The query's model
    /// identities gate the KNN: rows stored under a different embedding
    /// model are excluded and counted in `excluded_model_mismatch` (the
    /// migration-window degradation callers surface).
    fn semantic_recall_fragments_for_vector(
        &self,
        query_vector: &[f32],
        limit: usize,
        query_model: &str,
        query_actual_model: Option<&str>,
    ) -> Result<SemanticRecall, SemanticRecallError> {
        let memories = read_memories_for_call(&self.db)
            .map_err(|source| SemanticRecallError::MemoryUnavailable { source })?;
        let knn_limit = limit.saturating_mul(MAX_FRAGMENTS_PER_ENTITY).max(limit);
        let mut fragments = Vec::new();
        let mut resolution_failures = 0usize;
        let mut excluded_model_mismatch = 0usize;
        // M5: the privacy filter's surfaced exclusion count (never a
        // silent drop).
        let mut excluded_private = 0usize;
        let mut seen_h_mem_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut per_entity_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for memory in &memories {
            let outcome = memory
                .search_similar(query_vector, knn_limit, query_model, query_actual_model)
                .map_err(|source| SemanticRecallError::Search { source })?;
            excluded_model_mismatch += outcome.excluded_model_mismatch;
            for result in outcome.results {
                let entity_ref = result.embedding.entity_ref.clone();
                if per_entity_counts.get(&entity_ref).copied().unwrap_or(0)
                    >= MAX_FRAGMENTS_PER_ENTITY
                {
                    continue;
                }
                match memory.query_deduped_untouched(&entity_ref) {
                    Ok(mut h_mems) => {
                        if let Some(passage) = result.embedding.passage_text.as_deref() {
                            h_mems.retain(|h_mem| {
                                hkask_memory::semantic_passage_for_h_mem(h_mem).as_deref()
                                    == Some(passage)
                            });
                        }
                        if h_mems.is_empty() {
                            // A missing entity or named passage is a resolution failure,
                            // not permission to substitute an unrelated sibling.
                            resolution_failures += 1;
                            tracing::warn!(
                                target: "hkask.mcp.curator",
                                entity_ref = %entity_ref,
                                "KNN hit resolved to zero matching h_mems — orphaned entity or passage (non-fatal)"
                            );
                            continue;
                        }
                        h_mems.sort_by_key(|h_mem| std::cmp::Reverse(h_mem.observed_at));
                        for h_mem in h_mems {
                            if per_entity_counts.get(&entity_ref).copied().unwrap_or(0)
                                >= MAX_FRAGMENTS_PER_ENTITY
                            {
                                break;
                            }
                            if !seen_h_mem_ids.insert(h_mem.id.to_string()) {
                                continue;
                            }
                            // M5 (falsifier (b)): a public call context never
                            // receives a private-marked row; the exclusion is
                            // counted, surfaced, and the row skipped — never
                            // silently dropped and never substituted.
                            if !call_admits(&h_mem) {
                                excluded_private += 1;
                                continue;
                            }
                            per_entity_counts
                                .entry(entity_ref.clone())
                                .and_modify(|count| *count += 1)
                                .or_insert(1);
                            fragments.push((h_mem, result.distance));
                        }
                    }
                    Err(error) => {
                        resolution_failures += 1;
                        tracing::warn!(
                            target: "hkask.mcp.curator",
                            error = %error,
                            entity_ref = %entity_ref,
                            "failed to resolve KNN hit to its h_mem — skipping (non-fatal)"
                        );
                    }
                }
            }
        }
        fragments.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(SemanticRecall {
            fragments,
            resolution_failures,
            excluded_model_mismatch,
            excluded_private,
        })
    }

    async fn semantic_recall_fragments(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<SemanticRecall, SemanticRecallError> {
        // zed-kask: D100's named follow-up (operator-approved 2026-11-06) —
        // the query embed routes by the CALL's provenance (the M5 `_meta`
        // mark): private call contexts embed through the private leg's
        // LOCAL model only; public contexts keep the configured model. The
        // private query vector lands in the local model's space, so the
        // KNN gate excludes cloud-model rows and surfaces
        // `excluded_model_mismatch` — never a widening to the cloud.
        let embedding_model = routed_model_for_provenance(
            hkask_mcp_server::current_call_provenance(),
            hkask_inference::model_constants::private_embedding_model(),
            curator_embedding_model(),
        )?;
        let batch = self
            .inference_port
            .embed_with_dimensions(
                &embedding_model,
                &[query.to_string()],
                Some(hkask_storage::embedding_dim() as u32),
            )
            .await
            .map_err(|source| SemanticRecallError::Embed { source })?;
        let query_vector = batch
            .vectors
            .into_iter()
            .next()
            .ok_or(SemanticRecallError::NoVector)?;
        // S-01 completion: the embedding call's measured usage flows to the
        // executing tool's envelope (the consult/search retrieval family).
        hkask_mcp_server::server::record_tool_usage(batch.usage.clone());
        self.semantic_recall_fragments_for_vector(
            &query_vector,
            limit,
            &batch.requested_model,
            batch.actual_model.as_deref(),
        )
    }

    #[tool(
        description = "Query the Curator's memory by semantic similarity to a free-text query (a question or topic). Embeds the query and returns the nearest stored memories by cosine similarity. Falls back to exact-entity-name lookup when embeddings are unavailable (noted in the output)."
    )]
    pub async fn curator_semantic_search(
        &self,
        Parameters(req): Parameters<SemanticSearchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_semantic_search", async {
            require_text("query", &req.query)?;
            let limit = req.limit.unwrap_or(10).clamp(1, 50);
            let memories = read_memories_for_call(&self.db)?;

            // Semantic leg: embed the query, KNN over stored embeddings,
            // resolve each hit to its h_mem. This is the path a natural-
            // language question actually matches — the exact-entity leg
            // below only matches when the query IS an entity name.
            match self.semantic_recall_fragments(&req.query, limit).await {
                Ok(recall) => {
                    let serialized: Vec<serde_json::Value> = recall
                        .fragments
                        .iter()
                        .take(limit)
                        .map(|(t, distance)| h_mem_with_distance(t, distance))
                        .collect();
                    let yielded = serialized.len();
                    let mut result = json!({
                        "count": yielded,
                        "mode": "semantic",
                        "results": serialized,
                    });
                    if recall.excluded_private > 0 {
                        result["excluded_private_rows"] = json!(recall.excluded_private);
                    }
                    if recall.excluded_model_mismatch > 0 {
                        result["model_mismatch_excluded"] = json!(recall.excluded_model_mismatch);
                    }
                    if let Some(note) = degradation_note(
                        recall.resolution_failures,
                        recall.excluded_model_mismatch,
                        yielded,
                    ) {
                        result["note"] = json!(note);
                    }
                    Ok(result)
                }
                // Degradation, not a silent fallback: the operator must be
                // able to tell "no similar memories" from "semantic recall
                // unavailable" (the unwrap_or(0) trap).
                Err(reason) => {
                    // The fallback is the same exact-entity leg consult
                    // uses: filtered, bounded, and only the returned rows
                    // count as recalled.
                    let leg = exact_recall_leg(
                        &memories,
                        |memory| memory.query_deduped_untouched(&req.query),
                        limit,
                        Some(&reason.to_string()),
                    )?;
                    let mut result = json!({
                        "count": leg["count"],
                        "mode": "entity_exact",
                        "note": leg["note"],
                        "results": leg["h_mems"],
                    });
                    if let Some(excluded) = leg.get("excluded_private_rows") {
                        result["excluded_private_rows"] = excluded.clone();
                    }
                    Ok(result)
                }
            }
        })
        .await
    }

    #[tool(
        description = "Search Curator memory and configured sealed corpus sources in one source-aware retrieval. Stores remain separate. Results preserve source and record provenance, corpus rows project passage_text only, and every source reports ready, unconfigured, incompatible, invalid, or unavailable status."
    )]
    pub async fn curator_federated_search(
        &self,
        Parameters(req): Parameters<FederatedSearchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_federated_search", async {
            if req.query.trim().is_empty() {
                return Err(McpToolError::invalid_argument("query must not be empty"));
            }
            let limit = req.limit.unwrap_or(10).clamp(1, 50);
            // CU-29: the stage timings — the two live samples (12,076 ms and
            // 4,213 ms against 747/1,346 ms for the other embedding reads)
            // never localized, so every stage carries its own duration on the
            // response and on a tracing line; a slow call now names its own
            // dominant stage.
            let started = std::time::Instant::now();
            // zed-kask: D100's named follow-up (operator-approved
            // 2026-11-06) — the federated query embed routes by the call's
            // provenance too: private contexts embed through the private
            // leg's LOCAL model only (each sealed source's model gate then
            // surfaces its per-source exclusion); unset/empty is the
            // surfaced degrade, never a widening to the cloud.
            // An unset model setting is absent state with a named remedy,
            // the same class the write paths and the backfill sweep report.
            let embedding_model = routed_model_for_provenance(
                hkask_mcp_server::current_call_provenance(),
                hkask_inference::model_constants::private_embedding_model(),
                curator_embedding_model(),
            )
            .map_err(|reason| model_not_configured("federated search did not run", &reason))?;
            let embed_started = std::time::Instant::now();
            let batch = self
                .inference_port
                .embed_with_dimensions(
                    &embedding_model,
                    std::slice::from_ref(&req.query),
                    Some(hkask_storage::embedding_dim() as u32),
                )
                .await
                .map_err(|error| embed_error(&error, "Federated query embedding failed"))?;
            let embed_ms = embed_started.elapsed().as_millis() as u64;
            let query_vector = batch.vectors.into_iter().next().ok_or_else(|| {
                McpToolError::internal("Federated query embedding returned no vector")
            })?;
            // S-01 completion: the federated embedding call's measured
            // usage flows to the executing tool's envelope.
            hkask_mcp_server::server::record_tool_usage(batch.usage.clone());

            let knn_started = std::time::Instant::now();
            let curator_recall =
                self.semantic_recall_fragments_for_vector(
                    &query_vector,
                    limit,
                    &batch.requested_model,
                    batch.actual_model.as_deref(),
                );
            let curator_knn_ms = knn_started.elapsed().as_millis() as u64;
            let mut statuses = Vec::new();
            let mut batches = Vec::new();
            match curator_recall {
                Ok(recall) => {
                    let hits = recall
                        .fragments
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, (h_mem, distance))| {
                            hkask_memory::semantic_passage_for_h_mem(&h_mem).map(|text| {
                                hkask_memory::FederatedHit {
                                    source_id: "curator".to_string(),
                                    source_kind: hkask_memory::FederatedSourceKind::Curator,
                                    record_id: h_mem.id.to_string(),
                                    entity_ref: h_mem.entity,
                                    text,
                                    run_id: None,
                                    model: embedding_model.clone(),
                                    confidence: Some(h_mem.confidence.value()),
                                    distance,
                                    source_rank: index + 1,
                                    fused_rank: 0,
                                }
                            })
                        })
                        .collect::<Vec<_>>();
                    statuses.push(hkask_memory::FederatedSourceStatus {
                        source_id: "curator".to_string(),
                        source_kind: hkask_memory::FederatedSourceKind::Curator,
                        state: hkask_memory::FederatedSourceState::Ready,
                        // Hits that failed h_mem resolution — or rows the model
                        // gate excluded — must not vanish behind a clean Ready:
                        // the counts ride the reason so Ready + result_count 0
                        // cannot read as "the curator store is empty".
                        reason: degradation_note(
                            recall.resolution_failures,
                            recall.excluded_model_mismatch,
                            hits.len(),
                        ),
                        result_count: hits.len(),
                    });
                    if recall.excluded_private > 0 {
                        if let Some(last) = statuses.last_mut() {
                            let excluded = format!(
                                "{} private-marked row(s) excluded by the call context's provenance",
                                recall.excluded_private
                            );
                            last.reason = Some(match last.reason.take() {
                                Some(reason) => format!("{reason}; {excluded}"),
                                None => excluded,
                            });
                        }
                    }
                    batches.push(hkask_memory::RankedSourceBatch {
                        source_id: "curator".to_string(),
                        hits,
                    });
                }
                Err(error) => statuses.push(hkask_memory::FederatedSourceStatus {
                    source_id: "curator".to_string(),
                    source_kind: hkask_memory::FederatedSourceKind::Curator,
                    state: hkask_memory::FederatedSourceState::Unavailable,
                    reason: Some(error.to_string()),
                    result_count: 0,
                }),
            }

            let registry = self.db.federated_sources();
            let mut external_statuses = registry.statuses();
            let mut sources_ms = serde_json::Map::new();
            for source in registry.sources() {
                let source_started = std::time::Instant::now();
                let searched = source.search(&embedding_model, &req.query, &query_vector, limit);
                sources_ms.insert(
                    source.identity().source_id.clone(),
                    json!(source_started.elapsed().as_millis() as u64),
                );
                match searched {
                    Ok(batch) => {
                        if let Some(status) = external_statuses
                            .iter_mut()
                            .find(|status| status.source_id == batch.source_id)
                        {
                            status.result_count = batch.hits.len();
                            if batch.excluded_model_mismatch > 0 {
                                status.reason =
                                    Some(model_mismatch_note(batch.excluded_model_mismatch));
                            }
                        }
                        let hits = batch
                            .hits
                            .into_iter()
                            .map(|hit| hkask_memory::FederatedHit {
                                source_id: hit.source_id,
                                source_kind: hkask_memory::FederatedSourceKind::Corpus,
                                record_id: hit.embedding_id,
                                entity_ref: hit.entity_ref,
                                text: hit.text,
                                run_id: Some(hit.run_id),
                                model: hit.model,
                                confidence: None,
                                distance: hit.distance,
                                source_rank: hit.source_rank,
                                fused_rank: 0,
                            })
                            .collect();
                        batches.push(hkask_memory::RankedSourceBatch {
                            source_id: source.identity().source_id.clone(),
                            hits,
                        });
                    }
                    Err(error) => {
                        if let Some(status) = external_statuses
                            .iter_mut()
                            .find(|status| status.source_id == source.identity().source_id)
                        {
                            status.state = hkask_memory::classify_federated_error(&error);
                            status.reason = Some(error.to_string());
                            status.result_count = 0;
                        }
                    }
                }
            }
            if registry.changed_during_search() {
                batches.retain(|batch| batch.source_id == "curator");
                for status in &mut external_statuses {
                    status.state = hkask_memory::FederatedSourceState::Unavailable;
                    status.reason =
                        Some("source identity changed during retrieval; retry search".to_string());
                    status.result_count = 0;
                }
            }
            statuses.extend(external_statuses);
            let results = hkask_memory::interleave_ranked_batches(batches, limit);
            let total_ms = started.elapsed().as_millis() as u64;
            tracing::info!(
                target: "hkask.mcp.curator",
                embed_ms,
                curator_knn_ms,
                total_ms,
                "Federated search stages"
            );
            Ok(json!({
                "query": req.query,
                "count": results.len(),
                "timings_ms": {
                    "embed_ms": embed_ms,
                    "curator_knn_ms": curator_knn_ms,
                    "sources_ms": serde_json::Value::Object(sources_ms),
                    "total_ms": total_ms,
                },
                "results": results,
                "sources": statuses,
            }))
        })
        .await
    }

    #[tool(
        description = "Recall h_mems by exact entity or along an ontology axis (one of the two), newest first, at most `limit`; entity recall counts as a use and resets the returned rows' decay clocks. Give `entity`, or give `ontology_axis` (dc_type | dc_subject | pko_procedure | ontology_namespace) with `ontology_value` — e.g. every step of procedure X, or every h_mem tagged by a domain ontology namespace. `limit` defaults to 100 and is at most 500; `total` and `limit_reached` say what the bound cut. Axis recall does not reset decay clocks."
    )]
    pub async fn curator_memory_recall(
        &self,
        Parameters(req): Parameters<MemoryRecallRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_memory_recall", async {
            let limit = req
                .limit
                .unwrap_or(RECALL_DEFAULT_LIMIT)
                .clamp(1, RECALL_MAX_LIMIT);
            let entity = req.entity.as_deref().filter(|entity| !entity.is_empty());
            if req.ontology_axis.is_none() && req.ontology_value.is_some() {
                return Err(McpToolError::invalid_argument(
                    "ontology_value requires ontology_axis",
                ));
            }
            if req.ontology_axis.is_some() && entity.is_some() {
                return Err(McpToolError::invalid_argument(
                    "give entity or ontology_axis, not both — the two are separate recalls",
                ));
            }

            // Ontology-axis recall (P5.4): when an axis is named, recall along
            // the dual-axis anchoring instead of the entity. This is what makes
            // the ontology blob a query axis rather than inert metadata —
            // "every step of procedure X" and "every bibo:Article" are
            // questions the entity index cannot answer.
            if let Some(axis) = req.ontology_axis.as_deref() {
                let Some(value) = req.ontology_value.as_deref() else {
                    return Err(McpToolError::invalid_argument(
                        "ontology_axis requires ontology_value",
                    ));
                };
                let memories = read_memories_for_call(&self.db)?;
                let mut raw_h_mems = Vec::new();
                let mut seen_ids = std::collections::HashSet::new();
                for memory in &memories {
                    let rows = match axis {
                        "dc_type" => memory.query_by_dc_type(value),
                        "dc_subject" => memory.query_by_dc_subject(value),
                        "pko_procedure" => memory.query_by_pko_procedure(value),
                        "ontology_namespace" => memory.query_by_ontology_namespace(value),
                        other => {
                            return Err(McpToolError::invalid_argument(format!(
                                "unknown ontology_axis '{other}' — expected one of: {}",
                                ONTOLOGY_AXES.join(", ")
                            )));
                        }
                    }
                    .map_err(|e| map_memory_store_error(e, "Ontology recall failed"))?;
                    for row in rows {
                        if seen_ids.insert(row.id) {
                            raw_h_mems.push(row);
                        }
                    }
                }
                raw_h_mems.sort_by_key(|row| std::cmp::Reverse(row.observed_at));
                // M5: the same partition as the recall legs — public call
                // contexts never receive private-marked rows.
                let (h_mems, excluded_private) = admitted(raw_h_mems);
                let total = h_mems.len();
                let serialized: Vec<serde_json::Value> = h_mems
                    .iter()
                    .take(limit)
                    .map(|t| {
                        let mut fragment = h_mem_fragment(t);
                        fragment["ontology"] = json!(t.ontology);
                        fragment
                    })
                    .collect();
                let mut result = json!({
                    "ontology_axis": axis,
                    "ontology_value": value,
                    "count": serialized.len(),
                    "total": total,
                    "limit_reached": total > limit,
                    "h_mems": serialized,
                });
                if excluded_private > 0 {
                    result["excluded_private_rows"] = json!(excluded_private);
                }
                return Ok(result);
            }

            let Some(entity) = entity else {
                return Err(McpToolError::invalid_argument(
                    "give an entity, or ontology_axis with ontology_value",
                ));
            };
            let memories = read_memories_for_call(&self.db)?;
            // CU-22: one list. The perspective-scoped half returned rows
            // whose perspective is the curator's WebID, and no production
            // writer sets a perspective — an always-empty half of every
            // response.
            let leg = exact_recall_leg(
                &memories,
                |memory| memory.query_deduped_untouched(entity),
                limit,
                None,
            )?;
            let mut result = json!({ "entity": entity });
            if let (Some(target), Some(fields)) = (result.as_object_mut(), leg.as_object()) {
                for (key, value) in fields {
                    target.insert(key.clone(), value.clone());
                }
            }
            Ok(result)
        })
        .await
    }

    // ── Consultation (Slice 8 — curator-as-callable-tool) ────────────────

    /// Consult the curator's memory with a question. A delegated agent calls
    /// this to get the curator's perspective on a topic, grounded in the
    /// curator's sovereign memory.
    ///
    /// This is a memory-grounded consultation, not a full curator agent
    /// turn — it returns the raw memory fragments matching the query by
    /// semantic similarity (the query is embedded through the zed IPC
    /// bridge, the same inference routing every other kask MCP server
    /// uses). The calling agent synthesizes the response from the
    /// fragments. When embeddings are unavailable, the consult degrades to
    /// exact-entity lookup with the reason surfaced in the output.
    ///
    /// A full inference-grounded response (where the curator agent itself
    /// synthesizes) requires the in-process `CuratorAgentServer`, which
    /// lives in the zed process, not in this MCP server. That path is a
    /// future enhancement (requires a new IPC method + recursion cap).
    #[tool(
        description = "Consult the curator's memory with a question — returns matching memory fragments by semantic similarity for the caller to synthesize, not a full curator agent turn. When embeddings are unavailable the consult degrades to exact-entity lookup with the reason surfaced in the output."
    )]
    pub async fn curator_consult(
        &self,
        Parameters(req): Parameters<CuratorConsultRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_consult", async {
            require_text("query", &req.query)?;
            let limit = req.limit.unwrap_or(5).clamp(1, 20);
            let memories = read_memories_for_call(&self.db)?;
            let mut result = json!({
                "query": req.query,
                "note": "Memory-grounded consultation — raw fragments, not a synthesized response. The calling agent synthesizes."
            });

            // Semantic leg shared by both scopes: embed the query once, KNN
            // over stored embeddings, resolve each hit to its h_mem. A
            // natural-language question matches here — the previous
            // implementation did exact-entity lookup on the raw question
            // text, which never matched anything and made every consult
            // return zero fragments.
            let semantic = self.semantic_recall_fragments(&req.query, limit).await;
            match &semantic {
                Ok(recall) => {
                    // CU-22: one list — every KNN-resolved h_mem. The
                    // perspective-scoped half filtered to h_mems the
                    // curator wrote, and no production writer sets a
                    // perspective, so it was always empty.
                    let fragments: Vec<serde_json::Value> = recall
                        .fragments
                        .iter()
                        .take(limit)
                        .map(|(t, distance)| h_mem_with_distance(t, distance))
                        .collect();
                    let fragment_count = fragments.len();
                    let mut fragments_json = json!({
                        "count": fragment_count,
                        "h_mems": fragments,
                    });

                    // Hits that failed h_mem resolution, rows the model gate
                    // excluded and rows the call's provenance excluded must
                    // not vanish: name the counts so partial (or empty)
                    // recall reads as degraded or filtered, not as missing
                    // memories. One arm serves the empty case too, so an
                    // empty result carries the same counts.
                    if let Some(note) = degradation_note(
                        recall.resolution_failures,
                        recall.excluded_model_mismatch,
                        fragment_count,
                    ) {
                        fragments_json["note"] = json!(note);
                    }
                    result["fragments"] = fragments_json;
                    if recall.excluded_private > 0 {
                        result["excluded_private_rows"] = json!(recall.excluded_private);
                    }
                    if recall.excluded_model_mismatch > 0 {
                        result["model_mismatch_excluded"] =
                            json!(recall.excluded_model_mismatch);
                    }
                }
                // Degradation, not a silent fallback — surface why semantic
                // recall is unavailable, then fall back to the exact-entity
                // lookup (which only matches when the query IS an entity).
                // The note is preserved alongside the fallback results so the
                // operator can distinguish "semantic recall broken" from
                // "no matching memories" — same pattern as
                // `curator_semantic_search`.
                Err(reason) => {
                    let reason = reason.to_string();
                    result["fragments"] = exact_recall_leg(
                        &memories,
                        |memory| memory.query_deduped_untouched(&req.query),
                        limit,
                        Some(&reason),
                    )?;
                }
            }

            Ok(result)
        })
        .await
    }

    // ── Algedonic History ──────────────────────────────────────────────

    #[tool(
        description = "Read the algedonic signals — deviation-carrying records — recorded in a lookback window (default 24 hours), newest first, at most 500; idle heartbeats are excluded and counted. excluded_heartbeats names how many zero-deviation heartbeat records the filter removed; limit_reached is true when the window may hold more."
    )]
    pub async fn curator_algedonic_log(
        &self,
        Parameters(req): Parameters<AlgedonicLogRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_algedonic_log", async {
            let hours = req.hours.unwrap_or(24);
            let since = lookback_start("hours", chrono::Duration::try_hours(i64::from(hours)))?;
            let store = self.db.regulation_store()?;
            // CU-24: an algedonic event is a signal that a subsystem
            // breached a threshold — the idle loop's heartbeat records
            // (zero deviations) are excluded before the limit is applied,
            // and the exclusion is counted, never a silent drop.
            let (raw_events, excluded_heartbeats) = store
                .query_recent_algedonic_signals(since, ALGEDONIC_LOG_LIMIT)
                .map_err(|e| map_infra_error(&e, "Algedonic query failed"))?;
            let mut excluded_private = 0usize;
            let mut events = Vec::with_capacity(raw_events.len());
            for event in raw_events {
                if record_admitted(&event) {
                    events.push(event);
                } else {
                    excluded_private += 1;
                }
            }
            let s: Vec<serde_json::Value> = events
                .iter()
                .map(|e| {
                    json!({
                        "timestamp": e.timestamp.to_rfc3339(),
                        "span": e.span.path,
                        "phase": e.phase.as_str(),
                        "observation": e.observation,
                    })
                })
                .collect();
            let mut out = json!({
                "window_hours": hours,
                "ordering": "newest_first",
                "count": s.len(),
                "excluded_heartbeats": excluded_heartbeats,
                "limit_reached": (s.len() + excluded_private) as u64 == ALGEDONIC_LOG_LIMIT,
                "events": s,
            });
            if excluded_private > 0 {
                out["excluded_private_records"] = json!(excluded_private);
            }
            Ok(out)
        })
        .await
    }

    // ── Regulation Query (for platform governance transparency) ────────────────

    #[tool(
        description = "Query Regulation records across namespaces within a time window, oldest first. An optional namespace prefix matches that path or dot-delimited descendants before the result limit is applied. At least one of `namespace` or `window_seconds` must be provided — a request with neither is rejected as invalid_argument (the truncated-argument signature), not defaulted to all records. limit_reached is true when the window may hold more than `limit`."
    )]
    pub async fn reg_query(
        &self,
        Parameters(req): Parameters<RegQueryRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reg_query", async {
            // A query with neither a namespace nor a window is the truncated-argument
            // signature: the tool-call JSON was cut before dispatch, so every `Option`
            // field landed as `None`. Reject it loudly rather than silently defaulting
            // to "all namespaces / last hour" and returning wrong-filtered data as a
            // success (`.rules`: a silent fallback is a broken feedback loop). A
            // deliberate `namespace: None` with a `window_seconds` still means the
            // documented all-namespaces query.
            if req.namespace.is_none() && req.window_seconds.is_none() {
                return Err(McpToolError::invalid_argument(
                    "reg_query requires a namespace or window_seconds; both absent means the \
                     arguments did not arrive (truncated tool-call JSON), not an all-records query",
                ));
            }
            let window_secs = req.window_seconds.unwrap_or(3600);
            let limit = req.limit.unwrap_or(100) as u64;
            if limit == 0 {
                return Err(McpToolError::invalid_argument("limit must be positive"));
            }
            let since = lookback_start(
                "window_seconds",
                i64::try_from(window_secs)
                    .ok()
                    .and_then(chrono::Duration::try_seconds),
            )?;
            let store = self.db.regulation_store()?;
            let sql_limit = limit.saturating_add(4096).min(i64::MAX as u64);
            let mut records = store
                .query_records(since, req.namespace.as_deref(), sql_limit)
                .map_err(|e| map_infra_error(&e, "Regulation query failed"))?;
            if hkask_mcp_server::current_call_provenance()
                == hkask_types::InferenceProvenance::PrivateInference
            {
                if let Some(private_stores) = self.db.private_get() {
                    if let Some(private_reg) = &private_stores.regulation_store {
                        if !Arc::ptr_eq(private_reg, &store) {
                            let private_records = private_reg
                                .query_records(since, req.namespace.as_deref(), sql_limit)
                                .map_err(|e| map_infra_error(&e, "Regulation query failed"))?;
                            records.extend(private_records);
                            records.sort_by_key(|record| record.timestamp);
                        }
                    }
                }
            }
            let mut excluded_private = 0usize;
            let mut admitted_records = Vec::new();
            let mut limit_reached = false;
            for record in records {
                if !record_admitted(&record) {
                    excluded_private += 1;
                    continue;
                }
                if (admitted_records.len() as u64) < limit {
                    admitted_records.push(record);
                } else {
                    limit_reached = true;
                }
            }
            let events: Vec<serde_json::Value> = admitted_records
                .into_iter()
                .map(|event| {
                    json!({
                        "timestamp": event.timestamp.to_rfc3339(),
                        "namespace": event.span.namespace.as_str(),
                        "path": event.span.path,
                        "phase": event.phase.as_str(),
                        "observation": event.observation,
                    })
                })
                .collect();

            let namespace_info = req.namespace.as_deref().unwrap_or("all");
            // Telemetry breadcrumb, not a persisted event (emit is tracing::info!).
            // Use the Curation span (not Tool) so this read-only observability
            // query is not mislabeled as a curator tool invocation in the
            // Regulation log — `reg.curation` / `reg_query_observed` reads as
            // “the curator observed a Regulation query”, not “the curator tool
            // was invoked”.
            RegulationSpan::Curation.emit("reg_query_observed");

            let mut out = json!({
                "namespace": namespace_info,
                "window_seconds": window_secs,
                "ordering": "oldest_first",
                "count": events.len(),
                "limit_reached": limit_reached,
                "events": events
            });
            if excluded_private > 0 {
                out["excluded_private_records"] = json!(excluded_private);
            }
            Ok(out)
        })
        .await
    }

    // ── Skill-use issue reporting (Co-evolution Phase 2) ────────────────

    /// Report a skill-use issue — submitted by a skill's `on_failure` config
    /// when an MCP tool call fails or produces unexpected output. The report
    /// is stored as an h_mem in the curator's memory store with
    /// entity `skill_use_issue:<skill_name>` so it is queryable via
    /// `curator_memory_recall` and `curator_semantic_search` — stored at the
    /// 0.5 confidence floor (not the `HMem::new` 1.0 default: unverified
    /// observations must not outrank verified facts in recall ranking) and
    /// with the report text embedded under the entity so semantic search
    /// finds it by meaning.
    ///
    /// This is the skill-reported input channel of the co-evolution loop:
    /// skills report MCP tool issues → the Curator analyzes patterns →
    /// CuratorDirectives evolve the MCP tool (add validation, improve error
    /// messages, add fallbacks). Complements the existing runtime telemetry
    /// (reg.* spans, algedonic events).
    #[tool(
        description = "Report a skill-use issue when an MCP tool call fails or produces unexpected output. Stored as an h_mem for Curator pattern analysis; the response carries its h_mem_id, which memory_insert accepts as evidence. The report includes skill/tool/step evidence, a granular failure_type, and a required controlled failure_origin ownership class."
    )]
    pub async fn curator_report_skill_use_issue(
        &self,
        Parameters(req): Parameters<ReportSkillUseIssueRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_report_skill_use_issue", async {
            require_text("skill_name", &req.skill_name)?;
            require_text("tool_name", &req.tool_name)?;
            require_text("error", &req.error)?;
            let call_provenance = hkask_mcp_server::current_call_provenance();
            let memory = self.db.write_memory_for_provenance(call_provenance)?;

            let entity = format!("skill_use_issue:{}", req.skill_name);
            let now = chrono::Utc::now();
            let recall_text = format!(
                "skill-use issue: {} / {} (step {}, origin {}): {}",
                req.skill_name,
                req.tool_name,
                req.step_ordinal,
                req.failure_origin.as_str(),
                req.error
            );

            let report_value = json!({
                "skill_name": req.skill_name,
                "tool_name": req.tool_name,
                "step_ordinal": req.step_ordinal,
                "error": req.error,
                "tool_input": req.tool_input,
                "failure_type": req.failure_type,
                "failure_origin": req.failure_origin.as_str(),
                "reported_at": now.to_rfc3339(),
                "recall_text": recall_text,
            });

            let h_mem = hkask_storage::HMem::new(
                &entity,
                &format!("tool_failure:{}", req.tool_name),
                report_value,
                self.webid,
            )
            // The 0.5 floor — NOT the `HMem::new` 1.0 default. Issue reports
            // are unverified observations; at 1.0 they outranked verified
            // facts in recall ranking (confidence is a ranking multiplier).
            .with_confidence(hkask_types::Confidence::new(0.5))
            // CU-19 (a): the report carries the CALL's inference provenance —
            // a report filed from a private thread lands private-marked,
            // embeds through the private leg's LOCAL model, and never
            // recalls into a public context. A public call is unchanged.
            // Never caller-supplied: the mark is the dispatch context's,
            // read host-side (the D100/M5 discipline).
            .with_inference_provenance(call_provenance);
            let write = memory.prepare_semantic_write(h_mem)
                .map_err(|e| map_memory_store_error(e, "Failed to prepare skill-use issue report"))?;
            let written_id = publish_semantic_write(self.inference_port.as_ref(), write).await?;

            RegulationSpan::Curation.emit("skill_use_issue_reported");

            Ok(json!({
                "reported": true,
                "h_mem_id": written_id.to_string(),
                "entity": entity,
                "skill_name": req.skill_name,
                "tool_name": req.tool_name,
                "step_ordinal": req.step_ordinal,
                "failure_type": req.failure_type,
                "failure_origin": req.failure_origin.as_str(),
                "semantic_recall": "embedded",
                "guidance": "The issue has been recorded in the curator's memory store. Use curator_memory_recall with entity 'skill_use_issue:<skill_name>' or curator_semantic_search to retrieve accumulated reports."
            }))
        })
        .await
    }

    // ── Curator memory edit tools (Priority 5) ───────────────────────────
    //
    // These tools give the curator agent write access to its own memory,
    // with evidence-grounding and confidence-floor constraints. User
    // threads cannot write to memory directly — only the curator (the one
    // agent with a feedback loop).
    //
    // Grounding: Dunning's Cassandra quandary (`138299529:16-17`) — poor
    // performers can't evaluate which memories are worth writing. MemGPT
    // (Packer et al., 2023) — OS-style memory management with permission
    // boundaries.

    /// Insert a new memory into the curator's store.
    ///
    /// The memory starts at confidence 0.5 (the floor — NOT the model's
    /// self-assessed confidence). Nothing changes it afterwards except a
    /// `memory_update` call.
    ///
    /// The value's text is embedded under the entity (the entity_ref
    /// invariant) so `curator_semantic_search` finds the memory by meaning,
    /// not just by exact entity name. An embedding failure fails the insert:
    /// no row lands without its vector.
    ///
    /// Evidence-grounding: the `evidence_h_mem_id` field must cite a
    /// specific h_mem ID that supports this memory. The tool
    /// rejects inserts without a citation.
    #[tool(
        description = "Insert a memory citing an existing h_mem as evidence: the citation is stored with the value at confidence 0.5; a differing value is refused; returns its h_mem_id. Whatever the caller believes, the stored confidence changes afterwards only through memory_update or memory_resolve_contradiction."
    )]
    pub async fn memory_insert(
        &self,
        Parameters(req): Parameters<MemoryInsertRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "memory_insert", async {
            require_text("attribute", &req.attribute)?;
            let (_evidence_memory, _evidence_id, evidence) = fetch_existing_h_mem_routed(
                &self.db,
                "evidence_h_mem_id",
                &req.evidence_h_mem_id,
                format!(
                    "Evidence h_mem '{}' not found — memory_insert requires an existing citation",
                    req.evidence_h_mem_id
                ),
                "Failed to verify evidence h_mem",
            )?;
            // S044-F01: monotonic provenance join — a memory inserted from a
            // PrivateInference call context is ALWAYS PrivateInference (and
            // routes to `curator-private.db` when split stores are configured),
            // even when citing a PublicProvider evidence row.
            let effective_provenance = join_provenance(
                hkask_mcp_server::current_call_provenance(),
                evidence.inference_provenance,
            );
            let memory = self.db.write_memory_for_provenance(effective_provenance)?;

            // The write-path contradiction check (R5, fermi's "unverified is
            // a work item" discipline inverted: a *conflicting* value is not
            // a work item, it is a finding): a colliding insert surfaces the
            // contradiction — never silently coexisting. Existing rows for
            // the same (entity, attribute) holding a different payload value
            // are surfaced by id and routed to the resolution paths; a
            // same-value insert (re-assertion) is not a contradiction.
            // `memory_update` (revision by id) and the machine ingestion
            // paths are unaffected — the gate is the deliberate
            // model-facing knowledge write only.
            // The untouched read: a contradiction check is not a recall, so
            // it must not reset the decay clocks of the rows it compares.
            // S044-F02: isolate contradiction comparison strictly to the same
            // provenance lane so public inserts never collide against, leak IDs
            // of, or act as a value-equality oracle over PrivateInference rows.
            let existing = memory
                .query_deduped_untouched(&req.entity)
                .map_err(|e| map_memory_store_error(e, "Failed to read existing memories for the contradiction check"))?;
            // CU-21: the citation travels with the payload — the `evidence`
            // array (the distillation path's convention) is stored with the
            // value, so an inserted h_mem is traceable to the evidence it
            // was accepted on. It is a derived key: comparable_payload
            // strips it, so the contradiction check still compares facts —
            // a same-value re-assertion on a different citation is a
            // re-affirmation (each row keeps its own citation), not a
            // contradiction. The key is the tool's — a caller who supplies
            // their own `evidence` field is refused rather than silently
            // overwritten.
            let mut incoming = serde_json::Value::from(req.value);
            if let Some(obj) = incoming.as_object() {
                if obj.contains_key("evidence") {
                    return Err(McpToolError::invalid_argument(
                        "the 'evidence' key is reserved — memory_insert stores the cited h_mem id itself",
                    ));
                }
            }
            if !incoming.is_object() {
                incoming = json!({ "value": incoming });
            }
            if let Some(obj) = incoming.as_object_mut() {
                obj.insert(
                    "evidence".to_string(),
                    json!([req.evidence_h_mem_id.clone()]),
                );
            }
            let new_payload = comparable_payload(&incoming);
            let contradictions: Vec<&hkask_storage::HMem> = existing
                .iter()
                .filter(|h| {
                    h.inference_provenance == effective_provenance
                        && h.attribute == req.attribute
                        && comparable_payload(&h.value) != new_payload
                })
                .collect();
            if !contradictions.is_empty() {
                let ids: Vec<String> = contradictions.iter().map(|h| h.id.to_string()).collect();
                return Err(McpToolError::failed_precondition(format!(
                    "contradiction: {} existing h_mem(s) for '{}/{}' hold a different value ({ids:?}). \
                     Resolve via memory_resolve_contradiction (forget or update_confidence on the \
                     wrong one) or memory_update (revise by id); a differing value for the same \
                     key may not coexist silently",
                    contradictions.len(),
                    req.entity,
                    req.attribute
                )));
            }

            // Evidence and notes are tool concerns; passage derivation and
            // atomic row/vector publication belong to the shared store.
            // The value is an object already (the evidence array needed
            // its home), so a note is never dropped — it has a field to
            // live in.
            let mut value = incoming;
            if let Some(note) = &req.note {
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("_note".to_string(), serde_json::Value::String(note.clone()));
                }
            }

            let h_mem = hkask_storage::HMem::new(
                &req.entity,
                &req.attribute,
                value,
                self.webid,
            )
            .with_confidence(hkask_types::Confidence::new(0.5))
            // The extraction ceiling, enforced here: the model's insert is
            // `model_inference` — permanently, no matter how often it is
            // recalled or re-asserted. The request schema offers no tier
            // field to claim; only the machine paths that hold a verbatim
            // tool outcome stamp `tool_observed`.
            .with_provenance(hkask_storage::Provenance::ModelInference)
            // FR-4 (D-2) + S044-F01: monotonic join of the dispatch call
            // context's provenance and the cited evidence row's provenance.
            // Pinned by `memory_insert_inherits_the_evidence_inference_provenance`
            // and `private_call_insert_and_update_on_public_rows_join_provenance_to_private`.
            .with_inference_provenance(effective_provenance);
            let provenance = h_mem.provenance.as_str();
            let write = memory.prepare_semantic_write(h_mem)
                .map_err(|e| map_memory_store_error(e, "Failed to prepare curator memory"))?;
            let written_id = publish_semantic_write(self.inference_port.as_ref(), write).await?;

            RegulationSpan::Curation.emit("memory_inserted");

            Ok(json!({
                "inserted": true,
                "h_mem_id": written_id.to_string(),
                "entity": req.entity,
                "attribute": req.attribute,
                "confidence": 0.5,
                "provenance": provenance,
                "evidence_h_mem_id": req.evidence_h_mem_id,
                "semantic_recall": "embedded",
                "guidance": "Memory stored at confidence 0.5 with a semantic embedding, provenance model_inference (the extraction ceiling — a model-synthesized memory never retrieves as tool-observed). Use memory_update to adjust confidence after outcome observation. Retrieve via curator_memory_recall with this entity, or curator_semantic_search by meaning."
            }))
        })
        .await
    }

    /// Update an existing memory's confidence via Bayesian combination.
    ///
    /// The new confidence is combined with the existing confidence using
    /// log-odds (Bayesian) pooling — not replacement.
    #[tool(
        description = "Combine a new confidence (0.0 to 1.0) into a memory's stored confidence by log-odds pooling, optionally replacing its value; the row is replaced and the response carries the replacement h_mem_id. Above 0.5 raises the stored confidence, below 0.5 lowers it, 0.5 leaves it unchanged."
    )]
    pub async fn memory_update(
        &self,
        Parameters(req): Parameters<MemoryUpdateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "memory_update", async {
            let new_confidence_raw = checked_confidence("new_confidence", req.new_confidence)?;
            let (source_memory, h_mem_id, existing_h_mem) = fetch_existing_h_mem_routed(
                &self.db,
                "h_mem_id",
                &req.h_mem_id,
                format!("h_mem '{}' not found", req.h_mem_id),
                "Failed to fetch h_mem for update",
            )?;

            // Bayesian-combine the new confidence with the existing one.
            let combined = hkask_memory::combine_confidences(
                existing_h_mem.confidence,
                new_confidence_raw,
            );

            // Use the new value if provided, otherwise keep the existing.
            let value = req
                .new_value
                .map(serde_json::Value::from)
                .unwrap_or_else(|| existing_h_mem.value.clone());

            let effective_provenance = join_provenance(
                hkask_mcp_server::current_call_provenance(),
                existing_h_mem.inference_provenance,
            );
            let target_memory = self.db.write_memory_for_provenance(effective_provenance)?;
            let written_id = if effective_provenance == existing_h_mem.inference_provenance
                && Arc::ptr_eq(&source_memory, &target_memory)
            {
                let write = source_memory
                    .prepare_semantic_revision(&h_mem_id, value, combined)
                    .map_err(|e| map_memory_store_error(e, "Failed to prepare memory revision"))?;
                publish_semantic_write(self.inference_port.as_ref(), write).await?
            } else {
                publish_provenance_upgraded_revision(
                    self.inference_port.as_ref(),
                    &source_memory,
                    &target_memory,
                    &h_mem_id,
                    &existing_h_mem,
                    value,
                    combined,
                    effective_provenance,
                )
                .await?
            };

            RegulationSpan::Curation.emit("memory_updated");

            // CU-31 (b): a write that leaves the row at or below the
            // consolidation floor is a deferred delete — the response says
            // so, never silently.
            let combined_confidence = combined.value();
            let floor = consolidation_floor();
            let mut response = json!({
                "updated": true,
                "h_mem_id": written_id.to_string(),
                "semantic_recall": "embedded",
                "previous_confidence": existing_h_mem.confidence.value(),
                "input_confidence": req.new_confidence,
                "combined_confidence": combined_confidence,
                "reason": req.reason,
                "guidance": "Confidence updated via Bayesian combination. Use curator_memory_recall to verify."
            });
            if combined_confidence <= floor {
                response["deferred_delete"] = json!(true);
                response["deferred_delete_note"] = json!(format!(
                    "the combined confidence {:.2} is at or below the editor's consolidation floor \
                     ({floor}) — the next consolidation pass deletes this row; \
                     memory_resolve_contradiction's 'forget' is the honest strategy for intentional removal",
                    combined_confidence
                ));
            }
            Ok(response)
        })
        .await
    }

    /// Resolve a contradiction between two or more memories.
    ///
    /// This is the therapy process tool — it resolves cognitive dissonance
    /// in the memory store by forgetting or de-conflicting contradictory
    /// h_mems.
    #[tool(
        description = "Resolve a contradiction between memories by acting on one target h_mem: 'forget' deletes it, 'update_confidence' sets its confidence to new_confidence and returns the replacement id. Requires a non-blank reason citing the contradiction; the reason and the ids are persisted with the deletion as a reg.curation record in the Regulation archive — never in the memory store."
    )]
    pub async fn memory_resolve_contradiction(
        &self,
        Parameters(req): Parameters<MemoryResolveContradictionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "memory_resolve_contradiction", async {
            if req.h_mem_ids.is_empty() || req.h_mem_ids.iter().any(|id| id.trim().is_empty()) {
                return Err(McpToolError::invalid_argument(
                    "h_mem_ids must contain at least one non-empty h_mem ID",
                ));
            }
            require_text("reason", &req.reason)?;
            if !RESOLUTION_STRATEGIES.contains(&req.strategy.as_str()) {
                return Err(McpToolError::invalid_argument(format!(
                    "Unknown strategy '{}' — must be one of: {}",
                    req.strategy,
                    RESOLUTION_STRATEGIES.join(", ")
                )));
            }
            let (source_memory, target_id, target_h_mem) = fetch_existing_h_mem_routed(
                &self.db,
                "target_h_mem_id",
                &req.target_h_mem_id,
                format!("Target h_mem '{}' not found", req.target_h_mem_id),
                "Failed to fetch target h_mem",
            )?;
            let effective_provenance = join_provenance(
                hkask_mcp_server::current_call_provenance(),
                target_h_mem.inference_provenance,
            );

            match req.strategy.as_str() {
                "forget" => {
                    source_memory
                        .delete_h_mem(&target_id)
                        .map_err(|e| map_memory_store_error(e, "Failed to forget h_mem"))?;
                    RegulationSpan::Curation.emit("contradiction_forgotten");
                    // CU-23 + S044-F03: the reason and contradicting ids are durable
                    // with the deletion, stamped with `effective_provenance` so
                    // public `reg_query` calls never leak private curation reasons.
                    persist_curation_record(
                        &self.db.stores_for_provenance(effective_provenance),
                        &self.webid,
                        "contradiction_forgotten",
                        json!({
                            "target_h_mem_id": req.target_h_mem_id,
                            "contradicting_h_mem_ids": req.h_mem_ids,
                            "reason": req.reason,
                        }),
                        effective_provenance,
                    );
                    Ok(json!({
                        "resolved": true,
                        "strategy": "forget",
                        "target_h_mem_id": req.target_h_mem_id,
                        "contradicting_h_mem_ids": req.h_mem_ids,
                        "reason": req.reason
                    }))
                }
                "update_confidence" => {
                    let new_confidence = req.new_confidence.ok_or_else(|| {
                        McpToolError::invalid_argument(
                            "new_confidence is required for 'update_confidence' strategy",
                        )
                    })?;
                    let confidence = checked_confidence("new_confidence", new_confidence)?;
                    let target_memory = self.db.write_memory_for_provenance(effective_provenance)?;
                    let written_id = if effective_provenance == target_h_mem.inference_provenance
                        && Arc::ptr_eq(&source_memory, &target_memory)
                    {
                        let write = source_memory
                            .prepare_semantic_revision(
                                &target_id,
                                target_h_mem.value,
                                confidence,
                            )
                            .map_err(|e| {
                                map_memory_store_error(
                                    e,
                                    "Failed to prepare contradiction revision",
                                )
                            })?;
                        publish_semantic_write(self.inference_port.as_ref(), write).await?
                    } else {
                        publish_provenance_upgraded_revision(
                            self.inference_port.as_ref(),
                            &source_memory,
                            &target_memory,
                            &target_id,
                            &target_h_mem,
                            target_h_mem.value.clone(),
                            confidence,
                            effective_provenance,
                        )
                        .await?
                    };
                    RegulationSpan::Curation.emit("contradiction_confidence_lowered");
                    // CU-31 (b): at or below the floor the row is a deferred
                    // delete — the response says so.
                    let floor = consolidation_floor();
                    let mut response = json!({
                        "resolved": true,
                        "strategy": "update_confidence",
                        "target_h_mem_id": written_id.to_string(),
                        "new_confidence": new_confidence,
                        "contradicting_h_mem_ids": req.h_mem_ids,
                        "reason": req.reason
                    });
                    if confidence.value() <= floor {
                        response["deferred_delete"] = json!(true);
                        response["deferred_delete_note"] = json!(format!(
                            "the new confidence {:.2} is at or below the editor's consolidation floor \
                             ({floor}) — the next consolidation pass deletes this row; \
                             'forget' is the honest strategy for intentional removal",
                            confidence.value()
                        ));
                    }
                    Ok(response)
                }
                other => Err(McpToolError::internal(format!(
                    "strategy '{other}' passed validation but has no handler"
                ))),
            }
        })
        .await
    }

    // ── Memory hygiene tools (age prune + dedup) ─────────────────────────
    //
    // Complements the confidence-based consolidation service with two
    // deterministic, non-LLM axes: age-based hard-delete and near-duplicate
    // string dedup. Both are operator-invoked — the curator proposes, the
    // operator approves (same consent model as therapy/contradiction
    // resolution).

    /// Prune h_mems older than a specified age. Hard-deletes h_mems whose
    /// observation timestamp is older than `max_age_days`, optionally
    /// sparing h_mems recalled within a grace window. Distinct from
    /// confidence decay (lowers weight, never deletes) and confidence-based
    /// consolidation (deletes low-confidence).
    #[tool(
        description = "Hard-delete curator h_mems older than max_age_days: by default turn storage only (curator:thread:), leaving knowledge-layer rows untouched. Set all_layers=true for the full store, or prefixes=[\"...\"] to prune only entities under those prefixes (e.g. the skill_use_issue: incident log); spare_recalled_within_days spares rows recalled within that many days. Deterministic, non-LLM. Distinct from confidence-based consolidation."
    )]
    pub async fn curator_memory_prune(
        &self,
        Parameters(req): Parameters<MemoryPruneRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_memory_prune", async {
            if req.max_age_days <= 0 {
                return Err(McpToolError::invalid_argument(
                    "max_age_days must be positive",
                ));
            }
            // The store subtracts these from now; an amount no calendar
            // offset can hold is refused here, before it reaches the store.
            lookback_start("max_age_days", chrono::Duration::try_days(req.max_age_days))?;
            if let Some(days) = req.spare_recalled_within_days {
                if days <= 0 {
                    return Err(McpToolError::invalid_argument(
                        "spare_recalled_within_days must be positive",
                    ));
                }
                lookback_start(
                    "spare_recalled_within_days",
                    chrono::Duration::try_days(days),
                )?;
            }
            let memories = read_memories_for_call(&self.db)?;
            let call_provenance = hkask_mcp_server::current_call_provenance();

            let all_layers = req.all_layers.unwrap_or(false);
            // Fail-closed scope: the default valve touches turn storage
            // only — knowledge-layer rows (rulings, verified status,
            // lessons) are destroyed only by explicit opt-in. A caller-
            // supplied `prefixes` list narrows the valve to those entity
            // prefixes (e.g. the skill_use_issue: incident log); empty
            // and all_layers-conflicting scope requests are rejected
            // visibly — a silent no-op or a silently-picked scope is a
            // broken feedback loop.
            if all_layers && req.prefixes.is_some() {
                return Err(McpToolError::invalid_argument(
                    "conflicting scopes: all_layers=true and prefixes are both set — name one scope",
                ));
            }
            if let Some(prefixes) = req.prefixes.as_deref() {
                if prefixes.is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "prefixes must name at least one entity prefix — an empty list would silently prune nothing",
                    ));
                }
            }
            let explicit_scope: Option<Vec<&str>> = if all_layers {
                None
            } else if let Some(prefixes) = req.prefixes.as_deref() {
                Some(prefixes.iter().map(String::as_str).collect())
            } else {
                Some(vec![thread_turns::SHARED_TURN_PREFIX])
            };

            let mut outcome = hkask_memory::memory_store::PruneOutcome {
                candidates: 0,
                deleted_count: 0,
                spared_count: 0,
                failed_count: 0,
            };
            let mut excluded_private = 0usize;
            for memory in &memories {
                let (part, excluded) = prune_admitted_by_age(
                    memory,
                    explicit_scope.as_deref(),
                    req.max_age_days,
                    req.spare_recalled_within_days,
                )?;
                outcome.candidates += part.candidates;
                outcome.deleted_count += part.deleted_count;
                outcome.spared_count += part.spared_count;
                outcome.failed_count += part.failed_count;
                excluded_private += excluded;
            }

            RegulationSpan::Curation.emit("memory_pruned");
            persist_curation_record(
                &self.db.stores_for_provenance(call_provenance),
                &self.webid,
                "memory_pruned",
                json!({
                    "all_layers": all_layers,
                    "prefixes": req.prefixes,
                    "max_age_days": req.max_age_days,
                    "candidates": outcome.candidates,
                    "deleted_count": outcome.deleted_count,
                    "spared_count": outcome.spared_count,
                    "failed_count": outcome.failed_count,
                }),
                call_provenance,
            );

            let mut out = json!({
                "pruned": true,
                "all_layers": all_layers,
                "prefixes": req.prefixes,
                "max_age_days": req.max_age_days,
                "spare_recalled_within_days": req.spare_recalled_within_days,
                "candidates": outcome.candidates,
                "deleted_count": outcome.deleted_count,
                "spared_count": outcome.spared_count,
                "failed_count": outcome.failed_count,
            });
            if excluded_private > 0 {
                out["excluded_private_rows"] = json!(excluded_private);
            }
            Ok(out)
        })
        .await
    }

    /// Deduplicate h_mems by normalized string value. Groups by
    /// (entity, attribute, normalized_value), keeps highest-confidence,
    /// deletes the rest. Non-string values skipped.
    #[tool(
        description = "Delete duplicate curator h_mems: among the newest `limit` rows (default 10000), rows sharing entity, attribute and normalized string value keep the highest-confidence one and lose the rest. Deterministic, non-LLM. Non-string values and turn storage (curator:thread:) skipped. limit_reached is true when older rows may have been left unscanned."
    )]
    pub async fn curator_memory_dedup(
        &self,
        Parameters(req): Parameters<MemoryDedupRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_memory_dedup", async {
            let limit = req.limit.unwrap_or(10_000);
            if limit == 0 {
                return Err(McpToolError::invalid_argument("limit must be positive"));
            }
            let memories = read_memories_for_call(&self.db)?;
            let call_provenance = hkask_mcp_server::current_call_provenance();

            let mut outcome = hkask_memory::memory_store::DedupOutcome {
                scanned: 0,
                groups_with_dupes: 0,
                deleted_count: 0,
                failed_count: 0,
                skipped_non_string: 0,
                skipped_turn_storage: 0,
            };
            let mut excluded_private = 0usize;
            let mut limit_reached = false;
            for memory in &memories {
                let remaining = limit.saturating_sub(outcome.scanned);
                if remaining == 0 {
                    limit_reached = true;
                    break;
                }
                let (part, excluded) = dedup_admitted_by_normalized_value(memory, remaining)?;
                if part.scanned == remaining {
                    limit_reached = true;
                }
                outcome.scanned += part.scanned;
                outcome.groups_with_dupes += part.groups_with_dupes;
                outcome.deleted_count += part.deleted_count;
                outcome.failed_count += part.failed_count;
                outcome.skipped_non_string += part.skipped_non_string;
                outcome.skipped_turn_storage += part.skipped_turn_storage;
                excluded_private += excluded;
            }

            RegulationSpan::Curation.emit("memory_deduped");
            persist_curation_record(
                &self.db.stores_for_provenance(call_provenance),
                &self.webid,
                "memory_deduped",
                json!({
                    "scanned": outcome.scanned,
                    "groups_with_dupes": outcome.groups_with_dupes,
                    "deleted_count": outcome.deleted_count,
                    "failed_count": outcome.failed_count,
                    "skipped_non_string": outcome.skipped_non_string,
                    "skipped_turn_storage": outcome.skipped_turn_storage,
                }),
                call_provenance,
            );

            let mut out = json!({
                "deduped": true,
                "scanned": outcome.scanned,
                "limit_reached": limit_reached,
                "groups_with_dupes": outcome.groups_with_dupes,
                "deleted_count": outcome.deleted_count,
                "failed_count": outcome.failed_count,
                "skipped_non_string": outcome.skipped_non_string,
                "skipped_turn_storage": outcome.skipped_turn_storage,
            });
            if excluded_private > 0 {
                out["excluded_private_rows"] = json!(excluded_private);
            }
            Ok(out)
        })
        .await
    }

    /// Repair semantic embeddings for knowledge-layer h_mems whose insert-time
    /// embedding failed. Embeddings-table-only — no h_mem is created,
    /// modified, or deleted. Turns (embedded at ingest), distillation
    /// watermarks and goal rows are excluded.
    #[tool(
        description = "Backfill semantic embeddings across the memory store — `missing` embeds h_mems whose exact canonical passage lacks a row; `model_mismatch` re-embeds rows a model migration gated out. Eligibility for mode=missing is exact (entity + canonical passage), not entity-level; turns, distillation watermarks, and goal rows are excluded. mode=model_mismatch re-embeds stored rows whose recorded model matches neither the current requested embedding model nor its provider-confirmed actual form — the same two-form predicate the search gate excludes on. dry_run lists candidates and changes nothing; in model_mismatch mode it still makes one embedding call to learn the provider's name for the model."
    )]
    pub async fn curator_memory_backfill_embeddings(
        &self,
        Parameters(req): Parameters<BackfillEmbeddingsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_memory_backfill_embeddings", async {
            let mode = match req.mode.as_deref().map(str::trim) {
                None | Some("") | Some("missing") => backfill::BackfillMode::Missing,
                Some("model_mismatch") => backfill::BackfillMode::ModelMismatch,
                Some(other) => {
                    return Err(McpToolError::invalid_argument(format!(
                        "unknown mode '{other}' — must be one of: {}",
                        BACKFILL_MODES.join(", ")
                    )));
                }
            };

            let memory = self
                .db
                .write_memory_for_provenance(hkask_mcp_server::current_call_provenance())?;

            match mode {
                backfill::BackfillMode::Missing => {
                    backfill::backfill_missing_passages(
                        self.inference_port.as_ref(),
                        &memory,
                        req.dry_run.unwrap_or(false),
                    )
                    .await
                }
                backfill::BackfillMode::ModelMismatch => {
                    backfill::backfill_model_mismatched_rows(
                        self.inference_port.as_ref(),
                        &memory,
                        req.dry_run.unwrap_or(false),
                    )
                    .await
                }
            }
        })
        .await
    }

    /// Queries the thread's turn h_mems (`curator:thread:<thread_id>`) via
    /// `thread_turns::thread_turns`, the one turn-discovery contract shared
    /// with the distillation pass. Returns their IDs and content as
    /// extraction candidates. The curator reviews and inserts the ones
    /// worth keeping via `memory_insert` (which requires evidence citation).
    /// This is the on-demand version of ALWAYS-mode learning — no
    /// background LLM call, no automatic insertion.
    #[tool(
        description = "List one thread's stored turns as memory_insert candidates — each returned h_mem_id is valid evidence; read-only, no model call, nothing inserted. Takes the complete thread id and returns that thread's turn h_mems with IDs and content."
    )]
    pub async fn curator_memory_extract(
        &self,
        Parameters(req): Parameters<MemoryExtractRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_memory_extract", async {
            require_text("thread_id", &req.thread_id)?;
            let memories = read_memories_for_call(&self.db)?;

            // The turn-discovery contract lives in `thread_turns`, shared
            // with the distillation pass: one thread's turns, exactly.
            let mut raw_h_mems = Vec::new();
            let mut seen_ids = std::collections::HashSet::new();
            for memory in &memories {
                let rows = thread_turns::thread_turns(memory, &req.thread_id)
                    .map_err(|e| map_memory_store_error(e, "Failed to query thread turns"))?;
                for row in rows {
                    if seen_ids.insert(row.id) {
                        raw_h_mems.push(row);
                    }
                }
            }
            raw_h_mems.sort_by_key(|row| row.observed_at);
            // The same provenance partition as every other read: a public
            // call context never receives a private-marked turn.
            let (h_mems, excluded_private) = admitted(raw_h_mems);

            let candidates: Vec<serde_json::Value> = h_mems
                .iter()
                .map(|h| {
                    json!({
                        "h_mem_id": h.id.to_string(),
                        "entity": h.entity,
                        "attribute": h.attribute,
                        "value": h.value,
                        "confidence": h.confidence.value(),
                        "observed_at": h.observed_at.to_rfc3339(),
                    })
                })
                .collect();

            RegulationSpan::Curation.emit("memory_extracted");

            let mut result = json!({
                "thread_id": req.thread_id,
                "turn_count": candidates.len(),
                "candidates": candidates,
                "guidance": "Review the candidates and insert the ones worth keeping via memory_insert, citing a candidate's h_mem_id as evidence_h_mem_id.",
            });
            if excluded_private > 0 {
                result["excluded_private_rows"] = json!(excluded_private);
            }
            Ok(result)
        })
        .await
    }

    /// Deterministic code-citation staleness sweep (therapy category g):
    /// flags knowledge-layer h_mems whose repository citations no longer
    /// resolve against the live workspace tree — missing files, `file:line`
    /// past the live file's end, `D<N>` seams retired from `DIVERGENCE.md`.
    /// Strictly read-only (`rows_deleted == 0`): flagged rows are therapy
    /// candidates the operator re-anchors or prunes, so a transient branch
    /// switch can never erase valid memories. A missing `DIVERGENCE.md` is
    /// reported as `divergence_md_present: false` and skips seam checks
    /// visibly rather than faking them.
    #[tool(
        description = "Read-only citation staleness sweep (rows_deleted == 0) over knowledge-layer h_mems: flags missing files, out-of-bounds file:line, and D<N> seams retired from DIVERGENCE.md. Never deletes rows; flagged rows are therapy candidates for operator-approved re-anchoring or pruning. repo_root is the workspace root to check citations against (the directory containing DIVERGENCE.md); a missing DIVERGENCE.md is reported as divergence_md_present=false. Deterministic, non-LLM."
    )]
    pub async fn curator_citation_staleness_sweep(
        &self,
        Parameters(req): Parameters<CitationStalenessSweepRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "curator_citation_staleness_sweep", async {
            require_text("repo_root", &req.repo_root)?;
            let repo_root = std::path::PathBuf::from(req.repo_root.trim());
            if !repo_root.is_dir() {
                return Err(McpToolError::invalid_argument(
                    "repo_root must be an existing directory — the workspace root containing DIVERGENCE.md",
                ));
            }
            let memories = read_memories_for_call(&self.db)?;

            let mut rows_scanned = 0usize;
            let mut citations_checked = 0usize;
            let mut rows_deleted = 0usize;
            let mut candidates: Vec<serde_json::Value> = Vec::new();
            let mut excluded_private = 0usize;
            for memory in &memories {
                let outcome = forgetting::sweep_stale_code_citations(memory, &repo_root).map_err(
                    |error| map_memory_store_error(error, "Failed to sweep stale code citations"),
                )?;
                rows_scanned += outcome.rows_scanned;
                citations_checked += outcome.citations_checked;
                rows_deleted += outcome.rows_deleted;
                excluded_private += outcome.excluded_private;
                for candidate in outcome.candidates {
                    candidates.push(json!({
                        "h_mem_id": candidate.h_mem_id,
                        "entity": candidate.entity,
                        "attribute": candidate.attribute,
                        "reasons": candidate
                            .reasons
                            .iter()
                            .map(citation_reason_note)
                            .collect::<Vec<_>>(),
                    }));
                }
            }

            RegulationSpan::Curation.emit("memory_citations_swept");

            let mut out = json!({
                "swept": true,
                "rows_scanned": rows_scanned,
                "citations_checked": citations_checked,
                "rows_deleted": rows_deleted,
                "divergence_md_present": repo_root.join("DIVERGENCE.md").is_file(),
                "candidates": candidates,
                "guidance": "Flagged rows are therapy candidates: re-anchor the citation via memory_update, or prune the row via curator_memory_prune, after operator approval. This sweep never deletes rows.",
            });
            if excluded_private > 0 {
                out["excluded_private_rows"] = json!(excluded_private);
            }
            Ok(out)
        })
        .await
    }
}

// ── Server startup ─────────────────────────────────────────────────────

/// The curator server's embedding model: the env-resolved model when
/// configured, else `None`. `None` is NOT a fallback to a hidden constant
/// (the operator's no-hidden-models spec) — the semantic paths degrade
/// VISIBLY: `curator_semantic_search` returns a typed
/// `EmbeddingNotConfigured` error naming the setting, and the insert
/// paths fail with `failed_precondition` + warn (the embed-at-publish
/// contract, therapy 2026-10-08 — no row lands without its embedding).
/// The 2026-09-04 regression this previously guarded against was the
/// degradation firing under default settings; the ratified fix is to
/// surface it (the operator sets `kask.models.embedding_model`), not to
/// hide a constant model behind it.
pub(crate) fn curator_embedding_model() -> Option<String> {
    hkask_inference::model_constants::embedding_model()
}

async fn semantic_embedding(
    inference_port: &dyn hkask_types::InferencePort,
    text: &str,
    provenance: hkask_types::InferenceProvenance,
) -> Result<(Vec<f32>, String), McpToolError> {
    // zed-kask: D100 Phase 2 (operator direction 2026-11-06) — the passage
    // embed routes by the ROW's provenance: a private-marked row's passage
    // embeds through the private leg's LOCAL model only (the model string
    // is the routing signal — the stored-vector model-string gate's
    // discipline, so the vector joins the private-call KNN space); a public
    // row keeps the configured model. Unset/empty private model fails the
    // write visibly (the child's embed-at-publish contract: the row does
    // NOT land without its embedding) — never a widening to the cloud.
    let model = routed_model_for_provenance(
        provenance,
        hkask_inference::model_constants::private_embedding_model(),
        curator_embedding_model(),
    )
    .map_err(|reason| model_not_configured("embedding failed — memory was NOT stored", &reason))?;
    let batch = inference_port
        .embed_with_dimensions(
            &model,
            &[text.to_string()],
            Some(hkask_storage::embedding_dim() as u32),
        )
        .await
        // An unset model (above) is absent state; a provider that does not
        // answer is classified by what failed: transient outages are
        // `unavailable` (retry), a missing credential `permission_denied`.
        .map_err(|error| embed_error(&error, "embedding failed — memory was NOT stored"))?;
    hkask_mcp_server::server::record_tool_usage(batch.usage);
    if batch.vectors.len() != 1 || batch.vectors[0].is_empty() {
        return Err(McpToolError::internal(
            "embedding failed — expected one nonempty vector; memory was NOT stored",
        ));
    }
    let model = batch.actual_model.unwrap_or(model);
    let vector = batch.vectors.into_iter().next().ok_or_else(|| {
        McpToolError::internal("embedding failed — no vector returned; memory was NOT stored")
    })?;
    Ok((vector, model))
}

async fn publish_semantic_write(
    inference_port: &dyn hkask_types::InferencePort,
    write: hkask_memory::memory_store::SemanticWrite<'_>,
) -> Result<hkask_storage::HMemId, McpToolError> {
    // zed-kask: D100 Phase 2 — the passage embed routes by the ROW's
    // provenance: a private-marked row's passage never crosses to the
    // configured cloud embedder.
    let (vector, model) = semantic_embedding(
        inference_port,
        write.passage(),
        write.inference_provenance(),
    )
    .await?;
    write
        .publish(&vector, &model)
        .map_err(|error| map_memory_store_error(error, "Failed to publish memory and embedding"))
}

#[allow(clippy::too_many_arguments)]
async fn publish_provenance_upgraded_revision(
    inference_port: &dyn hkask_types::InferencePort,
    source_memory: &Arc<hkask_memory::MemoryStore>,
    target_memory: &Arc<hkask_memory::MemoryStore>,
    previous_id: &hkask_storage::HMemId,
    existing_h_mem: &hkask_storage::HMem,
    value: serde_json::Value,
    confidence: hkask_types::Confidence,
    effective_provenance: hkask_types::InferenceProvenance,
) -> Result<hkask_storage::HMemId, McpToolError> {
    let old_passage = hkask_memory::semantic_passage_for_h_mem(existing_h_mem);
    let now = chrono::Utc::now();
    let mut upgraded = existing_h_mem.clone();
    upgraded.id = hkask_storage::HMemId::new();
    upgraded.value = value;
    upgraded.confidence = confidence;
    upgraded.observed_at = now;
    upgraded.recalled_at = now;
    upgraded.inference_provenance = effective_provenance;

    if !Arc::ptr_eq(source_memory, target_memory) {
        let write = target_memory
            .prepare_semantic_write(upgraded)
            .map_err(|e| map_memory_store_error(e, "Failed to prepare private memory revision"))?;
        return publish_semantic_write(inference_port, write).await;
    }

    let passage = hkask_memory::semantic_passage_for_h_mem(&upgraded).unwrap_or_else(|| {
        let derived = format!(
            "{} {}: {}",
            upgraded.entity, upgraded.attribute, upgraded.value
        );
        if let Some(object) = upgraded.value.as_object_mut() {
            object.insert(
                "recall_text".to_string(),
                serde_json::Value::String(derived.clone()),
            );
        } else {
            upgraded.value = json!({"value": upgraded.value, "recall_text": derived});
        }
        derived
    });
    let (vector, model) =
        semantic_embedding(inference_port, &passage, effective_provenance).await?;
    let entity_for_proj = upgraded.entity.clone();
    let owner_for_proj = upgraded.access.owner_webid;
    source_memory
        .embedding_store()
        .publish_h_mem(
            &upgraded,
            Some((previous_id, old_passage.as_deref())),
            &vector,
            &model,
            &passage,
            |attribute, val| {
                let probe = hkask_storage::HMem::new(
                    &entity_for_proj,
                    attribute,
                    val.clone(),
                    owner_for_proj,
                );
                hkask_memory::semantic_passage_for_h_mem(&probe)
            },
        )
        .map_err(|error| {
            map_memory_store_error(
                hkask_memory::MemoryStoreError::from(error),
                "Failed to publish upgraded memory revision",
            )
        })?;
    Ok(upgraded.id)
}

/// Embed `text` under `entity` so semantic recall finds the h_mem being
/// stored — the entity_ref invariant (memory-system-specification.md §3).
/// Used by embedding-only backfill and distillation's guarded batch
/// publication. Standalone knowledge inserts and revisions instead use
/// `publish_semantic_write`, which cannot commit a row without its vector.
/// Returns whether the embedding landed; every failure warns naming the cause.
pub(crate) async fn embed_for_semantic_recall(
    inference_port: &dyn hkask_types::InferencePort,
    memory: &hkask_memory::MemoryStore,
    entity: &str,
    text: &str,
    provenance: hkask_types::InferenceProvenance,
) -> bool {
    let result = match semantic_embedding(inference_port, text, provenance).await {
        Ok((vector, model)) => memory
            .store_embedding(entity, &vector, &model, Some(text))
            .map(|_| ())
            .map_err(|error| map_memory_store_error(error, "Failed to store semantic embedding")),
        Err(error) => Err(error),
    };
    match result {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(target: "hkask.mcp.curator", %error, entity,
                "Embedding unavailable — semantic recall degraded for this memory");
            false
        }
    }
}

pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    // The async resolution must happen before the sync server-construction
    // closure — the canonical explanation lives on
    // `hkask_inference::resolve_inference_port` (the near-verbatim copies
    // collapsed there, 2026-11-07). Used by `curator_semantic_search` and
    // `curator_consult` to embed recall queries, by the insert paths
    // (`memory_insert`, `curator_report_skill_use_issue`) to embed stored
    // memories, and by the distillation timer to embed lessons.
    let inference_port = hkask_inference::resolve_inference_port().await;
    hkask_mcp_server::run_server(
        SERVER_NAME,
        env!("CARGO_PKG_VERSION"),
        move |ctx: hkask_mcp_server::server::ServerContext| {
            let db = Arc::new(CuratorDb::from_context(&ctx));
            // ALWAYS-mode distillation: the background pass shares the
            // server's DB handle, inference port, and webid, so lessons
            // enter through the same evidence + 0.5-floor invariants the
            // memory_insert tool enforces.
            distillation::spawn_distillation_timer(
                Arc::clone(&db),
                inference_port.clone(),
                ctx.webid,
                distillation::DistillationConfig::from_env(),
            );
            Ok(CuratorServer::new(ctx.webid, db, inference_port.clone()))
        },
        vec![hkask_mcp_server::db_passphrase_requirement()],
    )
    .await
}

#[cfg(test)]
mod tests {
    /// zed-kask: D100's named follow-up (operator-approved 2026-11-06) — the
    /// query-embed routing decision's table, over pure decision inputs (no
    /// process env): private contexts take the private model or the surfaced
    /// degrade; public contexts take the configured model or its surfaced
    /// error. The env plumbing and the recording-port falsifier live in
    /// tests/tool_behavior.rs.
    #[test]
    fn routed_model_for_provenance_pins_the_provenance_table() {
        use hkask_types::InferenceProvenance;

        assert!(
            matches!(
                super::routed_model_for_provenance(
                    InferenceProvenance::PrivateInference,
                    Some("ollama/qwen3-embedding:0.6b".to_string()),
                    None,
                ),
                Ok(model) if model == "ollama/qwen3-embedding:0.6b"
            ),
            "a private call embeds through the private model"
        );
        assert!(
            matches!(
                super::routed_model_for_provenance(
                    InferenceProvenance::PrivateInference,
                    None,
                    Some("cloud/model".to_string()),
                ),
                Err(super::SemanticRecallError::PrivateEmbeddingNotConfigured)
            ),
            "a private call with no private model degrades — never the configured cloud model"
        );
        assert!(
            matches!(
                super::routed_model_for_provenance(
                    InferenceProvenance::PublicProvider,
                    None,
                    Some("cloud/model".to_string()),
                ),
                Ok(model) if model == "cloud/model"
            ),
            "a public call keeps the configured model unchanged"
        );
        assert!(
            matches!(
                super::routed_model_for_provenance(
                    InferenceProvenance::PublicProvider,
                    Some("ollama/x".to_string()),
                    None,
                ),
                Err(super::SemanticRecallError::EmbeddingNotConfigured)
            ),
            "the public path's surfaced error is unchanged"
        );
    }

    /// expect: "The curator server refuses startup without its DB
    /// passphrase — 13 of 15 tools fail without its stores (ping reports
    /// them; federated search reports each source's state) and there is
    /// no in-memory store to fall back to, so a limping start would surface
    /// permission_denied on every memory tool instead of one startup error
    /// naming HKASK_DB_PASSPHRASE." [P1] Motivating: User Sovereignty.
    /// [P2] Constraining: Transparent Imperfection — kata-kanban precedent
    /// (`kanban_startup_requires_durable_storage`).
    /// pre: the production startup source is compiled
    /// post: HKASK_DB_PASSPHRASE is declared required, not optional
    #[test]
    fn curator_startup_requires_durable_storage() {
        let source = include_str!("hkask_mcp_curator.rs");
        assert!(
            source.contains("db_passphrase_requirement()"),
            "HKASK_DB_PASSPHRASE must be declared required via the shared helper — the curator \
             has no in-memory fallback (CuratorStores::empty), so an optional declaration \
             starts a server whose every DB-backed tool call fails"
        );
    }
}

// Pins the registered tool-surface count end-to-end against the router the
// `#[tool_router(server_handler)]` block generates. Adding or removing a
// curator tool is an intentional surface change — this pin catches
// accidental drift (a dropped `#[tool]` attribute, a registration change)
// instead of shipping as an undocumented surface change. Mirrors
// the shared `tool_surface_pin!` macro every built-in server carries.
#[cfg(test)]
mod tool_surface_tests {
    use super::CuratorServer;

    include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));
    hkask_mcp_server::tool_surface_pin!(
        CuratorServer::tool_router(),
        "tool_router",
        16,
        tool_surface_is_exactly_16_registered_tools,
    );

    /// mcp-tool-review CU-02 pin: every registered tool's live input schema
    /// is closed (`additionalProperties: false`), so a caller sees before the
    /// call that an undeclared argument is refused (bare-boolean absence is
    /// enforced fleet-wide by `tool_surface_pin!`).
    #[test]
    fn every_tool_schema_is_closed_to_unknown_arguments() {
        let tools = CuratorServer::tool_router().list_all();
        assert_eq!(tools.len(), 16);
        for tool in tools {
            let schema = serde_json::to_value(&tool.input_schema).expect("schema serializes");
            assert_eq!(
                schema["additionalProperties"],
                serde_json::json!(false),
                "{} must refuse undeclared arguments in its schema",
                tool.name
            );
        }
    }

    /// mcp-tool-review CU-15 pin: each closed-vocabulary input names its
    /// accepted values in the live schema, read from the list the handler
    /// refuses against, so the two cannot drift.
    #[test]
    fn closed_vocabulary_inputs_enumerate_their_values_in_the_live_schema() {
        let tools = CuratorServer::tool_router().list_all();
        let enum_of = |tool: &str, property: &str| -> Vec<String> {
            let schema = tools
                .iter()
                .find(|candidate| candidate.name == tool)
                .map(|candidate| serde_json::to_value(&candidate.input_schema).expect("schema"))
                .unwrap_or_else(|| panic!("{tool} is registered"));
            let node = &schema["properties"][property];
            let values = node
                .get("enum")
                .or_else(|| {
                    node["anyOf"]
                        .as_array()
                        .and_then(|arms| arms.iter().find_map(|arm| arm.get("enum")))
                })
                .and_then(|values| values.as_array())
                .unwrap_or_else(|| panic!("{tool}.{property} carries an enum: {node}"));
            values
                .iter()
                .map(|value| value.as_str().expect("string").to_string())
                .collect()
        };
        assert_eq!(
            enum_of("curator_memory_recall", "ontology_axis"),
            crate::types::ONTOLOGY_AXES
        );
        assert_eq!(
            enum_of("memory_resolve_contradiction", "strategy"),
            crate::types::RESOLUTION_STRATEGIES
        );
        assert_eq!(
            enum_of("curator_memory_backfill_embeddings", "mode"),
            crate::types::BACKFILL_MODES
        );
    }

    /// The text the editor forwards to the model for a description
    /// (`build_request_tools` in `crates/agent/src/thread.rs`): the whole
    /// description up to 200 bytes, else its first sentence within 200
    /// bytes, else a 200-byte cut ending in "...".
    fn forwarded(description: &str) -> String {
        if description.len() <= 200 {
            return description.to_string();
        }
        let mut cutoff = 200;
        while !description.is_char_boundary(cutoff) {
            cutoff -= 1;
        }
        match description[..cutoff].find(". ") {
            Some(position) => description[..=position].to_string(),
            None => format!("{}...", &description[..cutoff]),
        }
    }

    /// mcp-tool-review CU-30 pin. The editor forwards a description only to
    /// its first sentence within 200 bytes; the input schema is forwarded
    /// whole. Measured on a build of 13dbe1cf79: the forwarded text of the
    /// two deleting tools did not say they delete, and three write tools'
    /// forwarded text had lost the evidence requirement, the pooling rule
    /// and the strategies. Each forwarded text is a complete sentence, and
    /// for these tools it carries the fact a caller must have before
    /// calling.
    #[test]
    fn forwarded_descriptions_are_complete_and_carry_the_load_bearing_fact() {
        let tools = CuratorServer::tool_router().list_all();
        let lead_of = |name: &str| {
            let tool = tools
                .iter()
                .find(|tool| tool.name == name)
                .unwrap_or_else(|| panic!("tool {name} present"));
            forwarded(tool.description.as_deref().unwrap_or_default())
        };
        for tool in &tools {
            let lead = lead_of(&tool.name);
            assert!(
                lead.ends_with('.') && !lead.ends_with("..."),
                "{}: the forwarded text is cut mid-sentence: {lead:?}",
                tool.name
            );
        }
        for (name, facts) in [
            (
                "curator_memory_prune",
                &["Hard-delete", "turn storage only"][..],
            ),
            ("curator_memory_dedup", &["Delete duplicate"][..]),
            ("memory_insert", &["evidence", "0.5", "h_mem_id"][..]),
            (
                "memory_update",
                &["log-odds pooling", "replacement h_mem_id"][..],
            ),
            (
                "memory_resolve_contradiction",
                &["'forget' deletes", "'update_confidence'"][..],
            ),
            (
                "curator_memory_recall",
                &["one of the two", "resets the returned rows' decay clocks"][..],
            ),
            (
                "curator_consult",
                &["semantic similarity", "not a full curator agent turn"][..],
            ),
            (
                "curator_memory_backfill_embeddings",
                &["exact canonical passage", "model migration"][..],
            ),
            (
                "curator_memory_extract",
                &["valid evidence", "read-only"][..],
            ),
            (
                "curator_citation_staleness_sweep",
                &["Read-only", "rows_deleted == 0"][..],
            ),
        ] {
            let lead = lead_of(name);
            for fact in facts {
                assert!(
                    lead.contains(fact),
                    "{name}: the forwarded text must carry {fact:?}: {lead:?}"
                );
            }
        }
    }

    // mcp-tool-review CU-32 pin: DELETED 2026-11-16 — the invariant moved to
    // the shared `tool_surface_pin!` macro
    // (kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs), which emits
    // `no_nullable_property_is_listed_as_required` for every server, this
    // one included (the 3fac9cfa (b) landing). The per-server name checks
    // (the two empty required lists and the one that must stay required)
    // are covered by the tool_behavior suite's schema pins and the
    // required-nullable.jq probe over the live listing.

    /// mcp-tool-review CU-31 pin (schema half). The editor's consolidation
    /// pass deletes every h_mem at or below `kask.memory.confidence_floor`,
    /// so a low confidence written by either tool is a deferred delete. The
    /// input schema is forwarded to a model whole; the parameter says so.
    /// The behaviour half is
    /// `a_confidence_at_or_below_the_consolidation_floor_is_deleted_by_the_next_pass`.
    #[test]
    fn low_confidence_parameters_name_the_consolidation_floor() {
        let tools = CuratorServer::tool_router().list_all();
        for name in ["memory_update", "memory_resolve_contradiction"] {
            let tool = tools
                .iter()
                .find(|tool| tool.name == name)
                .unwrap_or_else(|| panic!("tool {name} present"));
            let schema = serde_json::to_value(&tool.input_schema).expect("schema serializes");
            let text = schema["properties"]["new_confidence"]["description"]
                .as_str()
                .unwrap_or_else(|| panic!("{name}: new_confidence is described"));
            assert!(
                text.contains("consolidation floor") && text.contains("default 0.3"),
                "{name}: new_confidence must name the floor: {text:?}"
            );
        }
    }

    /// The retired escalation/advice tool family must not reappear — the
    /// algedonic-review board replaced it (2026-09-27). Split from the
    /// count pin so a re-introduction fails with its own name.
    #[test]
    fn retired_escalation_tools_stay_unregistered() {
        let tools = CuratorServer::tool_router().list_all();
        for removed in [
            "curator_escalations",
            "curator_escalation_resolve",
            "curator_escalation_dismiss",
            "curator_escalation_dismiss_by_pattern",
            "curator_advice_mark_applied",
            "curator_advice_reviews",
        ] {
            assert!(
                !tools.iter().any(|tool| tool.name == removed),
                "removed tool {removed} must not be registered"
            );
        }
    }
}
