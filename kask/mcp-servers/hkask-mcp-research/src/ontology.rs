//! Dual-axis ontology dispatch for hkask-mcp-research (P5.4, operator
//! ruling B.2 2026-10-18: every tool RESPONSE carries both axes).
//!
//! Every artifact in hKask has both a state identity (what it is) and a
//! process identity (how it came to be). This module is the research
//! server's mapping onto those two axes, wired as the central
//! `ToolContext::annotate_response` hook via the `mcp_server!` macro's
//! annotate override — one point stamps every tool response at the
//! `execute_tool` boundary. No per-site wraps.
//!
//! - **Process axis (PKO)** — research is an evidence-gathering
//!   procedure: begin/get/finish/annotate runs are the run lifecycle's
//!   steps executing (beginning a run specifies the procedure; finishing
//!   verifies it; annotating records an observation on a source);
//!   searches, extracts, and reads observe external providers and the
//!   ledger — readings over prior executions; cite_sources is a
//!   deterministic formatting step; resolve_paper resolves a
//!   bibliographic identity to a typed identity; evaluate_evidence
//!   adjudicates retrieved evidence; the RSS tools manage feed
//!   subscriptions and read feed entries; the synthetic-feed tools
//!   create, list, fetch, and delete non-feed extractions.
//! - **State axis (DC+BIBO)** — write responses are datasets (a stored
//!   or modified measurement — a ledger row, a subscription, a stored
//!   extraction); read responses are compiled readings over stored
//!   records — reports.
//!
//! The vocabulary itself lives in the shared `hkask-bridge-ontology`
//! crate; this module holds only the research-specific tool→concept
//! table. That is the server's business, not the ontology's — the
//! architectural invariant (user directive 2026-08-05) that no ontology
//! vocabulary is defined inside an MCP server.

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::{PROCEDURE_EXECUTION, STEP_EXECUTION};

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// The process-axis (PKO) concept a research tool's response occupies.
///
/// Returns `None` only for a name that is not a registered tool — every
/// registered tool has an explicit arm, pinned by
/// `dual_axis_covers_all_registered_tools` (below) and the tool-surface
/// pin over the live router.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        // Research-run lifecycle: beginning a run specifies the evidence
        // procedure; finishing records its terminal state; annotating
        // records a verification observation on a source; reading a run
        // observes the ledger's prior executions.
        "begin_research_run" => Some(STEP_EXECUTION),
        "finish_research_run" => Some(STEP_EXECUTION),
        "get_research_run" => Some(PROCEDURE_EXECUTION),
        "annotate_research_run" => Some(STEP_EXECUTION),
        // Web search/extract/browse/find-similar/ping: searches and
        // extractions observe external providers — steps executing that
        // read; browse drives an interactive session; find-similar reads
        // provider recommendations; ping verifies liveness.
        "web_search" | "web_extract" => Some(STEP_EXECUTION),
        "web_browse" => Some(STEP_EXECUTION),
        "web_find_similar" => Some(PROCEDURE_EXECUTION),
        "web_ping" => Some(PROCEDURE_EXECUTION),
        // Bibliographic resolution: resolving a paper reference to a
        // typed identity — a verification step.
        "resolve_paper" => Some(STEP_EXECUTION),
        // Evidence adjudication: evaluating retrieved evidence against a
        // research question — a compiled reading over the run's sources.
        "evaluate_evidence" => Some(PROCEDURE_EXECUTION),
        // Citation generation: deterministic formatting of retrieved
        // sources — a step.
        "cite_sources" => Some(STEP_EXECUTION),
        // RSS subscription management: subscribe/import mutate the
        // subscription store; list/read-count/search observe entries.
        "rss_subscribe" | "rss_import_opml" => Some(STEP_EXECUTION),
        "rss_list_subscriptions" | "rss_get_unread_count" | "rss_search" => {
            Some(PROCEDURE_EXECUTION)
        }
        // RSS feed operations: discover probes a site for feeds; fetch
        // syncs entries; entries read out; tag edits mutate entry state;
        // mark-all-read mutates; export renders a reading.
        "rss_discover_feeds" | "rss_fetch" => Some(STEP_EXECUTION),
        "rss_get_entries" => Some(PROCEDURE_EXECUTION),
        "rss_edit_tag" | "rss_mark_all_read" => Some(STEP_EXECUTION),
        "rss_export_opml" => Some(PROCEDURE_EXECUTION),
        "rss_unsubscribe" => Some(STEP_EXECUTION),
        // Synthetic feeds: create/list/fetch/delete non-feed-site
        // extractions — creation is a step; list observes stored
        // extractions.
        "rss_synthesize" => Some(STEP_EXECUTION),
        "rss_list_synthetic" => Some(PROCEDURE_EXECUTION),
        "rss_delete_synthetic" => Some(STEP_EXECUTION),
        _ => None,
    }
}

