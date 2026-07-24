//! `kask daemon` — Start the hKask daemon process
//!
//! Binds a Unix domain socket at `~/.config/hkask/daemon.sock`, starts the
//! Regulation runtime with all 6 loops, enables contract test monitoring, and serves
//! authentication/capability queries from MCP server binaries.
//!
//! The daemon is the persistent runtime that ACP clients, MCP servers, and
//! the `kask test` CLI surface all depend on. Without it, Regulation background
//! monitoring dies when the CLI command exits.

use crate::cli::DaemonAction;
use crate::error::CliError;
use hkask_mcp_server::daemon::{DaemonHandler, DaemonListener, daemon_socket_path, ping_daemon};
use std::sync::Arc;

/// pre:  rt is a valid tokio Runtime; action is a valid DaemonAction variant
/// post: starts, checks status, or stops the daemon; prints result to stdout
pub fn run(rt: &tokio::runtime::Runtime, action: DaemonAction) {
    match action {
        DaemonAction::Start => {
            if let Err(e) = rt.block_on(run_daemon()) {
                eprintln!("Daemon failed to start: {}", e);
                std::process::exit(1);
            }
        }
        DaemonAction::Status => {
            let path = daemon_socket_path();
            match rt.block_on(ping_daemon(&path)) {
                Ok(()) => {
                    println!("Daemon is running (socket: {})", path.display());
                }
                Err(reason) => {
                    if path.exists() {
                        println!(
                            "Daemon is not running (stale socket at {} — {})",
                            path.display(),
                            reason
                        );
                        println!(
                            "  Run `kask daemon stop` to remove the stale socket, then `kask daemon start`."
                        );
                    } else {
                        println!("Daemon is not running (no socket at {})", path.display());
                    }
                }
            }
        }
        DaemonAction::Stop => {
            let path = daemon_socket_path();
            if path.exists() {
                std::fs::remove_file(&path).ok();
                println!("Daemon socket removed.");
            } else {
                println!("Daemon is not running (no socket at {})", path.display());
            }
        }
    }
}

async fn run_daemon() -> Result<(), CliError> {
    // Build config from env. If the DB passphrase can't be resolved or the DB
    // can't be opened, fail fast — do NOT fall back to in-memory. The in-memory
    // fallback was corrupting disk databases because the daemon process would
    // touch disk DBs with an empty passphrase when the in-memory config was
    // used. The daemon must use the same encrypted DB as the REPL.
    let config = hkask_services_core::ServiceConfig::from_env().map_err(|e| {
        CliError::Daemon(format!(
            "Failed to resolve service config from env: {e}. \
             Set HKASK_DB_PASSPHRASE or store it in the keychain via `kask init`."
        ))
    })?;

    // Try build — if wallet fails, retry with in-memory config (wallet is
    // optional for the daemon; the DB is not).
    let nonce = std::sync::Arc::new(hkask_api::email::NonceStore::new(
        std::time::Duration::from_secs(86400),
    ));
    let ctx = match hkask_services_context::AgentService::build_with_email(
        config,
        hkask_api::email::CuratorAlertEmailSink::try_from_env_with_nonce(nonce.clone()),
    )
    .await
    {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::warn!(target: "hkask.daemon", error = %e, "Build with env config failed, retrying in-memory");
            hkask_services_context::AgentService::build_with_email(
                hkask_services_core::ServiceConfig::in_memory(),
                None,
            )
            .await
            .map_err(|e| {
                CliError::Daemon(format!(
                    "Failed to build service context (in-memory fallback): {e}"
                ))
            })?
        }
    };
    hkask_api::email::wire_inbox_poller(
        &ctx,
        hkask_api::email::inbox_poll_interval_secs(),
        Some(nonce),
    );
    hkask_api::email::wire_digest_task(&ctx);

    // Start the loop system
    ctx.ledger().loops.start().await;

    let loop_ids = ctx.ledger().loops.registered_loop_ids().await;
    tracing::info!(target: "hkask.daemon", loops_num = loop_ids.len(), "Loop system started");

    // Bind daemon socket
    let mut listener = DaemonListener::new();
    listener
        .bind()
        .await
        .map_err(|e| CliError::Daemon(format!("Failed to bind daemon socket: {e}")))?;

    let handler_raw = Arc::clone(&ctx.infra().daemon);
    let handler: Arc<dyn DaemonHandler> = handler_raw;

    let interval = std::env::var("HKASK_CONTRACT_TEST_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(3600);

    println!(
        "hKask daemon started — socket: {}, {} loops active",
        daemon_socket_path().display(),
        loop_ids.len()
    );
    println!(
        "Regulation contract monitoring active (interval: {}s)",
        interval
    );
    println!("Press Ctrl+C to stop.");

    tokio::select! {
        result = listener.serve(handler) => {
            if let Err(e) = result {
                eprintln!("Daemon serve error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down daemon...");
        }
    }

    ctx.ledger().loops.shutdown();
    println!("Daemon shut down.");
    Ok(())
}
