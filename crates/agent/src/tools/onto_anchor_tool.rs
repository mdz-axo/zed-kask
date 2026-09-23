use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, Task};
use hkask_bridge_ontology::{
    ontology_graph::{TraversalResult, graph},
    term_resolution::{TermResolution, resolve_term},
};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ui::SharedString;

/// Anchor a domain term on a published ontology concept by walking the
/// fallback ladder (P8.3, `hkask-bridge-ontology` root docs) in
/// term-resolution form — the same ladder the portfolio widget walks
/// per metric (`hkask-portfolio-widget/src/view.rs`: IRR has a real FIBO
/// term, rung 1; a metric with no FIBO term is a `sumo:Quantity`,
/// rung 3):
///
/// 1. **Domain supplement** — the fixture-pinned registries (FIBO, PKO,
///    SEPIO, GOLEM, SDMX, ML-Schema, OMC, schema.org, RDF). Never force a
///    term into an ontology that has no place for it in its graph.
/// 2. **Derived concepts** — recorded compositions over anchored
///    constituents, each carrying its identity and its authority
///    citation (`derived::DERIVED_CONCEPTS`). This is where operator
///    rulings become durable anchors: "net margin" resolves here with its
///    identity (net income / revenue, post-interest) and its authority
///    (operator ruling 2026-09-10).
/// 3. **Upper ontology** — SUMO: formal categorization (Entity, Process,
///    Quantity, Proposition) when no domain or derived concept fits.
/// 4. **Interrogative ground** — the 5W1H core: the guaranteed final rung.
///
/// The invariant (`axis.rs` P8.3): **nothing is ever untagged.** The walk
/// always terminates on a real anchor; there is no "unanchored" verdict. A
/// term that lands on the core rung carries the ruling path — the anchor
/// is real but coarse, and an operator ruling (recorded in the derived
/// registry) improves it.
///
/// Matching is exact on the full prefixed URI, then case/separator-
/// insensitive on the term name — never fuzzy. A false anchor is worse
/// than a coarse one.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct OntoAnchorToolInput {
    /// The domain term to anchor (e.g. "net margin", "corporation",
    /// "market capitalization") or a full prefixed URI
    /// (e.g. "fibo-be-le-cb:Corporation").
    term: String,
    /// Optional relation question. Omit for the original resolution-only result.
    #[serde(default)]
    relation_query: Option<RelationQuery>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RelationQuery {
    /// Destination term for a shortest directed path; omit to inspect outgoing edges.
    #[serde(default)]
    to: Option<String>,
    /// Bounded hop count (1–4); defaults to 2 for a path, 1 for neighbors.
    #[serde(default)]
    max_hops: Option<u8>,
}

/// Existing resolution JSON stays unchanged unless a relation query is made.
#[derive(Debug, Serialize, Deserialize)]
pub struct OntoAnchorToolOutput {
    #[serde(flatten)]
    resolution: TermResolution,
    #[serde(skip_serializing_if = "Option::is_none")]
    traversal: Option<TraversalResult>,
}

impl std::ops::Deref for OntoAnchorToolOutput {
    type Target = TermResolution;

    fn deref(&self) -> &Self::Target {
        &self.resolution
    }
}

impl From<TermResolution> for OntoAnchorToolOutput {
    fn from(value: TermResolution) -> Self {
        Self {
            resolution: value,
            traversal: None,
        }
    }
}

impl From<OntoAnchorToolOutput> for LanguageModelToolResultContent {
    fn from(value: OntoAnchorToolOutput) -> Self {
        serde_json::to_string_pretty(&value)
            .unwrap_or_else(|_| "null".into())
            .into()
    }
}

pub struct OntoAnchorTool;

impl AgentTool for OntoAnchorTool {
    type Input = OntoAnchorToolInput;
    type Output = OntoAnchorToolOutput;

