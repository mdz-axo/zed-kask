//! Shared helper functions for CLI command handlers

use std::path::{Path, PathBuf};

use hkask_services_context::AgentService;
use hkask_services_core::ServiceConfig;
use hkask_services_onboarding::ResolvedSecrets;
use hkask_templates::SqliteRegistry;
use hkask_types::RegistryIndex;

use crate::error::CliError;

/// Unwrap a `Result` or print an error message and exit.
pub fn or_exit<T, E: std::fmt::Display>(result: Result<T, E>, label: &str) -> T {
    match result {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}: {}", label, e);
            std::process::exit(1);
        }
    }
}

/// List templates from an in-memory SqliteRegistry (for REPL host use).
///
/// Returns an empty Vec on registry creation failure (graceful degradation).
pub fn list_templates_local() -> Vec<hkask_types::RegistryEntry> {
    let registry = match SqliteRegistry::new(None) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(target: "hkask.cli", error = %e, "SqliteRegistry in-memory failed, retrying");
            match SqliteRegistry::new(None) {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(target: "hkask.cli", error = %e, "SqliteRegistry in-memory failed twice, returning empty");
                    return Vec::new();
                }
            }
        }
    };
    registry.list(None)
}

/// Build an AgentService from environment config.
///
/// Always creates a fresh tokio runtime. Suitable for one-shot CLI commands.
pub fn build_agent_service() -> AgentService {
    or_exit(
        build_agent_service_inner(None),
        "Failed to build AgentService",
    )
}

/// Build an AgentService from environment config, returning Result.
///
/// For callers that need graceful error handling (e.g., async functions
/// that return `Result<_, ServiceError>`). The panicking variant
/// `build_agent_service()` is preferred for one-shot CLI entry points.
pub fn build_agent_service_result() -> Result<AgentService, CliError> {
    build_agent_service_inner(None)
}

/// Build an AgentService from pre-resolved secrets (used by the chat path).
///
/// Returns `Result` — callers handle errors gracefully rather than exiting.
pub fn build_agent_service_from_secrets(
    from_secrets: Option<(&str, &ResolvedSecrets)>,
) -> Result<AgentService, CliError> {
    build_agent_service_inner(from_secrets)
}

/// Shared implementation — safe to call from any context.
fn build_agent_service_inner(
    from_secrets: Option<(&str, &ResolvedSecrets)>,
) -> Result<AgentService, CliError> {
    let config = match from_secrets {
        Some((name, secrets)) => ServiceConfig::from_secrets(
            secrets.a2a_secret.clone(),
            secrets.db_passphrase.clone(),
            name.to_string(),
        ),
        None => ServiceConfig::from_env()
            .map_err(|e| CliError::Config(format!("Failed to resolve service config: {}", e)))?,
    };
    // `Runtime::block_on` panics if the current thread is driving a tokio
    // runtime (e.g., when called from within `rt.block_on(...)`). Use
    // `block_in_place` to move blocking work to the blocking thread pool.
    match tokio::runtime::Handle::try_current() {
        Ok(_handle) => tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| CliError::Config(format!("Failed to create runtime: {}", e)))?;
            rt.block_on(AgentService::build_with_email(
                config,
                hkask_api::email::CuratorAlertEmailSink::try_from_env(),
            ))
            .map_err(|e| CliError::AgentService(e.to_string()))
        }),
        Err(_) => {
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| CliError::Config(format!("Failed to create runtime: {}", e)))?;
            rt.block_on(AgentService::build_with_email(
                config,
                hkask_api::email::CuratorAlertEmailSink::try_from_env(),
            ))
            .map_err(|e| CliError::AgentService(e.to_string()))
        }
    }
}

/// Write content to a file or print to stdout.
pub fn write_or_print(content: &str, output: Option<&Path>, label: &str) {
    match output {
        Some(path) => {
            std::fs::write(path, content).unwrap_or_else(|e| {
                eprintln!("Failed to write {} to {}: {}", label, path.display(), e);
                std::process::exit(1);
            });
        }
        None => println!("{}", content),
    }
}

/// Resolve the current user's WebID.
pub fn resolve_user_webid() -> hkask_types::WebID {
    let name = std::env::var("HKASK_USER_WEBID").unwrap_or_else(|_| "cli-user".to_string());
    hkask_types::WebID::from_persona_with_namespace(name.as_bytes(), "userpod")
}

/// Resolve an agent name from an optional argument or environment.
pub fn resolve_userpod_name(userpod: Option<&str>) -> String {
    if let Some(name) = userpod {
        return name.to_string();
    }
    std::env::var("HKASK_MCP_HOST").unwrap_or_else(|_| "anonymous".to_string())
}

/// Resolve a WebID from an optional argument or derive from agent name.
pub fn resolve_webid(userpod: Option<&str>) -> hkask_types::WebID {
    let name = resolve_userpod_name(userpod);
    hkask_types::WebID::from_persona_with_namespace(name.as_bytes(), "userpod")
}

