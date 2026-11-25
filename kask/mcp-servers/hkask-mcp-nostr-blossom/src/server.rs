//! The Blossom sidecar MCP server: state, listener announcement, decision log,
//! and three read-only inspection tools (`nostr_blossom_status`,
//! `nostr_blossom_list`, `nostr_blossom_log`).

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_ipc_wire::nostr_wire::MAX_BLOSSOM_UPLOAD_BYTES;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::{Config, parse_pubkey};
use crate::storage::BlobStore;

pub const DECISION_LOG_CAPACITY: usize = 200;
pub const DEFAULT_LIST_BLOBS: usize = 50;
pub const MAX_LIST_BLOBS: usize = 200;
pub const DEFAULT_LOG_DECISIONS: usize = 50;

pub const ACCESS_LIMITS: [&str; 4] = [
    "The HTTP listener is bound to loopback (127.0.0.1) with BUD-01 CORS headers (Access-Control-Allow-Origin: *) for media playback.",
    "GET and HEAD blob reads are unauthenticated by content hash; PUT /upload, HEAD /upload, and DELETE require a Schnorr-signed kind 24242 event from an operator-allowlisted pubkey.",
    "DELETE removes only the signing pubkey's ownership reference; the blob file is unlinked only when zero owners remain.",
    "No MCP tool modifies the allowlist, quota, port, or public_base_url, and this server holds no secret key.",
];

#[derive(Debug, Clone)]
pub enum ListenerState {
    Listening(SocketAddr),
    Down(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct BlossomDecision {
    pub at: Option<u64>,
    pub op: &'static str,
    pub sha256: Option<String>,
    pub author: Option<String>,
    pub outcome: String,
}

pub struct BlossomState {
    pub store: Arc<BlobStore>,
    pub config: Config,
    pub store_dir: PathBuf,
    pub listener: ListenerState,
    decisions: Mutex<VecDeque<BlossomDecision>>,
    admitted: AtomicU64,
    refused: AtomicU64,
    nonce: AtomicU64,
}

impl std::fmt::Debug for BlossomState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlossomState")
            .field("store_dir", &self.store_dir)
            .field("listener", &self.listener)
            .finish()
    }
}

impl BlossomState {
    pub fn new(
        store: Arc<BlobStore>,
        config: Config,
        store_dir: PathBuf,
        listener: ListenerState,
    ) -> Self {
        Self {
            store,
            config,
            store_dir,
            listener,
            decisions: Mutex::new(VecDeque::with_capacity(DECISION_LOG_CAPACITY)),
            admitted: AtomicU64::new(0),
            refused: AtomicU64::new(0),
            nonce: AtomicU64::new(1),
        }
    }

    pub fn next_nonce(&self) -> u64 {
        self.nonce.fetch_add(1, Ordering::Relaxed)
    }

    pub fn record_decision(
        &self,
        op: &'static str,
        sha256: Option<String>,
        author: Option<String>,
        ok: bool,
        outcome: String,
    ) {
        if ok {
            self.admitted.fetch_add(1, Ordering::Relaxed);
        } else {
            self.refused.fetch_add(1, Ordering::Relaxed);
        }
        let decision = BlossomDecision {
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs()),
            op,
            sha256,
            author,
            outcome,
        };
        let mut guard = self
            .decisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if guard.len() == DECISION_LOG_CAPACITY {
            guard.pop_front();
        }
        guard.push_back(decision);
    }

    pub fn recent_decisions(&self, limit: usize) -> Vec<BlossomDecision> {
        let guard = self
            .decisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        guard.iter().rev().take(limit).cloned().collect()
    }

    pub fn totals(&self) -> (u64, u64) {
        (
            self.admitted.load(Ordering::Relaxed),
            self.refused.load(Ordering::Relaxed),
        )
    }
}

/// Withdraws `mcp/nostr-blossom/listener.json` on clean shutdown if this
/// process still owns it.
#[must_use = "dropping the guard withdraws this server's listener.json announcement"]
pub struct AnnouncementGuard {
    owned: Option<(PathBuf, String)>,
}

impl AnnouncementGuard {
    #[must_use]
    pub fn empty() -> Self {
        Self { owned: None }
    }
}

impl Drop for AnnouncementGuard {
    fn drop(&mut self) {
        let Some((path, expected_address)) = self.owned.take() else {
            return;
        };
        if read_announced_loopback(&path)
            .as_ref()
            .map(|(raw, _)| raw.as_str())
            == Some(expected_address.as_str())
        {
            let _ = std::fs::remove_file(&path);
        }
    }
}

pub fn default_announcement_path() -> PathBuf {
    hkask_types::agent_paths::resolve_under_data_dir(
        &hkask_ipc_wire::nostr_wire::local_blossom_announcement_path(),
    )
}

fn read_announced_loopback(path: &Path) -> Option<(String, SocketAddr)> {
    let text = std::fs::read_to_string(path).ok()?;
    let announcement: hkask_ipc_wire::nostr_wire::LocalBlossomAnnouncement =
        serde_json::from_str(&text).ok()?;
    let host_port = announcement.address.strip_prefix("http://")?;
    let addr: SocketAddr = host_port.parse().ok()?;
    if addr.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) || addr.port() == 0 {
        return None;
    }
    Some((announcement.address, addr))
}

fn loopback_listener_is_live(addr: SocketAddr) -> bool {
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(200)).is_ok()
}

