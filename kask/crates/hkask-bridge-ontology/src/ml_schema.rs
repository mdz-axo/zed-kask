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
//! Every term is verified against the ML-Schema specification —
//! `fixtures/ml-schema-terms.txt` pins the term list, and
//! `all_terms_are_official` fails the build if a term drifts from it.
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

/// All ML-Schema concepts, for validation or iteration.
pub const ALL_CONCEPTS: &[MlConcept] = &[
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

    /// Fabrication guard: every term in this module must appear in the
    /// official ML-Schema term list checked in as a fixture.
    #[test]
    fn all_terms_are_official() {
        let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/ml-schema-terms.txt");
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
                "{term} is not in the official ML-Schema term list ({fixture_path})"
            );
        }
    }
}
