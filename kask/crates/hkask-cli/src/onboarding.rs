//! Session resolution for hKask CLI.
//!
//! Two modes:
//! - **Operating mode**: keys configured, a userpod exists — sign in, no prompts.
//! - **Setup**: no keys or no userpod — create the user's first (and only) userpod.
//!
//! hKask is a tool platform for users. Each user has exactly one persistent
//! UserPod (1:1, Solid-Pod-modeled). No multi-persona, no spin-up/spin-down.
//!
//! After setup, derived secrets are stored in the OS keychain for future
//! sessions.

use hkask_inference::InferenceConfig;
use hkask_inference::{FusionMode, ProviderId};
pub use hkask_repl::host::OnboardingOutcome;
use hkask_services_core::{DomainKind, ErrorKind, ServiceConfig, ServiceError};
use hkask_services_onboarding::ResolvedSecrets;
use thiserror::Error;

use hkask_repl::display;

mod discovery;
pub(crate) mod ui;
pub(crate) use ui::read_line;
use ui::{prompt_choice, prompt_line, prompt_passphrase, prompt_passphrase_with_confirm};

#[derive(Error, Debug)]
pub enum OnboardingError {
    #[error("Onboarding cancelled by user")]
    Cancelled,
    #[error(transparent)]
    Service(#[from] hkask_services_core::ServiceError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Public entry points ────────────────────────────────────────────────────

/// Resolve the user's session.
///
/// Operating mode: keys work and a userpod exists — sign in, no prompts.
/// Setup: no keys or no userpod — create the user's first userpod.
///
/// In operating mode, this also creates a UserStore session for the userpod
/// so that the daemon's `check_auth` (which queries `list_sessions`) returns
/// `authenticated: true`. Without this, MCP servers bootstrapping via
/// `verify_startup_gates` would fall back to direct mode (daemon_client: None),
/// bypassing P4 OCAP verification.
pub async fn run_onboarding() -> Result<OnboardingOutcome, OnboardingError> {
    // Operating mode: keys work and at least one userpod exists.
    if let Ok(config) = ServiceConfig::from_env() {
        let userpods = list_userpods(&config);
        if let Ok(pods) = userpods
            && !pods.is_empty()
        {
            // 1:1 model — one userpod per user. Sign in with the first (only) pod.
            let userpod_name = pods[0].userpod_name.clone();

            // Ensure the userpod's directory space exists on disk.
            let _ = hkask_types::agent_paths::ensure_userpod_dirs(&userpod_name);

            // Create a UserStore session so the daemon authenticates the userpod.
            match create_user_session(&config, &userpod_name) {
                Ok(session_id) => {
                    tracing::info!(
                        target: "hkask.onboarding",
                        userpod = %userpod_name,
                        session_id = %session_id,
                        "Created UserStore session for daemon authentication"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.onboarding",
                        userpod = %userpod_name,
                        error = %e,
                        "UserStore login failed — MCP servers will fall back to direct mode"
                    );
                }
            }

            // Resolve secrets from keychain for the REPL.
            let resolved_secrets = resolve_secrets_from_keychain(&config);

            return Ok(OnboardingOutcome {
                signed_in_agent: userpod_name,
                resolved_secrets,
                selected_model: None,
                is_first_run: false,
            });
        }
    }

    // Setup: create the user's first userpod.
    display::print_onboarding_banner();
    create_first_userpod_flow().await
}

// ── Private helpers ────────────────────────────────────────────────────────

/// pre:  user must not cancel at any interactive prompt
/// post: returns OnboardingOutcome with signed_in_agent, resolved_secrets, selected_model, is_first_run=true; all secrets derived and stored in keychain; userpod created in SQLCipher DB; userpod registered in A2A
/// inv:  does not modify any external state before derive_secrets; cancellation at any prompt returns OnboardingError::Cancelled with zero side effects
/// Flow: Create the user's first (and only) userpod.
///
/// Prompt order per spec: username → password (confirm) → email → model.
async fn create_first_userpod_flow() -> Result<OnboardingOutcome, OnboardingError> {
    println!("\n  \x1b[1mWelcome to hKask!\x1b[0m");
    println!("  Let's set up your account.\n");

    // Q1: Username (becomes the userpod name)
    let username = prompt_line("  Choose a username:")?;
    let username = username.trim().to_string();
    if username.is_empty() {
        return Err(OnboardingError::Cancelled);
    }

    // Q2: Password (with confirm)
    println!();
    println!("  Choose a \x1b[1mmaster passphrase\x1b[0m to encrypt your data.");
    println!("  This passphrase derives all your internal security keys.");
    println!("  \x1b[2mStore it in a password manager — it cannot be recovered if lost.\x1b[0m");
    let passphrase = prompt_passphrase_with_confirm()?;

    // Q3: Email
    println!();
    let email = prompt_line("  Your email address:")?;
    let email = email.trim().to_string();

    // Q4: Model selection
    println!();
    println!("  \x1b[1mChoose a model\x1b[0m for your userpod to use.");
    println!("  Models determine how your userpod thinks and responds.");
    setup_provider().await?;
    let selected_model = select_model().await?;

    // Use the username as first_name, empty last_name (1:1 model — no persona).
    let first_name = username.clone();
    let last_name = String::new();

    // ── Run the state machine for all service calls ──
    use crate::onboarding_session::OnboardingSession;
    let session = OnboardingSession::new(username, email, first_name, last_name);
    let completed = session
        .run(|| Ok(selected_model.clone()), || Ok(passphrase.clone()))
        .await
        .map_err(|(_session, e)| e)?;

    // Post-creation summary
    print_creation_summary(&completed.userpod_name, &completed.selected_model);

    // Create the userpod's directory space on disk.
    let _ = hkask_types::agent_paths::ensure_userpod_dirs(&completed.userpod_name);

    Ok(OnboardingOutcome {
        signed_in_agent: completed.userpod_name,
        resolved_secrets: completed.resolved_secrets,
        selected_model: Some(completed.selected_model),
        is_first_run: true,
    })
}

/// Resolve the display name for the currently active provider.
pub(crate) fn provider_display_name(config: &InferenceConfig) -> &'static str {
    match config.default_provider {
        ProviderId::KiloCode => "KiloCode",
        ProviderId::DeepInfra => "DeepInfra",
        ProviderId::Together => "Together AI",
        ProviderId::Fal => "fal.ai",
        ProviderId::OpenRouter => "OpenRouter",
        _ => "your provider",
    }
}

/// Let the user select a model using the dynamic discovery pipeline.
async fn select_model() -> Result<String, OnboardingError> {
    let config = InferenceConfig::from_env();
    let default_model = config.default_model.clone();

    // Run the discovery pipeline (OpenRouter → filters → top 12 → fallback)
    let (models, source_label) = discovery::discover_models(&config).await;
    let is_dynamic = models
        .first()
        .map(|m| m.source == discovery::ModelSource::Dynamic)
        .unwrap_or(false);

    let display_source = if is_dynamic {
        format!("via {}", source_label)
    } else {
        source_label
    };

    println!("  \x1b[1mAvailable models\x1b[0m ({}):", display_source);
    println!();

    // Group by family for structured display
    let mut families: Vec<String> = models.iter().map(|m| m.family.clone()).collect();
    families.sort();
    families.dedup();

    let mut idx = 1usize;
    for family in &families {
        let family_models: Vec<&discovery::OnboardingModel> =
            models.iter().filter(|m| &m.family == family).collect();
        if family_models.is_empty() {
            continue;
        }

        println!("  \x1b[1m{}\x1b[0m", discovery::shorten_for_display(family));
        for m in &family_models {
            let marker = if m.full_id == default_model {
                " \x1b[33m(default)\x1b[0m"
            } else {
                ""
            };
            let kind_icon = match m.kind {
                discovery::ModelKind::Thinking => "\x1b[35m🧠\x1b[0m",
                discovery::ModelKind::Instruct => "\x1b[32m📋\x1b[0m",
            };
            println!(
                "    {}. {} \x1b[36m{}\x1b[0m{}  \x1b[2m{}\x1b[0m",
                idx, kind_icon, m.label, marker, m.description
            );
            idx += 1;
        }
        println!();
    }

    // Offer hKask fusion (our own orchestrator, provider-agnostic)
    let fusion_configured = config.fusion.is_some();
    if fusion_configured {
        let fusion = config
            .fusion
            .as_ref()
            .map(|f| {
                let mode = match f.mode {
                    FusionMode::BestOfN => "Best-of-N",
                    FusionMode::Synthesis => "Synthesis",
                    FusionMode::Critique => "Critique",
                    FusionMode::Deliberation => "Deliberation",
                    FusionMode::PlanImplement => "Plan/Implement",
                };
                format!("⚡ Fusion [{}] — {}", mode, f.description())
            })
            .unwrap_or_else(|| "⚡ Fusion (kask defaults)".to_string());
        println!("    {}. \x1b[1;33m{}\x1b[0m", idx, fusion);
        idx += 1;
        println!();
    }

    let manual_idx = idx;
    println!("    {}. Enter a model name manually", manual_idx);
    println!();

    let choice = prompt_choice(
        &format!(
            "  Select a model (1-{}, default: \x1b[36m{}\x1b[0m):",
            manual_idx, default_model
        ),
        1..=(manual_idx),
    );

    let fusion_idx = models.len() + 1;
    let model_count = models.len();
    match choice {
        Ok(n) if n <= model_count => Ok(models[n - 1].full_id.clone()),
        Ok(n) if fusion_configured && n == fusion_idx => Ok(config
            .fusion
            .as_ref()
            .map(|f| f.model_id())
            .unwrap_or_else(|| "fusion/default".to_string())),
        Ok(_) => {
            let input = prompt_line("  Model name:")?;
            if input.trim().is_empty() {
                Ok(default_model)
            } else {
                Ok(input.trim().to_string())
            }
        }
        Err(e) => Err(e),
    }
}

/// Interactive provider setup during first-run onboarding.
///
/// Checks if a provider API key is already configured (env var or keychain —
/// Secrets are resolved from the OS keychain at startup). If not, prompts to enter a key
/// directly or skip.
async fn setup_provider() -> Result<(), OnboardingError> {
    let config = InferenceConfig::from_env();

    // Check if any cloud provider is already configured
    let has_deepinfra = !config.deepinfra_api_key.is_empty();
    let has_together = !config.together_api_key.is_empty();
    let has_fal = !config.fal_api_key.is_empty();
    let has_kilocode = !config.kilocode_api_key.is_empty();
    let has_openrouter = !config.openrouter_api_key.is_empty();
    let has_cline = !config.cline_api_key.is_empty();

    if has_deepinfra || has_together || has_fal || has_kilocode || has_openrouter || has_cline {
        let provider_name = if has_kilocode {
            "KiloCode"
        } else if has_openrouter {
            "OpenRouter"
        } else if has_cline {
            "Cline"
        } else if has_deepinfra {
            "DeepInfra"
        } else if has_together {
            "Together AI"
        } else {
            "fal.ai"
        };
        println!(
            "  \x1b[32m✓\x1b[0m {} API key found — using {} as default provider.",
            provider_name, provider_name
        );
        // Auto-load into keychain so future sessions don't need .env in cwd
        let keychain = hkask_keystore::Keychain::default();
        if has_kilocode {
            let _ = keychain.store_by_key("KC_API_KEY", &config.kilocode_api_key);
        }
        if has_openrouter {
            let _ = keychain.store_by_key("OR_API_KEY", &config.openrouter_api_key);
        }
        if has_cline {
            let _ = keychain.store_by_key("CLINE_API_KEY", &config.cline_api_key);
        }
        if has_deepinfra {
            let _ = keychain.store_by_key("DI_API_KEY", &config.deepinfra_api_key);
        }
        if has_together {
            let _ = keychain.store_by_key("TG_API_KEY", &config.together_api_key);
        }
        if has_fal {
            let _ = keychain.store_by_key("FA_API_KEY", &config.fal_api_key);
        }
        return Ok(());
    }

    // No cloud provider configured — prompt the user
    println!("  \x1b[1mInference provider\x1b[0m");
    println!();
    println!("  hKask requires an inference provider to generate responses.");
    println!("  Without one, your userpod cannot reply to you.");
    println!();
    println!("  An API key is like a password that lets hKask use a cloud");
    println!("  AI service. You can get a free key at:");
    println!();
    println!(
        "    \x1b[36mhttps://kilo.ai/\x1b[0m         (KiloCode — unified gateway, recommended)"
    );
    println!("    \x1b[36mhttps://deepinfra.com/\x1b[0m  (free tier, wide model catalog)");
    println!("    \x1b[36mhttps://together.ai/\x1b[0m  (inference + fine-tuning)");
    println!("    \x1b[36mhttps://fal.ai/\x1b[0m      (specialized vision/OCR models)");
    println!();
    println!("  Set your key in .env (auto-loaded at startup) or enter it now.");
    println!();
    println!("    1. Enter API key directly (input is hidden)");
    println!("    2. Skip for now");
    println!();

    let choice = prompt_choice("  Choice (1-2):", 1..=2)?;

    match choice {
        1 => {
            // Enter API key directly
            println!();
            println!("  Supported providers:");
            println!("    KC — KiloCode (unified gateway, recommended)");
            println!("    OR — OpenRouter (multi-provider gateway)");
            println!("    DI — DeepInfra (wide model catalog)");
            println!("    TG — Together AI (inference + fine-tuning)");
            println!("    FA — fal.ai (specialized vision/OCR models)");
            println!("    CL — Cline (OpenAI-compatible cloud gateway)");
            println!();

            let provider_str = prompt_line("  Provider code (KC/OR/DI/TG/FA/CL):")?;
            let provider_str = provider_str.trim().to_uppercase();

            let key_name = match provider_str.as_str() {
                "KC" => "KC_API_KEY",
                "OR" => "OR_API_KEY",
                "DI" => "DI_API_KEY",
                "TG" => "TG_API_KEY",
                "FA" => "FA_API_KEY",
                "CL" => "CLINE_API_KEY",
                _ => {
                    println!(
                        "  \x1b[31m✗\x1b[0m Unknown provider '{}'. Use KC, OR, DI, TG, FA, or CL.",
                        provider_str
                    );
                    return Err(OnboardingError::Cancelled);
                }
            };

            let api_key = prompt_passphrase(&format!("  {} API key:", key_name))?;
            let api_key = api_key.trim();
            if api_key.is_empty() {
                println!("  No key entered — skipping provider setup.");
                return Ok(());
            }

            let keychain = hkask_keystore::Keychain::default();
            keychain.store_by_key(key_name, api_key).map_err(|e| {
                eprintln!("  \x1b[31m✗\x1b[0m Failed to store key: {}", e);
                OnboardingError::Service(ServiceError::Domain {
                    kind: ErrorKind::BadRequest,
                    domain: DomainKind::Infrastructure,
                    source: Some(Box::new(e)),
                    message: format!("Failed to store {}", key_name),
                })
            })?;

            // Also set default provider to match
            let _ = keychain.store_by_key(
                hkask_types::keychain_keys::KEY_DEFAULT_PROVIDER,
                &provider_str,
            );

            println!("  \x1b[32m✓\x1b[0m Key stored in OS keychain.");
            println!("  Default provider set to {}", provider_str);
        }
        2 => {
            println!();
            println!("  \x1b[33m⚠\x1b[0m  Skipping cloud provider setup.");
            println!("  hKask requires a cloud inference provider to generate responses.");
            println!("  Without one, your userpod cannot reply to you.");
            println!();
            println!("  To add a cloud provider later, add your key to .env and restart.");
        }
        _ => unreachable!(),
    }

    Ok(())
}

// ── Private helpers ────────────────────────────────────────────────────────

/// Print a summary after successful userpod creation (first-run).
fn print_creation_summary(name: &str, model: &str) {
    println!();
    println!("  \x1b[1;32m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
    println!("  \x1b[1;32m  ✓  UserPod created successfully!\x1b[0m");
    println!("  \x1b[1;32m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
    println!();
    println!("  \x1b[1mUserPod:\x1b[0m  \x1b[36m{}\x1b[0m", name);
    println!("  \x1b[1mModel:\x1b[0m     \x1b[36m{}\x1b[0m", model);
    println!("  \x1b[1mSecurity:\x1b[0m  Keys stored in OS keychain (encrypted DB)");
    println!();
    println!("  \x1b[1mGetting started:\x1b[0m");
    println!("  • Just type to chat with your userpod");
    println!("  • \x1b[36m/help\x1b[0m   — see all available commands");
    println!("  • \x1b[36m/model\x1b[0m  — switch models anytime");
    println!("  • \x1b[36m/tools\x1b[0m  — discover available MCP tools");
    println!("  • \x1b[36m/start\x1b[0m  — take a guided tour of hKask");
    println!();
    println!("  \x1b[2mTry asking: \"What can you help me with?\"\x1b[0m");
    println!();
}

/// List userpods from the user store. Returns an empty vec if the DB can't be opened.
fn list_userpods(config: &ServiceConfig) -> Result<Vec<hkask_identity::UserPod>, ServiceError> {
    use hkask_storage::user_store::UserStore;
    let store = UserStore::open(&config.db_path, &config.db_passphrase).map_err(|e| {
        ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: format!("Failed to open user DB: {e}"),
        }
    })?;
    store.list_userpods().map_err(|e| ServiceError::Domain {
        kind: ErrorKind::BadRequest,
        domain: DomainKind::Storage,
        source: None,
        message: format!("Failed to list userpods: {e}"),
    })
}

/// Resolve secrets from the OS keychain for operating-mode sessions.
///
/// In operating mode, `run_onboarding` doesn't derive secrets (the
/// passphrase isn't available). Instead, this helper reads the previously-
/// stored secrets from the keychain: `HKASK_MASTER_KEY`, `a2a-secret`, and
/// `hkask-db-passphrase`.
///
/// Returns `Some(ResolvedSecrets)` if all three secrets are found, or
/// `None` if any are missing.
fn resolve_secrets_from_keychain(config: &ServiceConfig) -> Option<ResolvedSecrets> {
    let keychain = hkask_keystore::Keychain::default();
    let master_key_hex = keychain
        .retrieve_by_key(hkask_types::keychain_keys::KEY_MASTER_KEY)
        .ok()?;
    // Try the keychain first (fast path). If a2a-secret is missing, fall back
    // to deriving from the master key.
    let a2a_secret = keychain
        .retrieve_by_key(hkask_types::keychain_keys::KEY_A2A_SECRET)
        .ok()
        .or_else(|| {
            tracing::warn!(
                target: "hkask.onboarding",
                "a2a-secret not in keychain — deriving from master key"
            );
            hkask_keystore::keychain::resolve_a2a_secret()
                .ok()
                .map(|s| String::from_utf8_lossy(&s).to_string())
        })?;
    Some(ResolvedSecrets {
        master_key_hex,
        a2a_secret,
        db_passphrase: config.db_passphrase.clone(),
    })
}

/// Error type for UserStore session creation during onboarding.
#[derive(Debug, thiserror::Error)]
enum SessionCreationError {
    #[error("DB open: {0}")]
    DbOpen(String),
    #[error("login: {0}")]
    Login(String),
}

/// Create a UserStore session for the userpod so the daemon's `check_auth`
/// returns `authenticated: true`.
///
/// Returns `Ok(session_id)` on success, or `Err(SessionCreationError)` on any
/// failure. Errors are non-fatal — the caller logs a warning and continues.
fn create_user_session(
    config: &ServiceConfig,
    userpod_name: &str,
) -> Result<String, SessionCreationError> {
    use hkask_storage::user_store::UserStore;

    let store = UserStore::open(&config.db_path, &config.db_passphrase)
        .map_err(|e| SessionCreationError::DbOpen(e.to_string()))?;
    let session = store
        .login(userpod_name, &config.db_passphrase)
        .map_err(|e| SessionCreationError::Login(e.to_string()))?;
    Ok(session.session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_display_kilocode() {
        let config = InferenceConfig {
            default_provider: ProviderId::KiloCode,
            ..Default::default()
        };
        assert_eq!(provider_display_name(&config), "KiloCode");
    }

    #[test]
    fn provider_display_deepinfra() {
        let config = InferenceConfig::default();
        assert_eq!(provider_display_name(&config), "DeepInfra");
    }
}
