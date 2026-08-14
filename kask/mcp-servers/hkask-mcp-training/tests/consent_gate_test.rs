//! Tests for the `confirmed` field on `TrainSubmitRequest` and the
//! P2 consent gate enforcement in `training_submit`.
//! # REQ: P8 — every test verifies a stated behavioral property of a public seam.

use hkask_mcp_training::types::TrainSubmitRequest;

#[test]
fn train_submit_request_accepts_confirmed_true() {
    let json = r#"{
        "dataset_path": "corpus/qa_pairs/train_chat.jsonl",
        "base_model": "unsloth/Qwen3.6-27B",
        "confirmed": true
    }"#;
    let req: TrainSubmitRequest = serde_json::from_str(json).expect("should parse");
    assert!(req.confirmed, "confirmed must be true");
}

#[test]
fn train_submit_request_confirmed_defaults_to_false() {
    let json = r#"{
        "dataset_path": "corpus/qa_pairs/train_chat.jsonl",
        "base_model": "unsloth/Qwen3.6-27B"
    }"#;
    let req: TrainSubmitRequest = serde_json::from_str(json).expect("should parse");
    assert!(
        !req.confirmed,
        "confirmed must default to false — consent gate blocks by default"
    );
}
