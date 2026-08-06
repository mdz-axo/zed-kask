//! ML-Schema dispatch for the training server — operation & hyperparameter mapping.
//!
//! The ML-Schema concept vocabulary (the canonical `mls:*` URIs) lives in
//! the shared `hkask-bridge-ontology` crate. This module re-exports those
//! constants so the training server's existing `mlschema::CONSTANT` call
//! sites keep working, and holds the server-specific mapping from training
//! operation names and hyperparameter names to their ML-Schema concepts.
//! That mapping is the server's business, not the ontology's.

// Re-export the ML-Schema vocabulary from the shared bridge crate.
pub use hkask_bridge_ontology::mlschema::MlConcept;
pub use hkask_bridge_ontology::mlschema::{
    ALL_CONCEPTS, DATA, EVALUATION, EVALUATION_MEASURE, HAS_DATA, HYPER_PARAMETER,
    HYPER_PARAMETER_SETTING, IMPLEMENTED_BY, MODEL, RUN, WAS_DERIVED_FROM,
};

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
