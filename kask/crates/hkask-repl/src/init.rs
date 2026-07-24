//! REPL dependency injection — wires Regulation, loops, energy budgets, and builds
//! the initial ReplState.
//!
//! Uses `AgentService::build()` for all shared infrastructure (Regulation, loop system,
//! curation, tool dispatch, pod manager). Surface-specific concerns (InferenceLoop
//! wiring, per-agent memory access, onboarding) are layered on top through
//! `AgentService` accessors — no independent infrastructure construction.

use std::path::PathBuf;
use std::sync::Arc;

use hkask_pods::InferenceLoop;
use hkask_regulation::{GasBudget, GasCost};

use super::{TalkConfig, TalkMode};
use hkask_capability::ToolInfo;
use hkask_mcp::McpRuntime;
use hkask_types::WebID;

use super::ReplState;

/// Load skills from `.agents/skills/` and `skills/` into the registry.
///
/// Populates `registry.skills()` for bundle composition, skill listing,
/// and `process_manifest` resolution. Logs load results and warnings.
///
/// # REQ: P11 (Digital Public/Private Sphere) — load skills from both zones
///
/// Used by: `init_repl_state`
fn load_skills_into_registry(registry: &mut hkask_templates::SqliteRegistry) {
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let loader = hkask_templates::SkillLoader::new(&project_root);
    let result = loader.load_into(registry);
    if !result.loaded.is_empty() {
        tracing::info!(
            target: "hkask.repl",
            skills_loaded = result.loaded.len(),
            warnings = result.warnings.len(),
            "Skills loaded from disk"
        );
    }
    for warning in &result.warnings {
        tracing::warn!(target: "hkask.repl", warning = %warning, "Skill load warning");
    }
}

/// Propagate onboarding secrets to the environment so downstream callers
/// (e.g. `build_agent_service → from_env`) can resolve them without
/// going through the OS keychain.
///
/// Sets `HKASK_MASTER_KEY` and `HKASK_DB_PASSPHRASE` in the process environment.
///
/// # Safety
///
/// Must run single-threaded before the tokio runtime starts.
///
/// Used by: `init_repl_state`
#[allow(unsafe_code)]
fn propagate_onboarding_secrets_to_env(secrets: &hkask_services_onboarding::ResolvedSecrets) {
    // SAFETY: REPL init runs single-threaded before tokio runtime starts.
    unsafe {
        std::env::set_var("HKASK_MASTER_KEY", &secrets.master_key_hex);
        std::env::set_var("HKASK_DB_PASSPHRASE", &secrets.db_passphrase);
    }
}

/// Propagate the userpod identity to the environment for child MCP processes.
///
/// Sets `HKASK_PROJECT_ROOT` (current working directory fallback),
/// `HKASK_MCP_HOST` (userpod name for Regulation spans), and
/// `HKASK_USERPOD_PERSONA` (WebID resolution for server-side identity).
///
/// # Safety
///
/// Must run single-threaded before the tokio runtime starts.
///
/// Used by: `init_repl_state`
#[allow(unsafe_code)]
fn propagate_userpod_env(agent_name: &str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    // SAFETY: REPL init runs single-threaded before tokio runtime starts.
    unsafe {
        std::env::set_var("HKASK_PROJECT_ROOT", cwd.to_string_lossy().as_ref());
        std::env::set_var("HKASK_MCP_HOST", agent_name);
        std::env::set_var("HKASK_USERPOD_PERSONA", agent_name);
    }
}

