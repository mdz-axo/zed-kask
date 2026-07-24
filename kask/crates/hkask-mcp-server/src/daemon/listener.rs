//! Unix domain socket listener for the hKask daemon.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use super::daemon_socket_path;
use super::handler::DaemonHandler;
use super::protocol::{DaemonRequest, DaemonResponse};

/// Unix domain socket listener for the hKask daemon.
///
/// Binds to `~/.config/hkask/daemon.sock` and handles JSON-RPC-style
/// queries from MCP server binaries.
pub struct DaemonListener {
    socket_path: PathBuf,
    listener: Option<UnixListener>,
}

impl Default for DaemonListener {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonListener {
    /// Create a listener bound to the default socket path.
    ///
    /// post: returns DaemonListener with default socket path, listener=None
    #[must_use]
    pub fn new() -> Self {
        Self {
            socket_path: daemon_socket_path(),
            listener: None,
        }
    }

    /// Create a listener with a custom socket path (for testing).
    ///
    /// pre:  path is a valid filesystem path
    /// post: returns DaemonListener with custom socket path, listener=None
    #[must_use]
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            socket_path: path,
            listener: None,
        }
    }

    /// Bind the socket and start listening.
    pub async fn bind(&mut self) -> std::io::Result<()> {
        // Remove stale socket file if present
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }
        // Ensure parent directory exists
        if let Some(parent) = self.socket_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let listener = UnixListener::bind(&self.socket_path)?;
        self.listener = Some(listener);
        tracing::info!(
            target: "hkask.daemon",
            path = %self.socket_path.display(),
            "Daemon socket bound"
        );
        Ok(())
    }

    /// Accept connections and handle requests in a loop.
    ///
    /// Runs until the listener is closed or an error occurs.
    pub async fn serve(&self, handler: Arc<dyn DaemonHandler>) -> std::io::Result<()> {
        let listener = self
            .listener
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "Not bound"))?;

        loop {
            let (stream, _addr) = listener.accept().await?;
            let handler = Arc::clone(&handler);
            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, handler.as_ref()).await {
                    tracing::warn!(
                        target: "hkask.daemon",
                        error = %e,
                        "Daemon connection error"
                    );
                }
            });
        }
    }
}

async fn handle_connection(stream: UnixStream, handler: &dyn DaemonHandler) -> std::io::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader.read_line(&mut line).await?;

    let request: DaemonRequest = serde_json::from_str(&line)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    let response = match request {
        DaemonRequest::AuthQuery { userpod } => {
            let (authenticated, webid) = handler.check_auth(&userpod).await;
            DaemonResponse::AuthResponse {
                authenticated,
                webid,
                action: if authenticated {
                    None
                } else {
                    Some("prompt_user".to_string())
                },
            }
        }
        DaemonRequest::CapabilityQuery { userpod, tool } => {
            let granted = handler.check_capability(&userpod, &tool).await;
            DaemonResponse::CapabilityResponse { granted }
        }
        DaemonRequest::StoreExperience {
            userpod,
            entity,
            attribute,
            value,
            confidence,
        } => {
            let (stored, episodic_id, semantic_id) = handler
                .store_experience(&userpod, &entity, &attribute, &value, confidence)
                .await;
            DaemonResponse::StoreResponse {
                stored,
                episodic_id,
                semantic_id,
            }
        }
        DaemonRequest::ToolDispatch {
            userpod,
            tool,
            input,
        } => {
            let (ok, output, error) = handler.dispatch_tool(&userpod, &tool, &input).await;
            DaemonResponse::ToolDispatchResponse { ok, output, error }
        }
        DaemonRequest::CuratorHealthQuery { userpod } => {
            let health = handler.curator_health(&userpod).await;
            DaemonResponse::CuratorHealthResponse { health }
        }
        DaemonRequest::RegStatusQuery { userpod, domain } => {
            let status = handler.reg_status(&userpod, domain.as_deref()).await;
            DaemonResponse::RegStatusResponse { status }
        }
    };

    let mut json = serde_json::to_string(&response)?;
    json.push('\n');
    writer.write_all(json.as_bytes()).await?;
    writer.shutdown().await?;
    Ok(())
}
