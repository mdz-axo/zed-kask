//! The full published vocabularies, compiled from the pinned sources in
//! `sources/` by `build.rs` and embedded in the crate.
//!
//! Loaded today: SUMO (every ontology file of the pinned distribution) and
//! schema.org (every type, enumeration member and property of the pinned
//! release). Each term carries its published definition and source file, so
//! a resolution always arrives with the ontology's own meaning — never a
//! private gloss.

use std::collections::HashMap;
use std::sync::OnceLock;

const INDEX: &str = include_str!(concat!(env!("OUT_DIR"), "/published_index.tsv"));
const LIST_SEP: char = '\u{1f}';

/// One published term: vocabulary and provenance, never axioms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedTerm {
    /// Publishing vocabulary (`SUMO`, `schema.org`).
    pub namespace: &'static str,
    /// Canonical concept id (`sumo:Game`, `schema:Game`).
    pub concept: &'static str,
    /// Local name as published.
    pub name: &'static str,
    /// `class`, `relation`, `instance`, `property`, `enumeration_member`, or `term`.
    pub kind: &'static str,
    /// Published English labels beyond the local name.
    pub labels: Vec<&'static str>,
    /// Direct parents as stated by the source (subclass, instance, subrelation,
    /// subTypeOf, subPropertyOf, enumeration type). Not a transitive closure.
    pub parents: Vec<&'static str>,
    /// Published inverse properties.
    pub inverse_of: Vec<&'static str>,
    /// The source's own definition text; empty when the source publishes none.
    pub definition: &'static str,
    /// Source file and pinned version.
    pub source: &'static str,
}

struct Index {
    terms: Vec<PublishedTerm>,
    by_concept: HashMap<&'static str, usize>,
    by_key: HashMap<String, Vec<usize>>,
}

/// The lookup key shared with the resolver: lowercase ASCII alphanumerics.
pub fn normalize(term: &str) -> String {
    term.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

fn split_list(field: &'static str) -> Vec<&'static str> {
    field
        .split(LIST_SEP)
        .filter(|item| !item.is_empty())
        .collect()
}

fn index() -> &'static Index {
    static INDEX_CELL: OnceLock<Index> = OnceLock::new();
    INDEX_CELL.get_or_init(|| {
        let mut terms = Vec::new();
        for line in INDEX.lines() {
            let fields: Vec<&'static str> = line.split('\t').collect();
            // build.rs writes exactly nine fields per line; a malformed line
            // is a build bug, and skipping it would silently drop a term.
            let [
                namespace,
                concept,
                name,
                kind,
                labels,
                parents,
                inverse,
                definition,
                source,
            ] = fields.as_slice()
            else {
                panic!(
                    "published index line has {} fields, expected 9: {line}",
                    fields.len()
                );
            };
            terms.push(PublishedTerm {
                namespace,
                concept,
                name,
                kind,
                labels: split_list(labels),
                parents: split_list(parents),
                inverse_of: split_list(inverse),
                definition,
                source,
            });
        }
        let mut by_concept = HashMap::with_capacity(terms.len());
        let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
        for (position, term) in terms.iter().enumerate() {
            by_concept.insert(term.concept, position);
            let mut keys = vec![normalize(term.name)];
            keys.extend(term.labels.iter().map(|label| normalize(label)));
            keys.sort();
            keys.dedup();
            for key in keys.into_iter().filter(|key| !key.is_empty()) {
                by_key.entry(key).or_default().push(position);
            }
        }
        Index {
            terms,
            by_concept,
            by_key,
        }
    })
}

/// Every published term, in source order.
pub fn terms() -> &'static [PublishedTerm] {
    &index().terms
}

/// The published term with this exact concept id.
pub fn get(concept: &str) -> Option<&'static PublishedTerm> {
    let index = index();
    index
        .by_concept
        .get(concept)
        .map(|&position| &index.terms[position])
}

/// Whether a concept id is published in a loaded vocabulary.
pub fn contains(concept: &str) -> bool {
    get(concept).is_some()
}

