//! ML-Schema ontology bridge for hkask-mcp-training.
//!
//! Maps hKask training server concepts to ML-Schema (W3C Community Group)
//! standard concepts for machine learning experiments. ML-Schema is the
//! closest thing to a FIBO-equivalent for ML training, though it has less
//! adoption than FIBO — this bridge is provisional and may be upgraded.
//!
//! Reference: <https://www.w3.org/community/ml-schema/>
//! Reference: <https://ml-schema.github.io/documentation/ML%20Schema.html>
//!
//! Pattern: thin mapping layer — canonical URI constants, mapping functions,
//! no dependencies, no reasoners, no overhead ≤100 lines.
//!
//! # Shared Bridge Integration
//!
//! Uses [`hkask_bridge_dublincore`] for dataset and model metadata
//! (e.g., `dctypes:Dataset`, `dctypes:Software`) and [`hkask_bridge_dublincore`]
//! for training procedure classification.

/// An ML-Schema concept URI.
pub type MlConcept = &'static str;

// ── Core ML concepts ──────────────────────────────────────────────────────

/// A machine learning model — the trained artifact.
/// hKask mapping: base model, fine-tuned model, LoRA adapter
pub const MODEL: MlConcept = "mls:Model";

/// A training or evaluation run — one execution of an ML workflow.
/// hKask mapping: training_submit job, training_sweep run
pub const RUN: MlConcept = "mls:Run";

/// A dataset used for training or evaluation.
/// hKask mapping: assembled ChatML JSONL, ingested datasets
pub const DATA: MlConcept = "mls:Data";

// ── Hyperparameters ───────────────────────────────────────────────────────

/// A hyperparameter definition.
pub const HYPER_PARAMETER: MlConcept = "mls:HyperParameter";

/// A specific hyperparameter value setting for a Run.
pub const HYPER_PARAMETER_SETTING: MlConcept = "mls:HyperParameterSetting";

// ── Evaluation ────────────────────────────────────────────────────────────

/// An evaluation of a Model's performance.
/// hKask mapping: training_evaluate results
pub const EVALUATION: MlConcept = "mls:Evaluation";

/// A specific metric measured during evaluation.
/// hKask mapping: accuracy, substring match, semantic comparison scores
pub const EVALUATION_MEASURE: MlConcept = "mls:EvaluationMeasure";

// ── Model derivation ──────────────────────────────────────────────────────

/// A Model was derived from another Model.
/// hKask mapping: LoRA adapter derived from base model
pub const WAS_DERIVED_FROM: MlConcept = "mls:wasDerivedFrom";

/// A Run used a specific Model.
pub const IMPLEMENTED_BY: MlConcept = "mls:implementedBy";

/// A Run used specific Data.
pub const HAS_DATA: MlConcept = "mls:hasData";

// ── Mapping helpers ───────────────────────────────────────────────────────

/// Map a training server operation to its ML-Schema concept.
pub fn training_op_to_mlschema(op: &str) -> Option<MlConcept> {
    match op {
        "training_submit" => Some(RUN),
        "training_assemble_dataset" => Some(DATA),
        "training_ingest_dataset" => Some(DATA),
        "training_ingest_qa" => Some(DATA),
        "training_evaluate" => Some(EVALUATION),
        "training_validate_config" => Some(EVALUATION),
        _ => None,
    }
}

/// Map a hyperparameter name to its ML-Schema concept.
pub fn hyperparam_to_mlschema(param: &str) -> Option<MlConcept> {
    match param.to_lowercase().as_str() {
        "learning_rate" | "lr" => Some(HYPER_PARAMETER),
        "lora_rank" | "rank" | "r" => Some(HYPER_PARAMETER),
        "lora_alpha" | "alpha" => Some(HYPER_PARAMETER),
        "batch_size" => Some(HYPER_PARAMETER),
        "epochs" | "num_epochs" => Some(HYPER_PARAMETER),
        "weight_decay" => Some(HYPER_PARAMETER),
        "warmup_steps" => Some(HYPER_PARAMETER),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn training_ops_map_to_mlschema() {
        assert_eq!(training_op_to_mlschema("training_submit"), Some(RUN));
        assert_eq!(
            training_op_to_mlschema("training_evaluate"),
            Some(EVALUATION)
        );
        assert_eq!(
            training_op_to_mlschema("training_assemble_dataset"),
            Some(DATA)
        );
        assert_eq!(
            training_op_to_mlschema("training_validate_config"),
            Some(EVALUATION)
        );
        assert_eq!(training_op_to_mlschema("unknown"), None);
    }

    #[test]
    fn hyperparams_map_to_mlschema() {
        assert_eq!(
            hyperparam_to_mlschema("learning_rate"),
            Some(HYPER_PARAMETER)
        );
        assert_eq!(hyperparam_to_mlschema("batch_size"), Some(HYPER_PARAMETER));
        assert_eq!(hyperparam_to_mlschema("lora_rank"), Some(HYPER_PARAMETER));
        assert_eq!(hyperparam_to_mlschema("random_seed"), None);
    }
}
