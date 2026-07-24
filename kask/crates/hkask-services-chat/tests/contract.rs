use hkask_services_chat::chat::{ChatTurnResponse, TokenUsage};
use hkask_types::StructuredToolCall;
use proptest::prelude::*;

proptest! {
    #[test]
    fn token_usage_total_equals_prompt_plus_completion(
        prompt in 0u32..1_000_000,
        completion in 0u32..1_000_000,
    ) {
        let usage = TokenUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
        };
        prop_assert_eq!(usage.total_tokens, usage.prompt_tokens + usage.completion_tokens);
    }
}

#[test]
fn token_usage_zero() {
    let u = TokenUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    };
    assert_eq!(u.total_tokens, 0);
}

#[test]
fn token_usage_nonzero() {
    let u = TokenUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    assert_eq!(u.prompt_tokens, 100);
    assert_eq!(u.completion_tokens, 50);
    assert_eq!(u.total_tokens, 150);
}

#[test]
fn chat_response_serde_roundtrip() {
    let resp = ChatTurnResponse {
        text: "Hello, world!".into(),
        usage: Some(TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
        }),
        finish_reason: "stop".into(),
        tool_calls: vec![],
        reasoning: None,
        messages: vec![],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let rt: ChatTurnResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.text, "Hello, world!");
    assert_eq!(rt.usage.unwrap().total_tokens, 15);
    assert_eq!(rt.finish_reason, "stop");
}

#[test]
fn chat_response_minimal_serde() {
    let resp = ChatTurnResponse {
        text: "ok".into(),
        usage: None,
        finish_reason: "length".into(),
        tool_calls: vec![],
        reasoning: None,
        messages: vec![],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let rt: ChatTurnResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.text, "ok");
    assert!(rt.usage.is_none());
}

#[test]
fn structured_tool_call_serde() {
    let tc = StructuredToolCall {
        call_id: Some("call_1".into()),
        server: String::new(),
        tool: "search".into(),
        args: serde_json::json!({"q": "hello"}),
    };
    let json = serde_json::to_string(&tc).unwrap();
    let rt: StructuredToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(rt.call_id.unwrap(), "call_1");
    assert_eq!(rt.tool, "search");
}
