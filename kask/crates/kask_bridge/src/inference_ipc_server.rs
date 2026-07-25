//! `InferenceIpcServer` — the zed-side listener that serves inference requests
//! from MCP server child processes over a Unix socket.
//!
//! When zed launches an MCP server, it creates a Unix socket, starts this
//! server listening on it, and passes the socket path to the child process
//! via the `HKASK_INFERENCE_SOCKET` env var. The MCP server connects and
//! sends inference requests; this server dispatches them to zed's
//! `InferencePort` (which uses `LanguageModelRegistry` with fusion, guard,
//! and zed's configured API keys).
//!
//! ## Architecture
//!
//! ```text
//! zed process
//!   ├── InferenceIpcServer (Unix socket listener)
//!   │     └── dispatches to Arc<dyn InferencePort>
//!   │           └── GuardedInferencePort → FusionLanguageModel → zed's LanguageModelRegistry
//!   │
//!   └── spawns MCP server child process
//!         └── InferenceIpcClient (connects to the socket)
//!               └── implements InferencePort
//! ```
//!
//! ## Connection handling
//!
//! Each MCP server gets its own socket. The server accepts connections in a
//! background task and handles each connection in its own task. Requests are
//! processed sequentially per connection (the protocol is request-response).

use std::path::PathBuf;
use std::sync::Arc;

use gpui::Task;
use hkask_types::inference_ipc::{
    InferenceErrorPayload, InferenceMethod, InferenceOutcome, InferenceRequest, InferenceResponse,
};
use hkask_types::{InferenceError, InferencePort, InferenceResult};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

/// The zed-side inference IPC server.
///
/// Listens on a Unix socket and dispatches inference requests to the
/// provided `InferencePort`. Each connection is handled in its own task.
pub struct InferenceIpcServer {
    /// The socket path — passed to MCP server child processes via env var.
    socket_path: PathBuf,
    /// The background listener task.
    _task: Task<()>,
}

impl InferenceIpcServer {
    /// Start listening on a new Unix socket.
    ///
    /// The socket path is randomly generated in the system temp directory.
    /// The socket is removed when the server is dropped.
    ///
    /// `inference_port` is the port to dispatch requests to (typically the
    /// `GuardedInferencePort` wrapping `LanguageModelInferencePort`).
    pub fn start(
        inference_port: Arc<dyn InferencePort>,
        cx: &gpui::App,
    ) -> Result<Self, std::io::Error> {
        // Generate a unique socket path.
        let socket_path = generate_socket_path();

        // Bind the listener on the tokio runtime (background executor).
        let executor = cx.background_executor();

        // Use a oneshot channel to get the bind result synchronously.
        let (tx, rx) = std::sync::mpsc::channel();
        let socket_path_for_bind = socket_path.clone();
        executor
            .spawn(async move {
                // Remove any stale socket file.
                let _ = std::fs::remove_file(&socket_path_for_bind);
                let result = UnixListener::bind(&socket_path_for_bind);
                let _ = tx.send(result);
            })
            .detach();

        let listener = rx
            .recv()
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("IPC socket bind channel failed: {e}"),
                )
            })?
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to bind inference IPC socket: {e}"),
                )
            })?;

        let port = inference_port.clone();
        let task = executor.spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let port = port.clone();
                        tokio::spawn(async move {
                            handle_connection(stream, port).await;
                        });
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.inference",
                            error = %e,
                            "Inference IPC accept failed — stopping listener"
                        );
                        break;
                    }
                }
            }
        });

        Ok(Self {
            socket_path,
            _task: task,
        })
    }

    /// The socket path — pass this to MCP server child processes via the
    /// `HKASK_INFERENCE_SOCKET` env var.
    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }
}

impl Drop for InferenceIpcServer {
    fn drop(&mut self) {
        // Clean up the socket file.
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Generate a unique Unix socket path in the system temp directory.
fn generate_socket_path() -> PathBuf {
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("kask-inference-{pid}-{nonce}.sock"))
}

/// Handle a single connection from an MCP server.
///
/// Reads newline-delimited JSON requests, dispatches them to the inference
/// port, and writes newline-delimited JSON responses.
async fn handle_connection(stream: tokio::net::UnixStream, port: Arc<dyn InferencePort>) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                // Connection closed.
                break;
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    target: "reg.inference",
                    error = %e,
                    "Inference IPC read failed — closing connection"
                );
                break;
            }
        }

        let request: InferenceRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    target: "reg.inference",
                    error = %e,
                    line = %line,
                    "Inference IPC parse failed — skipping"
                );
                continue;
            }
        };

        let id = request.id;
        let outcome = dispatch(&port, request).await;

        let response = InferenceResponse { id, outcome };
        let response_json = match serde_json::to_string(&response) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    target: "reg.inference",
                    error = %e,
                    "Inference IPC response serialize failed — skipping"
                );
                continue;
            }
        };

        if let Err(e) = writer.write_all(response_json.as_bytes()).await {
            tracing::warn!(
                target: "reg.inference",
                error = %e,
                "Inference IPC write failed — closing connection"
            );
            break;
        }
        if let Err(e) = writer.write_all(b"\n").await {
            tracing::warn!(
                target: "reg.inference",
                error = %e,
                "Inference IPC write failed — closing connection"
            );
            break;
        }
        if let Err(e) = writer.flush().await {
            tracing::warn!(
                target: "reg.inference",
                error = %e,
                "Inference IPC flush failed — closing connection"
            );
            break;
        }
    }
}

/// Dispatch a single request to the inference port.
async fn dispatch(port: &Arc<dyn InferencePort>, request: InferenceRequest) -> InferenceOutcome {
    let params = request.params;
    let result: Result<InferenceResult, InferenceError> = match request.method {
        InferenceMethod::Generate => {
            let prompt = params.prompt.as_deref().unwrap_or("");
            let tools = params.tools.as_deref();
            port.generate(prompt, &params.parameters, tools).await
        }
        InferenceMethod::GenerateWithModel => {
            let prompt = params.prompt.as_deref().unwrap_or("");
            let tools = params.tools.as_deref();
            port.generate_with_model(
                prompt,
                &params.parameters,
                params.model_override.as_deref(),
                tools,
            )
            .await
        }
        InferenceMethod::GenerateWithMessages => {
            let messages = params.messages.as_deref().unwrap_or(&[]);
            let tools = params.tools.as_deref();
            port.generate_with_messages(
                messages,
                &params.parameters,
                params.model_override.as_deref(),
                tools,
            )
            .await
        }
        InferenceMethod::GenerateVision => {
            let prompt = params.prompt.as_deref().unwrap_or("");
            let images = params.images.as_deref().unwrap_or(&[]);
            port.generate_vision(
                prompt,
                images,
                &params.parameters,
                params.model_override.as_deref(),
            )
            .await
        }
    };

    match result {
        Ok(result) => InferenceOutcome::Result { result },
        Err(error) => InferenceOutcome::Error {
            error: InferenceErrorPayload::from(error),
        },
    }
}
