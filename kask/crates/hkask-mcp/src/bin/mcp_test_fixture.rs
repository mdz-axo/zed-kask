//! A controllable MCP server for `tests/reconnect_integration.rs`.
//! Kept feature-gated so it does not affect normal builds. Discovery can be
//! paused via FIXTURE_DISCOVERY_ENTERED/FIXTURE_DISCOVERY_RELEASE files or
//! failed via FIXTURE_DISCOVERY_ERROR to exercise startup ownership.
//!
//! The connection-healing paths in `McpRuntime` (reap-on-death, liveness-on-read,
//! reconnect-on-demand) can only be exercised against a **real child process**
//! whose transport genuinely dies. The unit tests in `runtime.rs` cover the
//! bookkeeping around those paths, but they cannot prove that a killed server is
//! actually reconnected and the next call actually succeeds. This fixture exists
//! to make that provable.
//!
//! It speaks just enough MCP over stdio to be useful: `initialize`, `tools/list`,
//! and one `ping` tool. Requests are read as newline-delimited JSON-RPC, which is
//! what rmcp's stdio transport writes.
//!
//! Behavior is driven by env vars so a test can script a failure:
//!
//! - `FIXTURE_PID_FILE` — write the process id here once serving, so the test can
//!   kill this exact process rather than guessing.
//! - `FIXTURE_EXIT_AFTER_CALLS=N` — exit(0) immediately *before* answering the
//!   Nth `tools/call`, simulating a server that dies mid-call.
//! - `FIXTURE_MARKER` — echoed back in the `ping` result, so a test can tell a
//!   reconnected (freshly-spawned) process from the original one.
//!
//! Not a product surface: this is a dev-dependency-grade test fixture that
//! happens to need its own binary because a child process is the thing under
//! test.

use std::io::{BufRead, Write};

fn main() {
    let marker = std::env::var("FIXTURE_MARKER").unwrap_or_default();
    let exit_after_calls: Option<u32> = std::env::var("FIXTURE_EXIT_AFTER_CALLS")
        .ok()
        .and_then(|raw| raw.parse().ok());

    if let Ok(path) = std::env::var("FIXTURE_PID_FILE") {
        // Best-effort: a test that needs the pid asserts on the file's presence,
        // so a failure here surfaces there rather than being silently swallowed.
        let _ = std::fs::write(&path, std::process::id().to_string());
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut call_count: u32 = 0;

    for line in stdin.lock().lines() {
        let Ok(line) = line else { return };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let method = message.get("method").and_then(|m| m.as_str()).unwrap_or("");
        // Notifications carry no id and expect no response.
        let Some(id) = message.get("id").cloned() else {
            continue;
        };

        if method == "tools/list" {
            if let Ok(path) = std::env::var("FIXTURE_DISCOVERY_ENTERED") {
                std::fs::write(path, b"entered").expect("write discovery marker");
            }
            if let Ok(path) = std::env::var("FIXTURE_DISCOVERY_RELEASE") {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                while !std::path::Path::new(&path).exists() {
                    if std::time::Instant::now() >= deadline {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            if std::env::var_os("FIXTURE_DISCOVERY_ERROR").is_some() {
                let response = serde_json::json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32603,"message":"injected discovery failure"}});
                if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
                    return;
                }
                continue;
            }
        }
        let result = match method {
            "initialize" => serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "mcp-test-fixture", "version": "0.0.0" },
            }),
            "tools/list" => serde_json::json!({
                "tools": [{
                    "name": "ping",
                    "description": "Returns the fixture marker.",
                    "inputSchema": { "type": "object", "properties": {} },
                }],
            }),
            "tools/call" => {
                call_count += 1;
                if exit_after_calls.is_some_and(|limit| call_count >= limit) {
                    // Die without responding: the client has already handed off a
                    // request, so this is the `Interrupted` (outcome-unknown)
                    // shape, not the `Unavailable` one.
                    std::process::exit(0);
                }
                serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::json!({ "marker": marker, "calls": call_count })
                            .to_string(),
                    }],
                    "isError": false,
                })
            }
            _ => serde_json::json!({}),
        };

        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
            // The parent closed the pipe; nothing left to serve.
            return;
        }
    }
}
