//! Dual-axis domain-selection logic (P5.4 / P8.1).
//!
//! The two universal axes — state (Dublin Core) and process (PKO) — are
//! always available. Domain supplements (FIBO, ESO, GOLEM, ML-Schema)
//! layer on top where the universal axes aren't specific enough for a domain.
//!
//! The invariant (user directive 2026-08-05): one axis is always Dublin Core
//! or PKO, so every artifact has a common mapping in process or state space
//! regardless of domain. The domain ontology replaces the *process* axis
//! when the domain has a richer process vocabulary; the state axis stays
//! Dublin Core so the artifact remains identifiable and retrievable.
//!
//! Fallback discipline: if a domain mapping fails or the domain ontology
//! can't place the concept, fall back to the generalists (DC + PKO). Never
//! force a domain ontology where it doesn't fit — the generalists are always
//! valid.

use crate::dc_bibo;
use crate::golem;
use crate::pko;
use crate::sdmx;
use crate::sepio;
use crate::sumo;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A Dublin Core concept URI (re-exported here for axis consumers).
pub type DcConcept = dc_bibo::DcConcept;
/// A PKO concept URI (re-exported here for axis consumers).
pub type PkoConcept = pko::PkoConcept;

/// Which axis of the dual-axis ontological framework (P5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OntologyAxis {
    /// Process (flow) axis — PKO: how did this come to be?
    Pko,
    /// State (entity) axis — Dublin Core + BIBO: what is this?
    DcBibo,
}

/// Domain supplement namespace — which domain-specific ontology (P8.1).
///
/// The process axis is *replaced* (not augmented) by the domain ontology
/// when the domain has a richer process vocabulary. The state axis stays
/// Dublin Core so the artifact remains identifiable regardless of domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OntologyNamespace {
    /// Financial Industry Business Ontology — financial / company analysis.
    Fibo,
    /// SEPIO — scientific evidence and provenance: evidence, support,
    /// dispute, contradiction, confidence.
    Sepio,
    /// GOLEM narrative ontology — literature, narrative, persona.
    Golem,
    /// ML-Schema — machine-learning experiments.
    MlSchema,
    /// SDMX (Statistical Data and Metadata eXchange) — statistical data
    /// from FRED, DBnomics, World Bank, IMF, OECD, ECB, INSEE.
    Sdmx,
    /// SUMO (Suggested Upper Merged Ontology) — the universal upper ontology
    /// and fallback for domains that don't map to a specific supplement.
    /// Provides foundational categories (Entity, Process, Object, Agent).
    Sumo,
}

impl OntologyNamespace {
    /// Map this domain supplement namespace to its canonical Dublin Core concept.
    pub fn dc_concept(&self) -> DcConcept {
        match self {
            OntologyNamespace::Fibo => dc_bibo::DATASET,
            OntologyNamespace::Sepio => dc_bibo::TEXT,
            OntologyNamespace::Golem => dc_bibo::TEXT,
            OntologyNamespace::MlSchema => dc_bibo::DATASET,
            OntologyNamespace::Sdmx => dc_bibo::DATASET,
            OntologyNamespace::Sumo => dc_bibo::TEXT,
        }
    }

    /// Map this domain supplement namespace to its canonical PKO concept.
    pub fn pko_concept(&self) -> PkoConcept {
        match self {
            OntologyNamespace::Fibo => pko::PROCEDURE,
            OntologyNamespace::Sepio => pko::STEP_VERIFICATION,
            OntologyNamespace::Golem => pko::PROCEDURE,
            OntologyNamespace::MlSchema => pko::PROCEDURE,
            OntologyNamespace::Sdmx => pko::PROCEDURE,
            OntologyNamespace::Sumo => pko::PROCEDURE,
        }
    }
}

impl std::str::FromStr for OntologyNamespace {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "fibo" => Ok(OntologyNamespace::Fibo),
            "sepio" => Ok(OntologyNamespace::Sepio),
            "golem" => Ok(OntologyNamespace::Golem),
            "mlschema" | "ml_schema" | "ml-schema" => Ok(OntologyNamespace::MlSchema),
            "sdmx" => Ok(OntologyNamespace::Sdmx),
            "sumo" => Ok(OntologyNamespace::Sumo),
            _ => Err(format!("Unknown ontology namespace: {s}")),
        }
    }
}

