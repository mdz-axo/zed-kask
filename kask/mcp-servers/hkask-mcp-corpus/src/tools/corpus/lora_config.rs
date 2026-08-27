//! LoRA/PEFT config recommendation heuristic for `corpus_prepare_training_dataset`.
//!
//! **This is a preview heuristic, not a substitute for the lora-training skill.**
//! The skill (`lora-training/select-method`) runs a full 8-gate refinement
//! (G0, G-D0, G1–G6) with operator-accept/override/reject and runtime contract
//! enforcement. This inline function models only 5 gates (G1–G5) with fixed
//! defaults so `corpus_prepare_training_dataset` can return a ready-to-use PEFT
//! config without invoking the `lora-training` skill. The operator should invoke the
//! `lora-training` skill for the authoritative recommendation before training.
//!
//! Drift hazard: keep the G1–G5 logic here aligned with the skill's gate
//! definitions (see `.agents/skills/lora-training/SKILL.md`). The skill is the
//! canonical source; this is a simplified preview. If the skill's gates change,
//! update this function in the same PR.
//!
//! The five gates modeled here:
//! - G1 inference: LoRA-family (must-merge) — fixed.
//! - G2 memory: model_size × 2 (bf16) > 24GB ⇒ QLoRA (NF4).
//! - G3 task distance: QA pairs from corpus are "moderate" (new domain knowledge).
//! - G4 quality/cost: default LoRA (not DoRA/PiSSA) with PEFT default init.
//! - G5 knowledge preservation: not required for new domain adaptation.

use serde_json::{Value, json};

/// Build a PEFT config recommendation for a base model and dataset size.
///
/// `model_name` is matched case-insensitively for size tokens (`1b`, `3b`, `7b`,
/// `13b`, `30b`, `70b`, …); unknown models default to 8B. `n_samples` drives the
/// G3 rank heuristic (`<1000 ⇒ r=16`, else `r=32`).
///
/// Returns the full `config_recommendation` JSON object emitted by
/// `corpus_prepare_training_dataset`. Callers that need individual gate outputs
/// (`use_qlora`, `recommended_r`) for tracing should read them off the returned
/// object's `model_size_b` / `use_qlora` / `lora.r` fields.
pub(crate) fn build_lora_config(model_name: &str, n_samples: usize) -> Value {
    let lower = model_name.to_lowercase();
    let model_size_b: u32 = if ["1b", "3b"].iter().any(|p| lower.contains(p)) {
        1
    } else if ["7b", "8b", "9b"].iter().any(|p| lower.contains(p)) {
        8
    } else if ["13b", "14b"].iter().any(|p| lower.contains(p)) {
        14
    } else if ["30b", "34b", "35b"].iter().any(|p| lower.contains(p)) {
        35
    } else if ["70b", "72b"].iter().any(|p| lower.contains(p)) {
        70
    } else {
        8 // default
    };

    // G2: Memory budget — model_size × 2 (bf16) > 24GB → QLoRA
    let use_qlora = (model_size_b * 2) > 24;

    // G3: Task distance — QA pairs from corpus are "moderate" (new domain knowledge)
    let recommended_r = if n_samples < 1000 { 16 } else { 32 };
    let recommended_alpha = recommended_r * 2;

    // G4: Quality/cost — default LoRA (not DoRA/PiSSA)
    let recommended_init = "true"; // PEFT default

    // G5: Knowledge preservation — not required for new domain adaptation
    let recommended_use_rslora = recommended_r > 64;

    json!({
        "base_model": model_name,
        "model_size_b": model_size_b,
        "use_qlora": use_qlora,
        "lora": {
            "r": recommended_r,
            "alpha": recommended_alpha,
            "dropout": 0.0,
            "target_modules": ["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"],
            "use_rslora": recommended_use_rslora,
            "use_dora": false,
            "init_lora_weights": recommended_init,
            "bias": "none"
        },
        "quantization": if use_qlora {
            json!({
                "load_in_4bit": true,
                "bnb_4bit_quant_type": "nf4",
                "bnb_4bit_compute_dtype": "bf16",
                "bnb_4bit_use_double_quant": true
            })
        } else {
            json!({"load_in_4bit": false})
        },
        "optimization": {
            "optimizer": if use_qlora { "paged_adamw_8bit" } else { "adamw_torch" },
            "lr_scheduler": "cosine",
            "gradient_accumulation_steps": 1
        },
        "advanced": {
            "bf16": true,
            "gradient_checkpointing": "true"
        },
        "gate_decisions": {
            "G1_inference": "must-merge (LoRA-family)",
            "G2_memory": if use_qlora { "QLoRA (NF4)" } else { "LoRA (bf16)" },
            "G3_task_distance": "moderate (new domain knowledge)",
            "G4_quality_cost": "default (LoRA with PEFT default init)",
            "G5_knowledge_preservation": "not required"
        }
    })
}
