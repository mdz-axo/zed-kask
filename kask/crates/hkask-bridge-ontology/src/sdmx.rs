//! SDMX ontology vocabulary bridge.
//!
//! Canonical concept names for statistical data and metadata exchange — the
//! ISO 17369 standard used by FRED, DBnomics, World Bank, IMF, OECD, ECB, and
//! INSEE. All three economic-data providers in `hkask-mcp-prediction-markets`
//! use SDMX as their underlying data model; DBnomics is explicitly a
//! multi-provider SDMX aggregator.
//!
//! Reference: <https://sdmx.org/> (SDMX ISO 17369)
//! Reference: SDMX Information Model —
//! <https://github.com/sdmx-twg/sdmx-im> (class names verified against the
//! fetched IM documentation)
//!
//! Naming honesty note: SDMX publishes the Information Model as a UML/XML
//! specification, not an RDF/OWL vocabulary — there is no official `sdmx:`
//! URI namespace. The `sdmx:` prefix below is an hKask rendering convention
//! over official IM class names; the class names themselves are verified
//! against the IM (`fixtures/sdmx-im-terms.txt`).
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
/// World Bank topic, DBnomics dataset). Official IM class name is
/// "Dataflow" (lowercase f).
pub const DATA_FLOW: SdmxConcept = "sdmx:Dataflow";
/// A data structure definition — the schema/dimensions of a dataset.
pub const DATA_STRUCTURE: SdmxConcept = "sdmx:DataStructureDefinition";
/// A time series — the per-series key identifying an observation sequence
/// within a dataset. Official IM class name is "SeriesKey" (the IM has no
/// TimeSeries class).
pub const TIME_SERIES: SdmxConcept = "sdmx:SeriesKey";
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

    /// Fabrication guard: every term in this module must appear in the
    /// official SDMX Information Model term list checked in as a fixture.
    #[test]
    fn all_terms_are_official() {
        let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sdmx-im-terms.txt");
        let fixture = std::fs::read_to_string(fixture_path)
            .unwrap_or_else(|e| panic!("failed to read {fixture_path}: {e}"));
        let official: std::collections::HashSet<&str> = fixture
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
        assert!(
            !official.is_empty(),
            "fixture {fixture_path} contains no terms"
        );
        for term in ALL_CONCEPTS {
            assert!(
                official.contains(term),
                "{term} is not in the official SDMX IM term list ({fixture_path})"
            );
        }
    }
}
