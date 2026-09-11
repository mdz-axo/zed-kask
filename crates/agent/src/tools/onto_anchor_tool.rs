use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, Task};
use hkask_bridge_ontology::{
    derived, fibo, golem, ml_schema, omc, pko, rdf, schema_org, sdmx, sepio, sumo,
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OntoAnchorToolOutput {
    /// The ladder rung that terminated the walk: "domain_supplement",
    /// "derived", "upper", or "core" (the 5W1H interrogative ground).
    pub tier: &'static str,
    /// The term as given.
    pub term: String,
    /// The vocabulary that publishes the concept (FIBO, SUMO, derived, core).
    pub namespace: &'static str,
    /// The published concept URI — or the derived concept's canonical term.
    pub concept: String,
    /// The derived concept's recorded identity (derived rung only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<&'static str>,
    /// The authority citation (derived rung only): an operator ruling with
    /// its date, or a published standard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<&'static str>,
    /// The ruling path (core rung only): the anchor is real but coarse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<&'static str>,
}

impl From<OntoAnchorToolOutput> for LanguageModelToolResultContent {
    fn from(value: OntoAnchorToolOutput) -> Self {
        serde_json::to_string_pretty(&value)
            .unwrap_or_else(|_| "null".into())
            .into()
    }
}

/// Rung 1 — the domain supplements' fixture-pinned registries, in
/// `OntologyNamespace` order. Dublin Core/BIBO is deliberately absent:
/// the state axis types artifacts (server-side pattern), not terms, and
/// BIBO was deprecated for this purpose in favor of SUMO (operator
/// ruling 2026-09-10).
const DOMAIN_REGISTRIES: &[(&str, &[&str])] = &[
    ("FIBO", fibo::ALL_TERMS),
    ("PKO", pko::ALL_TERMS),
    ("SEPIO", sepio::ALL_TERMS),
    ("GOLEM", golem::ALL_TERMS),
    ("SDMX", sdmx::ALL_CONCEPTS),
    ("ML-Schema", ml_schema::ALL_CONCEPTS),
    ("OMC", omc::ALL_CONCEPTS),
    ("schema.org", schema_org::ALL_TERMS),
    ("RDF", rdf::ALL_TERMS),
];

/// Lowercase alphanumeric characters only — separators and case are
/// normalized away so "market capitalization", "MarketCapitalization" and
/// "market-capitalization" compare equal. Exact URI matches bypass this.
fn normalize(term: &str) -> String {
    term.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Walk the fallback ladder (P8.3) for a term. Always terminates on a real
/// anchor — the nothing-ever-untagged invariant.
pub(crate) fn resolve_term(term: &str) -> OntoAnchorToolOutput {
    let trimmed = term.trim();

    // Rung 1 — domain supplements.
    for (namespace, registry) in DOMAIN_REGISTRIES {
        for uri in *registry {
            let name = uri.rsplit(':').next().unwrap_or(uri);
            if trimmed == *uri || (!name.is_empty() && normalize(trimmed) == normalize(name)) {
                return OntoAnchorToolOutput {
                    tier: "domain_supplement",
                    term: trimmed.to_string(),
                    namespace,
                    concept: (*uri).to_string(),
                    identity: None,
                    authority: None,
                    note: None,
                };
            }
        }
    }

    // Rung 2 — derived concepts (recorded compositions, authority-cited).
    if let Some(concept) = derived::resolve_derived(trimmed) {
        return OntoAnchorToolOutput {
            tier: "derived",
            term: trimmed.to_string(),
            namespace: "derived",
            concept: concept.term.to_string(),
            identity: Some(concept.identity),
            authority: Some(concept.authority),
            note: None,
        };
    }

    // Rung 3 — the SUMO upper ontology.
    for uri in sumo::ALL_CONCEPTS {
        let name = uri.rsplit(':').next().unwrap_or(uri);
        if trimmed == *uri || (!name.is_empty() && normalize(trimmed) == normalize(name)) {
            return OntoAnchorToolOutput {
                tier: "upper",
                term: trimmed.to_string(),
                namespace: "SUMO",
                concept: (*uri).to_string(),
                identity: None,
                authority: None,
                note: None,
            };
        }
    }

    // Rung 4 — the 5W1H interrogative ground. The anchor is real but
    // coarse; the ruling path improves it (a ruling lands in the derived
    // registry and the term resolves there ever after).
    OntoAnchorToolOutput {
        tier: "core",
        term: trimmed.to_string(),
        namespace: "core",
        concept: "5w1h_core".to_string(),
        identity: None,
        authority: None,
        note: Some(
            "No domain, derived, or upper concept matched. Anchored on the 5W1H \
             interrogative ground — a real but coarse anchor. Request a ruling from \
             the operator to improve it: the ruling is recorded in the derived \
             registry (hkask-bridge-ontology/src/derived.rs) with its identity and \
             authority, and the term resolves there ever after. Never assign the \
             term a private definition in the meantime.",
        ),
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
            let input = input.recv().await.map_err(|e| OntoAnchorToolOutput {
                tier: "core",
                term: String::new(),
                namespace: "core",
                concept: "5w1h_core".to_string(),
                identity: None,
                authority: None,
                note: Some(&format!("failed to receive input: {e}")),
            })?;
            Ok(resolve_term(&input.term))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(resolved.identity, Some("net income / revenue"));
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
                    resolved.tier,
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
