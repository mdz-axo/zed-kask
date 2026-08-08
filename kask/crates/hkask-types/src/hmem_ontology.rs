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
//!   GOLEM, OMC, ESO, ML-Schema, SUMO) — adding a new ontology doesn't
//!   require a schema change

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::Dimension;

/// Dual-axis ontological anchoring for an h_mem (P5.4).
///
/// Serialized as a JSON blob in the `ontology` column of the `hmems` table.
/// Queryable via `json_extract(ontology, '$.dc_type')` etc. The open-world
/// `ontology_tags` map lets domain ontologies annotate h_mems without schema
/// changes — the same pattern as `corpus::TaggedChunk::ontology_tags`.
///
/// Semantic h_mems (facts) anchor primarily to the state axis (DC+BIBO):
/// `dc_type`, `dc_subject`, `dc_source`. Episodic h_mems (experiences) anchor
/// primarily to the process axis (PKO): `pko_procedure`, `pko_step`. Both
/// carry 5W1H dimensions as universal ground.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HMemOntology {
    /// 5W1H interrogatory dimensions (Who/What/When/Where/Why/How).
    /// Universal ground — every h_mem answers at least one interrogative (P5.2).
    /// Stored as strings so the open-world set isn't constrained to the enum;
    /// `Dimension::as_str()` produces the canonical lowercase form.
    #[serde(default)]
    pub dimensions: Vec<String>,

    // ── State axis: Dublin Core + BIBO (the noun — "what is this?") ───────
    /// Dublin Core / BIBO type (e.g., "bibo:Article", "dcterms:Dataset",
    /// "pko:StepExecution"). The state-axis type identity of this h_mem.
    #[serde(default)]
    pub dc_type: String,

    /// Dublin Core subject — the concepts as ontology terms. The state-axis
    /// subject classification. For semantic facts, this is the topic; for
    /// episodic experiences, this is what the experience was about.
    #[serde(default)]
    pub dc_subject: Vec<String>,

    /// Dublin Core source — provenance. Where this h_mem came from: a file,
    /// a conversation, a tool invocation, a corpus chunk. The state-axis
    /// provenance anchor.
    #[serde(default)]
    pub dc_source: String,

    // ── Process axis: PKO (the verb — "how did this come to be?") ────────
    /// PKO procedure identifier — which procedure this h_mem is a step of.
    /// For episodic h_mems, this is the process the experience belongs to
    /// (e.g., "diagnose-bug-123", "corpus_ingest_qa"). `None` for semantic
    /// facts that aren't part of a procedure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko_procedure: Option<String>,

    /// PKO step identifier — which step of the procedure this h_mem is.
    /// For episodic h_mems, this is the specific step (e.g., "reproduce",
    /// "hypothesize", "fix"). `None` for semantic facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pko_step: Option<String>,

    // ── Open-world domain ontology tags ─────────────────────────────────
    /// Domain-specific ontology concepts, keyed by namespace.
    /// Examples:
    ///   {"fibo": ["competitive advantage", "ROIC"], "golem": ["metaphor"], "omc": ["scene"]}
    ///
    /// Adding a new ontology is just a new key — no struct change needed.
    /// Mirrors `corpus::TaggedChunk::ontology_tags` so h_mems and corpus
    /// chunks share the same open-world tagging substrate.
    #[serde(default)]
    pub ontology_tags: HashMap<String, Vec<String>>,
}

impl HMemOntology {
    /// Create a new empty ontology blob.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a semantic-fact ontology (state-axis anchored).
    ///
    /// Semantic h_mems are facts: Dublin Core type + subject + source, with
    /// 5W1H dimensions as universal ground. No PKO procedure/step (facts
    /// aren't steps in a process).
    pub fn semantic(
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
            ontology_tags: HashMap::new(),
        }
    }

    /// Create an episodic-experience ontology (process-axis anchored).
    ///
    /// Episodic h_mems are experiences: PKO procedure + step, with 5W1H
    /// dimensions as universal ground. The DC type defaults to
    /// `pko:StepExecution`; the DC source carries the session/provenance.
    pub fn episodic(
        pko_procedure: impl Into<String>,
        pko_step: impl Into<String>,
        dc_source: impl Into<String>,
    ) -> Self {
        Self {
            dimensions: vec![
                Dimension::How.as_str().to_string(),
                Dimension::When.as_str().to_string(),
            ],
            dc_type: "pko:StepExecution".to_string(),
            dc_subject: Vec::new(),
            dc_source: dc_source.into(),
            pko_procedure: Some(pko_procedure.into()),
            pko_step: Some(pko_step.into()),
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

    /// Add a domain ontology tag under a namespace.
    pub fn with_ontology_tag(
        mut self,
        namespace: impl Into<String>,
        concept: impl Into<String>,
    ) -> Self {
        self.ontology_tags
            .entry(namespace.into())
            .or_default()
            .push(concept.into());
        self
    }

    /// Does this ontology carry a tag from the given namespace?
    pub fn has_ontology(&self, namespace: &str) -> bool {
        self.ontology_tags.contains_key(namespace)
    }

    /// Concepts from a specific ontology namespace (empty if absent).
    pub fn ontology_concepts(&self, namespace: &str) -> &[String] {
        self.ontology_tags
            .get(namespace)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_ontology_has_what_dimension() {
        let ont = HMemOntology::semantic("bibo:Article", vec!["ROIC".to_string()], "10-K 2025");
        assert_eq!(ont.dimensions, vec!["what".to_string()]);
        assert_eq!(ont.dc_type, "bibo:Article");
        assert_eq!(ont.dc_subject, vec!["ROIC".to_string()]);
        assert_eq!(ont.dc_source, "10-K 2025");
        assert!(ont.pko_procedure.is_none());
        assert!(ont.pko_step.is_none());
    }

    #[test]
    fn episodic_ontology_has_how_and_when_dimensions() {
        let ont = HMemOntology::episodic("diagnose-bug-123", "reproduce", "session 2026-08-06");
        assert_eq!(ont.dc_type, "pko:StepExecution");
        assert_eq!(ont.pko_procedure, Some("diagnose-bug-123".to_string()));
        assert_eq!(ont.pko_step, Some("reproduce".to_string()));
        assert!(ont.dimensions.contains(&"how".to_string()));
        assert!(ont.dimensions.contains(&"when".to_string()));
    }

    #[test]
    fn open_world_tags_round_trip() {
        let ont = HMemOntology::semantic("bibo:Article", vec![], "")
            .with_ontology_tag("fibo", "competitive advantage")
            .with_ontology_tag("fibo", "ROIC")
            .with_ontology_tag("golem", "metaphor");
        assert!(ont.has_ontology("fibo"));
        assert_eq!(
            ont.ontology_concepts("fibo"),
            &["competitive advantage", "ROIC"]
        );
        assert_eq!(ont.ontology_concepts("golem"), &["metaphor"]);
        assert!(!ont.has_ontology("omc"));

        let json = ont.to_json_string().unwrap();
        let parsed = HMemOntology::from_json_str(&json).unwrap();
        assert_eq!(ont, parsed);
    }

    #[test]
    fn empty_ontology_serializes_and_parses() {
        let ont = HMemOntology::new();
        let json = ont.to_json_string().unwrap();
        let parsed = HMemOntology::from_json_str(&json).unwrap();
        assert_eq!(ont, parsed);
    }

    #[test]
    fn with_dimension_does_not_duplicate() {
        let ont = HMemOntology::new()
            .with_dimension(Dimension::What)
            .with_dimension(Dimension::What);
        assert_eq!(ont.dimensions.len(), 1);
    }
}
