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
    let mut guard = match INFERENCE_SOCKET_PATH.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!(
                target: "hkask.inference_socket",
                "INFERENCE_SOCKET_PATH mutex poisoned — recovering via into_inner"
            );
            poisoned.into_inner()
        }
    };
    if !guard.is_empty() && *guard != path {
        tracing::info!(
            target: "hkask.inference_socket",
            old = %*guard,
            new = %path,
            "Inference IPC socket path updated — IPC server restarted"
        );
    }
    *guard = path.to_string();

    // Also write the socket path to a well-known file so MCP server child
    // processes that were relaunched with a stale LaunchSpec (missing
    // HKASK_INFERENCE_SOCKET in their env) can discover the socket via the
    // file fallback in `InferenceIpcClient::from_env`.
    let xdg = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| "/run/user/1000".to_string());
    let file_path = format!("{xdg}/kask/inference-socket-path");
    if let Err(e) = std::fs::write(&file_path, path) {
        tracing::warn!(
            target: "hkask.inference_socket",
            "Failed to write inference socket path to {file_path}: {e} — \
             MCP servers relaunched with stale env will not find the socket"
        );
    }
}

/// Get the current inference IPC socket path, or `None` if not yet set.
///
/// Called from `KaskMcpDescriptor::command()` and `kask_server_env()` in
/// `main.rs` when building the env map for MCP server child processes.
#[must_use]
pub fn get_inference_socket_path() -> Option<String> {
    let guard = match INFERENCE_SOCKET_PATH.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!(
                target: "hkask.inference_socket",
                "INFERENCE_SOCKET_PATH mutex poisoned — recovering via into_inner"
            );
            poisoned.into_inner()
        }
    };
    if guard.is_empty() {
        None
    } else {
        Some(guard.clone())
    }
}

/// The inference establishment timeout in seconds, published to MCP server
/// child processes so IPC clients can align their read deadline with the
/// server's. See `hkask_types::inference_ipc::INFERENCE_TIMEOUT_ENV` for the
/// rationale.
static INFERENCE_TIMEOUT_SECS: Mutex<u64> = Mutex::new(0);

/// Set the inference establishment timeout (seconds). Replaces any previous
/// value.
///
/// Called from the deferred task in `main.rs` after `LanguageModelInferencePort`
/// is constructed, so the value matches what the server is actually enforcing.
/// Zero means "unset" — `get_inference_timeout_secs` returns `None` for it.
pub fn set_inference_timeout_secs(secs: u64) {
    let mut guard = match INFERENCE_TIMEOUT_SECS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!(
                target: "hkask.inference_socket",
                "INFERENCE_TIMEOUT_SECS mutex poisoned — recovering via into_inner"
            );
            poisoned.into_inner()
        }
    };
    if *guard != 0 && *guard != secs {
        tracing::info!(
            target: "hkask.inference_socket",
            old = %*guard,
            new = %secs,
            "Inference IPC timeout updated — IPC server restarted"
        );
    }
    *guard = secs;
}

/// Get the inference establishment timeout in seconds, or `None` if not yet
/// set (or set to zero, which means "unset").
///
/// Called from the same sites as `get_inference_socket_path` to inject
/// `HKASK_INFERENCE_TIMEOUT_SECS` into MCP server child-process env maps.
#[must_use]
pub fn get_inference_timeout_secs() -> Option<u64> {
    let guard = match INFERENCE_TIMEOUT_SECS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!(
                target: "hkask.inference_socket",
                "INFERENCE_TIMEOUT_SECS mutex poisoned — recovering via into_inner"
            );
            poisoned.into_inner()
        }
    };
    (*guard != 0).then_some(*guard)
}
