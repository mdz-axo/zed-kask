#![forbid(unsafe_code)]
//! hKask MCP Swarm — Agent Bestiary World (ABW) integration server.
//!
//! Exposes ABW's agent catalogue, workspaces ("swarms"), and the Xaman Ek
//! curator as MCP tools, governed by the kask MCP runtime (OCAP, gas, spans).
//!
//! ## API surface (verified 2026-08-01 against the live service)
//! - Base URL: `https://agent-bestiary.world` (no `api.` subdomain)
//! - Auth: `Authorization: Bearer <key>` (Pro-tier API key, scopes read/write/execute)
//! - Open: `GET /api/agents`, `GET /api/models/catalogue`
//! - Authed: `/api/workspaces`, `/api/agents/{name}/execute`, `/api/xaman/sessions`, `/api/wallet`
//!
//! ## Error model
//! ABW returns HTTP 200 envelopes containing upstream LLM errors in the body
//! (e.g. Xaman Ek passing through Anthropic credit exhaustion verbatim), and
//! HTTP 500 for domain failures like unfunded agents. `SwarmError` mapping
//! therefore inspects response bodies, not just status codes.
//!
//! ## Tools (4 — v1 read/consult surface)
//! - `swarm_list_agents` — catalogue browse (keyless-capable)
//! - `swarm_get_swarm` — workspace roster + budget
//! - `swarm_execute_agent` — text-only agent consultation (spend: token fees)
//! - `swarm_curate` — Xaman Ek curator session (consent-gated)
//!
//! Spend-mutating tools (hire/fire/delegate) are deferred to v2 behind the
//! cost/consent gate — see `kask/docs/plans/abw-swarm-intelligence.md` §3.6.

use hkask_mcp_server::server::{CredentialRequirement, McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

// ── Configuration ──────────────────────────────────────────────────────────

/// Runtime configuration for the ABW client. Validated at construction.
///
/// Defaults are the single source of truth; env vars override. No secrets are
/// stored here — `api_key` is the resolved credential value, passed in from
/// the `ServerContext` credentials map at server construction.
#[derive(Debug, Clone)]
pub struct SwarmConfig {
    /// ABW API base URL (apex — endpoints are `/api/*` under it).
    pub api_base_url: String,
    /// Resolved ABW API key. `None` = unauthenticated (catalogue-only mode).
    pub api_key: Option<String>,
    /// Per-dispatch credit ceiling for future spend tools (S3 budget gate).
    pub max_credits_per_dispatch: u32,
    /// Whether Xaman Ek sessions may be initiated without per-call opt-in (S5 policy).
    pub curator_consent_default: bool,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            api_base_url: "https://agent-bestiary.world".to_string(),
            api_key: None,
            max_credits_per_dispatch: 50,
            curator_consent_default: false,
        }
    }
}

