//! A stub `llama-server` for the supervisor integration tests (slice plan
//! step 7): answers `--version`/`--help` for the startup probe, serves the
//! manifest's health/models endpoints on the plan's host/port, ignores the
//! plan's model flags (it is a process-world stub, not a model), and can
//! be told to die (`--stub-exit-after-secs`) or to lie about its surface
//! (`--stub-emit-commit`, `--stub-drop-flag`) so every refusal class is
//! reproducible without the real engine or the 79 GB weights.
//!
//! Test-only fixture: feature-gated (`test-fixture`), the supervisor tests
//! locate it via `env!("CARGO_BIN_EXE_stub-llama-server")` and run the
//! suite single-threaded (the house live-mutation pattern).

use std::io::Write;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The flag surface of the pinned engine (mirrors the shipped manifest's
/// [flags] table; the supervisor tests also decode the real manifest, so a
/// manifest drift surfaces here as a test failure to fix the fixture).
const FLAG_SURFACE: &[&str] = &[
    "--model",
    "-ngl",
    "--cpu-moe",
    "--override-tensor",
    "--cache-type-k",
    "--cache-type-v",
    "--ctx-size",
    "--flash-attn",
    "--host",
    "--port",
    "--threads",
    "--alias",
    "--api-key-file",
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut host = "127.0.0.1".to_string();
    let mut port: u16 = 8010;
    let mut emit_version = "stub-version".to_string();
    let mut emit_commit = "stub-commit".to_string();
    let mut drop_flags: Vec<String> = Vec::new();
    let mut exit_after_secs: Option<u64> = None;
    let mut mode = "serve";
    let mut i = 0;
    while i < args.len() {
        let value = |args: &Vec<String>, i: usize| -> String {
            args.get(i + 1).cloned().unwrap_or_default()
        };
        match args[i].as_str() {
            "--version" => mode = "version",
            "--help" => mode = "help",
            "--host" => {
                host = value(&args, i);
                i += 1;
            }
            "--port" => {
                port = value(&args, i).parse().unwrap_or(8010);
                i += 1;
            }
            "--stub-emit-version" => {
                emit_version = value(&args, i);
                i += 1;
            }
            "--stub-emit-commit" => {
                emit_commit = value(&args, i);
                i += 1;
            }
            "--stub-drop-flag" => {
                drop_flags.push(value(&args, i));
                i += 1;
            }
            "--stub-exit-after-secs" => {
                exit_after_secs = value(&args, i).parse().ok();
                i += 1;
            }
            // Every other arg is a plan flag (--model, --ctx-size, the
            // alias/api-key-file pairs, …) — taken and ignored.
            _ => {}
        }
        i += 1;
    }

    match mode {
        "version" => {
            println!("version: {emit_version} (build 1, commit {emit_commit})");
        }
        "help" => {
            let mut out = std::io::stdout().lock();
            for flag in FLAG_SURFACE {
                if drop_flags.iter().any(|dropped| dropped.as_str() == *flag) {
                    continue;
                }
                writeln!(out, "{flag} VALUE").expect("write help");
            }
        }
        _ => {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("stub runtime")
                .block_on(serve(host, port, exit_after_secs));
        }
    }
}

/// Serve the manifest's two endpoints until the process is told to exit.
async fn serve(host: String, port: u16, exit_after_secs: Option<u64>) {
    if let Some(secs) = exit_after_secs {
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            std::process::exit(0);
        });
    }
    let listener = tokio::net::TcpListener::bind((host.as_str(), port))
        .await
        .expect("stub binds the plan endpoint");
    loop {
        let Ok((mut socket, _)) = listener.accept().await else {
            continue;
        };
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split(' ').nth(1))
                .unwrap_or("");
            let (status, body) = match path {
                "/health" => ("200 OK", "OK".to_string()),
                "/v1/models" => ("200 OK", r#"{"data":[{"id":"stub"}]}"#.to_string()),
                _ => ("404 Not Found", String::new()),
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.ok();
        });
    }
}
