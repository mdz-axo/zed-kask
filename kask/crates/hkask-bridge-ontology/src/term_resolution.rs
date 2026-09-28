//! Exact term resolution over the published ontology registries.
//!
//! Models may propose descriptive candidate terms, but they never choose an
//! ontology namespace or canonical URI. This module is the single authority
//! that resolves those terms and derives the grouped annotation fields used by
//! corpus chunks and h_mems.

use std::collections::{HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::sepio;
use crate::{derived, published};

/// Protocol stamped on records classified through the published term resolver.
/// v2 (2026-09-27): resolution walks the full published SUMO and schema.org
/// vocabularies, so v1 records no longer reconcile and are re-tagged.
pub const TERM_RESOLUTION_PROTOCOL: &str = "published-term-resolution-v2";

/// One published or recorded sense of a term.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TermSense {
    /// Ladder rung of this sense.
    pub tier: String,
    /// Vocabulary that publishes the concept.
    pub namespace: String,
    /// Published concept URI or derived term.
    pub concept: String,
    /// The vocabulary's own definition, when it publishes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    /// Source file and pinned version of a published sense.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// The result of walking the published-ontology fallback ladder for one term.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TermResolution {
    /// Ladder rung: domain_supplement, derived, upper, general_vocabulary,
    /// state_axis, or core.
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
    /// The chosen vocabulary's own definition of the concept (for a derived
    /// concept, the recorded ruling's definition).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    /// Source file and pinned version of a published concept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Every other sense the ladder found, in ladder order. The same word
    /// often names different concepts in different vocabularies (schema.org's
    /// `Game` is a creative work; SUMO's is a contest); all are shown so the
    /// choice stays visible.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<TermSense>,
}

