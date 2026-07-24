//! Daemon socket — Unix domain socket transport for MCP binary ↔ hKask communication.
//!
//! MCP server binaries connect to the hKask daemon over a Unix domain socket at
//! `~/.config/hkask/daemon.sock` to authenticate, verify role assignments, and
//! check capability tokens. The protocol is newline-delimited JSON.
//!
//! # Protocol
//!
//! Request (MCP binary → daemon):
//! ```json
//! {"type":"auth_query","userpod":"bob"}
//! {"type":"capability_query","userpod":"bob","tool":"web_search"}
//! ```rust,no_run
//!
//! Response (daemon → MCP binary):
//! ```json
//! {"type":"auth_response","authenticated":true,"webid":"bob-xxxx"}
//! {"type":"auth_response","authenticated":false,"action":"prompt_user"}
//! {"type":"assignment_response","assigned":true}
//! {"type":"capability_response","granted":true}
//! ```

use std::path::PathBuf;

pub mod client;
pub mod handler;
pub mod listener;
pub mod protocol;

#[cfg(test)]
mod tests;

pub use client::DaemonClient;
pub use handler::DaemonHandler;
pub use listener::DaemonListener;
pub use protocol::{DaemonRequest, DaemonResponse};

/// Well-known path for the hKask daemon socket.
///
/// post: returns PathBuf to the daemon socket (config dir or /tmp fallback)
#[must_use]
pub fn daemon_socket_path() -> PathBuf {
    let base = dirs_next().unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("daemon.sock")
}

fn dirs_next() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("hkask"))
}

/// Error type for daemon socket probing.
///
/// Used by `ping_daemon` to distinguish failure modes when checking whether
/// the daemon is live. Each variant maps to a specific failure in the
/// connect → write → read → parse chain.
#[derive(Debug, thiserror::Error)]
pub enum DaemonPingError {
    #[error("connect failed: {0}")]
    Connect(#[source] std::io::Error),
    #[error("serialize request: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("write failed: {0}")]
    Write(#[source] std::io::Error),
    #[error("shutdown failed: {0}")]
    Shutdown(#[source] std::io::Error),
    #[error("read failed: {0}")]
    Read(#[source] std::io::Error),
    #[error("empty response")]
    EmptyResponse,
    #[error("invalid JSON response: {0}")]
    InvalidJson(#[source] serde_json::Error),
}

/// Probe the daemon socket by sending a sentinel `auth_query`.
///
/// Returns `Ok(())` if the socket accepts a connection and responds with valid
/// JSON; `Err(DaemonPingError)` otherwise. This distinguishes a live daemon from
/// a stale socket file left behind by a crashed process.
///
/// pre:  path is the daemon socket path
/// post: returns Ok(()) when the daemon is live, Err with a classified reason when not
pub async fn ping_daemon(path: &std::path::Path) -> Result<(), DaemonPingError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let stream = UnixStream::connect(path)
        .await
        .map_err(DaemonPingError::Connect)?;
    let (reader, mut writer) = stream.into_split();

    // Send a minimal auth_query. The sentinel name "__ping__" is not a real
    // userpod; the daemon will return authenticated:false, but any valid
    // JSON response proves the socket is live.
    let request = serde_json::json!({
        "type": "auth_query",
        "userpod": "__ping__"
    });
    let mut json = serde_json::to_string(&request).map_err(DaemonPingError::Serialize)?;
    json.push('\n');
    writer
        .write_all(json.as_bytes())
        .await
        .map_err(DaemonPingError::Write)?;
    writer.shutdown().await.map_err(DaemonPingError::Shutdown)?;

    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader
        .read_line(&mut line)
        .await
        .map_err(DaemonPingError::Read)?;

    if line.trim().is_empty() {
        return Err(DaemonPingError::EmptyResponse);
    }
    serde_json::from_str::<serde_json::Value>(&line).map_err(DaemonPingError::InvalidJson)?;
    Ok(())
}
