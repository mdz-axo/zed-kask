//! The client MCP server: its state, its one tool, and startup.

use std::sync::Arc;

use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::config::{Config, Identity};
use crate::relays::{BUILD_ADDRESS_POLICY, probe_seed_relays};

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
        description = "Report the Nostr identity configured in settings (its npub and the same public key in hex, or a named not_configured or invalid state) and whether each seed relay answers. Contacts the network: it tries one websocket connection to each of up to 8 seed relays, with a 5 second limit each, and reports connected, unreachable, handshake_failed, timed_out or failed per relay; it sends nothing else and reads no events. A seed relay this build will not connect to is reported as refused and is not contacted. Changes nothing. This server holds no key and cannot sign; the identity and relays are set by the operator in settings and no tool changes them."
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
            let (seed_relays, not_probed) =
                probe_seed_relays(&self.config.seed_relays, BUILD_ADDRESS_POLICY).await;
            Ok(json!({
                "server": "hkask-mcp-nostr-client",
                "identity": identity,
                "seed_relays": seed_relays,
                "seed_relays_not_probed": not_probed,
                "relay_address_policy": BUILD_ADDRESS_POLICY.name(),
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
    // The workspace's TLS provider, for `wss://` relays. Errors only if one
    // is already installed.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .ok();

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
