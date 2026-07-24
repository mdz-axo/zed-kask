//! Integration tests for the ACP userpod protocol.

use futures_util::Stream;
use hkask_acp::HkaskAcpAgent;
use hkask_acp::main_impl::protocol::StdioTransport;
use hkask_types::InferencePort;
use hkask_types::inference_port::InferenceStreamChunk;
use hkask_types::inference_types::{
    ChatToolDefinition, InferenceError, InferenceResult, InferenceUsage,
};
use hkask_types::template::LLMParameters;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};

struct MockInferencePort {
    chunks: Vec<InferenceStreamChunk>,
    model: String,
}

impl MockInferencePort {
    fn new(model: &str, chunks: Vec<InferenceStreamChunk>) -> Self {
        Self {
            chunks,
            model: model.to_string(),
        }
    }
}

impl InferencePort for MockInferencePort {
    fn generate(
        &self,
        _p: &str,
        _params: &LLMParameters,
        _tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        Box::pin(async {
            Ok(InferenceResult {
                text: self.chunks.iter().map(|c| c.text_delta.as_str()).collect(),
                model: self.model.clone(),
                usage: InferenceUsage {
                    prompt_tokens: 10,
                    completion_tokens: 50,
                    total_tokens: 60,
                },
                finish_reason: "stop".into(),
                token_probabilities: None,
                tool_calls: vec![],
                reasoning: None,
            })
        })
    }

    fn generate_stream(
        &self,
        _p: &str,
        _params: &LLMParameters,
        _tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + '_>> {
        let chunks = self.chunks.clone();
        Box::pin(futures_util::stream::iter(chunks.into_iter().map(Ok)))
    }
}

fn make_chunk(text: &str) -> InferenceStreamChunk {
    InferenceStreamChunk {
        text_delta: text.into(),
        reasoning_delta: String::new(),
        model: "test".into(),
        finish_reason: None,
        usage: None,
        tool_calls: vec![],
    }
}

fn make_final_chunk(text: &str, finish: &str, tokens: u32) -> InferenceStreamChunk {
    InferenceStreamChunk {
        text_delta: text.into(),
        reasoning_delta: String::new(),
        model: "test".into(),
        finish_reason: Some(finish.into()),
        usage: Some(InferenceUsage {
            prompt_tokens: 10,
            completion_tokens: tokens,
            total_tokens: 10 + tokens,
        }),
        tool_calls: vec![],
    }
}

async fn read_responses<R: AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    count: usize,
    timeout_ms: u64,
) -> Vec<serde_json::Value> {
    let mut results = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    while results.len() < count {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let mut line = String::new();
        match tokio::time::timeout(remaining, reader.read_line(&mut line)).await {
            Ok(Ok(_)) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    results.push(val);
                }
            }
            _ => break,
        }
    }
    results
}

#[tokio::test]
async fn test_initialize() {
    let inference: Arc<dyn InferencePort> = Arc::new(MockInferencePort::new("test", vec![]));
    let agent = Arc::new(HkaskAcpAgent::for_testing(inference));

    let (test_side, server_side) = tokio::io::duplex(4096);
    let (server_read, mut server_write) = tokio::io::split(server_side);

    let handle = tokio::spawn(async move {
        StdioTransport::new()
            .serve_with_streams(agent, server_read, &mut server_write)
            .await
            .unwrap();
    });

    let (test_read, mut test_write) = tokio::io::split(test_side);

    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":1,"clientInfo":{"name":"test","version":"1.0"}}});
    let mut bytes = serde_json::to_string(&req).unwrap();
    bytes.push('\n');
    test_write.write_all(bytes.as_bytes()).await.unwrap();

    let mut reader = BufReader::new(test_read);
    let responses = read_responses(&mut reader, 1, 5000).await;

    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["jsonrpc"], "2.0");
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["protocolVersion"], 1);
    assert_eq!(responses[0]["result"]["agentInfo"]["name"], "hkask-acp");

    handle.abort();
}