impl TermResolution {
    /// The guaranteed final rung, with the ruling path.
    pub fn core(term: &str, note: String) -> Self {
        Self {
            tier: "core".to_string(),
            term: term.to_string(),
            namespace: "core".to_string(),
            concept: "5w1h_core".to_string(),
            identity: None,
            authority: None,
            note: Some(note),
            definition: None,
            source: None,
            alternatives: Vec::new(),
        }
    }
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

/// Full published vocabularies after FIBO and the remaining local SDMX
/// registry, in stable resolution order. OMC precedes PKO so media terms
/// keep their published sense.
const PUBLISHED_DOMAIN: &[&str] = &[
    "OMC",
    "PKO",
    "P-Plan",
    "PROV",
    "SEPIO",
    "GOLEM",
    "CIDOC-CRM",
    "LRMoo",
    "ML-Schema",
    "RDF",
    "RDFS",
];

/// The remaining local identifier registry is not yet a published source index.
const DOMAIN_REGISTRIES: &[(&str, &[&str])] = &[("SDMX", crate::sdmx::ALL_CONCEPTS)];

/// Full published vocabularies consulted after the domain and derived rungs,
/// in ladder order: SUMO (formal upper ontology) first, then schema.org (a
/// general web vocabulary), then the state axis (Dublin Core, BIBO, CiTO) —
/// artifact-typing vocabularies whose senses stay visible as alternatives but
/// never outrank a formal category (operator rulings 2026-09-10, 2026-09-27).
const PUBLISHED_RUNGS: &[(&str, &str)] = &[
    ("SUMO", "upper"),
    ("schema.org", "general_vocabulary"),
    ("Dublin Core", "state_axis"),
    ("BIBO", "state_axis"),
    ("CiTO", "state_axis"),
];

struct Sense {
    sense: TermSense,
    identity: Option<String>,
    authority: Option<String>,
}

fn sense(tier: &str, namespace: &str, concept: &str) -> Sense {
    Sense {
        sense: TermSense {
            tier: tier.to_string(),
            namespace: namespace.to_string(),
            concept: concept.to_string(),
            definition: None,
            source: None,
        },
        identity: None,
        authority: None,
    }
}

use published::normalize;

fn published_senses(tier: &str, namespace: &str, term: &str) -> Vec<Sense> {
    published::lookup(namespace, term)
        .into_iter()
        .map(|published_term| {
            let mut found = sense(tier, namespace, published_term.concept);
            found.sense.definition = Some(published_term.definition)
                .filter(|text| !text.is_empty())
                .map(str::to_string);
            found.sense.source = Some(published_term.source.to_string());
            found
        })
        .collect()
}

/// Walk the exact fallback ladder. A false specific anchor is worse than the
/// real but coarse 5W1H ground, so matching is never fuzzy. The first sense
/// in ladder order is the resolution; every other sense found is listed in
/// `alternatives`.
pub fn resolve_term(term: &str) -> TermResolution {
    let trimmed = term.trim();
    let key = normalize(trimmed);
    let mut senses: Vec<Sense> = Vec::new();

    // Preserve the pre-index domain ordering: FIBO before SDMX, then OMC
    // and the other published supplements. FIBO now resolves from the
    // source-backed index rather than its named-constant registry.
    senses.extend(published_senses("domain_supplement", "FIBO", trimmed));
    for (namespace, registry) in DOMAIN_REGISTRIES {
        let found = registry.iter().find(|uri| {
            let name = uri.rsplit(':').next().unwrap_or(uri);
            trimmed == **uri || (!name.is_empty() && key == normalize(name))
        });
        if let Some(uri) = found {
            senses.push(sense("domain_supplement", namespace, uri));
        }
    }

    for namespace in PUBLISHED_DOMAIN {
        senses.extend(published_senses("domain_supplement", namespace, trimmed));
    }

    if let Some(concept) = derived::resolve_derived(trimmed) {
        let mut found = sense("derived", "derived", concept.term);
        found.sense.definition = Some(concept.definition.to_string());
        found.identity = Some(concept.identity.to_string());
        found.authority = Some(concept.authority.to_string());
        senses.push(found);
    }

    for (namespace, tier) in PUBLISHED_RUNGS {
        senses.extend(published_senses(tier, namespace, trimmed));
    }

    let mut seen = HashSet::new();
    senses.retain(|found| seen.insert(found.sense.concept.clone()));
    let mut senses = senses.into_iter();
    let Some(primary) = senses.next() else {
        return TermResolution::core(
            trimmed,
            "No published or derived concept matched. Anchored on the 5W1H \
             interrogative ground — a real but coarse anchor. Request a ruling from \
             the operator to improve it: the ruling is recorded in the derived \
             registry (hkask-bridge-ontology/src/derived.rs) with its identity and \
             authority, and the term resolves there ever after. Never assign the \
             term a private definition in the meantime."
                .to_string(),
        );
    };
    TermResolution {
        tier: primary.sense.tier,
        term: trimmed.to_string(),
        namespace: primary.sense.namespace,
        concept: primary.sense.concept,
        identity: primary.identity,
        authority: primary.authority,
        note: None,
        definition: primary.sense.definition,
        source: primary.sense.source,
        alternatives: senses.map(|found| found.sense).collect(),
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
            ("Provenance", "domain_supplement", "OMC", "omc:Provenance"),
            ("State", "domain_supplement", "OMC", "omc:State"),
            ("net margin", "derived", "derived", "net_margin"),
            (
                "term structure",
                "domain_supplement",
                "FIBO",
                "fibo-ind-ind-ind:TermStructure",
            ),
            (
                "interest rate",
                "domain_supplement",
                "FIBO",
                "fibo-fnd-acc-cur:InterestRate",
            ),
            ("quantity", "upper", "SUMO", "sumo:Quantity"),
            ("zephyr coefficient", "core", "core", "5w1h_core"),
            // Accepted alias loss (stopgap retired 2026-09-20): the derived
            // aliases died with the entry; core ground, never a false FIBO pin.
            ("cmp term structure", "core", "core", "5w1h_core"),
        ] {
            let resolved = resolve_term(term);
            assert_eq!(resolved.tier, tier, "{term}: {resolved:?}");
            assert_eq!(resolved.namespace, namespace, "{term}: {resolved:?}");
            assert_eq!(resolved.concept, concept, "{term}: {resolved:?}");
        }
    }

    /// expect: a FIBO Release concept outside the named constants resolves
    /// with the source's definition and its real, pinned module identity.
    #[test]
    fn full_fibo_release_resolves_beyond_named_constants() {
        let result = resolve_term("fibo-be-le-cb:BenefitCorporation");
        assert_eq!(result.tier, "domain_supplement");
        assert_eq!(result.namespace, "FIBO");
        assert_eq!(result.concept, "fibo-be-le-cb:BenefitCorporation");
        assert!(
            result
                .definition
                .as_deref()
                .is_some_and(|text| text.contains("not-for-profit"))
        );
        assert!(result.source.as_deref().is_some_and(|source| {
            source.contains("BE/LegalEntities/CorporateBodies.rdf") && source.contains("f59157fe")
        }));
    }

    /// expect: terms the fragment lists missed now resolve on the full
    /// published vocabularies, carrying the source's own definition.
    #[test]
    fn full_vocabularies_resolve_with_published_definitions() {
        for (term, tier, concept) in [
            ("deductive argument", "upper", "sumo:DeductiveArgument"),
            ("ProbabilityFn", "upper", "sumo:ProbabilityFn"),
            ("regulatory process", "upper", "sumo:RegulatoryProcess"),
            ("Recipe", "general_vocabulary", "schema:Recipe"),
        ] {
            let resolved = resolve_term(term);
            assert_eq!(
                (resolved.tier.as_str(), resolved.concept.as_str()),
                (tier, concept),
                "{resolved:?}"
            );
            assert!(
                resolved
                    .definition
                    .as_deref()
                    .is_some_and(|text| !text.is_empty()),
                "{resolved:?}"
            );
            assert!(resolved.source.is_some(), "{resolved:?}");
        }
    }