/// The state-axis (DC+BIBO) concept a research tool's response occupies.
///
/// Write responses are datasets (a stored or modified measurement — a
/// ledger row, a subscription, a stored extraction); read responses are
/// compiled readings over stored records — reports.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        // Reads: run manifests, searches, extractions, readings,
        // liveness, listings, entry reads, searches, exports — reports.
        "get_research_run"
        | "web_search"
        | "web_extract"
        | "web_browse"
        | "web_find_similar"
        | "web_ping"
        | "resolve_paper"
        | "evaluate_evidence"
        | "cite_sources"
        | "rss_list_subscriptions"
        | "rss_get_unread_count"
        | "rss_search"
        | "rss_get_entries"
        | "rss_export_opml"
        | "rss_list_synthetic" => Some(REPORT),
        // Writes and mutations: run lifecycle events, annotations,
        // subscriptions, syncs, tag edits, synthetic extractions —
        // datasets.
        "begin_research_run"
        | "finish_research_run"
        | "annotate_research_run"
        | "rss_subscribe"
        | "rss_import_opml"
        | "rss_discover_feeds"
        | "rss_fetch"
        | "rss_edit_tag"
        | "rss_mark_all_read"
        | "rss_unsubscribe"
        | "rss_synthesize"
        | "rss_delete_synthetic" => Some(DATASET),
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
/// annotate override, so every research tool response carries the
/// process (PKO) and state (DC+BIBO) concept its artifact occupies
/// without a per-site wrap at each of the 27 tool returns. The
/// annotation is additive — the existing body is untouched, so no
/// caller contract changes.
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
mod dual_axis_tests {
    use super::*;

