//! Published RDF Data Cube vocabulary for statistical datasets.
//!
//! The W3C Recommendation defines the `qb:` namespace. Its complete Turtle
//! vocabulary is vendored under `sources/rdf-data-cube/` with publisher license
//! and SHA-256 pin. These constants are selections, not the vocabulary's extent.
//! SDMX Information Model class names without matching published RDF classes
//! are deliberately not exposed as ontology concept IDs.

/// A published RDF Data Cube concept ID.
pub type DataCubeConcept = &'static str;

/// A collection of observations with a shared dimensional structure.
pub const DATA_SET: DataCubeConcept = "qb:DataSet";
/// The dimensional structure of a data set or slice; not the narrower
/// SDMX-RDF multiple-measures subclass.
pub const DATA_STRUCTURE_DEFINITION: DataCubeConcept = "qb:DataStructureDefinition";
/// A single observation carrying one or more measured values.
pub const OBSERVATION: DataCubeConcept = "qb:Observation";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{published, term_resolution::resolve_term};

    /// expect: The statistical bridge uses the complete published vocabulary,
    /// not aliases transcribed from a different Information Model.
    #[test]
    fn published_cube_replaces_local_sdmx_aliases() {
        let count = published::terms()
            .iter()
            .filter(|term| term.namespace == "RDF Data Cube")
            .count();
        assert!(
            count > 3,
            "full published vocabulary, not three selected terms: {count}"
        );
        for concept in [DATA_SET, DATA_STRUCTURE_DEFINITION, OBSERVATION] {
            let source = published::get(concept).expect("published cube term");
            assert_eq!(source.namespace, "RDF Data Cube");
            assert!(
                !source.definition.is_empty(),
                "{concept}: source definition"
            );
            let resolved = resolve_term(concept);
            assert_eq!(resolved.concept, concept);
            assert_eq!(resolved.source.as_deref(), Some(source.source));
        }
        for local in [
            "sdmx:DataSet",
            "sdmx:Dataflow",
            "sdmx:DataStructureDefinition",
            "sdmx:SeriesKey",
            "sdmx:Observation",
            "sdmx:Category",
            "sdmx:DataProvider",
        ] {
            assert!(
                !published::contains(local),
                "no fictitious publisher identity: {local}"
            );
            assert_ne!(resolve_term(local).concept, local, "{local}");
        }
    }
}
