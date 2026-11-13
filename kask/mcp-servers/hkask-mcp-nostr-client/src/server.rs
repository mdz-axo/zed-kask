//! The client MCP server: its state, its one tool, and startup.

use std::sync::Arc;

use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::config::{Config, Identity};

hkask_mcp_server::mcp_server!(
    pub struct NostrClientServer {
        pub config: Arc<Config>,
    } = crate::ontology::annotate
);

/// `nostr_account` takes no parameters.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct AccountRequest {}

#[tool_router(router = nostr_client_router, vis = "pub")]
impl NostrClientServer {
    #[tool(
        description = "Report the Nostr identity configured in settings: its npub, the same public key in hex, and the seed relays, or a named not_configured or invalid state. Read-only: it contacts no relay. This server holds no key and cannot sign; the identity and relays are set by the operator in settings and no tool changes them."
    )]
    pub async fn nostr_account(
        &self,
        Parameters(_request): Parameters<AccountRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "nostr_account", async {
            let identity = match &self.config.identity {
                Identity::NotConfigured => json!({
                    "state": "not_configured",
                    "how_to_configure": "set kask.nostr.npub in settings",
                }),
                Identity::Configured { npub, public_key } => json!({
                    "state": "configured",
                    "npub": npub,
                    "public_key_hex": hex_lower(public_key),
                }),
                Identity::Invalid { reason } => json!({
                    "state": "invalid",
                    "reason": reason,
                }),
            };
            Ok(json!({
                "server": "hkask-mcp-nostr-client",
                "identity": identity,
                "seed_relays": self.config.seed_relays,
                "holds_key": false,
                "can_sign": false,
                "set_by": "the operator, in settings; no tool changes it",
            }))
        })
        .await
    }
}

#[tool_handler(router = Self::nostr_client_router())]
impl rmcp::ServerHandler for NostrClientServer {}

/// Lower-case hex of a public key, as NIP-01 writes it.
fn hex_lower(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Run the server over stdio. It asks for no credential.
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    let config = Arc::new(Config::from_env());
    hkask_mcp_server::run_server(
        "hkask-mcp-nostr-client",
        env!("CARGO_PKG_VERSION"),
        move |ctx: hkask_mcp_server::ServerContext| {
            if let Identity::Invalid { reason } = &config.identity {
                tracing::warn!(%reason, "Nostr client identity is not usable");
            }
            Ok(NostrClientServer::new(ctx.webid, config))
        },
        Vec::new(),
    )
    .await
}
