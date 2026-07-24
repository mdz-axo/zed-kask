//! MCP server and tool management routes
//!
//! These endpoints list and invoke MCP tools. Servers must be started
//! (e.g., via the `API_SERVERS` table in `serve.rs`) for tools to be
//! discoverable and invokable. If no servers are running, the listing
//! endpoints return empty results and invocations will fail.
//!
//! # Service layer depth test
//!
//! McpService was considered but **rejected** as shallow: every handler is a
//! thin delegation to `McpRuntime` methods (`list_servers`, `discover_tools`,
//! `invoke`, `get_tool_info`) plus HTTP response mapping.
//! No CLI MCP commands share this logic (CLI `commands/mcp.rs` uses a separate
//! `create_mcp_dispatcher_with_servers` path). An McpService would just be
//! `self.infra().mcp.clone().discover_tools()` — a pure pass-through.
//!
//! Decision: Guideline — keep direct `service_context.infra().mcp.clone()`/`mcp_dispatcher()`
//! access. Revisit if MCP orchestration logic (e.g., server health monitoring,
//! tool result caching) grows beyond simple discovery/invocation.

use axum::extract::Extension;
use axum::{Json, extract::State};
use hkask_capability::ToolPort;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;
use crate::error::ServiceErrorResponse;
use crate::middleware::auth::AuthContext;

/// Create MCP router
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with MCP routes registered
pub fn mcp_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_servers))
        .routes(routes!(list_tools))
        .routes(routes!(mcp_invoke))
}

/// List MCP servers
#[utoipa::path(
    get,
    path = "/api/mcp/servers",
    tag = "mcp",
    responses(
        (status = 200, description = "List of MCP servers", body = Vec<String>),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn list_servers(State(state): State<ApiState>) -> Json<Vec<String>> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "mcp_servers", "REG");
    let servers = state.agent_service.infra().mcp.clone().list_servers().await;
    Json(servers.iter().map(|s| s.id.clone()).collect())
}

/// List MCP tools
///
/// Returns all tools discovered from running MCP servers. If no servers
/// are started, the list will be empty.
#[utoipa::path(
    get,
    path = "/api/mcp/tools",
    tag = "mcp",
    responses(
        (status = 200, description = "List of MCP tool names", body = Vec<String>),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn list_tools(State(state): State<ApiState>) -> Json<Vec<String>> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "mcp_tools", "REG");
    let tools = state
        .agent_service
        .infra()
        .mcp
        .clone()
        .discover_tools()
        .await;
    Json(tools)
}

/// MCP invoke request — invoke a tool on a running MCP server.
///
/// `tool` is the fully-qualified tool name. Use GET /api/mcp/tools to
/// discover available tools before invoking. `input` is a JSON object of
/// tool arguments (defaults to {}).
#[derive(Debug, Deserialize, ToSchema)]
pub struct McpInvokeRequest {
    /// Tool name to invoke (e.g., "inference_generate")
    pub tool: String,
    /// JSON input arguments (defaults to null)
    #[serde(default)]
    pub input: serde_json::Value,
}

/// MCP invoke response — result of a tool invocation.
///
/// The `result` shape varies by tool. `server` identifies which MCP server
/// handled the invocation.
#[derive(Debug, Serialize, ToSchema)]
pub struct McpInvokeResponse {
    /// MCP server that provided the tool
    pub server: String,
    /// Tool name that was invoked
    pub tool: String,
    /// Result from the tool invocation
    pub result: serde_json::Value,
}

/// Invoke an MCP tool directly.
///
/// Requires authentication via Bearer token. Dispatches the tool call
/// through the MCP runtime with capability verification. The server
/// that owns the tool is resolved automatically from the tool name.
#[utoipa::path(
    post,
    path = "/api/mcp/invoke",
    tag = "mcp",
    request_body = McpInvokeRequest,
    responses(
        (status = 200, description = "Tool invocation result", body = McpInvokeResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Tool or server not found"),
        (status = 500, description = "Tool invocation error"),
    ),
)]
pub(crate) async fn mcp_invoke(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<McpInvokeRequest>,
) -> Result<Json<McpInvokeResponse>, ServiceErrorResponse> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "mcp_invoke", tool = %req.tool, "REG");

    // Input length validation
    if req.tool.len() > 256 {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: format!(
                "Tool name exceeds 256 byte limit (received {} bytes)",
                req.tool.len()
            ),
        }
        .into());
    }
    if let serde_json::Value::String(ref s) = req.input
        && s.len() > 1_000_000
    {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: format!("Input exceeds 1MB limit (received {} bytes)", s.len()),
        }
        .into());
    }

    let input = if req.input.is_null() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        req.input
    };

    let runtime = &state.agent_service.infra().mcp;
    let server_id = runtime
        .get_tool_info(&req.tool)
        .await
        .map(|tool| tool.server_id)
        .ok_or_else(|| ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::Infrastructure,
            message: format!("Tool '{}' is not registered", req.tool),
            source: None,
        })?;
    let result = runtime
        .invoke(
            &server_id,
            &req.tool,
            input,
            auth.token.as_ref().ok_or_else(|| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                message: "Session auth not supported for MCP invoke".to_string(),
                source: None,
            })?,
        )
        .await
        .map_err(|error| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Infrastructure,
            message: error.to_string(),
            source: None,
        })?;

    Ok(Json(McpInvokeResponse {
        server: server_id,
        tool: req.tool,
        result,
    }))
}