#[tokio::test]
async fn test_session_new_and_prompt_streaming() {
    let chunks = vec![
        make_chunk("Hello"),
        make_chunk(", "),
        make_chunk("world"),
        make_final_chunk("!", "stop", 4),
    ];
    let inference: Arc<dyn InferencePort> = Arc::new(MockInferencePort::new("test", chunks));
    let agent = Arc::new(HkaskAcpAgent::for_testing(inference));

    let (test_side, server_side) = tokio::io::duplex(4096);
    let (server_read, mut server_write) = tokio::io::split(server_side);

    let handle = tokio::spawn(async move {
        StdioTransport::new()
            .serve_with_streams(agent, server_read, &mut server_write)
            .await
            .unwrap();
    });

    let (test_read, mut test_write) = tokio::io::split(test_side);
    let mut reader = BufReader::new(test_read);

    // Create session
    let req =
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":"/tmp"}});
    let mut bytes = serde_json::to_string(&req).unwrap();
    bytes.push('\n');
    test_write.write_all(bytes.as_bytes()).await.unwrap();

    let responses = read_responses(&mut reader, 1, 5000).await;
    assert_eq!(responses.len(), 1);
    let sid = responses[0]["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_string();

    // Send prompt
    let req = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"session/prompt",
        "params":{"sessionId":sid,"prompt":[{"type":"text","text":"hi"}]}});
    let mut bytes = serde_json::to_string(&req).unwrap();
    bytes.push('\n');
    test_write.write_all(bytes.as_bytes()).await.unwrap();

    // Read streaming notifications + final response
    let responses = read_responses(&mut reader, 10, 5000).await;
    assert!(!responses.is_empty());

    let last = responses.last().unwrap();
    assert_eq!(last["id"], 2);
    assert_eq!(last["result"]["stopReason"], "end_turn");

    let has_chunks = responses.iter().any(|r| {
        r["method"] == "session/update"
            && r["params"]["update"]["sessionUpdate"] == "agent_message_chunk"
    });
    assert!(has_chunks, "Expected agent_message_chunk notifications");

    let has_usage = responses.iter().any(|r| {
        r["method"] == "session/update" && r["params"]["update"]["sessionUpdate"] == "usage_update"
    });
    assert!(has_usage, "Expected usage_update notification");

    handle.abort();
}

#[tokio::test]
async fn test_empty_prompt_returns_end_turn() {
    let inference: Arc<dyn InferencePort> = Arc::new(MockInferencePort::new("test", vec![]));
    let agent = Arc::new(HkaskAcpAgent::for_testing(inference));

    let (test_side, server_side) = tokio::io::duplex(4096);
    let (server_read, mut server_write) = tokio::io::split(server_side);

    let handle = tokio::spawn(async move {
        StdioTransport::new()
            .serve_with_streams(agent, server_read, &mut server_write)
            .await
            .unwrap();
    });

    let (test_read, mut test_write) = tokio::io::split(test_side);
    let mut reader = BufReader::new(test_read);

    let req =
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":"/tmp"}});
    let mut bytes = serde_json::to_string(&req).unwrap();
    bytes.push('\n');
    test_write.write_all(bytes.as_bytes()).await.unwrap();
    let _ = read_responses(&mut reader, 1, 5000).await;

    let req = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"session/prompt",
        "params":{"sessionId":"any","prompt":[]}});
    let mut bytes = serde_json::to_string(&req).unwrap();
    bytes.push('\n');
    test_write.write_all(bytes.as_bytes()).await.unwrap();

    let responses = read_responses(&mut reader, 1, 5000).await;
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["result"]["stopReason"], "end_turn");

    handle.abort();
}

#[tokio::test]
async fn test_unknown_method_returns_error() {
    let inference: Arc<dyn InferencePort> = Arc::new(MockInferencePort::new("test", vec![]));
    let agent = Arc::new(HkaskAcpAgent::for_testing(inference));

    let (test_side, server_side) = tokio::io::duplex(4096);
    let (server_read, mut server_write) = tokio::io::split(server_side);

    let handle = tokio::spawn(async move {
        StdioTransport::new()
            .serve_with_streams(agent, server_read, &mut server_write)
            .await
            .unwrap();
    });

    let (test_read, mut test_write) = tokio::io::split(test_side);
    let mut reader = BufReader::new(test_read);

    let req = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"nonexistent","params":{}});
    let mut bytes = serde_json::to_string(&req).unwrap();
    bytes.push('\n');
    test_write.write_all(bytes.as_bytes()).await.unwrap();

    let responses = read_responses(&mut reader, 1, 5000).await;
    assert_eq!(responses.len(), 1);
    assert!(responses[0]["error"].is_object());
    assert_eq!(responses[0]["error"]["code"], -32601);

    handle.abort();
}