    // The generated canonical tool-name list — included from OUT_DIR, so
    // the totality test derives its expectations from the live tool
    // surface (build.rs scan), never a hardcoded list.
    include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));

    /// P5.4: every registered tool must carry a process AND a state
    /// identity — a tool added without an explicit arm fails here rather
    /// than silently losing its axes.
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
        // The stamp's own JSON keys are not tool names — excluded by name,
        // not by pattern (a bare contains-'_' filter would catch them).
        const STAMP_KEYS: [&str; 3] = ["ontology", "pko_procedure", "dc_type"];
        for chunk in ontology_src.split('"').skip(1).step_by(2) {
            if !chunk.is_empty()
                && (chunk.starts_with("web_")
                    || chunk.starts_with("rss_")
                    || chunk.contains("research_run")
                    || chunk.contains("paper")
                    || chunk.contains("evidence")
                    || chunk.contains("cite"))
                && chunk
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
                && !STAMP_KEYS.contains(&chunk)
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

    /// The mapping is defined only over the registered surface: an
    /// unknown name returns None on both axes rather than a default
    /// concept.
    #[test]
    fn unknown_tool_has_no_axes() {
        assert!(tool_to_pko("not_a_tool").is_none());
        assert!(tool_to_dc("not_a_tool").is_none());
        assert!(tool_to_dual_axis("not_a_tool").is_none());
    }

    /// The axes must discriminate: a run-reading and a run-beginning are
    /// different process concepts, and a read and a write are different
    /// state concepts.
    #[test]
    fn axes_discriminate_tool_families() {
        assert_eq!(tool_to_pko("get_research_run"), Some(PROCEDURE_EXECUTION));
        assert_eq!(tool_to_pko("begin_research_run"), Some(STEP_EXECUTION));

        assert_eq!(tool_to_dc("get_research_run"), Some(REPORT));
        assert_eq!(tool_to_dc("begin_research_run"), Some(DATASET));
    }

    /// The stamp is additive: the body's own fields survive, and both
    /// axis concepts appear under `ontology`.
    #[test]
    fn annotate_stamps_both_axes_without_disturbing_the_body() {
        let body = serde_json::json!({"run_id": "run-1", "sources": []});
        let stamped = annotate("get_research_run", body.clone());
        assert_eq!(stamped["run_id"], serde_json::json!("run-1"), "{stamped:?}");
        assert_eq!(
            stamped["ontology"]["pko_procedure"],
            serde_json::json!(PROCEDURE_EXECUTION)
        );
        assert_eq!(stamped["ontology"]["dc_type"], serde_json::json!(REPORT));
        // An unmapped name returns the body unchanged — never an
        // invented axis.
        let untouched = annotate("not_a_registered_tool", body.clone());
        assert_eq!(untouched, body);
    }

    /// End-to-end: a real tool invocation through `execute_tool` carries
    /// the dual-axis stamp on its response — the central hook works, not
    /// just the mapping table. `web_ping` needs only the pool's health
    /// check (a stub returning an empty table — no network).
    #[tokio::test]
    async fn a_live_tool_response_carries_the_dual_axis_stamp() {
        use crate::research::cache::ResponseCache;
        use crate::research::providers::{CrossrefCandidate, ProviderSearchOutput, WebSearchPort};
        use crate::research::types::{
            BrowseResult, CompoundSearchResult, ExtractOptions, ExtractedContent, PaperMetadata,
            ProviderHealthEntry, ProviderRecommendation, RateLimiter, SearchQuery, SearchStrategy,
            WebError,
        };
        use async_trait::async_trait;
        use std::sync::Arc;
        use std::time::Duration;

        struct StubPort;
        #[async_trait]
        impl WebSearchPort for StubPort {
            async fn search(
                &self,
                _query: &SearchQuery,
                _strategy: SearchStrategy,
                _provider: Option<&str>,
            ) -> Result<CompoundSearchResult, WebError> {
                Err(WebError::NoProvider)
            }
            async fn find_similar(
                &self,
                _url: &str,
                _num_results: u32,
            ) -> Result<ProviderSearchOutput, WebError> {
                Err(WebError::NoProvider)
            }
            async fn extract(
                &self,
                _url: &str,
                _opts: &ExtractOptions,
            ) -> Result<ExtractedContent, WebError> {
                Err(WebError::NoProvider)
            }
            async fn browse(
                &self,
                _url: &str,
                _instruction: &str,
                _timeout: Duration,
            ) -> Result<BrowseResult, WebError> {
                Err(WebError::NoProvider)
            }
            async fn health_check(&self) -> Vec<ProviderHealthEntry> {
                Vec::new()
            }
            fn provider_fingerprint(&self) -> String {
                "stub".to_string()
            }
            fn provider_kinds(&self) -> Vec<String> {
                Vec::new()
            }
            fn score_providers(
                &self,
                _query: &str,
                _intent: Option<&str>,
            ) -> Vec<ProviderRecommendation> {
                Vec::new()
            }
            async fn resolve_paper_id(
                &self,
                _id: &crate::research::paper_id::PaperId,
            ) -> Result<Option<PaperMetadata>, WebError> {
                Ok(None)
            }
            async fn resolve_title(
                &self,
                _title: &str,
                _rows: u32,
            ) -> Result<Vec<CrossrefCandidate>, WebError> {
                Ok(Vec::new())
            }
        }

        struct NoopInference;
        impl hkask_types::InferencePort for NoopInference {
            fn generate(
                &self,
                _: &str,
                _: &hkask_types::template::LLMParameters,
                _: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> std::pin::Pin<
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
        }

        let server = crate::ResearchServer::new(
            hkask_types::WebID::new(),
            Arc::new(StubPort),
            Arc::new(ResponseCache::new(0, Duration::from_secs(1))),
            RateLimiter::new(30, 60),
            None,
            reqwest::Client::new(),
            reqwest::Client::new(),
            Arc::new(NoopInference),
            None,
            None,
        );
        let response = server
            .web_ping()
            .await
            .expect("web_ping succeeds on a stub port");
        let value: serde_json::Value = serde_json::from_str(&response)
            .unwrap_or_else(|error| panic!("response is JSON: {error}: {response}"));
        let content = value
            .get("content")
            .cloned()
            .unwrap_or_else(|| panic!("envelope carries content: {response}"));
        assert_eq!(
            content["ontology"]["pko_procedure"],
            serde_json::json!(PROCEDURE_EXECUTION),
            "the live web_ping response carries the process axis: {content}"
        );
        assert_eq!(
            content["ontology"]["dc_type"],
            serde_json::json!(REPORT),
            "the live web_ping response carries the state axis: {content}"
        );
    }
}
