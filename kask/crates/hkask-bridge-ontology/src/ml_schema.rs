//! ML-Schema ontology vocabulary bridge.
//!
//! Canonical concept URIs for machine-learning experiments — models, runs,
//! datasets, hyperparameters, evaluations. ML-Schema is the W3C Community
//! Group standard for ML experiments; this bridge is provisional and may be
//! upgraded as adoption grows.
//!
//! Reference: <https://www.w3.org/community/ml-schema/>
//! Reference: <https://ml-schema.github.io/documentation/ML%20Schema.html>
//! (namespace `http://www.w3.org/ns/mls#`)
//!
//! Every term is checked against the complete, checksum-pinned published
//! ML-Schema 1.0 vocabulary loaded in `published`.
//! Note: ML-Schema publishes no `wasDerivedFrom` property (that is PROV-O);
//! derivation is modeled with `mls:hasOutput`.
//!
//! This module holds the ML-Schema concept vocabulary only. Server-specific
//! dispatch (mapping a training operation or hyperparameter name to its
//! ML-Schema concept) lives in the training server.

/// An ML-Schema concept URI.
pub type MlConcept = &'static str;

// ── Core ML concepts ──────────────────────────────────────────────────────

/// A machine learning model — the trained artifact.
pub const MODEL: MlConcept = "mls:Model";
/// A training or evaluation run — one execution of an ML workflow.
pub const RUN: MlConcept = "mls:Run";
/// A dataset used for training or evaluation.
pub const DATA: MlConcept = "mls:Data";

// ── Hyperparameters ───────────────────────────────────────────────────────

/// A hyperparameter definition.
pub const HYPER_PARAMETER: MlConcept = "mls:HyperParameter";
/// A specific hyperparameter value setting for a Run.
pub const HYPER_PARAMETER_SETTING: MlConcept = "mls:HyperParameterSetting";

// ── Evaluation ────────────────────────────────────────────────────────────

/// An evaluation of a Model's performance.
pub const MODEL_EVALUATION: MlConcept = "mls:ModelEvaluation";
/// A specific metric measured during evaluation.
pub const EVALUATION_MEASURE: MlConcept = "mls:EvaluationMeasure";

// ── Run relations ─────────────────────────────────────────────────────────

/// A Run's input data.
pub const HAS_INPUT: MlConcept = "mls:hasInput";
/// A Run's output (e.g. a produced Model).
pub const HAS_OUTPUT: MlConcept = "mls:hasOutput";
/// An Implementation implements an Algorithm.
pub const IMPLEMENTS: MlConcept = "mls:implements";

/// Bridge constants checked against the full published vocabulary.
#[cfg(test)]
const ALL_CONCEPTS: &[MlConcept] = &[
    MODEL,
    RUN,
    DATA,
    HYPER_PARAMETER,
    HYPER_PARAMETER_SETTING,
    MODEL_EVALUATION,
    EVALUATION_MEASURE,
    HAS_INPUT,
    HAS_OUTPUT,
    IMPLEMENTS,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabrication guard: every bridge constant must occur in the pinned
    /// published ML-Schema source.
    #[test]
    fn all_terms_are_official() {
        for term in ALL_CONCEPTS {
            assert!(
                crate::published::contains(term),
                "{term} missing from ML-Schema 1.0"
            );
        }
        let published = crate::published::terms()
            .iter()
            .filter(|term| term.namespace == "ML-Schema")
            .count();
        assert!(
            published > ALL_CONCEPTS.len(),
            "full ML-Schema vocabulary: {published}"
        );
        let run = crate::term_resolution::resolve_term("mls:Run");
        assert_eq!(run.concept, RUN);
        assert_eq!(run.namespace, "ML-Schema");
        assert!(
            run.definition
                .as_deref()
                .is_some_and(|text| text.starts_with("Run is an execution"))
        );
        assert!(
            run.source
                .as_deref()
                .is_some_and(|source| source.contains("ML-Schema 1.0"))
        );
    }
}
