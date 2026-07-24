//! Registry + wallet: agent records, A2A restore, rJoule payments.

use super::super::*;
use super::foundation::Foundation;
use super::loops::LoopWiring;
use hkask_services_core::{DomainKind, ErrorKind, ServiceConfig, ServiceError};
use hkask_services_self_heal::{HealAction, HealContext, HealRegistry, HealStrategy, SelfHealer};
use hkask_types::RegistryIndex;
use hkask_wallet::manager::{WalletManager, WalletSelfHealer};
use std::collections::HashMap;
use std::sync::Arc;

/// Registry + wallet: agent records, A2A restore, rJoule payments.
pub(super) struct RegWallet {
    pub registry: Arc<tokio::sync::Mutex<SqliteRegistry>>,
    pub wallet_service: Option<Arc<WalletService>>,
    pub wallet_store: Option<Arc<WalletStore>>,
    pub wallet_gas_calibrator: Option<Arc<hkask_regulation::WalletGasCalibrator>>,
}

pub(super) async fn build_registry_and_wallet(
    config: &ServiceConfig,
    f: &Foundation,
    l: &LoopWiring,
) -> Result<RegWallet, ServiceError> {
    // Registry
    let registry_pool = f.db.sqlite_pool().map_err(|e| ServiceError::Domain {
        kind: ErrorKind::BadRequest,
        domain: DomainKind::Storage,
        source: None,
        message: format!("SQLite pool for registry: {e}"),
    })?;
    let registry = Arc::new(tokio::sync::Mutex::new(
        SqliteRegistry::new_with_pool(registry_pool).map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?,
    ));

    // Restore A2A state from persistent storage (UserStore)
    let registered_userpods = {
        let user_guard = f.user_store.lock().map_err(|_| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: hkask_types::InfrastructureError::LockPoisoned.to_string(),
        })?;
        user_guard
            .list_userpods()
            .map_err(|e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Agent,
                source: None,
                message: e.to_string(),
            })?
    };
    if !registered_userpods.is_empty() {
        let agents: Vec<hkask_pods::a2a::A2AAgent> = registered_userpods
            .iter()
            .map(|up| hkask_pods::a2a::A2AAgent {
                webid: up.webid,
                capabilities: up.capabilities.clone(),
                registered_at: up.created_at,
                active: true,
            })
            .collect();
        let tokens = std::collections::HashMap::new();
        l.a2a_runtime
            .restore_from_storage(agents, tokens)
            .await
            .map_err(|e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Agent,
                source: None,
                message: e.to_string(),
            })?;
    }

    // Wallet — non-fatal if config or build fails (daemon can run without wallet)
    let (wallet_service, wallet_store, wallet_gas_calibrator) = match build_wallet(config, f, l) {
        Ok(tuple) => tuple,
        Err(e) => {
            tracing::warn!(target: "reg.wallet", error = %e, "Wallet unavailable — running without rJoule");
            (None, None, None)
        }
    };

    // Wire the skill catalog snapshot into CuratorContext for epistemic routing.
    // The catalog is a snapshot of all registered templates — sufficient for
    // skill-router because the catalog changes infrequently.
    let catalog = registry.lock().await.list(None);
    l.curator_context.set_skill_catalog(catalog).await;
    tracing::info!(target: "hkask.startup", "Skill catalog wired into CuratorContext — epistemic routing enabled");

    Ok(RegWallet {
        registry,
        wallet_service,
        wallet_store,
        wallet_gas_calibrator,
    })
}

/// Build wallet subsystem — returns (service, store, gas_calibrator) or error.
#[allow(clippy::type_complexity)]
fn build_wallet(
    config: &ServiceConfig,
    f: &Foundation,
    l: &LoopWiring,
) -> Result<
    (
        Option<Arc<WalletService>>,
        Option<Arc<WalletStore>>,
        Option<Arc<hkask_regulation::WalletGasCalibrator>>,
    ),
    ServiceError,
