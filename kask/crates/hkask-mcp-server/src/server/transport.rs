//! Server bootstrap — credential resolution, WebID derivation, and rmcp stdio serve.

use std::collections::HashMap;
use std::str::FromStr;

use super::context::{CapabilityTier, CredentialRequirement, ServerContext};
use super::credentials::{load_dotenv, resolve_credential};
use super::error::McpError;

/// Common bootstrap for hKask MCP server binaries.
///
/// Handles:
/// 1. Tracing subscriber initialization
/// 2. Credential requirement checks (.env file → keystore → env var)
/// 3. WebID resolution (HKASK_WEBID → HKASK_USERPOD_NAME → anonymous)
/// 4. Server construction via factory (only after credential checks pass)
/// 5. rmcp stdio serve
///
/// The factory pattern ensures server constructors that need credentials
/// only run AFTER credential availability is confirmed. The factory receives
/// a `ServerContext` containing resolved credentials and shared infrastructure
/// — no ambient authority via `std::env::var`.
///
/// # Arguments
/// - `server_name` — Human-readable server name for logging (e.g., `"hkask-mcp-web"`)
/// - `version` — SemVer version string (use `env!("CARGO_PKG_VERSION")`)
/// - `server_factory` — Closure that constructs the server, receiving a `ServerContext`
/// - `credentials` — Declared credential requirements
///
/// # Errors
/// Returns an error if a required credential is missing or server construction fails.
#[must_use = "result must be used"]
pub async fn run_stdio_server<S, F>(
    server_name: &str,
    version: &str,
    server_factory: F,
    credentials: Vec<CredentialRequirement>,
) -> Result<(), McpError>
where
    S: rmcp::ServiceExt<rmcp::RoleServer> + rmcp::Service<rmcp::RoleServer>,
    F: FnOnce(ServerContext) -> Result<S, McpError>,
{
    let preloaded = load_dotenv();
    run_stdio_server_impl(
        server_name,
        version,
        server_factory,
        credentials,
        Some(preloaded),
    )
    .await
}

/// Like `run_stdio_server`, but with pre-resolved credentials from .env files.
///
/// Preloaded credentials take precedence over `resolve_credential()` results.
/// This allows .env file values to be injected without mutating the process
/// environment (no `unsafe set_var`).
#[must_use = "result must be used"]
pub async fn run_stdio_server_with_preloaded<S, F>(
    server_name: &str,
    version: &str,
    server_factory: F,
    credentials: Vec<CredentialRequirement>,
    preloaded: HashMap<String, String>,
) -> Result<(), McpError>
where
    S: rmcp::ServiceExt<rmcp::RoleServer> + rmcp::Service<rmcp::RoleServer>,
    F: FnOnce(ServerContext) -> Result<S, McpError>,
{
    run_stdio_server_impl(
        server_name,
        version,
        server_factory,
        credentials,
        Some(preloaded),
    )
    .await
}

/// Unified stdio server bootstrap — resolves credentials, constructs ServerContext,
/// and serves via rmcp stdio transport. Accepts optional preloaded credentials
/// for .env file injection (used by `run_stdio_server_with_preloaded`).
async fn run_stdio_server_impl<S, F>(
    server_name: &str,
    version: &str,
    server_factory: F,
    credentials: Vec<CredentialRequirement>,
    preloaded: Option<HashMap<String, String>>,
) -> Result<(), McpError>
where
    S: rmcp::ServiceExt<rmcp::RoleServer> + rmcp::Service<rmcp::RoleServer>,
    F: FnOnce(ServerContext) -> Result<S, McpError>,
{
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut resolved = HashMap::new();
    let mut missing_required = Vec::new();
    for cred in &credentials {
        if let Some(ref pre) = preloaded
            && let Some(val) = pre.get(&cred.env_var)
        {
            tracing::debug!(credential = %cred.env_var, source = "preloaded", "Credential resolved from preloaded .env");
            resolved.insert(cred.env_var.clone(), val.clone());
            continue;
        }
        match resolve_credential(&cred.env_var) {
            Ok(val) => {
                tracing::debug!(credential = %cred.env_var, "Credential resolved");
                resolved.insert(cred.env_var.clone(), val);
            }
            Err(_) if cred.required => {
                tracing::error!(credential = %cred.env_var, description = %cred.description, "Required credential not set — server cannot function");
                missing_required.push(cred.env_var.clone());
            }
            Err(_) => {
                tracing::info!(credential = %cred.env_var, description = %cred.description, "Optional credential not set — server will use default or in-memory fallback")
            }
        }
    }
    if !missing_required.is_empty() {
        return Err(McpError::MissingCredentials {
            missing: missing_required.join(", "),
        });
    }

    let webid = if let Some(ref pre) = preloaded {
        if let Some(uuid_str) = pre.get("HKASK_WEBID") {
            hkask_types::WebID::from_str(uuid_str).unwrap_or_else(|_| {
                tracing::warn!(
                    "HKASK_WEBID set but invalid format — falling back to anonymous identity"
                );
                hkask_types::WebID::from_persona(b"anonymous")
            })
        } else if let Ok(uuid_str) = std::env::var("HKASK_WEBID") {
            hkask_types::WebID::from_str(&uuid_str).unwrap_or_else(|_| {
                tracing::warn!(
                    "HKASK_WEBID set but invalid format — falling back to anonymous identity"
                );
                hkask_types::WebID::from_persona(b"anonymous")
            })
        } else if let Some(persona) = pre.get("HKASK_USERPOD_NAME") {
            hkask_types::WebID::from_persona(persona.as_bytes())
        } else if let Ok(persona) = std::env::var("HKASK_USERPOD_NAME") {
            hkask_types::WebID::from_persona(persona.as_bytes())
        } else {
            tracing::warn!(
                "No HKASK_WEBID or HKASK_USERPOD_NAME set — MCP server starting with anonymous identity. Set HKASK_WEBID for P12-compliant attribution."
            );
            hkask_types::WebID::from_persona(b"anonymous")
        }
    } else if let Ok(uuid_str) = std::env::var("HKASK_WEBID") {
        hkask_types::WebID::from_str(&uuid_str).unwrap_or_else(|_| {
            tracing::warn!(
                "HKASK_WEBID set but invalid format — falling back to anonymous identity"
            );
            hkask_types::WebID::from_persona(b"anonymous")
        })
    } else if let Ok(persona) = std::env::var("HKASK_USERPOD_NAME") {
        hkask_types::WebID::from_persona(persona.as_bytes())
    } else {
        tracing::warn!(
            "No HKASK_WEBID or HKASK_USERPOD_NAME set — MCP server starting with anonymous identity. Set HKASK_WEBID for P12-compliant attribution."
        );
        hkask_types::WebID::from_persona(b"anonymous")
    };

    tracing::info!(webid = %webid.redacted_display(), "Agent identity resolved");
    let capability_tier = CapabilityTier::detect(&resolved);
    tracing::info!(
        embedded = capability_tier.embedded,
        keystore = capability_tier.keystore_available,
        persistence = capability_tier.persistence_available,
        "Capability tier detected"
    );
    let ctx = ServerContext {
        credentials: resolved,
        webid,
        capability_tier,
    };
    let server = server_factory(ctx)?;
    tracing::info!(
        server = server_name,
        version = version,
        "MCP server starting"
    );
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(rmcp::RmcpError::from)?;
    running.waiting().await.map_err(rmcp::RmcpError::from)?;
    Ok(())
}
