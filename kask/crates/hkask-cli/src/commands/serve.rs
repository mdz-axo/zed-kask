#![cfg(feature = "api")]
//! `kask serve` — Start the HTTP API server sharing CLI state
//!
//! Creates an `ApiState` via `AgentService::build()` and starts the
//! axum HTTP server. CLI commands issued while the server is running
//! operate on the same shared state.

use hkask_api::ApiState;
use hkask_mcp::runtime::McpRuntime;
use hkask_mcp_server::BUILTIN_SERVERS;

/// Run the API server, sharing state with the CLI.
///
/// Resolves configuration from the keystore and environment, builds a
/// `AgentService` with all shared infrastructure, starts API MCP servers
/// on the AgentService's runtime, and creates an `ApiState` from it.
/// pre:  port is a valid u16; host is a non-empty bind address string
/// post: starts the HTTP API server on the given host:port; returns Ok(()) on successful bind or Error on failure
pub async fn run_server(port: u16, host: &str) -> Result<(), Box<dyn std::error::Error>> {
    // P9: Regulation span
    tracing::info!(target: "hkask.cli", operation = "serve", host = %host, port = port, "REG");
    // Resolve configuration from keystore and environment.
    // Refuse to start with in-memory fallback — a server without proper
    // keystore configuration has no security, no persistence, and no auth.
    let config = hkask_services_core::ServiceConfig::from_env().map_err(|e| {
        format!(
            "Failed to resolve service configuration: {e}\n\
             Run 'kask init' first to set up the server, or 'kask chat' to\n\
             complete onboarding with your master passphrase."
        )
    })?;

    // Build AgentService with all shared infrastructure
    let nonce = std::sync::Arc::new(hkask_api::email::NonceStore::new(
        std::time::Duration::from_secs(86400),
    ));
    let email_sink =
        hkask_api::email::CuratorAlertEmailSink::try_from_env_with_nonce(nonce.clone());
    let ctx = hkask_services_context::AgentService::build_with_email(config, email_sink)
        .await
        .map_err(|e| {
            Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error>
        })?;
    hkask_api::email::wire_inbox_poller(
        &ctx,
        hkask_api::email::inbox_poll_interval_secs(),
        Some(nonce),
    );
    hkask_api::email::wire_digest_task(&ctx);

    // Start API MCP servers on the AgentService's runtime.
    // Derived from hkask_mcp_server::BUILTIN_SERVERS (canonical registry).
    let userpod_name = ctx.config().user_name.clone();
    let server_count = start_api_servers(&ctx.infra().mcp, &userpod_name).await;
    if server_count > 0 {
        tracing::info!(target: "hkask.serve", servers = server_count, "MCP servers started");
    } else {
        tracing::warn!(target: "hkask.serve", "No MCP servers started — tool endpoints will return empty results");
    }

    // Build ApiState from AgentService
    let state = ApiState::from_service_context(ctx)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    // Build router (OpenApiRouter -> axum::Router via From impl)
    let app: axum::Router = hkask_api::create_router(state).into();

    // Swagger UI at /docs — serves spec from /api-docs/openapi.json
    let app = app.route("/docs", axum::routing::get(swagger_ui));

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(target: "hkask.serve", addr = %addr, "Starting hKask API server");
    println!("hKask API server listening on {}", addr);

    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

/// Start all API MCP servers and discover their tools.
///
/// Started servers are derived from `hkask_mcp_server::BUILTIN_SERVERS`.
/// excluded from the API sandbox for security: the API server is a
/// headless HTTP endpoint — it does not expose local filesystem access,
/// governance, registry management, or kanban task coordination.
const API_EXCLUDED: &[&str] = &["filesystem", "curator", "kanban", "skill"];

async fn start_api_servers(runtime: &McpRuntime, userpod_name: &str) -> usize {
    let mut started = 0;
    let extra_env = super::helpers::userpod_env_map(userpod_name);

    for (server_id, command) in BUILTIN_SERVERS {
        if API_EXCLUDED.contains(server_id) {
            continue;
        }
        match runtime
            .start_server_with_env(server_id, command, extra_env.clone())
            .await
        {
            Ok(()) => {
                tracing::debug!(target: "hkask.serve", server_id = %server_id, "MCP server started");
                started += 1;
            }
            Err(e) => {
                tracing::warn!(
                    target: "hkask.serve",
                    server_id = %server_id,
                    error = %e,
                    "Failed to start MCP server — tools will be unavailable"
                );
            }
        }
    }

    started
}

/// Serve Swagger UI HTML page that loads the OpenAPI spec from /api-docs/openapi.json.
async fn swagger_ui() -> axum::response::Html<&'static str> {
    axum::response::Html(SWAGGER_HTML)
}

const SWAGGER_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>hKask API Docs</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js" crossorigin></script>
  <script>
    SwaggerUIBundle({
      url: "/api-docs/openapi.json",
      dom_id: "#swagger-ui",
      deepLinking: true,
      presets: [SwaggerUIBundle.presets.apis],
    });
  </script>
</body>
</html>"##;