/// Every published sense of a term in `namespace`: an exact concept id, or a
/// local name / published label equal under [`normalize`]. Exact matching
/// only — a false specific anchor is worse than none.
pub fn lookup(namespace: &str, term: &str) -> Vec<&'static PublishedTerm> {
    let index = index();
    if let Some(found) = get(term.trim()).filter(|found| found.namespace == namespace) {
        return vec![found];
    }
    index
        .by_key
        .get(&normalize(term))
        .into_iter()
        .flatten()
        .map(|&position| &index.terms[position])
        .filter(|found| found.namespace == namespace)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: [P8.3] The full SUMO distribution is loaded — terms far beyond
    /// the former 13-term seed list resolve with their Merge.kif definitions.
    #[test]
    fn full_sumo_is_loaded_with_published_definitions() {
        for (name, parent) in [
            ("Game", "sumo:Contest"),
            ("DeductiveArgument", "sumo:Argument"),
            ("ComputerProgram", "sumo:Procedure"),
            ("RegulatoryProcess", "sumo:Guiding"),
        ] {
            let found = lookup("SUMO", name);
            let term = found.first().unwrap_or_else(|| panic!("{name} missing"));
            assert_eq!(term.concept, format!("sumo:{name}"));
            assert!(term.parents.contains(&parent), "{name}: {:?}", term.parents);
            assert!(!term.definition.is_empty(), "{name} has a definition");
            assert!(
                term.source.starts_with("Merge.kif"),
                "{name}: {}",
                term.source
            );
        }
        let probability = get("sumo:ProbabilityFn").expect("ProbabilityFn");
        assert_eq!(probability.kind, "relation");
        let game = get("sumo:Game").expect("Game");
        assert!(game.definition.contains("Contest"), "{}", game.definition);
        assert!(!game.definition.contains("&%"), "markup stripped");
        // Domain files are loaded too, not only Merge.kif.
        assert!(terms().iter().any(|term| term.namespace == "SUMO"
            && !term.source.starts_with("Merge.kif")
            && !term.source.starts_with("Mid-level-ontology.kif")));
        assert!(
            terms()
                .iter()
                .filter(|term| term.namespace == "SUMO")
                .count()
                > 10_000,
            "the full distribution, not a seed list"
        );
    }

    /// expect: The full schema.org release is loaded: types, enumeration
    /// members and properties, each with its published comment.
    #[test]
    fn full_schema_org_is_loaded_with_published_comments() {
        let game = get("schema:Game").expect("schema:Game");
        assert_eq!(game.kind, "class");
        assert!(game.parents.contains(&"schema:CreativeWork"));
        assert!(!game.definition.is_empty());
        assert!(
            !game.definition.contains('<'),
            "HTML stripped: {}",
            game.definition
        );
        assert_eq!(
            get("schema:Monday").map(|term| term.kind),
            Some("enumeration_member")
        );
        let has_part = get("schema:hasPart").expect("hasPart");
        assert_eq!(has_part.kind, "property");
        assert_eq!(has_part.inverse_of, ["schema:isPartOf"]);
        let count = terms()
            .iter()
            .filter(|term| term.namespace == "schema.org")
            .count();
        assert!(count > 2_000, "the full release, not a seed list: {count}");
    }

    /// expect: Case and separator variants are one lookup key, and a key that
    /// names several published terms returns every one of them.
    #[test]
    fn lookup_returns_every_sense_for_a_key() {
        assert_eq!(
            lookup("SUMO", "deductive argument")[0].concept,
            "sumo:DeductiveArgument"
        );
        assert_eq!(
            lookup("schema.org", "schema:Game")[0].concept,
            "schema:Game"
        );
        assert!(lookup("SUMO", "zephyr coefficient").is_empty());
        let senses = lookup("schema.org", "haspart");
        assert!(senses.iter().any(|term| term.concept == "schema:hasPart"));
    }
}
