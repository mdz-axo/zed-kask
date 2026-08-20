//! ML-Schema ontology vocabulary bridge.
//!
//! Canonical concept URIs for machine-learning experiments — models, runs,
//! datasets, hyperparameters, evaluations. ML-Schema is the W3C Community
//! Group standard for ML experiments; this bridge is provisional and may be
//! upgraded as adoption grows.
//!
//! Reference: <https://www.w3.org/community/ml-schema/>
//! Reference: <https://ml-schema.github.io/documentation/ML%20Schema.html>
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
pub const EVALUATION: MlConcept = "mls:Evaluation";
/// A specific metric measured during evaluation.
pub const EVALUATION_MEASURE: MlConcept = "mls:EvaluationMeasure";

// ── Model derivation ──────────────────────────────────────────────────────

/// A Model was derived from another Model.
pub const WAS_DERIVED_FROM: MlConcept = "mls:wasDerivedFrom";
/// A Run used a specific Model.
pub const IMPLEMENTED_BY: MlConcept = "mls:implementedBy";
/// A Run used specific Data.
pub const HAS_DATA: MlConcept = "mls:hasData";

/// All ML-Schema concepts, for validation or iteration.
pub const ALL_CONCEPTS: &[MlConcept] = &[
    MODEL,
    RUN,
    DATA,
    HYPER_PARAMETER,
    HYPER_PARAMETER_SETTING,
    EVALUATION,
    EVALUATION_MEASURE,
    WAS_DERIVED_FROM,
    IMPLEMENTED_BY,
    HAS_DATA,
];