impl std::fmt::Display for OntologyNamespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OntologyNamespace::Fibo => write!(f, "fibo"),
            OntologyNamespace::Sepio => write!(f, "sepio"),
            OntologyNamespace::Golem => write!(f, "golem"),
            OntologyNamespace::MlSchema => write!(f, "mlschema"),
            OntologyNamespace::Sdmx => write!(f, "sdmx"),
            OntologyNamespace::Sumo => write!(f, "sumo"),
        }
    }
}

/// Domain ontology tier for content produced by an MCP tool.
///
/// Every piece of content in hKask exists within the 3-tier ontology
/// weighting — different ontologies carry different confidence baselines
/// and information density expectations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum OntologyAnchor {
    /// Universal 5W1H core — no domain supplement (P5.2 default ground).
    /// Content anchored only to Who/What/When/Where/Why/How.
    #[serde(rename = "core")]
    #[default]
    Core,
    /// Process axis (PKO) or state axis (DC+BIBO) — dual-axis framework (P5.4).
    /// `concept` is the canonical concept URI, e.g. "pko:StepExecution" or "bibo:Article".
    DualAxis { axis: OntologyAxis, concept: String },
    /// Domain supplement — FIBO, ESO, GOLEM, ML-Schema, or SUMO (P8.1).
    /// Layered on top of the dual-axis core for domain-specific precision.
    /// SUMO is the universal fallback for domains without a specific supplement.
    DomainSupplement {
        namespace: OntologyNamespace,
        concept: String,
    },
}

impl OntologyAnchor {
    /// Return the confidence modifier for this ontology tier.
    /// FIBO: +0.10 (OMG standard, high adoption)
    /// SUMO: +0.05 (actively maintained upper ontology, broad coverage)
    /// Others: ±0.00 (standard baseline)
    pub fn confidence_modifier(&self) -> f64 {
        match self {
            OntologyAnchor::Core => 0.0,
            OntologyAnchor::DualAxis { .. } => 0.0,
            OntologyAnchor::DomainSupplement { namespace, .. } => match namespace {
                OntologyNamespace::Fibo => 0.10,
                OntologyNamespace::Sumo => 0.05,
                _ => 0.0,
            },
        }
    }

    /// Return the information density expectation for this ontology tier.
    pub fn density_factor(&self) -> f64 {
        match self {
            OntologyAnchor::Core => 1.0,
            OntologyAnchor::DualAxis { axis, .. } => match axis {
                OntologyAxis::Pko => 1.0,
                OntologyAxis::DcBibo => 1.0,
            },
            OntologyAnchor::DomainSupplement { namespace, .. } => match namespace {
                OntologyNamespace::Fibo => 1.3,
                OntologyNamespace::Sepio => 1.0,
                OntologyNamespace::Golem => 1.0,
                OntologyNamespace::MlSchema => 1.1,
                OntologyNamespace::Sdmx => 1.1,
                OntologyNamespace::Sumo => 1.0,
            },
        }
    }

    /// Which axis of the dual-axis framework this anchor belongs to (P5.4).
    pub fn axis(&self) -> Option<OntologyAxis> {
        match self {
            OntologyAnchor::Core => None,
            OntologyAnchor::DualAxis { axis, .. } => Some(*axis),
            OntologyAnchor::DomainSupplement { .. } => None,
        }
    }

    /// Human-readable label for the ontology tier.
    pub fn tier_label(&self) -> &str {
        match self {
            OntologyAnchor::Core => "5w1h_core",
            OntologyAnchor::DualAxis { .. } => "dual_axis",
            OntologyAnchor::DomainSupplement { .. } => "domain_supplement",
        }
    }
}