    /// expect: one word, several published senses — the formal upper sense
    /// is chosen and the web-vocabulary sense is listed, never dropped.
    #[test]
    fn every_sense_of_an_ambiguous_word_is_listed() {
        let game = resolve_term("Game");
        assert_eq!(game.concept, "sumo:Game", "{game:?}");
        assert!(
            game.definition
                .as_deref()
                .is_some_and(|text| text.contains("Contest"))
        );
        let web = game
            .alternatives
            .iter()
            .find(|sense| sense.concept == "schema:Game")
            .expect("schema.org sense listed");
        assert_eq!(web.tier, "general_vocabulary");
        assert!(web.definition.is_some() && web.source.is_some());
        let concepts: Vec<_> = std::iter::once(&game.concept)
            .chain(game.alternatives.iter().map(|sense| &sense.concept))
            .collect();
        let unique: HashSet<_> = concepts.iter().collect();
        assert_eq!(unique.len(), concepts.len(), "senses are deduplicated");
    }

    /// expect: the process-axis family (PKO, P-Plan, PROV-O) resolves on the
    /// domain rung from full sources; state-axis vocabularies resolve only
    /// where nothing formal publishes the word, and otherwise stay visible
    /// as alternatives.
    #[test]
    fn full_axis_vocabularies_resolve_on_their_rungs() {
        // The PROV source publishes no definition text for wasGeneratedBy
        // (its meaning is in the PROV-DM prose): none is invented.
        let generated = resolve_term("wasGeneratedBy");
        assert_eq!(
            (generated.tier.as_str(), generated.concept.as_str()),
            ("domain_supplement", "prov:wasGeneratedBy")
        );
        assert_eq!(generated.definition, None);
        for (term, tier, concept) in [
            ("Delegation", "domain_supplement", "prov:Delegation"),
            ("MultiStep", "domain_supplement", "pplan:MultiStep"),
            (
                "procedure execution",
                "domain_supplement",
                "pko:ProcedureExecution",
            ),
            ("academic article", "state_axis", "bibo:AcademicArticle"),
            ("cites as evidence", "state_axis", "cito:citesAsEvidence"),
        ] {
            let resolved = resolve_term(term);
            assert_eq!(
                (resolved.tier.as_str(), resolved.concept.as_str()),
                (tier, concept),
                "{resolved:?}"
            );
            assert!(resolved.definition.is_some(), "{resolved:?}");
        }
        let title = resolve_term("title");
        assert_ne!(
            title.tier, "state_axis",
            "a formal sense outranks DC: {title:?}"
        );
        assert!(
            title
                .alternatives
                .iter()
                .any(|sense| sense.concept == "dcterms:title" && sense.tier == "state_axis"),
            "{title:?}"
        );
    }

    /// expect: a derived concept carries its recorded definition alongside
    /// its identity and authority.
    #[test]
    fn derived_resolution_carries_its_recorded_definition() {
        let resolved = resolve_term("net margin");
        assert_eq!(resolved.tier, "derived");
        assert!(
            resolved
                .definition
                .as_deref()
                .is_some_and(|text| text.contains("post-interest"))
        );
    }

    #[test]
    fn omc_precedes_pko_within_domain_supplement_resolution() {
        let omc = PUBLISHED_DOMAIN
            .iter()
            .position(|namespace| *namespace == "OMC");
        let pko = PUBLISHED_DOMAIN
            .iter()
            .position(|namespace| *namespace == "PKO");
        assert!(omc.is_some() && pko.is_some() && omc < pko);
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

    /// P8.4 (entropy-matched computation): the five routing rulings resolve
    /// on the derived rung through the full ladder — the path onto_anchor
    /// walks — never falling through to the coarse core ground.
    #[test]
    fn p84_routing_terms_resolve_on_the_derived_rung() {
        for term in [
            "entropy",
            "deterministic computation",
            "probabilistic computation",
            "verification oracle",
            "calibrated forecast",
        ] {
            let resolved = resolve_term(term);
            assert_eq!(resolved.tier, "derived", "{term}: {resolved:?}");
            assert_eq!(resolved.namespace, "derived", "{term}: {resolved:?}");
            assert!(
                resolved.identity.is_some() && resolved.authority.is_some(),
                "{term}: derived rung carries identity and authority"
            );
        }
        let entropy = resolve_term("entropy");
        assert!(
            entropy
                .authority
                .as_deref()
                .is_some_and(|value| value.contains("Jaynes (1957)"))
        );
    }
}