impl SwarmConfig {
    /// Build from environment, returning the config plus any warnings about
    /// degraded operation (missing key → catalogue-only mode).
    fn from_env(api_key: Option<String>) -> (Self, Option<String>) {
        let default = Self::default();
        let api_base_url = std::env::var("HKASK_ABW_API_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default.api_base_url);
        let max_credits_per_dispatch = std::env::var("HKASK_ABW_MAX_CREDITS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default.max_credits_per_dispatch);
        let warning = if api_key.is_none() {
            Some(
                "HKASK_ABW_API_KEY not set — swarm server in catalogue-only mode; \
                 authenticated tools (get_swarm, execute_agent, curate) will return Auth errors"
                    .to_string(),
            )
        } else {
            None
        };
        (
            Self {
                api_base_url,
                api_key,
                max_credits_per_dispatch,
                curator_consent_default: default.curator_consent_default,
            },
            warning,
        )
    }
}

// ── Error type ─────────────────────────────────────────────────────────────

/// Errors from the ABW swarm client. Maps ABW HTTP errors AND body-embedded
/// domain errors; never leaks reqwest types.
#[derive(Debug, thiserror::Error)]
pub enum SwarmError {
    /// 401 / missing or invalid API key.
    #[error("ABW authentication failed: {0}. Set HKASK_ABW_API_KEY (Pro tier required).")]
    Auth(String),
    /// 402 — credits exhausted (algedonic).
    #[error("ABW payment required: {0}")]
    PaymentRequired(String),
    /// 500 "not funded" — the agent's owner has not configured an LLM key on
    /// their ABW profile. Execution funding is owner-side, not caller-side.
    #[error("ABW agent '{agent}' is not funded: {message}")]
    AgentNotFunded { agent: String, message: String },
    /// HTTP 200 envelope containing an upstream LLM/provider error string.
    /// Algedonic-adjacent: surface verbatim, do not retry blindly.
    #[error("ABW upstream model error ({provider}): {message}")]
    UpstreamModelError { provider: String, message: String },
    /// 429.
    #[error("ABW rate limited: {0}")]
    RateLimited(String),
    /// Xaman Ek session creation failed.
    #[error("ABW curator unavailable: {0}")]
    CuratorUnavailable(String),
    /// Serde parse failure on a known endpoint — possible API drift (S4).
    #[error("ABW API version mismatch: {0}")]
    ApiVersionMismatch(String),
    /// Network/transport failure.
    #[error("ABW request failed: {0}")]
    Unavailable(String),
}

impl SwarmError {
    /// Convert into the MCP tool error surface with the appropriate kind.
    fn into_tool_error(self) -> McpToolError {
        match self {
            Self::Auth(m) => McpToolError::permission_denied(m),
            Self::PaymentRequired(m) => McpToolError::permission_denied(m),
            Self::AgentNotFunded { .. } => McpToolError::unavailable(self.to_string()),
            Self::UpstreamModelError { .. } => McpToolError::unavailable(self.to_string()),
            Self::RateLimited(m) => McpToolError::rate_limited(m),
            Self::CuratorUnavailable(m) => McpToolError::unavailable(m),
            Self::ApiVersionMismatch(m) => McpToolError::internal(m),
            Self::Unavailable(m) => McpToolError::unavailable(m),
        }
    }
}

// ── HTTP client ────────────────────────────────────────────────────────────

/// Thin reqwest wrapper isolating every ABW-specific assumption (base URL,
/// auth header, error mapping) behind one seam. The panel, settings, and
/// tools never construct raw requests.
pub struct SwarmClient {
    http: reqwest::Client,
    config: SwarmConfig,
}

impl SwarmClient {
    fn new(http: reqwest::Client, config: SwarmConfig) -> Self {
        Self { http, config }
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/api{}",
            self.config.api_base_url.trim_end_matches('/'),
            path
        )
    }

    /// True when an API key is configured. Read tools that need auth check
    /// this first and fail with a remediation message rather than a raw 401.
    fn is_authenticated(&self) -> bool {
        self.config.api_key.is_some()
    }

    fn require_auth(&self) -> Result<&str, SwarmError> {
        self.config
            .api_key
            .as_deref()
            .ok_or_else(|| SwarmError::Auth("no API key configured".to_string()))
    }

    /// Send a request, attaching the bearer token when present, and map the
    /// response (status AND body) into `Result<Value, SwarmError>`.
    async fn send(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<serde_json::Value, SwarmError> {
        let builder = match &self.config.api_key {
            Some(key) => builder.bearer_auth(key),
            None => builder,
        };
        let resp = builder
            .send()
            .await
            .map_err(|e| SwarmError::Unavailable(e.to_string()))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        match status.as_u16() {
            200..=299 => {
                let value: serde_json::Value = serde_json::from_str(&body)
                    .map_err(|e| SwarmError::ApiVersionMismatch(format!("parse error: {e}")))?;
                // ABW wraps upstream LLM errors into 200 envelopes. Detect the
                // pattern ("I encountered an error" / "credit balance is too low")
                // so callers get a typed error instead of a success-looking payload.
                if let Some(err) = detect_embedded_error(&value) {
                    return Err(err);
                }
                Ok(value)
            }
            401 | 403 => Err(SwarmError::Auth(body.trim().to_string())),
            402 => Err(SwarmError::PaymentRequired(body.trim().to_string())),
            429 => Err(SwarmError::RateLimited(body.trim().to_string())),
            500 if body.contains("not funded") => {
                let agent = extract_quoted(&body).unwrap_or_default();
                Err(SwarmError::AgentNotFunded {
                    agent,
                    message: body.trim().to_string(),
                })
            }
            _ => Err(SwarmError::Unavailable(format!(
                "HTTP {status}: {}",
                body.trim()
            ))),
        }
    }

    async fn get(&self, path: &str) -> Result<serde_json::Value, SwarmError> {
        self.send(self.http.get(self.url(path))).await
    }

    async fn post(
        &self,
        path: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, SwarmError> {
        self.send(self.http.post(self.url(path)).json(payload))
            .await
    }
}

/// Inspect a 200-response body for ABW's embedded upstream-error pattern.
/// Returns a typed `SwarmError` when the payload is an error in disguise.
fn detect_embedded_error(value: &serde_json::Value) -> Option<SwarmError> {
    // Xaman Ek puts upstream failures in the `response` string field.
    let text = value
        .get("response")
        .and_then(|r| r.as_str())
        .or_else(|| value.get("error").and_then(|e| e.as_str()))?;
    if !(text.contains("I encountered an error") || text.contains("Execution failed")) {
        return None;
    }
    if text.contains("credit balance is too low") || text.contains("credit balance") {
        return Some(SwarmError::UpstreamModelError {
            provider: "anthropic".to_string(),
            message: text.to_string(),
        });
    }
    if text.contains("not funded") {
        return Some(SwarmError::AgentNotFunded {
            agent: extract_quoted(text).unwrap_or_default(),
            message: text.to_string(),
        });
    }
    Some(SwarmError::UpstreamModelError {
        provider: "unknown".to_string(),
        message: text.to_string(),
    })
}

/// Extract the first 'single-quoted' token (ABW uses it for agent names in
/// error strings like "Agent 'david_dunning' is not funded").
fn extract_quoted(text: &str) -> Option<String> {
    let start = text.find('\'')? + 1;
    let end = text[start..].find('\'')? + start;
    Some(text[start..end].to_string())
}

// ── Request types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListAgentsRequest {
    /// Filter by agent type (e.g. "research", "creative", "meta"). Optional.
    pub agent_type: Option<String>,
    /// Filter by tag. Optional.
    pub tag: Option<String>,
    /// Maximum number of agents to return. Default 50.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetSwarmRequest {
    /// Workspace ID (UUID) or slug. Lists workspaces when omitted.
    pub workspace_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecuteAgentRequest {
    /// Agent name (e.g. "market_analyst").
    pub agent_name: String,
    /// The query or task for the agent.
    pub query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CurateRequest {
    /// Message for the Xaman Ek curator. A new session is created per call.
    pub message: String,
    /// Existing session ID to continue a curator conversation. Optional.
    pub session_id: Option<String>,
}

// ── Server struct ──────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub struct SwarmServer {
        pub client: std::sync::Arc<SwarmClient>,
    }
);

impl SwarmServer {
    fn combined_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::swarm_router()
    }
}

// ── MCP Tools ──────────────────────────────────────────────────────────────

#[tool_router(router = swarm_router, vis = "pub")]
impl SwarmServer {
    /// Browse the ABW agent catalogue. Works without an API key.
    #[tool(
        description = "List Agent Bestiary World catalogue agents with metadata (name, type, description, tags, pricing, execution stats). Optionally filter by agent_type or tag. Keyless."
    )]
    pub async fn swarm_list_agents(&self, parameters: Parameters<ListAgentsRequest>) -> String {
        execute_tool_semantic(self, "swarm_list_agents", Some("dublin-core"), async {
            let req = parameters.0;
            let data = self
                .client
                .get("/agents")
                .await
                .map_err(SwarmError::into_tool_error)?;

            let empty = Vec::new();
            let agents = data
                .get("agents")
                .and_then(|a| a.as_array())
                .unwrap_or(&empty);

            let limit = req.limit.unwrap_or(50);
            let filtered: Vec<serde_json::Value> = agents
                .iter()
                .filter(|a| {
                    req.agent_type.as_ref().is_none_or(|t| {
                        a.get("agent_type").and_then(|v| v.as_str()) == Some(t.as_str())
                    })
                })
                .filter(|a| {
                    req.tag.as_ref().is_none_or(|t| {
                        a.get("tags")
                            .and_then(|v| v.as_array())
                            .is_some_and(|tags| tags.iter().any(|x| x.as_str() == Some(t.as_str())))
                    })
                })
                .take(limit)
                .map(|a| {
                    serde_json::json!({
                        "agent_id": a.get("agent_id"),
                        "agent_type": a.get("agent_type"),
                        "description": a.get("description"),
                        "author": a.get("author"),
                        "tags": a.get("tags"),
                        "model": a.get("capabilities").and_then(|c| c.get("model")),
                        "dependencies": a.get("dependencies"),
                        "execution_stats": a.get("execution_stats"),
                        "dreaming": a.get("dreaming"),
                    })
                })
                .collect();

            Ok(serde_json::json!({
                "count": filtered.len(),
                "authenticated": self.client.is_authenticated(),
                "agents": filtered,
            }))
        })
        .await
    }

    /// List the operator's workspaces, or get one workspace's full roster.
    #[tool(
        description = "List your Agent Bestiary World workspaces (agent swarms) with budgets and agent counts, or pass workspace_id (UUID or slug) for the full roster of hired agents. Requires API key."
    )]
    pub async fn swarm_get_swarm(&self, parameters: Parameters<GetSwarmRequest>) -> String {
        execute_tool_semantic(self, "swarm_get_swarm", Some("dublin-core"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;

            match req.workspace_id {
                Some(id) => {
                    let data = self
                        .client
                        .get(&format!("/workspaces/{id}"))
                        .await
                        .map_err(SwarmError::into_tool_error)?;
                    Ok(data)
                }
                None => {
                    let data = self
                        .client
                        .get("/workspaces")
                        .await
                        .map_err(SwarmError::into_tool_error)?;
                    Ok(data)
                }
            }
        })
        .await
    }

    /// Run a text-only consultation with an ABW agent (token fees apply).
    #[tool(
        description = "Execute an Agent Bestiary World agent with a query (single turn, no tools — text consultation). Costs token fees. Requires API key; the agent's owner must have funded it."
    )]
    pub async fn swarm_execute_agent(&self, parameters: Parameters<ExecuteAgentRequest>) -> String {
        execute_tool_semantic(self, "swarm_execute_agent", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.agent_name.trim().is_empty() || req.query.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "agent_name and query must be non-empty".to_string(),
                ));
            }

            let data = self
                .client
                .post(
                    &format!("/agents/{}/execute", req.agent_name),
                    &serde_json::json!({ "query": req.query }),
                )
                .await
                .map_err(SwarmError::into_tool_error)?;

            Ok(serde_json::json!({
                "agent_name": req.agent_name,
                "result": data,
            }))
        })
        .await
    }

    /// Ask the Xaman Ek curator (creates or continues a session).
    #[tool(
        description = "Ask Xaman Ek, the Agent Bestiary World platform curator/navigator. Creates a session (or continues one via session_id) and returns the curator's response. Requires API key."
    )]
    pub async fn swarm_curate(&self, parameters: Parameters<CurateRequest>) -> String {
        execute_tool_semantic(self, "swarm_curate", Some("pko"), async {
            self.client
                .require_auth()
                .map_err(SwarmError::into_tool_error)?;
            let req = parameters.0;
            if req.message.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "message must be non-empty".to_string(),
                ));
            }

            // Resolve or create the curator session.
            let session_id = match req.session_id {
                Some(id) => id,
                None => {
                    let created =
                        self.client
                            .post("/xaman/sessions", &serde_json::json!({}))
                            .await
                            .map_err(|e| match e {
                                SwarmError::Auth(m) => McpToolError::permission_denied(m),
                                other => SwarmError::CuratorUnavailable(other.to_string())
                                    .into_tool_error(),
                            })?;
                    created
                        .get("session_id")
                        .and_then(|s| s.as_str())
                        .map(str::to_string)
                        .ok_or_else(|| {
                            SwarmError::ApiVersionMismatch(
                                "xaman session create returned no session_id".to_string(),
                            )
                            .into_tool_error()
                        })?
                }
            };

            let data = self
                .client
                .post(
                    &format!("/xaman/sessions/{session_id}/message"),
                    &serde_json::json!({ "message": req.message }),
                )
                .await
                .map_err(SwarmError::into_tool_error)?;

            Ok(serde_json::json!({
                "session_id": session_id,
                "response": data.get("response"),
                "ready_to_create": data.get("ready_to_create"),
            }))
        })
        .await
    }
}