/// Build the standard userpod environment variable map for MCP subprocesses.
///
/// Sets `HKASK_MCP_HOST` (userpod identity) and `HKASK_USERPOD_PERSONA`
/// (WebID resolution). If `HKASK_DB_PASSPHRASE` is set in the current process,
/// it is forwarded so subprocess servers can decrypt their per-agent databases.
pub(crate) fn userpod_env_map(userpod_name: &str) -> std::collections::HashMap<String, String> {
    let mut env = std::collections::HashMap::new();
    env.insert("HKASK_MCP_HOST".to_string(), userpod_name.to_string());
    env.insert(
        "HKASK_USERPOD_PERSONA".to_string(),
        userpod_name.to_string(),
    );
    // Pass DB passphrase to subprocess — the server resolves it from env var
    // via resolve_db_passphrase_string(), which checks std::env::var first.
    // Without this, the subprocess uses the keychain passphrase which may differ.
    if let Ok(passphrase) = std::env::var("HKASK_DB_PASSPHRASE") {
        env.insert("HKASK_DB_PASSPHRASE".to_string(), passphrase);
    }
    env
}

/// Start a specific MCP server.
pub fn start_mcp_server(
    rt: &tokio::runtime::Runtime,
    ctx: &AgentService,
    server_id: &str,
    binary: &str,
) -> bool {
    let mcp_runtime = ctx.infra().mcp.clone();
    let userpod_name = ctx.config().user_name.clone();
    let env = userpod_env_map(&userpod_name);
    match rt.block_on(
        mcp_runtime
            .as_ref()
            .start_server_with_env(server_id, binary, env),
    ) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("Failed to start MCP server '{}': {}", server_id, e);
            false
        }
    }
}

/// Start MCP servers with custom environment.
pub fn start_mcp_servers_with_env(
    rt: &tokio::runtime::Runtime,
    ctx: &AgentService,
    servers: &[(&str, &str)],
    userpod_name: &str,
) {
    let mcp_runtime = ctx.infra().mcp.clone();
    let env = userpod_env_map(userpod_name);
    for (server_id, binary) in servers {
        if let Err(e) = rt.block_on(mcp_runtime.as_ref().start_server_with_env(
            server_id,
            binary,
            env.clone(),
        )) {
            eprintln!("Failed to start MCP server '{}': {}", server_id, e);
        }
    }
}

/// Resolve the deploy directory from environment or default.
pub fn resolve_deploy_dir() -> Result<PathBuf, CliError> {
    if let Ok(d) = std::env::var("HKASK_DEPLOY_DIR") {
        let p = PathBuf::from(&d);
        if p.is_dir() {
            return Ok(p);
        }
        return Err(CliError::Config(format!(
            "HKASK_DEPLOY_DIR set but not a directory: {d}"
        )));
    }
    let cwd = std::env::current_dir().map_err(|e| CliError::Io(format!("current_dir: {e}")))?;
    let candidate = cwd.join("deploy").join("k8s");
    if candidate.is_dir() {
        return Ok(candidate);
    }
    Err(CliError::Config(format!(
        "deploy/k8s not found at {} and HKASK_DEPLOY_DIR not set",
        cwd.display()
    )))
}

/// Print a list of items with a header.
pub fn print_item_list<T>(
    items: &[T],
    empty_label: &str,
    label: &str,
    format_item: impl Fn(&T) -> String,
) {
    if items.is_empty() {
        println!("{}", empty_label);
        return;
    }
    println!("{} ({}):", label, items.len());
    for item in items {
        println!("  - {}", format_item(item));
    }
}

#[cfg(test)]
mod tests {
    /// Regression: `AgentService::build()` must not panic when called from
    /// inside `rt.block_on()`. The old `build_service_context_inner` used
    /// `Handle::block_on` which panicked with "Cannot start a runtime from
    /// within a runtime" when a tokio handle was active.
    ///
    /// Uses `ServiceConfig::in_memory()` to avoid touching real databases
    /// or OS keychain — this test only needs to verify the block_on contract.
    #[test]
    fn build_agent_service_from_within_block_on() {
        struct RestoreMasterKey(Option<std::ffi::OsString>);

        impl Drop for RestoreMasterKey {
            fn drop(&mut self) {
                unsafe {
                    match self.0.take() {
                        Some(value) => std::env::set_var("HKASK_MASTER_KEY", value),
                        None => std::env::remove_var("HKASK_MASTER_KEY"),
                    }
                }
            }
        }

        let _restore = RestoreMasterKey(std::env::var_os("HKASK_MASTER_KEY"));
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
            );
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        // This must not panic — the original bug caused a panic here
        let _svc = rt.block_on(async {
            hkask_services_context::AgentService::build(
                hkask_services_core::ServiceConfig::in_memory(),
            )
            .await
            .unwrap()
        });
    }
}