pub fn announce_listener(
    listener: &ListenerState,
    path: &Path,
) -> std::io::Result<AnnouncementGuard> {
    match listener {
        ListenerState::Listening(address) => {
            if let Some((_, existing_addr)) = read_announced_loopback(path)
                && existing_addr != *address
                && loopback_listener_is_live(existing_addr)
            {
                return Ok(AnnouncementGuard { owned: None });
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let address_text = format!("http://{address}");
            let announcement = hkask_ipc_wire::nostr_wire::LocalBlossomAnnouncement {
                address: address_text.clone(),
            };
            let text = serde_json::to_string(&announcement).map_err(std::io::Error::other)?;
            let staging = path.with_extension("json.new");
            std::fs::write(&staging, text)?;
            std::fs::rename(&staging, path)?;
            Ok(AnnouncementGuard {
                owned: Some((path.to_path_buf(), address_text)),
            })
        }
        ListenerState::Down(_) => {
            if let Some((_, existing_addr)) = read_announced_loopback(path)
                && !loopback_listener_is_live(existing_addr)
            {
                let _ = std::fs::remove_file(path);
            }
            Ok(AnnouncementGuard::empty())
        }
    }
}

hkask_mcp_server::mcp_server!(
    pub struct NostrBlossomServer {
        pub state: Arc<BlossomState>,
    } = crate::ontology::annotate
);

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusRequest {}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListRequest {
    /// Optional author filter (npub or 64-character hex public key).
    pub author: Option<String>,
    /// Maximum blobs to return, newest first (default 50, maximum 200).
    pub limit: Option<u32>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogRequest {
    /// Maximum decisions to return, newest first (default 50, maximum 200).
    pub limit: Option<u32>,
}

#[tool_router(router = nostr_blossom_router, vis = "pub")]
impl NostrBlossomServer {
    #[tool(
        description = "Report the local Blossom media sidecar's state: HTTP listener address, blob count, disk usage, configured quota, optional public_base_url, allowlisted author count, and access limits. Read-only; changes nothing."
    )]
    pub async fn nostr_blossom_status(
        &self,
        Parameters(_request): Parameters<StatusRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "nostr_blossom_status", async {
            let stats = self
                .state
                .store
                .stats()
                .map_err(|e| McpToolError::internal(e.to_string()))?;
            let (admitted, refused) = self.state.totals();
            let listener = match &self.state.listener {
                ListenerState::Listening(addr) => json!({
                    "state": "listening",
                    "address": format!("http://{addr}"),
                }),
                ListenerState::Down(reason) => json!({
                    "state": "down",
                    "reason": reason,
                }),
            };
            let allowed_authors: Vec<String> = self
                .state
                .config
                .allowed_authors
                .iter()
                .map(hex::encode)
                .collect();
            Ok(json!({
                "server": "hkask-mcp-nostr-blossom",
                "listener": listener,
                "store_dir": self.state.store_dir.display().to_string(),
                "blob_count": stats.blob_count,
                "total_bytes": stats.total_bytes,
                "max_bytes": self.state.config.max_bytes,
                "max_upload_bytes": MAX_BLOSSOM_UPLOAD_BYTES,
                "public_base_url": self.state.config.public_base_url,
                "allowed_authors": allowed_authors,
                "allowed_author_count": allowed_authors.len(),
                "decisions_admitted": admitted,
                "decisions_refused": refused,
                "refused_config": self.state.config.refused,
                "access_limits": ACCESS_LIMITS,
                "holds_key": false,
                "set_by": "the operator, in settings; no tool changes it",
            }))
        })
        .await
    }

    #[tool(
        description = "List blobs stored in the local Blossom sidecar, newest first, optionally filtered by owning author (npub or 64-character hex). Read-only; changes nothing."
    )]
    pub async fn nostr_blossom_list(
        &self,
        Parameters(request): Parameters<ListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "nostr_blossom_list", async {
            let limit = match request.limit {
                None => DEFAULT_LIST_BLOBS,
                Some(0) => return Err(McpToolError::invalid_argument("limit must be at least 1")),
                Some(n) if n as usize > MAX_LIST_BLOBS => {
                    return Err(McpToolError::invalid_argument(format!(
                        "limit {n} is over the maximum of {MAX_LIST_BLOBS}"
                    )));
                }
                Some(n) => n as usize,
            };
            let author_hex = match request.author.as_deref() {
                Some(raw) => {
                    let pk = parse_pubkey(raw).ok_or_else(|| {
                        McpToolError::invalid_argument(
                            "author must be an npub or 64-character hex public key",
                        )
                    })?;
                    Some(hex::encode(pk))
                }
                None => None,
            };
            let blobs = self
                .state
                .store
                .list_by_owner(author_hex.as_deref(), limit)
                .map_err(|e| McpToolError::internal(e.to_string()))?;
            Ok(json!({
                "returned": blobs.len(),
                "limit": limit,
                "blobs": blobs,
            }))
        })
        .await
    }

    #[tool(
        description = "Read the local Blossom sidecar's recent admission, upload, and delete decisions, newest first (up to 200 kept in memory). Read-only; changes nothing."
    )]
    pub async fn nostr_blossom_log(
        &self,
        Parameters(request): Parameters<LogRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "nostr_blossom_log", async {
            let limit = match request.limit {
                None => DEFAULT_LOG_DECISIONS,
                Some(0) => return Err(McpToolError::invalid_argument("limit must be at least 1")),
                Some(n) if n as usize > DECISION_LOG_CAPACITY => {
                    return Err(McpToolError::invalid_argument(format!(
                        "limit {n} is over the maximum of {DECISION_LOG_CAPACITY}"
                    )));
                }
                Some(n) => n as usize,
            };
            let (admitted, refused) = self.state.totals();
            let decisions = self.state.recent_decisions(limit);
            Ok(json!({
                "returned": decisions.len(),
                "limit": limit,
                "total_admitted": admitted,
                "total_refused": refused,
                "decisions": decisions,
            }))
        })
        .await
    }
}

#[tool_handler(router = Self::nostr_blossom_router())]
impl rmcp::ServerHandler for NostrBlossomServer {}
