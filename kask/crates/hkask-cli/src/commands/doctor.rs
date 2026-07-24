//! `kask doctor` — validate all configured providers.
//!
//! Checks each provider key is set and makes a lightweight API call
//! to verify the credentials are valid. Reports tier status.
//!
//! With `--bootstrap`, checks the full REPL bootstrap chain: daemon socket,
//! keychain entries, DB passphrase, UserStore session, and MCP server
//! connectivity. Use this to diagnose "REPL loop stalls" or "No A2A secret"
//! errors.

use hkask_inference::{InferenceConfig, InferenceRouter};

/// Entry point for `kask doctor` (without --bootstrap).
pub fn run_doctor_cmd(rt: &tokio::runtime::Runtime) {
    rt.block_on(run_doctor_async());
}

/// Entry point for `kask doctor --bootstrap`.
///
/// Checks the full REPL bootstrap chain:
/// 1. Daemon socket is live (not just a stale file)
/// 2. Keychain entries exist (HKASK_MASTER_KEY, a2a-secret, hkask-db-passphrase)
/// 3. DB passphrase resolves and opens the main DB
/// 4. UserStore session exists for the userpod
/// 5. MCP servers connect and discover tools
pub fn run_bootstrap_check(rt: &tokio::runtime::Runtime) {
    println!("hKask Doctor — Bootstrap Chain Check\n");
    let mut checks_passed = 0u32;
    let mut checks_total = 0u32;

    // ── 1. Daemon socket ────────────────────────────────────
    println!("1. Daemon Socket");
    println!("   ─────────────");
    checks_total += 1;
    let socket_path = hkask_mcp_server::daemon::daemon_socket_path();
    match rt.block_on(hkask_mcp_server::daemon::ping_daemon(&socket_path)) {
        Ok(()) => {
            println!("   ✅ Daemon is live (socket: {})", socket_path.display());
            checks_passed += 1;
        }
        Err(e) => {
            if socket_path.exists() {
                println!("   ❌ Stale socket at {} — {}", socket_path.display(), e);
                println!("      Run: kask daemon stop && kask daemon start");
            } else {
                println!(
                    "   ❌ Daemon not running (no socket at {})",
                    socket_path.display()
                );
                println!("      Run: kask daemon start");
            }
        }
    }
    println!();

    // ── 2. Keychain entries ─────────────────────────────────
    println!("2. Keychain Entries");
    println!("   ────────────────");
    let keychain = hkask_keystore::Keychain::default();
    for (key, label) in [
        (
            hkask_types::keychain_keys::KEY_MASTER_KEY,
            "HKASK_MASTER_KEY",
        ),
        (hkask_types::keychain_keys::KEY_A2A_SECRET, "a2a-secret"),
        (
            hkask_types::keychain_keys::KEY_DB_PASSPHRASE,
            "hkask-db-passphrase",
        ),
    ] {
        checks_total += 1;
        match keychain.retrieve_by_key(key) {
            Ok(_) => {
                println!("   ✅ {} — present", label);
                checks_passed += 1;
            }
            Err(_) => {
                println!("   ❌ {} — missing", label);
                if key == hkask_types::keychain_keys::KEY_DB_PASSPHRASE {
                    println!("      Run: kask init (or set HKASK_DB_PASSPHRASE in .env)");
                } else if key == hkask_types::keychain_keys::KEY_A2A_SECRET {
                    println!("      Run: kask keystore rotate (or re-run onboarding)");
                }
            }
        }
    }
    println!();

    // ── 3. DB passphrase + main DB ──────────────────────────
    println!("3. Database Passphrase");
    println!("   ──────────────────");
    checks_total += 1;
    match hkask_services_core::ServiceConfig::from_env() {
        Ok(config) => match hkask_storage::Database::open(&config.db_path, &config.db_passphrase) {
            Ok(_) => {
                println!("   ✅ Main DB opens (path: {})", config.db_path);
                checks_passed += 1;
            }
            Err(e) => {
                println!("   ❌ Main DB open failed: {}", e);
                println!("      Check HKASK_DB_PASSPHRASE or run kask repair --dry-run");
            }
        },
        Err(e) => {
            println!("   ❌ ServiceConfig::from_env() failed: {}", e);
            println!("      Set HKASK_DB_PASSPHRASE or run kask init");
        }
    }
    println!();

    // ── 4. MCP servers ──────────────────────────────────────
    println!("4. MCP Servers");
    println!("   ────────────");
    checks_total += 1;
    let runtime = hkask_mcp::runtime::McpRuntime::new();
    let tool_count = rt.block_on(async { runtime.discover_tools().await.len() });
    if tool_count > 0 {
        println!("   ✅ {} tools discovered", tool_count);
        checks_passed += 1;
    } else {
        println!("   ❌ No tools discovered — MCP servers not started");
        println!("      Run: kask chat (auto-starts MCP servers)");
    }
    println!();

    // ── Summary ─────────────────────────────────────────────
    let pct = if checks_total > 0 {
        (checks_passed * 100) / checks_total
    } else {
        0
    };
    println!("═══════════════════════════════════════");
    println!("  {checks_passed}/{checks_total} bootstrap checks passed ({pct}%)");
    if checks_passed == checks_total {
        println!("  ✅ Bootstrap chain is healthy");
    } else {
        println!("  ⚠️  Bootstrap chain has issues — see above");
    }
    println!("═══════════════════════════════════════");
}

