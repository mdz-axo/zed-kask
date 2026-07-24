//! hKask CLI — Binary entry point
//!
//! Thin dispatcher: setup → route to command handler → done.
//! All business logic and display formatting lives in the `commands` module.

#![allow(unused_crate_dependencies)] // All deps used in this binary — lint produces false positives

use clap::Parser;
use hkask_cli::cli::Commands;
use hkask_cli::commands;
use hkask_inference::InferenceConfig;
use hkask_templates::BundleRegistryIndex;
use hkask_templates::SqliteRegistry;
use hkask_templates::load_manifest_from_yaml;
use std::time::Instant;

/// Check fusion model configuration at startup.
///
/// Fusion is opt-in — only active when HKASK_FUSION_JUDGE_MODEL
/// and HKASK_FUSION_PANEL_MODELS are explicitly set.
fn check_fusion_startup() {
    let config = InferenceConfig::from_env();
    let fusion = match &config.fusion {
        Some(f) => f,
        None => return,
    };

    let (model, desc) = (fusion.model_id(), fusion.description());
    eprintln!(
        "\n  \x1b[1;33m⚡ Fusion mode active\x1b[0m — model: \x1b[36m{model}\x1b[0m\n     {desc}"
    );
}

/// ── Main ─────────────────────────────────────────────────────────────────
fn main() {
    // Secrets are resolved from the OS keychain (preferred) or environment
    // variables. The .env file is deprecated — use `kask keystore load` to
    // load keys into the keychain from a key_load_template.env file.
    let cli = hkask_cli::cli::Cli::parse();
    hkask_cli::cli::init_logging(cli.verbose, cli.json_logs);

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let handle = rt.handle().clone();

    // Verify fusion model if configured (P9: proactive cost-safety)
    check_fusion_startup();

    let mut registry = commands::helpers::or_exit(
        match &cli.registry {
            Some(path) => SqliteRegistry::new(Some(&path.to_string_lossy())),
            None => SqliteRegistry::new(None),
        },
        "Failed to initialize registry",
    );

    // Register platform engineering FlowDef manifests at startup.
    // These are loyalty-anchored platform maintenance skills.
    // Non-fatal: if parsing fails, log and continue.
    {
        let platform_manifests: [(&str, &str); 8] = [
            (
                "platform-governance-transparency-reporter",
                include_str!(
                    "../../../registry/manifests/platform-governance-transparency-reporter.yaml"
                ),
            ),
            (
                "platform-consent-auditor",
                include_str!("../../../registry/manifests/platform-consent-auditor.yaml"),
            ),
            (
                "platform-portability-verifier",
                include_str!("../../../registry/manifests/platform-portability-verifier.yaml"),
            ),
            (
                "platform-health-scorer",
                include_str!("../../../registry/manifests/platform-health-scorer.yaml"),
            ),
            (
                "platform-loyalty-scorecard",
                include_str!("../../../registry/manifests/platform-loyalty-scorecard.yaml"),
            ),
            (
                "platform-bulkhead-auditor",
                include_str!("../../../registry/manifests/platform-bulkhead-auditor.yaml"),
            ),
            (
                "platform-wardley-mapper",
                include_str!("../../../registry/manifests/platform-wardley-mapper.yaml"),
            ),
            (
                "platform-dx-analyzer",
                include_str!("../../../registry/manifests/platform-dx-analyzer.yaml"),
            ),
        ];
        for (name, yaml) in platform_manifests {
            match load_manifest_from_yaml(yaml) {
                Ok(bundle) => {
                    if let Err(e) = registry.register_bundle(bundle) {
                        tracing::warn!(target: "bootstrap", name, error = %e, "Failed to register platform manifest");
                    } else {
                        tracing::info!(target: "bootstrap", name, "Registered platform engineering manifest");
                    }
                }
                Err(e) => {
                    tracing::warn!(target: "bootstrap", name, error = %e, "Failed to load platform manifest");
                }
            }
        }
    }

    // P9: Regulation span
    let reg_start = Instant::now();
    tracing::info!(
        target: "hkask.cli",
        operation = "command_dispatched",
        command = %cli.command.label(),
        "REG"
    );

    match cli.command {
        Commands::Tui {
            template,
            input,
            mcp_servers,
            agent,
            model,
        } => {
            commands::tui::run_tui(
                &rt,
                &mut registry,
                &handle,
                template,
                input,
                mcp_servers,
                agent,
                model,
            );
        }

        Commands::Pod { action } => commands::pod::run_pod(&rt, action),

        Commands::Mcp { action } => commands::mcp::run(&rt, action),

        Commands::Sovereignty { action } => commands::sovereignty::run(action),

        Commands::Git { action } => commands::git_cmd::run(&rt, action),

        Commands::Backup { action } => commands::backup_cmd::run(&rt, action),

        Commands::Token { action } => commands::token::run_token(&rt, action),

        Commands::UserPod { action } => commands::user::run_userpod(&rt, action),

        Commands::Keystore { action } => commands::keystore::run(action),

        Commands::Skill { action } => commands::skill::run(action),

        Commands::Doctor { bootstrap } => {
            if bootstrap {
                commands::doctor::run_bootstrap_check(&rt);
            } else {
                commands::doctor::run_doctor_cmd(&rt);
            }
        }

        Commands::Onboard => {
            // Multi-persona onboarding was removed (1:1 model: one persistent
            // UserPod per user). `kask onboard` no longer adds userpods.
            eprintln!("hKask now uses a single persistent UserPod per user (1:1 model).");
            eprintln!("Multi-persona onboarding is no longer supported.");
            eprintln!("To set up your first UserPod, run: \x1b[36mkask chat\x1b[0m");
            std::process::exit(0);
        }

        Commands::Settings { action } => commands::settings::run(action),

        Commands::Daemon { action } => commands::daemon::run(&rt, action),

        Commands::Serve {
            port: _port,
            host: _host,
        } => {
            #[cfg(feature = "api")]
            {
                if let Err(e) = rt.block_on(commands::serve::run_server(_port, &_host)) {
                    eprintln!("Server error: {}", e);
                    std::process::exit(1);
                }
            }
            #[cfg(not(feature = "api"))]
            {
                eprintln!("HTTP API server not built — rebuild with `cargo build --features api`");
                std::process::exit(1);
            }
        }

        Commands::Init => {
            if let Err(e) = commands::init::run_init() {
                eprintln!("Init error: {}", e);
                std::process::exit(1);
            }
        }

        Commands::Export { action } => {
            commands::export_cmd::run(&rt, action);
        }

        Commands::Wallet { action } => commands::wallet::run(&rt, action),

        Commands::Matrix { action } => commands::matrix::run(action),

        Commands::Repair { dry_run, force } => commands::repair::run(dry_run, force),

        Commands::Deploy { action } => commands::deploy::run(&rt, action),
    }

    // P9: Regulation span
    tracing::info!(target: "hkask.cli", operation = "command_completed", latency_ms = reg_start.elapsed().as_millis(), "REG");
}