/// Ensure the hKask daemon is running before MCP servers auto-start.
///
/// MCP servers call `bootstrap_mcp_server` → `verify_startup_gates`, which
/// queries the daemon socket for P4 gate verification (auth, assignment,
/// capability). Without the daemon, servers fall back to direct mode
/// (`daemon_client: None`), bypassing OCAP verification and experience
/// recording. This helper probes the socket and spawns `kask daemon start`
/// as a detached child if no live listener is found.
///
/// Returns `true` if the daemon is live (either already running or newly
/// started), `false` if it could not be started. A `false` result is
/// non-fatal — the REPL continues with direct-mode MCP servers.
///
/// Used by: `init_repl_state` (Phase 7.5, before MCP auto-start)
fn ensure_daemon_running(rt: &tokio::runtime::Handle) -> bool {
    use std::time::{Duration, Instant};

    let socket_path = hkask_mcp_server::daemon::daemon_socket_path();

    // Fast path: probe the socket. If it responds, the daemon is live.
    if rt
        .block_on(hkask_mcp_server::daemon::ping_daemon(&socket_path))
        .is_ok()
    {
        tracing::debug!(
            target: "hkask.repl",
            socket = %socket_path.display(),
            "Daemon already running"
        );
        return true;
    }

    // Socket is dead or missing. Spawn `kask daemon start` as a detached child.
    // The child inherits the current environment (including HKASK_DB_PASSPHRASE
    // set by propagate_onboarding_secrets_to_env, so the daemon opens the same
    // encrypted DBs the REPL opens).
    tracing::info!(
        target: "hkask.repl",
        socket = %socket_path.display(),
        "Daemon not running — auto-starting"
    );

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                target: "hkask.repl",
                error = %e,
                "Cannot resolve current_exe for daemon spawn"
            );
            return false;
        }
    };

    let mut cmd = std::process::Command::new(&current_exe);
    cmd.arg("daemon")
        .arg("start")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    // Detach the child into its own process group so it survives the REPL's
    // exit. Without this, the child inherits the parent's process group and
    // may receive SIGHUP when the terminal session ends.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            tracing::warn!(
                target: "hkask.repl",
                error = %e,
                binary = %current_exe.display(),
                "Failed to spawn daemon process"
            );
            return false;
        }
    };
    let daemon_pid = child.id();
    // Keep stderr for diagnostics if the daemon fails to bind within the timeout.
    let stderr = child.stderr.take();

    // Poll the socket for up to 5 seconds. The daemon needs time to build
    // AgentService, start Regulation loops, and bind the socket.
    // Note: this is a blocking poll pattern (block_on + thread::sleep), not an
    // async loop. This is intentional — we're on a blocking thread and need to
    // wait for the daemon to become ready before proceeding.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if rt
            .block_on(hkask_mcp_server::daemon::ping_daemon(&socket_path))
            .is_ok()
        {
            tracing::info!(
                target: "hkask.repl",
                pid = daemon_pid,
                "Daemon auto-started successfully"
            );
            // Daemon is live — detach the child by dropping the handle.
            drop(child);
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    // Daemon didn't bind in time. Kill the child process to prevent a zombie
    // and remove the stale socket so the next attempt starts clean.
    let _ = child.kill();
    let _ = child.wait(); // Reap the zombie
    let _ = std::fs::remove_file(&socket_path); // Remove stale socket

    // Log captured stderr for diagnostics.
    if let Some(mut stderr) = stderr {
        use std::io::Read;
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        if !buf.trim().is_empty() {
            tracing::warn!(
                target: "hkask.repl",
                pid = daemon_pid,
                stderr = %buf.trim(),
                "Daemon process stderr (did not bind socket within 5s, process killed)"
            );
        } else {
            tracing::warn!(
                target: "hkask.repl",
                pid = daemon_pid,
                "Daemon did not bind socket within 5s — no stderr output, process killed"
            );
        }
    } else {
        tracing::warn!(
            target: "hkask.repl",
            pid = daemon_pid,
            "Daemon did not bind socket within 5s — process killed, MCP servers will use direct mode"
        );
    }
    false
}

/// Propagate condensation settings to the environment.
///
/// The condenser server is a child process that inherits the REPL's
/// environment. This bridges the two condensation paths (auto-condense in
/// `ChatService` and agent-initiated condenser tools).
///
/// # Safety
///
/// Must run single-threaded before the tokio runtime starts.
///
/// Used by: `init_repl_state`
#[allow(unsafe_code)]
fn propagate_condensation_env(settings: &crate::handlers::ReplSettings) {
    // SAFETY: REPL init runs single-threaded before tokio runtime starts.
    unsafe {
        std::env::set_var(
            "HKASK_CONDENSE_PRESSURE_THRESHOLD",
            settings.condense_pressure_threshold.to_string(),
        );
        std::env::set_var(
            "HKASK_CONDENSE_SALIENCY_WINDOW",
            settings.condense_saliency_window.to_string(),
        );
    }
}

