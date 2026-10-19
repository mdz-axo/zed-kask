//! Dual-axis ontology dispatch for hkask-mcp-corpus (P5.4, operator ruling
//! B.2 2026-10-18: every tool RESPONSE carries both axes).
//!
//! Every artifact in hKask has both a state identity (what it is) and a
//! process identity (how it came to be). This module is the corpus
//! server's mapping onto those two axes:
//!
//! - **Process axis (PKO)** — the corpus pipeline is a procedure whose
//!   steps execute: converting, chunking, embedding, tagging, deduping,
//!   consolidating and QA-generating are pipeline steps executing;
//!   querying, listing, inventorying, and centroid computation observe
//!   the executions that produced the stored index — readings over
//!   prior procedure executions.
//! - **State axis (DC+BIBO)** — write and cache responses are datasets
//!   (stored or modified measurements); query/inventory/status reads
//!   are compiled readings over many stored records — reports.
//!
//! The vocabulary itself lives in the shared `hkask-bridge-ontology`
//! crate; this module holds only the corpus-specific tool→concept table.
//! That is the server's business, not the ontology's — the architectural
//! invariant (user directive 2026-08-05) that no ontology vocabulary is
//! defined inside an MCP server. The constants are re-exported so call
//! sites and tests reference the single fixture-guarded source.

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::{PROCEDURE_EXECUTION, STEP_EXECUTION};

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// The process-axis (PKO) concept a corpus tool's response occupies.
///
/// Returns `None` only for a name that is not a registered tool — every
/// registered tool has an explicit arm, pinned by
/// `dual_axis_covers_all_registered_tools` (below) and the tool-surface
/// pin over the live router.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        // Document conversion and chunking are pipeline steps executing:
        // converting a document to text, chunking it, caching it, and
        // clearing the index each execute one step of the ingestion
        // procedure.
        "corpus_convert" | "corpus_chunk" | "corpus_cache" | "corpus_cache_work"
        | "corpus_clear_index" => Some(STEP_EXECUTION),
        // OCR and complexity triage are steps too: forcing the OCR path
        // executes the recognition step; is_complex verifies whether the
        // triage step is needed.
        "corpus_ocr" | "corpus_is_complex" => Some(STEP_EXECUTION),
        // Embedding, tagging, dedup, consolidation, and centroid
        // computation modify or derive from stored chunks — each is one
        // step of the corpus-building procedure executing.
        "corpus_embed"
        | "corpus_tag_chunks"
        | "corpus_dedup_chunks"
        | "corpus_consolidate_chunks"
        | "corpus_centroid" => Some(STEP_EXECUTION),
        // QA generation is a multi-step composition: building prompts,
        // generating, grounding, and ingesting are each one step of the
        // dataset-generation procedure.
        "corpus_build_prompts"
        | "corpus_generate_qa_batch"
        | "corpus_ground_generated_qa"
        | "corpus_ingest_qa"
        | "corpus_purge_qa" => Some(STEP_EXECUTION),
        // Sealed-run machinery: building representations and exporting a
        // bundle execute the sealing procedure's steps; the embedding
        // inventory reconciles a shard against a durable database.
        "corpus_build_chunk_representations"
        | "corpus_export_bundle"
        | "corpus_embedding_inventory" => Some(STEP_EXECUTION),
        // Assertion extraction and author discovery observe or produce
        // derived knowledge about sources.
        "corpus_extract_assertions" | "corpus_discover" => Some(STEP_EXECUTION),
        // Query, compose, and rewrite observe the executions that
        // produced the stored index — compiled readings over the
        // procedure's prior executions (KNN recall, style-composition
        // retrieval).
        "corpus_query" | "corpus_compose" | "corpus_rewrite" => Some(PROCEDURE_EXECUTION),
        _ => None,
    }
}

