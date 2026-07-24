//! End-to-end test with real inference (DeepInfra).
//!
//! Requires: running DeepInfra with API key.
//! Run with: cargo test -p hkask-acp --test e2e_inference -- --ignored --nocapture

use hkask_acp::HkaskAcpAgent;
use hkask_acp::main_impl::protocol::StdioTransport;
use hkask_inference::{InferenceConfig, InferenceRouter};
use hkask_types::InferencePort;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};

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
#[ignore = "requires running DeepInfra"]
async fn e2e_real_inference_streaming() {
    let config = InferenceConfig::from_env();
    let router = Arc::new(InferenceRouter::new(config));
    let agent = Arc::new(
        HkaskAcpAgent::for_testing(router as Arc<dyn InferencePort>)
            .with_model(hkask_inference::model_constants::TEST_MODEL_SMALL),
    );

    let (test_side, server_side) = tokio::io::duplex(65536);
    let (server_read, mut server_write) = tokio::io::split(server_side);

    let handle = tokio::spawn(async move {
        StdioTransport::new()
            .serve_with_streams(agent, server_read, &mut server_write)
            .await
            .unwrap();
    });

    let (test_read, mut test_write) = tokio::io::split(test_side);
    let mut reader = BufReader::new(test_read);

    // Initialize
    let init = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize",
        "params":{"protocolVersion":1,"clientInfo":{"name":"e2e","version":"1.0"}}});
    let mut bytes = serde_json::to_string(&init).unwrap();
    bytes.push('\n');
    test_write.write_all(bytes.as_bytes()).await.unwrap();

    let responses = read_responses(&mut reader, 1, 10000).await;
    assert_eq!(responses.len(), 1);
    eprintln!(
        "✅ initialize — agent: {}",
        responses[0]["result"]["agentInfo"]["title"]
    );

    // Create session
    let sess =
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp"}});
    let mut bytes = serde_json::to_string(&sess).unwrap();
    bytes.push('\n');
    test_write.write_all(bytes.as_bytes()).await.unwrap();

    let responses = read_responses(&mut reader, 1, 10000).await;
    let sid = responses[0]["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_string();
    eprintln!("✅ session/new → {}", sid);

    // Send prompt
    let prompt = serde_json::json!({"jsonrpc":"2.0","id":3,"method":"session/prompt",
        "params":{"sessionId":sid,"prompt":[{"type":"text","text":"Reply with exactly three words: hello from hkask"}]}});
    let mut bytes = serde_json::to_string(&prompt).unwrap();
    bytes.push('\n');
    test_write.write_all(bytes.as_bytes()).await.unwrap();

    let mut chunks = 0;
    let mut usage = 0;
    let mut final_response = false;
    let responses = read_responses(&mut reader, 30, 60000).await;
    assert!(!responses.is_empty(), "No responses from inference");

    for r in &responses {
        if r["method"] == "session/update" {
            let update = &r["params"]["update"]["sessionUpdate"];
            if update == "agent_message_chunk" {
                chunks += 1;
                eprint!(
                    "{}",
                    r["params"]["update"]["content"]["text"]
                        .as_str()
                        .unwrap_or("?")
                );
            } else if update == "usage_update" {
                usage += 1;
            }
        }
        if r["id"] == 3 {
            assert_eq!(r["result"]["stopReason"], "end_turn");
            final_response = true;
        }
    }
    eprintln!();
    assert!(chunks > 0, "No agent_message_chunk — streaming failed");
    assert!(usage > 0, "No usage_update");
    assert!(final_response, "No session/prompt response");
    eprintln!(
        "✅ Real inference: {} chunks, {} usage updates",
        chunks, usage
    );

    handle.abort();
}
