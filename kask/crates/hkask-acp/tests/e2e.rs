//! End-to-end test: spawn hkask-acp binary, send real JSON-RPC, verify.
//!
//! Requires: running daemon, running DeepInfra backend, built hkask-acp binary.
//! Run with: cargo test -p hkask-acp --test e2e -- --ignored --nocapture
//! or:       cargo test -p hkask-acp --test e2e -- --include-ignored

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

const BINARY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/debug/hkask-acp");
const USERPOD: &str = "e2e-test-acp";
const MODEL: &str = hkask_inference::model_constants::TEST_MODEL_SMALL;

async fn read_line_timeout<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    timeout_ms: u64,
) -> Option<String> {
    let mut line = String::new();
    match tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        reader.read_line(&mut line),
    )
    .await
    {
        Ok(Ok(n)) if n > 0 => Some(line),
        _ => None,
    }
}

#[tokio::test]
#[ignore = "requires running daemon and DeepInfra"]
async fn e2e_initialize_and_prompt() {
    // Verify binary exists
    assert!(
        std::path::Path::new(BINARY).exists(),
        "Binary not found at {}. Run `cargo build -p hkask-acp` first.",
        BINARY
    );

    // Spawn the ACP binary
    let mut child = Command::new(BINARY)
        .env("HKASK_MCP_HOST", USERPOD)
        .env("HKASK_ACP_MODEL", MODEL)
        .env("RUST_LOG", "hkask.acp=info")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn hkask-acp. Is the daemon running?");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Read stderr in background for debugging
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
            eprintln!("[ACP stderr] {}", line.trim());
            line.clear();
        }
    });

    let mut reader = BufReader::new(stdout);

    // 1. Initialize
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientInfo": { "name": "e2e-test", "version": "1.0" }
        }
    });
    let mut bytes = serde_json::to_string(&init).unwrap();
    bytes.push('\n');
    stdin.write_all(bytes.as_bytes()).await.unwrap();

    let resp = read_line_timeout(&mut reader, 10000)
        .await
        .expect("No initialize response within 10s");
    let v: serde_json::Value =
        serde_json::from_str(resp.trim()).expect("Invalid JSON in initialize response");
    assert_eq!(v["id"], 1);
    assert_eq!(v["result"]["protocolVersion"], 1);
    assert_eq!(v["result"]["agentInfo"]["name"], "hkask-acp");
    eprintln!("✅ initialize");

    // 2. Create session
    let new_sess = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/new",
        "params": { "cwd": "/tmp" }
    });
    let mut bytes = serde_json::to_string(&new_sess).unwrap();
    bytes.push('\n');
    stdin.write_all(bytes.as_bytes()).await.unwrap();

    let resp = read_line_timeout(&mut reader, 10000)
        .await
        .expect("No session/new response");
    let v: serde_json::Value = serde_json::from_str(resp.trim()).unwrap();
    let session_id = v["result"]["sessionId"].as_str().unwrap().to_string();
    assert!(!session_id.is_empty());
    eprintln!("✅ session/new → {}", session_id);

    // 3. Send prompt
    let prompt = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": "Say hello in exactly 3 words." }]
        }
    });
    let mut bytes = serde_json::to_string(&prompt).unwrap();
    bytes.push('\n');
    stdin.write_all(bytes.as_bytes()).await.unwrap();

    // Read streaming notifications + final response
    let mut agent_chunks = 0;
    let mut usage_updates = 0;
    let mut got_end_turn = false;

    for _ in 0..50 {
        match read_line_timeout(&mut reader, 30000).await {
            Some(line) => {
                let v: serde_json::Value = match serde_json::from_str(line.trim()) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if v["method"] == "session/update" {
                    let update = &v["params"]["update"]["sessionUpdate"];
                    if update == "agent_message_chunk" {
                        agent_chunks += 1;
                        let text = &v["params"]["update"]["content"]["text"];
                        eprintln!("  ← chunk: {}", text.as_str().unwrap_or("?"));
                    } else if update == "usage_update" {
                        usage_updates += 1;
                    }
                } else if v["id"] == 3 {
                    assert_eq!(v["result"]["stopReason"], "end_turn");
                    got_end_turn = true;
                    eprintln!("✅ session/prompt → end_turn");
                    break;
                }
            }
            None => break,
        }
    }

    assert!(got_end_turn, "Never received session/prompt response");
    assert!(agent_chunks > 0, "No agent_message_chunk notifications");
    assert!(usage_updates > 0, "No usage_update notification");
    eprintln!(
        "✅ E2E complete: {} chunks, {} usage updates",
        agent_chunks, usage_updates
    );

    // Cleanup
    drop(stdin);
    let _ = child.kill().await;
}
