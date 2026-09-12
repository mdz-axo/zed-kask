//! Exact term resolution over the published ontology registries.
//!
//! Models may propose descriptive candidate terms, but they never choose an
//! ontology namespace or canonical URI. This module is the single authority
//! that resolves those terms and derives the grouped annotation fields used by
//! corpus chunks and h_mems.

use std::collections::{HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{derived, fibo, golem, ml_schema, omc, pko, rdf, schema_org, sdmx, sepio, sumo};

/// Protocol stamped on records classified through the published term resolver.
pub const TERM_RESOLUTION_PROTOCOL: &str = "published-term-resolution-v1";

/// The result of walking the published-ontology fallback ladder for one term.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TermResolution {
    /// Ladder rung: domain_supplement, derived, upper, or core.
    pub tier: String,
    /// Candidate term supplied by the caller, trimmed but otherwise preserved.
    pub term: String,
    /// Vocabulary that publishes the canonical concept.
    pub namespace: String,
    /// Published concept URI, derived term, or the 5W1H core anchor.
    pub concept: String,
    /// Recorded identity for a derived concept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// Recorded authority for a derived concept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
    /// Ruling path when resolution reaches the coarse core rung.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Canonical annotation derived from raw candidate terms.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonicalTerms {
    /// Trimmed, deduplicated descriptive terms. Coarse resolution never erases them.
    pub candidate_terms: Vec<String>,
    /// Canonical concepts grouped only under resolver-selected namespaces.
    pub ontology_tags: HashMap<String, Vec<String>>,
    /// First-seen union of canonical concepts for salience and retrieval.
    pub concepts: Vec<String>,
}

/// Rung 1 registries, in stable resolution order.
const NAMED_DOMAIN_REGISTRIES: &[(&str, &[(&str, &str)])] = &[
    ("SEPIO", sepio::ALL_NAMED_TERMS),
    ("GOLEM", golem::ALL_NAMED_TERMS),
];

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