/// The state-axis (DC+BIBO) concept a corpus tool's response occupies.
///
/// Write and cache responses are datasets (a stored or modified
/// measurement); query/inventory/status reads are compiled readings
/// over many stored records — reports.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        // Tools that store, modify, cache, or durably produce rows:
        // datasets.
        "corpus_convert"
        | "corpus_chunk"
        | "corpus_cache"
        | "corpus_cache_work"
        | "corpus_clear_index"
        | "corpus_ocr"
        | "corpus_embed"
        | "corpus_tag_chunks"
        | "corpus_dedup_chunks"
        | "corpus_consolidate_chunks"
        | "corpus_centroid"
        | "corpus_build_prompts"
        | "corpus_generate_qa_batch"
        | "corpus_ground_generated_qa"
        | "corpus_ingest_qa"
        | "corpus_purge_qa"
        | "corpus_build_chunk_representations"
        | "corpus_export_bundle"
        | "corpus_extract_assertions" => Some(DATASET),
        // is_complex is a triage verdict over a document — a report.
        "corpus_is_complex" => Some(REPORT),
        // Query, compose, rewrite, inventory, discover: compiled readings
        // over stored records — reports.
        "corpus_query"
        | "corpus_compose"
        | "corpus_rewrite"
        | "corpus_embedding_inventory"
        | "corpus_discover" => Some(REPORT),
        _ => None,
    }
}

/// Both axes for a registered tool, or `None` when the name is not one.
///
/// P5.4's dual-axis core: a caller never gets one axis without the other,
/// so a partial mapping cannot silently satisfy the contract.
pub(crate) fn tool_to_dual_axis(tool_name: &str) -> Option<(PkoConcept, DcConcept)> {
    match (tool_to_pko(tool_name), tool_to_dc(tool_name)) {
        (Some(process), Some(state)) => Some((process, state)),
        _ => None,
    }
}

