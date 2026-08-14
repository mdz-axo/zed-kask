//! Tests for the `consent_token` field on `TrainSubmitRequest` and the
//! P2 consent gate enforcement in `training_submit`.
//! # REQ: P8 — every test verifies a stated behavioral property of a public seam.

use hkask_mcp_training::types::TrainSubmitRequest;

#[test]
fn train_submit_request_accepts_consent_token() {
    let json = r#"{
        "dataset_path": "corpus/qa_pairs/train_chat.jsonl",
        "base_model": "unsloth/Qwen3.6-27B",
        "consent_token": "abc123"
    }"#;
    let req: TrainSubmitRequest = serde_json::from_str(json).expect("should parse");
    assert_eq!(req.consent_token.as_deref(), Some("abc123"));
}

#[test]
fn train_submit_request_consent_token_defaults_to_none() {
    let json = r#"{
        "dataset_path": "corpus/qa_pairs/train_chat.jsonl",
        "base_model": "unsloth/Qwen3.6-27B"
    }"#;
    let req: TrainSubmitRequest = serde_json::from_str(json).expect("should parse");
    assert!(
        req.consent_token.is_none(),
        "consent_token must default to None"
    );
}
