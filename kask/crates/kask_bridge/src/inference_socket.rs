//! Inference IPC socket path management (D8).
//!
//! The inference IPC socket path is set by the deferred post-login task in
//! `main.rs` when the `InferenceIpcServer` starts. It must be re-settable
//! because the IPC server can restart — e.g. when the real model is configured
//! after the no-op fallback, or when the user re-logs in. A `OnceLock` cannot
//! be updated once set, so the first `set()` wins and subsequent attempts are
//! silently dropped, leaving MCP servers with a stale or no-op socket path.
//!
//! This module provides a re-settable `Mutex<String>` so the socket path can
//! be updated whenever the IPC server restarts. `main.rs` calls
//! [`set_inference_socket_path`] after the IPC server starts, and
//! [`get_inference_socket_path`] when building MCP server env maps.

use std::sync::Mutex;

static INFERENCE_SOCKET_PATH: Mutex<String> = Mutex::new(String::new());

/// Set the inference IPC socket path. Replaces any previous value.
///
/// Called from the deferred task in `main.rs` after `InferenceIpcServer::start`
/// succeeds. Unlike `OnceLock::set`, this can be called multiple times — the
/// latest value wins.
pub fn set_inference_socket_path(path: &str) {
    let mut guard = INFERENCE_SOCKET_PATH.lock().expect("INFERENCE_SOCKET_PATH mutex poisoned");
    if !guard.is_empty() && *guard != path {
        tracing::info!(
            target: "hkask.inference_socket",
            old = %*guard,
            new = %path,
            "Inference IPC socket path updated — IPC server restarted"
        );
    }
    *guard = path.to_string();
}

/// Get the current inference IPC socket path, or `None` if not yet set.
///
/// Called from `KaskMcpDescriptor::command()` and `kask_server_env()` in
/// `main.rs` when building the env map for MCP server child processes.
#[must_use]
pub fn get_inference_socket_path() -> Option<String> {
    let guard = INFERENCE_SOCKET_PATH.lock().expect("INFERENCE_SOCKET_PATH mutex poisoned");
    if guard.is_empty() {
        None
    } else {
        Some(guard.clone())
    }
}