/// Stamp the dual-axis identity onto a tool response.
///
/// This is the production consumer of the mapping: wired as the central
/// `ToolContext::annotate_response` hook via the `mcp_server!` macro's
/// annotate override, so every corpus tool response carries the process
/// (PKO) and state (DC+BIBO) concept its artifact occupies without a
/// per-site wrap at each of the 25 tool returns. The annotation is
/// additive — the existing body is untouched, so no caller contract
/// changes.
///
/// A name with no mapping returns the body unchanged rather than
/// inventing an axis: `dual_axis_covers_all_registered_tools` is what
/// guarantees the registered surface is total, and a silent default
/// would defeat it.
pub(crate) fn annotate(tool_name: &str, mut body: serde_json::Value) -> serde_json::Value {
    let Some((process, state)) = tool_to_dual_axis(tool_name) else {
        return body;
    };
    if let Some(object) = body.as_object_mut() {
        object.insert(
            "ontology".to_string(),
            serde_json::json!({
                "pko_procedure": process,
                "dc_type": state,
            }),
        );
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    // The generated canonical tool-name list — included from OUT_DIR, so
    // the totality test derives its expectations from the live tool
    // surface (build.rs scan), never a hardcoded list.
    include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));

    /// P5.4: "Every MCP server uses BOTH axes." Every registered tool must
    /// carry a process AND a state identity — a tool added without an
    /// explicit arm fails here rather than silently losing its axes.
    #[test]
    fn dual_axis_covers_all_registered_tools() {
        for name in TOOL_NAMES {
            let mapped = tool_to_dual_axis(name);
            assert!(
                mapped.is_some(),
                "no dual-axis mapping for registered tool '{name}'; add an explicit arm \
                 to tool_to_pko and tool_to_dc in src/ontology.rs"
            );
        }
        // The reverse direction: every name the mapping claims must be a
        // registered tool. Coverage alone let a phantom name sit in the
        // swarm mapping undetected (live 2026-10-18: `swarm_delete_local`,
        // a typo, matched no call site and still passed coverage).
        let ontology_src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ontology.rs"),
        )
        .expect("src/ontology.rs must be readable");
        let mut mapped: Vec<&str> = Vec::new();
        for chunk in ontology_src.split('"').skip(1).step_by(2) {
            if chunk.starts_with("corpus_")
                && chunk
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
            {
                mapped.push(chunk);
            }
        }
        mapped.sort_unstable();
        mapped.dedup();
        assert!(
            !mapped.is_empty(),
            "the reverse scan found no mapping names — the instrument is broken"
        );
        for name in &mapped {
            assert!(
                TOOL_NAMES.contains(name),
                "mapping names '{name}', which is not a registered tool — a stale or \
                 misspelled arm in src/ontology.rs"
            );
        }
    }

    /// The mapping is defined only over the registered surface: an unknown
    /// name returns None on both axes rather than a default concept.
    #[test]
    fn unknown_tool_has_no_axes() {
        assert!(tool_to_pko("not_a_tool").is_none());
        assert!(tool_to_dc("not_a_tool").is_none());
        assert!(tool_to_dual_axis("not_a_tool").is_none());
    }

    /// The axes must discriminate: a pipeline step and a compiled reading
    /// are different state concepts, and a write step and a read-observe
    /// are different process concepts. A table that collapsed to one
    /// value per axis would satisfy coverage while carrying no
    /// information.
    #[test]
    fn axes_discriminate_tool_families() {
        assert_eq!(tool_to_dc("corpus_query"), Some(REPORT));
        assert_eq!(tool_to_dc("corpus_embed"), Some(DATASET));

        assert_eq!(tool_to_pko("corpus_chunk"), Some(STEP_EXECUTION));
        assert_eq!(tool_to_pko("corpus_query"), Some(PROCEDURE_EXECUTION));
    }

    /// The stamp is additive: the body's own fields survive, and both
    /// axis concepts appear under `ontology`.
    #[test]
    fn annotate_stamps_both_axes_without_disturbing_the_body() {
        let body = serde_json::json!({"count": 3, "results": []});
        let stamped = annotate("corpus_query", body.clone());
        assert_eq!(stamped["count"], serde_json::json!(3), "{stamped:?}");
        assert_eq!(
            stamped["ontology"]["pko_procedure"],
            serde_json::json!(PROCEDURE_EXECUTION)
        );
        assert_eq!(stamped["ontology"]["dc_type"], serde_json::json!(REPORT));
        // An unmapped name returns the body unchanged — never an
        // invented axis.
        let untouched = annotate("not_a_registered_tool", body.clone());
        assert_eq!(untouched, body);
        // A non-object body (string results) is returned unchanged too.
        let stringy = annotate("corpus_query", serde_json::json!("ok"));
        assert_eq!(stringy, serde_json::json!("ok"));
    }

    /// End-to-end: a real tool invocation through `execute_tool` carries
    /// the dual-axis stamp on its response — the central hook works, not
    /// just the mapping table.
    #[tokio::test]
    async fn a_live_tool_response_carries_the_dual_axis_stamp() {
        use crate::CorpusServer;
        use hkask_types::InferencePort;
        use rmcp::handler::server::wrapper::Parameters;
        use std::pin::Pin;
        use std::sync::Arc;

        struct NoopPort;
        impl InferencePort for NoopPort {
            fn generate(
                &self,
                _: &str,
                _: &hkask_types::template::LLMParameters,
                _: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<
                                hkask_types::InferenceResult,
                                hkask_types::InferenceError,
                            >,
                        > + Send
                        + '_,
                >,
            > {
                Box::pin(async {
                    Err(hkask_types::InferenceError::Connection(
                        "noop inference port — not configured for this test".into(),
                    ))
                })
            }
            fn embed_with_dimensions<'a>(
                &'a self,
                _model: &str,
                _texts: &[String],
                _dimensions: Option<u32>,
            ) -> hkask_types::EmbedWithIdentityFuture<'a> {
                Box::pin(async {
                    Err(hkask_types::EmbeddingGenerationError::Connection(
                        "noop".to_string(),
                    ))
                })
            }
        }

        crate::helpers::seed_test_passphrase();
        let port: Arc<dyn InferencePort> = Arc::new(NoopPort);
        let llm_ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(Arc::clone(&port)));
        let server = CorpusServer::new(
            hkask_types::WebID::new(),
            None,
            port,
            Arc::default(),
            llm_ocr,
        );
        let response = server
            .corpus_clear_index(Parameters(crate::tools::storage::ClearIndexRequest {}))
            .await
            .expect("corpus_clear_index succeeds on a fresh server");
        let value: serde_json::Value = serde_json::from_str(&response)
            .unwrap_or_else(|error| panic!("response is JSON: {error}: {response}"));
        let content = value
            .get("content")
            .cloned()
            .unwrap_or_else(|| panic!("envelope carries content: {response}"));
        assert_eq!(
            content["ontology"]["pko_procedure"],
            serde_json::json!(STEP_EXECUTION),
            "the live corpus_clear_index response carries the process axis: {content}"
        );
        assert_eq!(
            content["ontology"]["dc_type"],
            serde_json::json!(DATASET),
            "the live corpus_clear_index response carries the state axis: {content}"
        );
    }
}
