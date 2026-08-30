//! Consumer struct for company-corpus source manifests
//! (`kask/registry/company-sources/*.yaml`).
//!
//! The manifest is the approved-source trust policy for
//! `corpus_discover_company` (documented in
//! `kask/docs/reference/mcp-servers/corpus.md`; original design doc in git
//! history). `deny_unknown_fields` makes schema drift
//! between operator-authored manifests and this struct fail loudly at parse
//! time, matching the `CorpusConfig` contract (`embed/types.rs`).
//!
//! The tier ordering is the MAIA self-description doctrine made mechanical:
//! tier_1 = the company describing itself, tier_2 = executives speaking
//! unmediated, tier_3 = external context (opt-in only, never citable as the
//! company's position).

use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Top-level consumer struct for a company-source manifest YAML.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompanySourceManifest {
    pub manifest: ManifestHeader,
    pub company: CompanyIdentity,
    pub source_tiers: SourceTiers,
    pub provenance_rule: String,
    pub exclusion_rule: String,
    #[serde(default)]
    pub ingestion: Option<IngestionDefaults>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestHeader {
    pub id: String,
    pub category: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub editor: Option<String>,
    #[serde(default)]
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyIdentity {
    pub symbol: String,
    pub name: String,
    pub cik: String,
    pub ir_base: String,
    #[serde(default)]
    pub ceo: Option<String>,
    #[serde(default)]
    pub cfo: Option<String>,
    /// Fiscal-year-end month (1–12); 6 = June FY end. Needed to map FMP
    /// year/quarter labels to fiscal periods for non-calendar fiscal years.
    #[serde(default)]
    pub fiscal_year_end_month: Option<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceTiers {
    #[serde(default)]
    pub tier_1_self_description: Vec<SourceEntry>,
    #[serde(default)]
    pub tier_2_executive_voice: Vec<SourceEntry>,
    #[serde(default)]
    pub tier_3_external: Vec<SourceEntry>,
}

/// One approved source. Fields are optional per `kind` — a `sec_filings`
/// entry has `forms`, a `youtube` entry has `queries` + `channels_allowlist`.
/// Validation of kind-required fields lives in `validate()`, not serde, so
/// the error names the kind and the missing field.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceEntry {
    pub kind: String,
    pub via: String,
    #[serde(default)]
    pub forms: Vec<String>,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub subpaths: Vec<String>,
    #[serde(default)]
    pub queries: Vec<String>,
    #[serde(default)]
    pub channels_allowlist: Vec<String>,
    #[serde(default)]
    pub speaker_attribution: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IngestionDefaults {
    pub entity_ref_prefix: String,
    #[serde(default)]
    pub chunking: Option<ChunkingDefaults>,
    #[serde(default)]
    pub tagging: Option<TaggingDefaults>,
    #[serde(default)]
    pub embedding: Option<EmbeddingDefaults>,
    #[serde(default)]
    pub centroids: Option<CentroidDefaults>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkingDefaults {
    #[serde(default)]
    pub multi_tier: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaggingDefaults {
    /// Ontology namespaces to tag with (e.g. ["fibo", "pko", "sumo"]).
    /// Validated against `OntologyNamespace::from_str` at `validate()` time —
    /// unknown namespaces are rejected, not silently accepted.
    #[serde(default)]
    pub ontologies: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingDefaults {
    pub dim: usize,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CentroidDefaults {
    #[serde(default)]
    pub group_by: Vec<String>,
}

/// Validation errors naming the offending tier/kind/field.
#[derive(Debug, thiserror::Error)]
pub enum ManifestValidationError {
    #[error("manifest.category must be 'company-source-manifest', got '{0}'")]
    WrongCategory(String),
    #[error(
        "tier_3 source '{kind}' is enabled by default (via '{via}'); tier_3 is opt-in only per the MAIA self-description doctrine — remove it or gate it behind an explicit operator decision"
    )]
    Tier3EnabledByDefault { kind: String, via: String },
    #[error("{tier} source kind '{kind}' requires field '{field}'")]
    MissingField {
        tier: &'static str,
        kind: String,
        field: &'static str,
    },
    #[error("company.fiscal_year_end_month must be 1–12, got {0}")]
    BadFiscalMonth(u8),
    #[error(
        "unknown ontology namespace '{0}' in tagging.ontologies — must be one of: fibo, sepio, golem, mlschema, omc, sumo"
    )]
    UnknownOntology(String),
}

impl CompanySourceManifest {
    /// Parse a manifest from YAML text.
    pub fn from_yaml(text: &str) -> Result<Self, serde_yaml_neo::Error> {
        serde_yaml_neo::from_str(text)
    }

    /// Validate the trust-policy invariants that serde cannot express.
    ///
    /// The tier_3 check is the load-bearing one: the MAIA doctrine (start
    /// with the company's self-description, not sell-side/media) is enforced
    /// here as a parse-time rejection, not a prompt-side instruction — the
    /// "advertised invariants need enforcement points" rule.
    pub fn validate(&self) -> Result<(), ManifestValidationError> {
        if self.manifest.category != "company-source-manifest" {
            return Err(ManifestValidationError::WrongCategory(
                self.manifest.category.clone(),
            ));
        }
        if let Some(month) = self.company.fiscal_year_end_month {
            if !(1..=12).contains(&month) {
                return Err(ManifestValidationError::BadFiscalMonth(month));
            }
        }
        if let Some(entry) = self.source_tiers.tier_3_external.first() {
            return Err(ManifestValidationError::Tier3EnabledByDefault {
                kind: entry.kind.clone(),
                via: entry.via.clone(),
            });
        }
        for entry in &self.source_tiers.tier_1_self_description {
            if entry.kind == "sec_filings" && entry.forms.is_empty() {
                return Err(ManifestValidationError::MissingField {
                    tier: "tier_1",
                    kind: entry.kind.clone(),
                    field: "forms",
                });
            }
        }
        for entry in &self.source_tiers.tier_2_executive_voice {
            if entry.kind == "youtube" && entry.channels_allowlist.is_empty() {
                return Err(ManifestValidationError::MissingField {
                    tier: "tier_2",
                    kind: entry.kind.clone(),
                    field: "channels_allowlist",
                });
            }
        }
        // Validate the tagging ontologies field (if present) against the
        // known ontology namespaces. This is the enforcement point for the
        // `tagging.ontologies` manifest field — without it, the field is
        // dead config (advertised invariant with no enforcement).
        if let Some(ingestion) = &self.ingestion
            && let Some(tagging) = &ingestion.tagging
        {
            for ontology in &tagging.ontologies {
                if hkask_bridge_ontology::axis::OntologyNamespace::from_str(ontology).is_err() {
                    return Err(ManifestValidationError::UnknownOntology(ontology.clone()));
                }
            }
        }
        Ok(())
    }
}