/// Select the ontology anchoring for a domain.
///
/// State axis is always Dublin Core. Process axis is the domain ontology when
/// one applies, PKO otherwise. The invariant: one axis is always DC or PKO.
///
/// `domain` is a lowercase domain hint supplied by the calling server (the
/// server knows its functional area) or overridden per-request. The hint may
/// be a bare domain ("finance", "media") or a tool-style name
/// matches and token prefixes so tool names resolve to their server's
/// domain. Unknown domains fall back to the generalists (DC + PKO) — never
/// force a domain ontology where it doesn't fit.
pub fn select_ontology_anchor(domain: &str) -> OntologyAnchor {
    let lower = domain.trim().to_lowercase();
    // Helper: does the hint start with the keyword, or contain it as a
    // token (preceded by `_` or space)? This matches both "finance" (exact)
    // and "company_profile" / "stock_screener" (tool-style) without
    // substring false positives ("logistics" does not match "log").
    let matches_kw = |kw: &str| -> bool {
        lower == kw
            || lower.starts_with(kw)
            || lower.contains(&format!("_{kw}"))
            || lower.contains(&format!(" {kw}"))
    };
    // Statistical data → SDMX (FRED, DBnomics, World Bank).
    if [
        "economic",
        "fred",
        "dbnomics",
        "worldbank",
        "world_bank",
        "world bank",
        "indicator",
        "timeseries",
        "time_series",
    ]
    .iter()
    .any(|kw| matches_kw(kw))
    {
        return OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Sdmx,
            concept: sdmx::DATASET.to_string(),
        };
    }
    // Financial / company analysis → FIBO.
    if [
        "finance",
        "financial",
        "company",
        "companies",
        "stock",
        "portfolio",
        "dcf",
        "screener",
        "forecast",
        "scenario",
        "prediction-markets",
        "prediction_markets",
        "prediction markets",
    ]
    .iter()
    .any(|kw| matches_kw(kw))
    {
        return OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Fibo,
            concept: dc_bibo::DATASET.to_string(),
        };
    }
    // Scientific reasoning → SEPIO.
    if [
        "science",
        "scientific",
        "research",
        "hypothesis",
        "evidence",
    ]
    .iter()
    .any(|kw| matches_kw(kw))
    {
        return OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Sepio,
            concept: dc_bibo::TEXT.to_string(),
        };
    }
    // Narrative / literature → GOLEM.
    if [
        "narrative",
        "literature",
        "persona",
        "author",
        "corpus",
        // "replica" removed — persona/replica system deleted
    ]
    .iter()
    .any(|kw| matches_kw(kw))
    {
        return OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Golem,
            concept: golem::WORK.to_string(),
        };
    }
    // ML training → ML-Schema.
    if ["training", "ml", "adapter", "sweep", "lora"]
        .iter()
        .any(|kw| matches_kw(kw))
    {
        return OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::MlSchema,
            concept: dc_bibo::DATASET.to_string(),
        };
    }
    // Process workflows → PKO dual-axis.
    if [
        "kanban",
        "board",
        "task",
        "spec",
        "skill",
        "docproc",
        "curator",
        "kata",
        "condenser",
    ]
    .iter()
    .any(|kw| matches_kw(kw))
    {
        return OntologyAnchor::DualAxis {
            axis: OntologyAxis::Pko,
            concept: pko::PROCEDURE.to_string(),
        };
    }
    // Entity metadata → DC+BIBO dual-axis.
    if ["file", "web", "registry", "wallet"]
        .iter()
        .any(|kw| matches_kw(kw))
    {
        return OntologyAnchor::DualAxis {
            axis: OntologyAxis::DcBibo,
            concept: dc_bibo::TEXT.to_string(),
        };
    }
    // Unknown → SUMO upper ontology (the universal fallback). SUMO provides
    // formal categorization (Entity, Process, Object, Agent) beyond the bare
    // 5W1H interrogative ground, so unknown domains get a real ontology anchor
    // rather than a no-op. The 5W1H core remains the tier for artifacts that
    // genuinely have no domain hint (the `Core` variant is still reachable
    // when `domain` is empty).
    if lower.is_empty() {
        return OntologyAnchor::Core;
    }
    OntologyAnchor::DomainSupplement {
        namespace: OntologyNamespace::Sumo,
        concept: sumo::ENTITY.to_string(),
    }
}