// ── Tool handler registration ──────────────────────────────────────────────

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for SwarmServer {}

// ── Entry point ────────────────────────────────────────────────────────────

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-swarm",
        SERVER_VERSION,
        |ctx| {
            let api_key = ctx.credentials.get("HKASK_ABW_API_KEY").cloned();
            let (config, warning) = SwarmConfig::from_env(api_key);
            // Catalogue-only mode is degraded, not broken — surface it so an
            // operator reading logs can distinguish "not configured" from
            // "configured but broken" (the startup-failure-signal rule).
            if let Some(w) = warning {
                tracing::warn!(target: "hkask.mcp.swarm", "{w}");
            }
            Ok(SwarmServer::new(
                ctx.webid,
                std::sync::Arc::new(SwarmClient::new(reqwest::Client::new(), config)),
            ))
        },
        vec![CredentialRequirement::optional(
            "HKASK_ABW_API_KEY",
            "Agent Bestiary World Pro API key (catalogue-only mode if absent)",
        )],
    )
    .await
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_error_detects_anthropic_credit_exhaustion() {
        let v = serde_json::json!({
            "response": "I encountered an error: Execution failed: API error: Your credit balance is too low to access the Anthropic API."
        });
        match detect_embedded_error(&v) {
            Some(SwarmError::UpstreamModelError { provider, .. }) => {
                assert_eq!(provider, "anthropic")
            }
            other => panic!("expected UpstreamModelError, got {other:?}"),
        }
    }

    #[test]
    fn embedded_error_detects_not_funded() {
        let v = serde_json::json!({
            "response": "Execution failed: Agent 'david_dunning' is not funded. Its owner has not set an ANTHROPIC_API_KEY."
        });
        match detect_embedded_error(&v) {
            Some(SwarmError::AgentNotFunded { agent, .. }) => {
                assert_eq!(agent, "david_dunning")
            }
            other => panic!("expected AgentNotFunded, got {other:?}"),
        }
    }

    #[test]
    fn embedded_error_ignores_clean_payload() {
        let v = serde_json::json!({"response": "The bestiary is a living ecology of AI agents."});
        assert!(detect_embedded_error(&v).is_none());
    }

    #[test]
    fn extract_quoted_pulls_agent_name() {
        assert_eq!(
            extract_quoted("Agent 'market_analyst' is not funded"),
            Some("market_analyst".to_string())
        );
        assert_eq!(extract_quoted("no quotes here"), None);
    }

    #[test]
    fn config_defaults_match_documented_surface() {
        let c = SwarmConfig::default();
        assert_eq!(c.api_base_url, "https://agent-bestiary.world");
        assert!(!c.curator_consent_default);
        assert!(c.api_key.is_none());
    }

    #[test]
    fn client_url_joins_apex_and_path() {
        let client = SwarmClient::new(reqwest::Client::new(), SwarmConfig::default());
        assert_eq!(
            client.url("/agents"),
            "https://agent-bestiary.world/api/agents"
        );
    }
}