fn normalize(term: &str) -> String {
    term.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// Walk the exact fallback ladder. A false specific anchor is worse than the
/// real but coarse 5W1H ground, so matching is never fuzzy.
pub fn resolve_term(term: &str) -> TermResolution {
    let trimmed = term.trim();

    // Some published URI suffixes carry numeric class codes. Their
    // fixture-backed bridge constant names provide exact descriptive labels.
    for (namespace, registry) in NAMED_DOMAIN_REGISTRIES {
        for (name, uri) in *registry {
            if normalize(trimmed) == normalize(name) {
                return TermResolution {
                    tier: "domain_supplement".to_string(),
                    term: trimmed.to_string(),
                    namespace: (*namespace).to_string(),
                    concept: (*uri).to_string(),
                    identity: None,
                    authority: None,
                    note: None,
                };
            }
        }
    }

    for (namespace, registry) in DOMAIN_REGISTRIES {
        for uri in *registry {
            let name = uri.rsplit(':').next().unwrap_or(uri);
            if trimmed == *uri || (!name.is_empty() && normalize(trimmed) == normalize(name)) {
                return TermResolution {
                    tier: "domain_supplement".to_string(),
                    term: trimmed.to_string(),
                    namespace: (*namespace).to_string(),
                    concept: (*uri).to_string(),
                    identity: None,
                    authority: None,
                    note: None,
                };
            }
        }
    }

    if let Some(concept) = derived::resolve_derived(trimmed) {
        return TermResolution {
            tier: "derived".to_string(),
            term: trimmed.to_string(),
            namespace: "derived".to_string(),
            concept: concept.term.to_string(),
            identity: Some(concept.identity.to_string()),
            authority: Some(concept.authority.to_string()),
            note: None,
        };
    }

    for uri in sumo::ALL_CONCEPTS {
        let name = uri.rsplit(':').next().unwrap_or(uri);
        if trimmed == *uri || (!name.is_empty() && normalize(trimmed) == normalize(name)) {
            return TermResolution {
                tier: "upper".to_string(),
                term: trimmed.to_string(),
                namespace: "SUMO".to_string(),
                concept: (*uri).to_string(),
                identity: None,
                authority: None,
                note: None,
            };
        }
    }

    TermResolution {
        tier: "core".to_string(),
        term: trimmed.to_string(),
        namespace: "core".to_string(),
        concept: "5w1h_core".to_string(),
        identity: None,
        authority: None,
        note: Some(
            "No domain, derived, or upper concept matched. Anchored on the 5W1H \
             interrogative ground — a real but coarse anchor. Request a ruling from \
             the operator to improve it: the ruling is recorded in the derived \
             registry (hkask-bridge-ontology/src/derived.rs) with its identity and \
             authority, and the term resolves there ever after. Never assign the \
             term a private definition in the meantime."
                .to_string(),
        ),
    }
}

/// Preserve raw terms and derive every canonical annotation through the same
/// resolver. Namespace keys are normalized only after the resolver chooses them.
pub fn canonicalize_terms<I, S>(terms: I) -> CanonicalTerms
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut canonical = CanonicalTerms::default();
    let mut seen_terms = HashSet::new();
    let mut seen_concepts = HashSet::new();

    for term in terms {
        let trimmed = term.as_ref().trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = normalize(trimmed);
        let dedup_key = if normalized.is_empty() {
            trimmed.to_lowercase()
        } else {
            normalized
        };
        if !seen_terms.insert(dedup_key) {
            continue;
        }

        canonical.candidate_terms.push(trimmed.to_string());
        let resolved = resolve_term(trimmed);
        let namespace = resolved.namespace.to_ascii_lowercase();
        let concepts = canonical.ontology_tags.entry(namespace).or_default();
        if !concepts.contains(&resolved.concept) {
            concepts.push(resolved.concept.clone());
        }
        if seen_concepts.insert(resolved.concept.clone()) {
            canonical.concepts.push(resolved.concept);
        }
    }

    canonical
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_each_ladder_rung_and_always_terminates() {
        for (term, tier, namespace, concept) in [
            (
                "corporation",
                "domain_supplement",
                "FIBO",
                "fibo-be-le-cb:Corporation",
            ),
            ("assertion", "domain_supplement", "SEPIO", sepio::ASSERTION),
            ("net margin", "derived", "derived", "net_margin"),
            ("quantity", "upper", "SUMO", "sumo:Quantity"),
            ("zephyr coefficient", "core", "core", "5w1h_core"),
        ] {
            let resolved = resolve_term(term);
            assert_eq!(resolved.tier, tier, "{term}: {resolved:?}");
            assert_eq!(resolved.namespace, namespace, "{term}: {resolved:?}");
            assert_eq!(resolved.concept, concept, "{term}: {resolved:?}");
        }
    }

    #[test]
    fn canonicalization_preserves_raw_terms_but_never_model_namespaces() {
        let canonical = canonicalize_terms([
            "Corporation",
            "corporation",
            "quantity",
            "zephyr coefficient",
            "another unknown term",
        ]);

        assert_eq!(
            canonical.candidate_terms,
            [
                "Corporation",
                "quantity",
                "zephyr coefficient",
                "another unknown term"
            ]
        );
        assert_eq!(
            canonical.ontology_tags["fibo"],
            ["fibo-be-le-cb:Corporation"]
        );
        assert_eq!(canonical.ontology_tags["sumo"], ["sumo:Quantity"]);
        assert_eq!(canonical.ontology_tags["core"], ["5w1h_core"]);
        assert_eq!(
            canonical.concepts,
            ["fibo-be-le-cb:Corporation", "sumo:Quantity", "5w1h_core"]
        );
    }

    #[test]
    fn derived_resolution_carries_identity_and_authority() {
        let resolved = resolve_term("net margin");
        assert_eq!(resolved.identity.as_deref(), Some("net income / revenue"));
        assert!(
            resolved
                .authority
                .is_some_and(|value| value.contains("2026-09-10"))
        );
    }
}