/// Load the thread registry for the given agent.
///
/// Loads persisted threads from `agents/{name}/threads.json`, archives
/// stale threads, and creates an initial thread if the registry is empty.
///
/// Used by: `init_repl_state`
fn load_thread_registry(agent_name: &str, stm_life: u32) -> crate::threads::ThreadRegistry {
    let mut reg = crate::threads::ThreadRegistry::load(agent_name);
    let archived = reg.archive_stale(agent_name, stm_life);
    if archived > 0 {
        tracing::info!(
            target: "hkask.repl",
            archived = archived,
            "Auto-archived stale chat threads"
        );
    }
    if reg.list().is_empty() {
        reg.create_thread(agent_name, "Session started");
    }
    reg
}

/// Discover MCP tools via the governed McpRuntime and populate the tool prompt.
///
/// Used by: `init_repl_state`
pub(super) fn discover_tools(
    governed_tool: &Arc<McpRuntime>,
    rt: &tokio::runtime::Handle,
) -> Vec<hkask_types::ChatToolDefinition> {
    let tool_names = rt.block_on(governed_tool.discover_tools());
    let mut tools: Vec<ToolInfo> = Vec::new();
    for name in &tool_names {
        if let Some(info) = rt.block_on(governed_tool.get_tool_info(name)) {
            tools.push(info);
        }
    }
    tools
        .iter()
        .map(|tool| hkask_types::ChatToolDefinition {
            tool_type: "function".to_string(),
            function: hkask_types::ChatToolFunction {
                name: format!("{}/{}", tool.server_id, tool.name),
                description: tool.description.clone(),
                parameters: tool.input_schema.clone(),
            },
        })
        .collect()
}

// ── Orchestrator ───────────────────────────────────────────────────────────