    const NAME: &'static str = "onto_anchor";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => {
                let term = input.term.chars().take(80).collect::<String>();
                format!("Anchoring: {term}").into()
            }
            Err(_) => "Ontology Anchor".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |_cx| {
            let input = input.recv().await.map_err(|e| {
                OntoAnchorToolOutput::from(TermResolution {
                    tier: "core".to_string(),
                    term: String::new(),
                    namespace: "core".to_string(),
                    concept: "5w1h_core".to_string(),
                    identity: None,
                    authority: None,
                    note: Some(format!("failed to receive input: {e}")),
                })
            })?;
            let traversal = input.relation_query.map(|query| {
                let max_hops = query
                    .max_hops
                    .unwrap_or(if query.to.is_some() { 2 } else { 1 });
                graph().traverse(&input.term, query.to.as_deref(), max_hops)
            });
            Ok(OntoAnchorToolOutput {
                resolution: resolve_term(&input.term),
                traversal,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_round_trips_through_owned_json() -> Result<()> {
        for term in [
            "corporation",
            "net margin",
            "quantity",
            "zephyr coefficient",
        ] {
            let expected = serde_json::to_value(resolve_term(term))?;
            let restored: OntoAnchorToolOutput = serde_json::from_value(expected.clone())?;
            assert_eq!(serde_json::to_value(restored)?, expected, "{term}");
        }
        Ok(())
    }

    #[gpui::test]
    async fn agent_receives_a_provenance_backed_path_and_an_honest_absence(
        cx: &mut gpui::TestAppContext,
    ) {
        let (event_stream, _event_rx) = ToolCallEventStream::test();
        let found = cx
            .update(|cx| {
                Arc::new(OntoAnchorTool).run(
                    ToolInput::ready(serde_json::json!({
                        "term": "sustainable growth rate",
                        "relation_query": {"to": "net margin", "max_hops": 2}
                    })),
                    event_stream,
                    cx,
                )
            })
            .await
            .expect("valid relation query");
        let json = serde_json::to_value(found).expect("serialize path");
        assert_eq!(json["concept"], "sustainable_growth_rate");
        assert_eq!(json["traversal"]["status"], "path_found");
        assert_eq!(json["traversal"]["edges"][0]["to"], "return_on_equity");
        assert_eq!(json["traversal"]["edges"][1]["to"], "net_margin");
        assert!(
            json["traversal"]["edges"][1]["authority"]
                .as_str()
                .is_some_and(|source| source.contains("operator ruling"))
        );

        let (event_stream, _event_rx) = ToolCallEventStream::test();
        let absent = cx
            .update(|cx| {
                Arc::new(OntoAnchorTool).run(
                    ToolInput::ready(serde_json::json!({
                        "term": "sustainable growth rate",
                        "relation_query": {"to": "net margin", "max_hops": 1}
                    })),
                    event_stream,
                    cx,
                )
            })
            .await
            .expect("valid shallow query");
        let json = serde_json::to_value(absent).expect("serialize absence");
        assert_eq!(json["traversal"]["status"], "no_supported_path");
        assert_eq!(json["traversal"]["edges"], serde_json::json!([]));
    }

    #[gpui::test]
    async fn published_relation_and_coarse_anchor_are_not_conflated(cx: &mut gpui::TestAppContext) {
        let (event_stream, _event_rx) = ToolCallEventStream::test();
        let found = cx
            .update(|cx| {
                Arc::new(OntoAnchorTool).run(
                    ToolInput::ready(serde_json::json!({
                        "term": "schema:hasPart",
                        "relation_query": {"to": "schema:isPartOf"}
                    })),
                    event_stream,
                    cx,
                )
            })
            .await
            .expect("valid published query");
        let json = serde_json::to_value(found).expect("serialize published path");
        assert_eq!(json["traversal"]["status"], "path_found");
        assert_eq!(json["traversal"]["edges"][0]["relation"], "inverse_of");
        assert_eq!(
            json["traversal"]["edges"][0]["authority"],
            "https://schema.org/hasPart"
        );

        let (event_stream, _event_rx) = ToolCallEventStream::test();
        let coarse = cx
            .update(|cx| {
                Arc::new(OntoAnchorTool).run(
                    ToolInput::ready(serde_json::json!({
                        "term": "zephyr coefficient",
                        "relation_query": {"to": "net margin"}
                    })),
                    event_stream,
                    cx,
                )
            })
            .await
            .expect("coarse query has a surfaced result");
        let json = serde_json::to_value(coarse).expect("serialize coarse result");
        assert_eq!(json["traversal"]["status"], "coarse_anchor");
        assert_eq!(json["traversal"]["edges"], serde_json::json!([]));
    }

    #[gpui::test]
    async fn invalid_input_returns_an_owned_error(cx: &mut gpui::TestAppContext) {
        let (event_stream, _event_rx) = ToolCallEventStream::test();
        let result = cx
            .update(|cx| {
                Arc::new(OntoAnchorTool).run(
                    ToolInput::invalid_json("invalid tool JSON".to_string()),
                    event_stream,
                    cx,
                )
            })
            .await;
        let error = result.expect_err("invalid JSON must be an error");
        let json = serde_json::to_value(error).expect("serialize error");
        let restored: OntoAnchorToolOutput =
            serde_json::from_value(json).expect("deserialize owned error");
        let note = restored
            .note
            .as_deref()
            .expect("input failure must be surfaced");
        assert_eq!(note, "failed to receive input: invalid tool JSON");
    }

    /// expect: [P5] Rung 1 — a published domain term resolves to its
    /// vocabulary and URI, across separator and case variants, exactly as
    /// the widget's IRR tile anchors on FIBO.
    #[test]
    fn domain_terms_resolve_on_rung_one() {
        for (term, expected_uri) in [
            ("corporation", "fibo-be-le-cb:Corporation"),
            (
                "MarketCapitalization",
                "fibo-ind-mkt-bas:MarketCapitalization",
            ),
            (
                "market capitalization",
                "fibo-ind-mkt-bas:MarketCapitalization",
            ),
            ("fibo-be-le-cb:Corporation", "fibo-be-le-cb:Corporation"),
        ] {
            let resolved = resolve_term(term);
            assert_eq!(resolved.tier, "domain_supplement", "{term}: {resolved:?}");
            assert_eq!(resolved.namespace, "FIBO", "{term}: {resolved:?}");
            assert_eq!(resolved.concept, expected_uri, "{term}: {resolved:?}");
        }
    }

    /// expect: [P1] Rung 2 — the contested term of the 2026-09-10 session
    /// resolves as a derived concept carrying its recorded identity and
    /// authority. FIBO publishes no ratio terms (verified 2026-08-29), so
    /// the derived registry — where the operator ruling is recorded — is
    /// the term's home. Never a void, never a private definition.
    #[test]
    fn net_margin_resolves_on_the_derived_rung() {
        let resolved = resolve_term("net margin");
        assert_eq!(resolved.tier, "derived", "{resolved:?}");
        assert_eq!(resolved.concept, "net_margin");
        assert_eq!(resolved.identity.as_deref(), Some("net income / revenue"));
        let authority = resolved.authority.expect("authority cited");
        assert!(
            authority.contains("2026-09-10"),
            "the ruling is the authority record: {authority}"
        );
    }

    /// expect: [P5] Rung 3 — a term the upper ontology publishes resolves
    /// on SUMO, exactly as the widget anchors FIBO-less metrics on
    /// `sumo:Quantity`.
    #[test]
    fn upper_ontology_terms_resolve_on_rung_three() {
        let resolved = resolve_term("quantity");
        assert_eq!(resolved.tier, "upper", "{resolved:?}");
        assert_eq!(resolved.namespace, "SUMO");
        assert_eq!(resolved.concept, "sumo:Quantity");
    }

    /// expect: [P1] Rung 4 — a term no vocabulary publishes still
    /// terminates on a real anchor (the 5W1H core) with the ruling path.
    /// The P8.3 invariant: nothing is ever untagged — there is no
    /// "unanchored" verdict, only a coarse anchor pending its ruling.
    #[test]
    fn unmatched_terms_terminate_on_the_core_rung() {
        let resolved = resolve_term("zephyr coefficient");
        assert_eq!(resolved.tier, "core", "{resolved:?}");
        assert_eq!(resolved.concept, "5w1h_core");
        let note = resolved.note.expect("ruling path on the core rung");
        assert!(
            note.contains("Request a ruling"),
            "the coarse anchor routes to the operator: {note}"
        );
        assert!(
            note.contains("Never assign"),
            "the core rung names the fabrication boundary: {note}"
        );
    }

    /// expect: [P1] The ladder invariant, walked exhaustively: every
    /// resolution terminates on a real anchor — tier is one of the four
    /// rungs and the concept is never empty. Nothing is ever untagged.
    #[test]
    fn the_ladder_always_terminates_on_a_real_anchor() {
        for term in [
            "corporation",
            "net margin",
            "quantity",
            "zephyr coefficient",
            "",
            "  ",
        ] {
            let resolved = resolve_term(term);
            assert!(
                matches!(
                    resolved.tier.as_str(),
                    "domain_supplement" | "derived" | "upper" | "core"
                ),
                "{term}: unrecognized tier {resolved:?}"
            );
            assert!(
                !resolved.concept.is_empty(),
                "{term}: the walk must terminate on a real anchor"
            );
        }
    }
}
