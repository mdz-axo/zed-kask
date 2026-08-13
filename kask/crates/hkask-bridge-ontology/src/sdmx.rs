//! SDMX ontology vocabulary bridge.
//!
//! Canonical concept URIs for statistical data and metadata exchange — the
//! ISO 17369 standard used by FRED, DBnomics, World Bank, IMF, OECD, ECB, and
//! INSEE. All three economic-data providers in `hkask-mcp-prediction-markets`
//! use SDMX as their underlying data model; DBnomics is explicitly a
//! multi-provider SDMX aggregator.
//!
//! Reference: <https://sdmx.org/> (SDMX ISO 17369)
//! Reference: <https://github.com/sdmx-twg/sdmx-ml> (RDF/OWL vocabulary)
//!
//! This module holds the SDMX concept vocabulary only. Server-specific
//! dispatch (mapping an economic-data tool name to its SDMX concept) lives in
//! the prediction-markets server's `ontology_anchor` function.

/// An SDMX concept URI.
pub type SdmxConcept = &'static str;

// ── Core statistical concepts ─────────────────────────────────────────────

/// A statistical dataset — the top-level container for a set of related
/// series (FRED series, DBnomics dataset, World Bank indicator).
pub const DATASET: SdmxConcept = "sdmx:DataSet";
/// A data flow — the publication channel for a dataset (FRED release,
/// World Bank topic, DBnomics dataset).
pub const DATA_FLOW: SdmxConcept = "sdmx:DataFlow";
/// A data structure definition — the schema/dimensions of a dataset.
pub const DATA_STRUCTURE: SdmxConcept = "sdmx:DataStructureDefinition";
/// A time series — the per-series observation sequence.
pub const TIME_SERIES: SdmxConcept = "sdmx:TimeSeries";
/// A single observation (period + value).
pub const OBSERVATION: SdmxConcept = "sdmx:Observation";

// ── Classification ─────────────────────────────────────────────────────────

/// A category in the SDMX category scheme (FRED category tree, World Bank
/// topics).
pub const CATEGORY: SdmxConcept = "sdmx:Category";
/// A data provider (IMF, OECD, ECB, INSEE, FRED, World Bank).
pub const DATA_PROVIDER: SdmxConcept = "sdmx:DataProvider";

/// All SDMX concepts, for validation or iteration.
pub const ALL_CONCEPTS: &[SdmxConcept] = &[
    DATASET,
    DATA_FLOW,
    DATA_STRUCTURE,
    TIME_SERIES,
    OBSERVATION,
    CATEGORY,
    DATA_PROVIDER,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdmx_concepts_are_sdmx_namespaced() {
        for concept in ALL_CONCEPTS {
            assert!(
                concept.starts_with("sdmx:"),
                "SDMX concept must be sdmx-namespaced: {concept}"
            );
        }
    }

    #[test]
    fn sdmx_dataset_concept() {
        assert_eq!(DATASET, "sdmx:DataSet");
    }
}