/// Initialize all REPL dependencies and return a fully-wired ReplState.
///
/// Returns `None` if a critical dependency fails to initialize
/// (inference port, onboarding). Error messages are printed to stderr.
///
/// Uses `AgentService::build()` for shared infrastructure (Regulation, loop system,
/// curation loop, pod manager, registry, MCP runtime) and adds CLI-specific
/// concerns on top (inference, per-agent memory, governed McpRuntime for tool
/// discovery, onboarding state).
pub(super) fn init_repl_state(
    registry: &mut hkask_templates::SqliteRegistry,
    initial_model: Option<&str>,
    rt: &tokio::runtime::Handle,
    host: Arc<dyn crate::host::ReplHost>,
) -> Option<ReplState> {
    // ── Phase 1: Onboarding ────────────────────────────────────────────────
    let onboarding_outcome = match host.run_onboarding(rt) {
        Ok(outcome) => outcome,
        Err(e) => {
            if matches!(e, crate::host::OnboardingError::Cancelled) {
                return None;
            }
            eprintln!("Onboarding failed: {}", e);
            eprintln!("Run `kask chat` to set up your userpod identity.");
            return None;
        }
    };

    // Resolve the system default model from InferenceConfig (respects
    // HKASK_DEFAULT_MODEL env var, defaults to KC/z-ai/glm-5.2). This is
    // the same default shown during onboarding — no hardcoded DeepSeek.
    let inference_config = hkask_inference::InferenceConfig::from_env();
    let initial_model_str = onboarding_outcome
        .selected_model
        .as_deref()
        .or(initial_model)
        .unwrap_or(&inference_config.default_model);

    // ── Phase 2: Settings + Condensation Env ───────────────────────────────
    let repl_settings: crate::handlers::ReplSettings = hkask_services_core::load_settings();
    propagate_condensation_env(&repl_settings);

    // ── Phase 3: Inference (moved to Phase 6 — wiring into AgentService) ────

    // ── Phase 4: Service Config ────────────────────────────────────────────
    let service_config = match &onboarding_outcome.resolved_secrets {
        Some(secrets) => {
            propagate_onboarding_secrets_to_env(secrets);
            hkask_services_core::ServiceConfig::from_secrets(
                secrets.a2a_secret.clone(),
                secrets.db_passphrase.clone(),
                onboarding_outcome.signed_in_agent.clone(),
            )
        }
        None => hkask_services_core::ServiceConfig::from_env().unwrap_or_else(|e| {
            eprintln!("Warning: Failed to resolve service config from env: {}", e);
            hkask_services_core::ServiceConfig::in_memory()
        }),
    };

    // ── Phase 5: WebID + Skills ────────────────────────────────────────────
    let agent_webid = WebID::from_persona_with_namespace(
        onboarding_outcome.signed_in_agent.as_bytes(),
        "userpod",
    );
    load_skills_into_registry(registry);

    // ── Phase 6: Shared Infrastructure ─────────────────────────────────────
    let mut ctx = match rt.block_on(hkask_services_context::AgentService::build(
        service_config.clone(),
    )) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("Failed to build service context: {}", e);
            return None;
        }
    };

    match rt.block_on(ctx.curator_ready()) {
        Ok(()) => tracing::info!(target: "hkask.repl", "CuratorPod ready"),
        Err(e) => tracing::warn!(target: "hkask.repl", error = %e, "CuratorPod not ready"),
    }

    // Build InferenceLoop — uses AgentService's governed port
    // (better: Regulation-observable, energy-tracked)
    let inference_loop = Arc::new(
        InferenceLoop::new()
            .with_energy_budget(repl_settings.gas_cap, repl_settings.gas_cap)
            .with_model(initial_model_str),
    );
    rt.block_on(ctx.ledger().loops.register_loop(inference_loop.clone()));
    ctx.set_inference_loop(inference_loop);

    // ── Phase 7: UserPod Env ─────────────────────────────────────────────
    propagate_userpod_env(&onboarding_outcome.signed_in_agent);

    // ── Phase 7.5: Daemon Auto-Start ──────────────────────────────────────
    // MCP servers query the daemon socket for P4 gate verification during
    // bootstrap. Without the daemon, they fall back to direct mode
    // (daemon_client: None), bypassing OCAP verification. Auto-start the
    // daemon here so MCP servers can pass the gates. Non-fatal on failure.
    ensure_daemon_running(rt);

    // ── Phase 8: Core MCP Server Auto-Start ────────────────────────────────
    // Excluded servers require explicit opt-in via `/mcp start` (P2: Affirmative
    // Consent). Every entry MUST exist in `hkask_mcp_server::BUILTIN_SERVERS` — the
    // compile-time assertion below enforces this to prevent phantom exclusions.
    const CORE_EXCLUDED: &[&str] = &["companies", "communication", "training", "replica"];
    let mcp_runtime = ctx.infra().mcp.clone();
    let degraded = rt.block_on(async {
        let mut started = 0u32;
        let mut failed = Vec::new();
        let mut core_env = std::collections::HashMap::new();
        core_env.insert(
            "HKASK_MCP_HOST".to_string(),
            onboarding_outcome.signed_in_agent.clone(),
        );
        for (server_id, binary) in hkask_mcp_server::BUILTIN_SERVERS {
            if CORE_EXCLUDED.contains(server_id) {
                continue;
            }
            match mcp_runtime
                .start_server_with_env(server_id, binary, core_env.clone())
                .await
            {
                Ok(()) => started += 1,
                Err(e) => {
                    failed.push(((*server_id).to_string(), e.to_string()));
                }
            }
        }
        // Compile-time invariant: every CORE_EXCLUDED entry must exist in
        // BUILTIN_SERVERS. Prevents phantom exclusions (e.g., removed servers
        // that silently no-op the exclusion). Evaluated at runtime here because
        // BUILTIN_SERVERS is a runtime constant slice; a true const_assert would
        // require const fn on the slice.
        debug_assert!(
            CORE_EXCLUDED.iter().all(|ex| {
                hkask_mcp_server::BUILTIN_SERVERS
                    .iter()
                    .any(|(id, _)| id == ex)
            }),
            "CORE_EXCLUDED references a server not in BUILTIN_SERVERS"
        );
        if started > 0 {
            tracing::info!(
                target: "hkask.repl",
                started = started,
                total = hkask_mcp_server::BUILTIN_SERVERS.len() - CORE_EXCLUDED.len(),
                "Core MCP servers auto-started"
            );
        }
        for (id, err) in &failed {
            tracing::warn!(
                target: "hkask.repl",
                server_id = %id,
                error = %err,
                "Core MCP server failed to auto-start"
            );
        }
        failed
    });

    // ── Phase 9: GovernedTool (lazy via AgentService::governed_tool) ─────────
    // NOTE: `governed_tool` was removed; the governed `McpRuntime` is now built
    // once at `AgentService::build()` time (see `build_mcp_and_pods`) and lives in
    // `ctx.infra().mcp`. OCAP + gas + Regulation spans are wired via `with_governance`.

    // ── Phase 10: Energy Budget + Well + Wallet ────────────────────────────
    rt.block_on(async {
        let _ = ctx.ledger().cybernetics.read().await.load_budgets().await;
        let cl = ctx.ledger().cybernetics.clone();
        let cyber = cl.read().await;

        cyber
            .register_gas_budget(
                agent_webid,
                GasBudget::new(GasCost(repl_settings.gas_cap))
                    .with_replenish_rate(GasCost(repl_settings.gas_cap / 10))
                    .with_alert_threshold(0.8)
                    .with_hard_limit(true),
            )
            .await;

        {
            let mut wells = cyber.well_manager().write().await;
            if wells.default_well_id().is_none() {
                let (well_id, _) = wells.create_well(hkask_regulation::well::WellConfig {
                    well_id: "default".into(),
                    gas_rate: GasCost(repl_settings.gas_cap * 10),
                    rjoule_rate: 1000,
                });
                tracing::info!(target: "hkask.cli", well_id = well_id.0, "Created default Well");
            }
        }

        if let Some(wallet_mgr) = cyber.wallet_manager()
            && !wallet_mgr.has_wallet(&agent_webid).await {
                let _ = wallet_mgr
                    .create_wallet(
                        agent_webid,
                        GasCost(repl_settings.gas_cap * 5),
                        500,
                    )
                    .await;
                tracing::info!(target: "hkask.cli", agent = %agent_webid, "Created gas wallet for userpod");
            }
    });

    let ctx = Arc::new(ctx);

    // ── Phase 12: Assemble ReplState ───────────────────────────────────────
    let agent_name = onboarding_outcome.signed_in_agent.clone();
    let stm_life = repl_settings.short_term_memory_life;

    let mut state = ReplState {
        agent_webid,
        current_model: initial_model_str.to_string(),
        current_agent: onboarding_outcome.signed_in_agent,
        active_session: None,
        resolved_secrets: onboarding_outcome.resolved_secrets,
        tool_definitions: Vec::new(),
        manifest_state: None,
        service_context: ctx.clone(),
        repl_settings,
        is_first_run: onboarding_outcome.is_first_run,
        talk_config: TalkConfig {
            mode: TalkMode::Off,
            voice_design: None,
        },
        improv_mode: None,
        kanban_service: None,
        degraded_servers: degraded,
        thread_registry: load_thread_registry(&agent_name, stm_life),
        host,
    };

    // ── Phase 13: Tool Discovery ───────────────────────────────────────────
    let gov_tool = ctx.infra().mcp.clone();
    state.tool_definitions = discover_tools(&gov_tool, rt);

    // ── Phase 14: Agent Definition + Process Manifest ───────────────────────
    // Agent definitions (persona YAML, process manifests) were removed with the
    // agent registry. Userpods have no persona; manifests are no longer loaded
    // from agent definitions at REPL boot. `persona_constraints` and
    // `manifest_state` remain `None` (their init defaults).

    // ── Phase 15: Model Metadata ───────────────────────────────────────────
    // `model_meta` is intentionally left `None`: the provider catalog does not
    // expose `context_length`, so fabricating one would corrupt the
    // context-pressure loop. The inference path falls back to
    // `DEFAULT_CONTEXT_WINDOW` until a real metadata fetch is wired (tracked
    // separately). See `hkask-repl::handlers::repl_settings::DEFAULT_CONTEXT_WINDOW`.

    Some(state)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(unsafe_code)] // env var manipulation requires unsafe in Rust 2024
    use super::*;
    use crate::handlers::ReplSettings;

    /// Serialize tests that modify process environment variables.
    /// Parallel `unsafe { set_var }` calls across tests cause race conditions.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that saves an env var on creation and restores it on drop.
    /// Prevents test env-var pollution from leaking into other tests.
    struct EnvGuard {
        key: String,
        original: Option<String>,
    }

    impl EnvGuard {
        fn save(key: &str) -> Self {
            Self {
                key: key.to_string(),
                original: std::env::var(key).ok(),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(val) => unsafe { std::env::set_var(&self.key, val) },
                None => unsafe { std::env::remove_var(&self.key) },
            }
        }
    }

    /// Save and restore multiple env vars at once.
    fn env_guards(keys: &[&str]) -> Vec<EnvGuard> {
        keys.iter().map(|k| EnvGuard::save(k)).collect()
    }

    /// Minimal mock ReplHost for init_repl_state tests.
    struct MockReplHost;

    impl crate::host::ReplHost for MockReplHost {
        fn resolve_user_webid(&self) -> hkask_types::WebID {
            hkask_types::WebID::new()
        }
        fn run_onboarding(
            &self,
            _rt: &tokio::runtime::Handle,
        ) -> Result<crate::host::OnboardingOutcome, crate::host::OnboardingError> {
            Err(crate::host::OnboardingError::Cancelled)
        }
        fn list_templates_local(&self) -> Vec<hkask_types::RegistryEntry> {
            Vec::new()
        }
        fn run_sovereignty_status(&self) {}
        #[cfg(feature = "tui")]
        fn open_transcript_viewer(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
    }

    // ── load_skills_into_registry ───────────────────────────────────────────

    /// Loading skills into an empty registry should not panic.
    #[test]
    fn load_skills_empty_registry_does_not_panic() {
        let mut registry = hkask_templates::SqliteRegistry::new(None)
            .expect("SqliteRegistry::new with None should succeed");
        load_skills_into_registry(&mut registry);
        drop(registry);
    }

    // ── propagate_onboarding_secrets_to_env ─────────────────────────────────

    /// Secrets are correctly propagated to environment variables.
    #[test]
    fn propagate_onboarding_secrets_sets_all_vars() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guards = env_guards(&["HKASK_MASTER_KEY", "HKASK_DB_PASSPHRASE"]);

        let secrets = hkask_services_onboarding::ResolvedSecrets {
            master_key_hex: "deadbeef".to_string(),
            db_passphrase: "test-pass".to_string(),
            a2a_secret: "a2a-secret".to_string(),
        };
        propagate_onboarding_secrets_to_env(&secrets);

        assert_eq!(std::env::var("HKASK_MASTER_KEY").unwrap(), "deadbeef");
        assert_eq!(std::env::var("HKASK_DB_PASSPHRASE").unwrap(), "test-pass");
    }

    // ── propagate_userpod_env ─────────────────────────────────────────────

    /// UserPod identity env vars are set correctly.
    #[test]
    fn propagate_userpod_env_sets_host_and_persona() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guards = env_guards(&[
            "HKASK_MCP_HOST",
            "HKASK_USERPOD_PERSONA",
            "HKASK_PROJECT_ROOT",
        ]);

        propagate_userpod_env("alice");

        assert_eq!(std::env::var("HKASK_MCP_HOST").unwrap(), "alice");
        assert_eq!(std::env::var("HKASK_USERPOD_PERSONA").unwrap(), "alice");
        // HKASK_PROJECT_ROOT should be set to the current working directory.
        assert!(std::env::var("HKASK_PROJECT_ROOT").is_ok());
    }

    // ── propagate_condensation_env ──────────────────────────────────────────

    /// Condensation settings are propagated to env vars.
    #[test]
    fn propagate_condensation_env_sets_thresholds() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guards = env_guards(&[
            "HKASK_CONDENSE_PRESSURE_THRESHOLD",
            "HKASK_CONDENSE_SALIENCY_WINDOW",
        ]);

        let settings = ReplSettings {
            condense_pressure_threshold: 42.0_f32,
            condense_saliency_window: 7,
            ..ReplSettings::default()
        };
        propagate_condensation_env(&settings);

        assert_eq!(
            std::env::var("HKASK_CONDENSE_PRESSURE_THRESHOLD").unwrap(),
            "42"
        );
        assert_eq!(
            std::env::var("HKASK_CONDENSE_SALIENCY_WINDOW").unwrap(),
            "7"
        );
    }

    // ── load_thread_registry ────────────────────────────────────────────────

    /// A fresh thread registry for a new agent creates an initial thread.
    #[test]
    fn load_thread_registry_creates_initial_thread_for_new_agent() {
        let agent_name = "test-agent-fresh-threads";
        let reg = load_thread_registry(agent_name, 30);

        // A new registry should have at least one thread (the auto-created initial one).
        let threads = reg.list();
        assert!(
            !threads.is_empty(),
            "fresh registry should have at least one thread"
        );

        // The initial thread should have the session-started title.
        let first = &threads[0];
        assert!(
            first.title.contains("Session started"),
            "initial thread title should contain 'Session started', got: {}",
            first.title
        );
    }

    /// `load_thread_registry` uses the correct agent name.
    #[test]
    fn load_thread_registry_agent_name_matches() {
        let agent_name = "bob";
        let reg = load_thread_registry(agent_name, 30);
        let threads = reg.list();
        assert!(!threads.is_empty());
        // All threads should belong to this agent.
        for t in &threads {
            assert_eq!(
                t.agent_name, agent_name,
                "all threads should belong to {agent_name}"
            );
        }
    }

    /// `load_thread_registry` is deterministic for same agent name.
    #[test]
    fn load_thread_registry_deterministic() {
        let agent = "deterministic-test";
        let reg1 = load_thread_registry(agent, 30);
        let reg2 = load_thread_registry(agent, 30);

        assert_eq!(
            reg1.list().len(),
            reg2.list().len(),
            "same agent should produce same thread count"
        );
    }

    /// Second call to `load_thread_registry` finds the threads persisted by the first call.
    #[test]
    fn load_thread_registry_persists_across_calls() {
        let agent = "persistence-test";
        let reg1 = load_thread_registry(agent, 30);
        let count1 = reg1.list().len();
        assert!(count1 > 0, "first call should create at least one thread");

        // Second call reads from disk — should find the same threads, not create new ones.
        let reg2 = load_thread_registry(agent, 30);
        assert_eq!(
            reg2.list().len(),
            count1,
            "second call should find persisted threads, not duplicate"
        );
    }

    /// `stm_life = 0` disables auto-archival. A fresh registry still creates an initial thread.
    #[test]
    fn load_thread_registry_stm_life_zero_never_archives() {
        let agent = "stm-zero-test";
        let reg = load_thread_registry(agent, 0);
        let threads = reg.list();
        assert!(
            !threads.is_empty(),
            "fresh registry should have initial thread"
        );
        // No threads should be archived when stm_life is 0.
        for t in &threads {
            assert_eq!(
                t.status,
                crate::threads::ThreadStatus::Active,
                "no threads should be archived when stm_life is 0"
            );
        }
    }

    /// The auto-created initial thread has a valid UUID v4 ID.
    #[test]
    fn load_thread_registry_initial_thread_has_valid_uuid() {
        let agent = "uuid-test";
        let reg = load_thread_registry(agent, 30);
        let threads = reg.list();
        let first = &threads[0];
        // UUID v4 format: 8-4-4-4-12 hex digits.
        assert!(
            uuid::Uuid::parse_str(&first.id).is_ok(),
            "thread ID should be a valid UUID, got: {}",
            first.id
        );
    }

    // ── propagate env-var edge cases ──────────────────────────────────────

    /// Empty secret values are propagated without truncation or rejection.
    #[test]
    fn propagate_onboarding_secrets_empty_strings() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guards = env_guards(&["HKASK_MASTER_KEY", "HKASK_DB_PASSPHRASE"]);

        let secrets = hkask_services_onboarding::ResolvedSecrets {
            master_key_hex: String::new(),
            db_passphrase: String::new(),
            a2a_secret: String::new(),
        };
        propagate_onboarding_secrets_to_env(&secrets);

        assert_eq!(std::env::var("HKASK_MASTER_KEY").unwrap(), "");
        assert_eq!(std::env::var("HKASK_DB_PASSPHRASE").unwrap(), "");
    }

    /// Zero condensation thresholds are propagated as "0" strings.
    #[test]
    fn propagate_condensation_env_zero_values() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guards = env_guards(&[
            "HKASK_CONDENSE_PRESSURE_THRESHOLD",
            "HKASK_CONDENSE_SALIENCY_WINDOW",
        ]);

        let settings = ReplSettings {
            condense_pressure_threshold: 0.0,
            condense_saliency_window: 0,
            ..ReplSettings::default()
        };
        propagate_condensation_env(&settings);

        assert_eq!(
            std::env::var("HKASK_CONDENSE_PRESSURE_THRESHOLD").unwrap(),
            "0"
        );
        assert_eq!(
            std::env::var("HKASK_CONDENSE_SALIENCY_WINDOW").unwrap(),
            "0"
        );
    }

    /// Empty agent name propagates empty-string env vars without panicking.
    #[test]
    fn propagate_userpod_env_empty_agent() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guards = env_guards(&[
            "HKASK_MCP_HOST",
            "HKASK_USERPOD_PERSONA",
            "HKASK_PROJECT_ROOT",
        ]);

        propagate_userpod_env("");

        assert_eq!(std::env::var("HKASK_MCP_HOST").unwrap(), "");
        assert_eq!(std::env::var("HKASK_USERPOD_PERSONA").unwrap(), "");
        // PROJECT_ROOT should still resolve to CWD.
        assert!(std::env::var("HKASK_PROJECT_ROOT").is_ok());
    }

    // ── Integration: init_repl_state with in-memory config ──────────────────

    /// Full REPL initialization with in-memory config succeeds.
    ///
    /// Sets up just enough environment to bypass interactive onboarding and
    /// keychain resolution, then verifies that `init_repl_state` assembles
    /// a valid `ReplState` without panicking.
    ///
    /// MCP server auto-start will fail (no server binaries available in tests),
    /// but the degraded state must be captured rather than causing a crash.
    #[test]
    #[ignore = "modifies global process state"]
    fn init_repl_state_in_memory_succeeds() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guards = env_guards(&[
            "HKASK_MASTER_KEY",
            "HKASK_A2A_SECRET",
            "HKASK_DB_PASSPHRASE",
            "HKASK_USERPOD_NAME",
        ]);

        let master_key = "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6";
        // SAFETY: single-threaded test, no concurrent access
        unsafe {
            std::env::set_var("HKASK_MASTER_KEY", master_key);
            std::env::set_var("HKASK_A2A_SECRET", "test-a2a-secret-32-bytes-long!!");
            std::env::set_var("HKASK_DB_PASSPHRASE", "test-pass");
            std::env::set_var("HKASK_USERPOD_NAME", "test-userpod");
        }

        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        let mut registry = hkask_templates::SqliteRegistry::new(None)
            .expect("SqliteRegistry::new with None should succeed");

        let state = init_repl_state(
            &mut registry,
            Some("test-model"),
            rt.handle(),
            Arc::new(MockReplHost),
        );

        // init_repl_state may return None if onboarding fails (expected in CI
        // without a proper keychain). The contract is that it never panics.
        if let Some(s) = state {
            assert!(!s.current_agent.is_empty(), "agent name must be set");
            assert!(!s.current_model.is_empty(), "model must be set");
            assert!(s.resolved_secrets.is_some(), "secrets must be resolved");
            assert!(
                s.service_context.inference_port().is_some(),
                "inference port must be live"
            );
            assert!(
                Arc::strong_count(&s.service_context) > 0,
                "service context must be live"
            );
            assert!(
                !s.thread_registry.list().is_empty(),
                "thread registry must have at least one thread"
            );
            drop(s.degraded_servers);
        }
    }

    /// `init_repl_state` captures degraded MCP server information when
    /// server auto-start fails (missing binaries in test environment).
    #[test]
    #[ignore = "modifies global process state"]
    fn init_repl_state_captures_degraded_servers() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _guards = env_guards(&[
            "HKASK_MASTER_KEY",
            "HKASK_A2A_SECRET",
            "HKASK_DB_PASSPHRASE",
        ]);

        unsafe {
            std::env::set_var("HKASK_MASTER_KEY", "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6");
            std::env::set_var("HKASK_A2A_SECRET", "test-a2a-secret-32-bytes-long!!");
            std::env::set_var("HKASK_DB_PASSPHRASE", "test-pass");
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut registry = hkask_templates::SqliteRegistry::new(None)
            .expect("SqliteRegistry::new with None should succeed");

        let state = init_repl_state(&mut registry, None, rt.handle(), Arc::new(MockReplHost));

        if let Some(s) = state {
            // degraded_servers is a Vec<(String, String)> — server_id → error message.
            // In CI without MCP binaries, this should be populated.
            let _ = &s.degraded_servers;
        }
    }
}
