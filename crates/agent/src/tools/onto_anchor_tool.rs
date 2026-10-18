use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v2 as acp;
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
/// term-resolution form — the same ladder corpus chunk tagging and hMem
/// content tagging walk per term (a metric with a real published term resolves
/// at rung 1; a term no domain source publishes resolves at the SUMO
/// rung):
///
/// 1. **Domain supplement** — pinned published sources (the RDF vocabularies,
///    W3C RDF Data Cube, OMC, PKO/P-Plan/PROV, SEPIO,
///    GOLEM/CIDOC-CRM/LRMoo, ML-Schema, RDF/RDFS, DPV, CSO).
///    Never force a term into an ontology that has no place for it.
/// 2. **Derived concepts** — recorded compositions over anchored
///    constituents, each carrying its identity and its authority
///    citation (`derived::DERIVED_CONCEPTS`). This is where operator
///    rulings become durable anchors: "net margin" resolves here with its
///    identity (net income / revenue, post-interest) and its authority
///    (operator ruling 2026-09-10).
/// 3. **Upper ontology** — the full SUMO distribution (every term of the
///    pinned release): formal categorization when no domain or derived
///    concept fits.
/// 4. **General vocabulary** — the full schema.org release, after SUMO so a
///    formal category is preferred.
/// 5. **State-axis senses** — published Dublin Core/BIBO/CiTO terms after
///    schema.org, so artifact typing does not outrank formal categories.
/// 6. **Interrogative ground** — the 5W1H core: the guaranteed final rung.
///
/// A published resolution carries the ontology's own `definition` when
/// supplied and its `source` file. Every other sense found on any rung is listed in
/// `alternatives` — the same word often names different concepts in
/// different vocabularies, and the choice stays visible.
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
    /// (e.g. "pko:Procedure").
    #[serde(deserialize_with = "deserialize_term_field")]
    term: String,
    /// Optional relation question. Omit for the original resolution-only result.
    #[serde(default, deserialize_with = "deserialize_relation_query_field")]
    relation_query: Option<RelationQuery>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RelationQuery {
    /// Destination term for a shortest directed path; omit to inspect outgoing edges.
    #[serde(default)]
    to: Option<String>,
    /// Bounded hop count (1–4); defaults to 2 for a path, 1 for neighbors.
    #[serde(default)]
    max_hops: Option<u8>,
}

fn deserialize_term_field<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    super::deserialize_teaching_field(
        deserializer,
        "term",
        "a domain term string like \"net margin\" or \"pko:Procedure\"",
        false,
    )
}

fn deserialize_relation_query_field<'de, D>(
    deserializer: D,
) -> Result<Option<RelationQuery>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    super::deserialize_teaching_field(
        deserializer,
        "relation_query",
        "an optional object like {\"to\": \"net margin\", \"max_hops\": 2} (stringified JSON or null also accepted)",
        true,
    )
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
                OntoAnchorToolOutput::from(TermResolution::core(
                    "",
                    format!("failed to receive input: {e}"),
                ))
            })?;
            Ok(resolve_onto_anchor(&input))
        })
    }
}

/// The tool's resolution and optional graph traversal, shared with delegated
/// `host/onto_anchor` dispatch.
pub fn resolve_onto_anchor(input: &OntoAnchorToolInput) -> OntoAnchorToolOutput {
    let traversal = input.relation_query.as_ref().map(|query| {
        let max_hops = query
            .max_hops
            .unwrap_or(if query.to.is_some() { 2 } else { 1 });
        graph().traverse(&input.term, query.to.as_deref(), max_hops)
    });
    OntoAnchorToolOutput {
        resolution: resolve_term(&input.term),
        traversal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_bridge_ontology::sepio;

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
        // The authority is the pinned source file that states the relation.
        assert!(
            json["traversal"]["edges"][0]["authority"]
                .as_str()
                .is_some_and(|source| source.contains("schemaorg-all-https-properties.csv")),
            "{json}"
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
    /// the widget's media tiles anchor on OMC.
    #[test]
    fn domain_terms_resolve_on_rung_one() {
        for (term, expected_uri) in [
            // `assertion` resolves to SEPIO's Assertion concept, the state-axis
            // type for extracted assertion h_mems. Bound to the constant rather
            // than a literal so a constant edit cannot silently drift the pin
            // (the 2026-10-18 FIBO removal landed `sepio:0000030` here — a
            // concept the resolver never produces).
            ("assertion", sepio::ASSERTION),
            ("Provenance", "omc:Provenance"),
            ("omc:Provenance", "omc:Provenance"),
        ] {
            let resolved = resolve_term(term);
            assert_eq!(resolved.tier, "domain_supplement", "{term}: {resolved:?}");
            assert_eq!(resolved.concept, expected_uri, "{term}: {resolved:?}");
        }
        assert_eq!(resolve_term("assertion").namespace, "SEPIO");
        assert_eq!(resolve_term("Provenance").namespace, "OMC");
    }

    /// expect: [P1] Rung 2 — the contested term of the 2026-09-10 session
    /// resolves as a derived concept carrying its recorded identity and
    /// authority. No published vocabulary carries ratio terms (verified
    /// 2026-08-29), so the derived registry — where the operator ruling is
    /// recorded — is the term's home. Never a void, never a private
    /// definition.
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
    /// on SUMO, exactly as the widget anchors unanchored metrics on
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
    /// resolution terminates on a real anchor — tier is one of the six
    /// rungs and the concept is never empty. Nothing is ever untagged.
    #[test]
    fn the_ladder_always_terminates_on_a_real_anchor() {
        for term in [
            "corporation",
            "net margin",
            "quantity",
            "academic article",
            "zephyr coefficient",
            "",
            "  ",
        ] {
            let resolved = resolve_term(term);
            assert!(
                matches!(
                    resolved.tier.as_str(),
                    "domain_supplement"
                        | "derived"
                        | "upper"
                        | "general_vocabulary"
                        | "state_axis"
                        | "core"
                ),
                "{term}: unrecognized tier {resolved:?}"
            );
            assert!(
                !resolved.concept.is_empty(),
                "{term}: the walk must terminate on a real anchor"
            );
        }
    }

    #[test]
    fn relation_query_accepts_stringified_json_and_rejects_unknown_fields_with_teaching() {
        let stringified: OntoAnchorToolInput = serde_json::from_value(serde_json::json!({
            "term": "sustainable growth rate",
            "relation_query": "{\"to\": \"net margin\", \"max_hops\": 2}"
        }))
        .expect("stringified relation_query must parse");
        let rq = stringified.relation_query.expect("relation_query present");
        assert_eq!(rq.to.as_deref(), Some("net margin"));
        assert_eq!(rq.max_hops, Some(2));

        let unknown_field = serde_json::from_value::<OntoAnchorToolInput>(serde_json::json!({
            "term": "sustainable growth rate",
            "relation_query": {"target": "net margin"}
        }))
        .expect_err("unknown field in relation_query must not silently degrade to neighbor query");
        let msg = unknown_field.to_string();
        assert!(
            msg.contains("relation_query: received an object")
                && msg.contains("unknown field `target`"),
            "got: {msg}"
        );

        let non_string_term = serde_json::from_value::<OntoAnchorToolInput>(serde_json::json!({
            "term": 99
        }))
        .expect_err("non-string term must fail with teaching error");
        assert!(
            non_string_term
                .to_string()
                .contains("term: received number 99"),
            "got: {non_string_term}"
        );
    }
}
