//! Ontological anchoring for h_mems — the dual-axis substrate (P5.4).
//!
//! Every h_mem carries both a state identity (DC+BIBO — the noun) and a
//! process identity (PKO — the verb). This is the Planck constant at the
//! architectural level: you cannot reduce one axis to the other. The
//! `HMemOntology` blob is the first-class column that makes h_mems queryable
//! by ontology, putting them on the same substrate as corpus `TaggedChunk`s.
//!
//! Design: open-world ontology tagging (mirrors `corpus::TaggedChunk`).
//! - 5W1H dimensions are structural (every h_mem has at least one)
//! - Dublin Core + BIBO anchor the state axis (what this is)
//! - PKO anchors the process axis (how this came to be)
//! - `ontology_tags` is the open-world map for domain supplements (FIBO,
//!   GOLEM, SEPIO, ML-Schema, SUMO) — adding a new ontology doesn't
//!   require a schema change

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::Dimension;
use crate::corpus::ExpertiseLevel;

/// Dual-axis ontological anchoring for an h_mem (P5.4).
///
/// Serialized as a JSON blob in the `ontology` column of the `hmems` table.
/// Queryable via `json_extract(ontology, '$.dc_type')` etc. The open-world
/// `ontology_tags` map lets domain ontologies annotate h_mems without schema
/// changes — the same pattern as `corpus::TaggedChunk::ontology_tags`.
///
/// Every h_mem carries both a state identity (DC+BIBO — the noun) and a
/// process identity (PKO — the verb). Both axes are optional — a chat turn
/// anchors primarily to the process axis (PKO); a fact anchors primarily to
/// the state axis (DC+BIBO). The 5W1H dimensions are universal ground.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HMemOntology {
    /// 5W1H interrogatory dimensions (Who/What/When/Where/Why/How).
    /// Universal ground — every h_mem answers at least one interrogative (P5.2).
    /// Stored as strings so the open-world set isn't constrained to the enum;
    /// `Dimension::as_str()` produces the canonical lowercase form.
    #[serde(default)]
    pub dimensions: Vec<String>,

    // ── State axis: Dublin Core + BIBO (the noun — "what is this?") ───────
    /// Dublin Core / BIBO type (e.g., "bibo:Article", "dcmitype:Dataset",
    /// "pko:StepExecution"). The state-axis type identity of this h_mem.
    #[serde(default)]
    pub dc_type: String,

    /// Dublin Core subject — the concepts as ontology terms. The topic of
    /// this h_mem.
    #[serde(default)]
    pub dc_subject: Vec<String>,

    /// Dublin Core source — provenance. Where this h_mem came from: a file,
    /// a conversation, a tool invocation, a corpus chunk. The state-axis
    /// provenance anchor.
    #[serde(default)]
    pub dc_source: String,

    // ── Process axis: PKO (the verb — "how did this come to be?") ────────
    /// PKO procedure identifier — which procedure this h_mem is a step of.
    /// For a chat turn, this is the process the turn belongs to
    /// (e.g., "chat"). `None` for h_mems that aren't part of a procedure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko_procedure: Option<String>,

    /// PKO step identifier — which step of the procedure this h_mem is.
    /// For a chat turn, this is the specific step (e.g., "turn").
    /// `None` for h_mems that aren't part of a procedure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko_step: Option<String>,

    // ── Canonical ontology terms ─────────────────────────────────────────
    /// Raw descriptive terms extracted from the h_mem content. Empty means
    /// that no content classification has been performed.
    #[serde(default)]
    pub candidate_terms: Vec<String>,

    /// Protocol used to derive canonical anchors. `None` marks structural-only
    /// h_mems; it never promotes old model-assigned tags to canonical status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology_protocol: Option<String>,

    /// Expertise supported by the content. This is metadata, not an ontology
    /// namespace, so it is stored separately from published anchors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expertise_level: Option<ExpertiseLevel>,

    /// Published concepts grouped under resolver-selected namespaces.
    #[serde(default)]
    pub ontology_tags: HashMap<String, Vec<String>>,
}

impl HMemOntology {
    /// Create a process-axis anchored ontology (PKO procedure + step).
    /// Used for chat turns and other process steps.
    pub fn process(
        pko_procedure: impl Into<String>,
        pko_step: impl Into<String>,
        dc_source: impl Into<String>,
    ) -> Self {
        Self {
            dimensions: vec![
                Dimension::How.as_str().to_string(),
                Dimension::When.as_str().to_string(),
            ],
            dc_type: hkask_bridge_ontology::pko::STEP_EXECUTION.to_string(),
            dc_subject: Vec::new(),
            dc_source: dc_source.into(),
            pko_procedure: Some(pko_procedure.into()),
            pko_step: Some(pko_step.into()),
            candidate_terms: Vec::new(),
            ontology_protocol: None,
            expertise_level: None,
            ontology_tags: HashMap::new(),
        }
    }

    /// Create a state-axis anchored ontology (DC+BIBO type + subject).
    /// Used for facts and documents.
    pub fn state(
        dc_type: impl Into<String>,
        dc_subject: Vec<String>,
        dc_source: impl Into<String>,
    ) -> Self {
        Self {
            dimensions: vec![Dimension::What.as_str().to_string()],
            dc_type: dc_type.into(),
            dc_subject,
            dc_source: dc_source.into(),
            pko_procedure: None,
            pko_step: None,
            candidate_terms: Vec::new(),
            ontology_protocol: None,
            expertise_level: None,
            ontology_tags: HashMap::new(),
        }
    }

    /// Add a 5W1H dimension.
    pub fn with_dimension(mut self, d: Dimension) -> Self {
        let s = d.as_str().to_string();
        if !self.dimensions.contains(&s) {
            self.dimensions.push(s);
        }
        self
    }

    /// Add a descriptive term through the shared published-ontology resolver.
    /// Callers cannot assign a namespace or canonical URI directly.
    pub fn with_candidate_term(mut self, term: impl Into<String>) -> Self {
        self.candidate_terms.push(term.into());
        let canonical =
            hkask_bridge_ontology::term_resolution::canonicalize_terms(&self.candidate_terms);
        self.candidate_terms = canonical.candidate_terms;
        self.ontology_tags = canonical.ontology_tags;
        self.ontology_protocol =
            Some(hkask_bridge_ontology::term_resolution::TERM_RESOLUTION_PROTOCOL.to_string());
        self
    }

    /// Serialize to a JSON string for storage in the `ontology` column.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from a JSON string (the `ontology` column value).
    pub fn from_json_str(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}