> {
    let wallet_db_path = if config.in_memory {
        None
    } else {
        let path = hkask_types::agent_paths::userpod_wallet_db(&config.user_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        Some(path)
    };

    let wallet_driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
        if let Some(ref path) = wallet_db_path {
            let path_str = path.to_string_lossy().to_string();
            match hkask_storage::Database::open(&path_str, &config.db_passphrase) {
                Ok(db) => {
                    tracing::info!(
                        target: "reg.wallet",
                        path = %path_str,
                        agent = %config.user_name,
                        "Per-agent wallet database opened"
                    );
                    let pool = db.sqlite_pool().map_err(|e| ServiceError::Domain {
                        kind: ErrorKind::BadRequest,
                        domain: DomainKind::Storage,
                        source: None,
                        message: format!("wallet pool: {e}"),
                    })?;
                    Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool))
                }
                Err(e) => {
                    tracing::warn!(
                        target: "reg.wallet",
                        path = %path_str,
                        error = %e,
                        "Failed to open per-agent wallet DB, falling back to shared connection"
                    );
                    let pool = f.db.sqlite_pool().map_err(|e| ServiceError::Domain {
                        kind: ErrorKind::BadRequest,
                        domain: DomainKind::Storage,
                        source: None,
                        message: format!("wallet pool: {e}"),
                    })?;
                    Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool))
                }
            }
        } else {
            let pool = f.db.sqlite_pool().map_err(|e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: None,
                message: format!("wallet pool: {e}"),
            })?;
            Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool))
        };
    let wallet_store = Arc::new(WalletStore::from_driver(wallet_driver));

    struct WalletSelfHealerAdapter {
        healer: SelfHealer,
        manager: Arc<WalletManager>,
    }

    impl WalletSelfHealer for WalletSelfHealerAdapter {
        fn heal(&self, operation: &str, error: &str) {
            if operation == "wallet.deposit.process"
                && error.starts_with("deposit address unresolvable:")
                && let Some((_, address)) = error.split_once(':')
            {
                let address = address.trim();
                if let Ok(true) = self.manager.repair_deposit_address_mapping(address) {
                    tracing::info!(
                        target: "reg.heal",
                        operation = %operation,
                        address = %address,
                        "Wallet self-heal repaired deposit address mapping"
                    );
                    return;
                }
            }

            let ctx = HealContext {
                operation: operation.to_string(),
                error_message: error.to_string(),
                env_vars: HashMap::new(),
                config_search_paths: Vec::new(),
                can_retry: true,
            };
            let _ = self.healer.attempt(error, &ctx);
        }
    }

    let mut heal_registry = HealRegistry::with_defaults();
    heal_registry.add(HealStrategy {
        name: "wallet-deposit-unresolvable".into(),
        error_pattern: "deposit address unresolvable".into(),
        description: "Deposit address unresolvable — retry after wallet auto-repair".into(),
        action: HealAction::RetryWithBackoff {
            max_attempts: 2,
            delay_ms: 500,
        },
    });
    let svc = WalletService::build(
        &config.wallet_config,
        Arc::clone(&wallet_store),
        Arc::clone(&f.reg_event_sink),
        Arc::clone(&l.cybernetics_loop),
    )?;
    let svc = Arc::new(
        svc.as_ref()
            .clone()
            .with_consent_manager(Arc::clone(&f.consent_manager)),
    );
    let wallet_manager = svc.manager();

    let wallet_self_healer = WalletSelfHealerAdapter {
        healer: SelfHealer::with_registry(heal_registry),
        manager: Arc::clone(wallet_manager),
    };
    wallet_manager.set_self_healer(Arc::new(wallet_self_healer));

    // Ensure default wallet
    let default_wallet = WalletId::default();
    wallet_manager
        .ensure_wallet(default_wallet)
        .map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Wallet,
            source: Some(Box::new(e)),
            message: "Failed to ensure default wallet".into(),
        })?;

    // Bind wallets to userpods
    {
        let user_guard = f.user_store.lock().map_err(|_| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: hkask_types::InfrastructureError::LockPoisoned.to_string(),
        })?;
        if let Ok(Some(system_identity)) = user_guard.get_userpod(&config.user_name) {
            let user_id = system_identity.user_id;
            if let Ok(Some(identity)) = user_guard.get_userpod_by_user(&user_id)
                && identity.wallet_id.is_none()
            {
                let wallet_id = WalletId::from_name(&identity.userpod_name);
                if let Err(e) = wallet_manager.ensure_wallet(wallet_id) {
                    tracing::warn!(
                        target: "reg.wallet",
                        userpod = %identity.userpod_name,
                        error = %e,
                        "Failed to create wallet for userpod"
                    );
                } else if let Err(e) = user_guard.set_wallet_id(&identity.userpod_name, wallet_id) {
                    tracing::warn!(
                        target: "reg.wallet",
                        userpod = %identity.userpod_name,
                        error = %e,
                        "Failed to bind wallet to userpod"
                    );
                } else {
                    tracing::info!(
                        target: "reg.wallet",
                        userpod = %identity.userpod_name,
                        wallet_id = %wallet_id,
                        "Wallet created and bound to userpod"
                    );
                }
            }
        }
    }

    // Spawn deposit monitor
    let monitor_manager = Arc::clone(wallet_manager);
    let interval_secs: u64 = std::env::var("HKASK_DEPOSIT_MONITOR_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    tokio::spawn(async move {
        tracing::info!(
            target: "reg.wallet.deposit",
            interval_secs = %interval_secs,
            "Deposit monitor started — polling every {}s",
            interval_secs
        );
        if let Err(e) = monitor_manager.start_deposit_monitor(interval_secs).await {
            tracing::error!(
                target: "reg.wallet.deposit",
                error = %e,
                "Deposit monitor loop exited with error"
            );
        }
    });

    // Spawn wallet gas calibrator (P9 feedback loop for gas→rJoule rate).
    let wallet_gas_calibrator = {
        let calibrator = Arc::new(
            hkask_regulation::WalletGasCalibrator::new(
                Arc::clone(&f.gas_event_store),
                Arc::clone(wallet_manager) as Arc<dyn hkask_types::WalletBudgetPort>,
            )
            .with_event_sink(Arc::clone(&f.reg_event_sink)),
        );
        calibrator
            .clone()
            .spawn_calibration(hkask_regulation::DEFAULT_WALLET_CALIBRATION_INTERVAL);
        Some(calibrator)
    };

    Ok((Some(svc), Some(wallet_store), wallet_gas_calibrator))
}