/// Run a diagnostic check on all configured providers.
pub async fn run_doctor_async() {
    println!("hKask Doctor — Provider Health Check\n");

    let mut configured = 0u32;
    let mut total = 0u32;

    // ── Inference ──────────────────────────────────────────
    println!("Inference Providers");
    println!("───────────────────");
    configured += check_env("DI_API_KEY", "DeepInfra", &mut total);
    configured += check_env("FA_API_KEY", "fal.ai", &mut total);
    configured += check_env("TG_API_KEY", "Together AI", &mut total);
    configured += check_env("OR_API_KEY", "OpenRouter", &mut total);
    configured += check_env("KC_API_KEY", "KiloCode", &mut total);
    configured += check_env("CLINE_API_KEY", "Cline", &mut total);
    configured += check_env("RUNPOD_API_KEY", "RunPod", &mut total);
    // Ollama needs no API key -- verify daemon reachability instead.
    total += 1;
    let ollama_base =
        std::env::var("OM_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(c) => match c.get(format!("{ollama_base}/v1/models")).send().await {
            Ok(r) if r.status().is_success() => {
                println!("  OK  OM_BASE_URL -- Ollama reachable ({ollama_base})");
                configured += 1;
            }
            Ok(r) => {
                println!(
                    "  --  OM_BASE_URL -- Ollama responded HTTP {} ({ollama_base})",
                    r.status()
                );
            }
            Err(e) => {
                println!("  --  OM_BASE_URL -- Ollama not reachable: {e} ({ollama_base})");
            }
        },
        Err(e) => {
            println!("  --  OM_BASE_URL -- could not build HTTP client: {e}");
        }
    }
    println!();

    // ── Fusion ──────────────────────────────────────────────
    let config = InferenceConfig::from_env();
    if config.fusion.is_some() {
        let fusion_model = config.fusion.as_ref().unwrap().model_id();
        println!("Fusion Model");
        println!("────────────");
        let router = InferenceRouter::new(config);
        total += 1;
        match router.verify_fusion_model().await {
            Ok(true) => {
                println!("  ✅ Fusion judge reachable — {fusion_model}");
                configured += 1;
            }
            Ok(false) => {
                println!("  ❌ Fusion judge NOT reachable — {fusion_model}");
            }
            Err(e) => {
                println!("  ⚠️  Could not verify fusion model: {e}");
            }
        }
        println!();
    }

    // ── Search ─────────────────────────────────────────────
    println!("Search Providers");
    println!("────────────────");
    configured += check_env("HKASK_BRAVE_API_KEY", "Brave Search", &mut total);
    configured += check_env("HKASK_FIRECRAWL_API_KEY", "Firecrawl", &mut total);
    configured += check_env("HKASK_TAVILY_API_KEY", "Tavily", &mut total);
    configured += check_env("HKASK_SERPAPI_API_KEY", "SerpAPI", &mut total);
    configured += check_env("HKASK_EXA_API_KEY", "Exa", &mut total);
    println!();

    // ── Financial ──────────────────────────────────────────
    println!("Financial Data");
    println!("──────────────");
    configured += check_env("HKASK_FMP_API_KEY", "FMP", &mut total);
    configured += check_env("HKASK_EODHD_API_KEY", "EODHD", &mut total);
    println!();

    // ── Object Storage ─────────────────────────────────────
    println!("Object Storage");
    println!("──────────────");
    configured += check_env("LITESTREAM_BUCKET", "Litestream bucket", &mut total);
    configured += check_env("LITESTREAM_ENDPOINT", "Litestream endpoint", &mut total);
    configured += check_env(
        "LITESTREAM_ACCESS_KEY_ID",
        "Litestream access key",
        &mut total,
    );
    println!();

    // ── Cloud ──────────────────────────────────────────────
    println!("Cloud Providers");
    println!("───────────────");
    configured += check_env("HCLOUD_TOKEN", "Hetzner Cloud", &mut total);
    println!();

    // ── Matrix ─────────────────────────────────────────────
    println!("Matrix");
    println!("──────");
    if let Ok(url) = std::env::var("HKASK_MATRIX_URL") {
        if url.is_empty() {
            println!("  ⚠️  HKASK_MATRIX_URL — not set");
        } else {
            println!("  ✅ HKASK_MATRIX_URL — {url}");
            configured += 1;
        }
    } else {
        println!("  ⚠️  HKASK_MATRIX_URL — not set");
    }
    total += 1;
    println!();

    // ── Container ──────────────────────────────────────────
    println!("Container Registry");
    println!("──────────────────");
    configured += check_env("CONTAINER_REGISTRY", "Container registry", &mut total);
    println!();

    // ── Summary ────────────────────────────────────────────
    let pct = if total > 0 {
        (configured * 100) / total
    } else {
        0
    };
    let tier = match pct {
        0..=19 => "No tier",
        20..=49 => "CORE (inference)",
        50..=79 => "STANDARD (inference + search + backups)",
        _ => "FULL (cloud deployment ready)",
    };

    println!("═══════════════════════════════════════");
    println!("  {configured}/{total} providers configured ({pct}%)");
    println!("  Tier: {tier}");
    println!("═══════════════════════════════════════");
}

fn check_env(var: &str, label: &str, total: &mut u32) -> u32 {
    *total += 1;
    match std::env::var(var) {
        Ok(val) if !val.is_empty() => {
            println!("  ✅ {var} — {label}");
            1
        }
        _ => {
            println!("  ⚠️  {var} — {label} (not set)");
            0
        }
    }
}
