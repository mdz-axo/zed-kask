//! Foundation: database connections, stores, Regulation runtime, seam watcher.

use super::super::*;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use tokio::sync::RwLock;

/// Foundation: database connections, stores, Regulation runtime, seam watcher.
pub(super) struct Foundation {
    pub db: Database,
    pub curation_inbox_tx: tokio::sync::mpsc::UnboundedSender<CurationInput>,
    pub curation_inbox_rx: Option<tokio::sync::mpsc::UnboundedReceiver<CurationInput>>,
    pub consent_manager: Arc<ConsentManager>,
    pub escalation_queue: Arc<EscalationQueue>,
    pub goal_repo: Arc<SqliteGoalRepository>,
    pub sovereignty_boundary_store: SovereigntyBoundaryStore,
    pub user_store: Arc<std::sync::Mutex<UserStore>>,
    pub ledger_runtime: Arc<RwLock<RegulationLedger>>,
    pub seam_watcher: Arc<RwLock<Option<SeamWatcher>>>,
    pub reg_event_sink: Arc<dyn RegulationSink>,
    /// Abstracted event store for gas report queries and calibration.
    pub gas_event_store: Arc<dyn LedgerStoragePort>,
    /// Concrete regulation record store for SLO evaluation and Regulation queries.
    pub regulation_store: Arc<RegulationArchive>,
    /// Last-resort alert email sink — injected by the API layer to close the
    /// algedonic feedback loop via email when live channel and archive are down.
    pub alert_email_sink: Option<Arc<dyn hkask_regulation::AlertEmailSink>>,
}

pub(super) async fn build_foundation(config: &ServiceConfig) -> Result<Foundation, ServiceError> {
    let db = if config.in_memory {
        Database::in_memory().map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: e.to_string(),
        })?
    } else {
        Database::open(&config.db_path, &config.db_passphrase).map_err(|e| {
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: None,
                message: e.to_string(),
            }
        })?
    };
    let shared_pool = db.sqlite_pool().map_err(|e| ServiceError::Domain {
        kind: ErrorKind::BadRequest,
        domain: DomainKind::Storage,
        source: None,
        message: format!("SQLite pool: {e}"),
    })?;
    let shared_driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> = Arc::new(
        hkask_storage::database::sqlite::SqliteDriver::new(shared_pool),
    );

    let gas_store: Arc<RegulationArchive> =
        Arc::new(RegulationArchive::from_driver(Arc::clone(&shared_driver)));
    let gas_event_store: Arc<dyn LedgerStoragePort> =
        Arc::clone(&gas_store) as Arc<dyn LedgerStoragePort>;
    let reg_event_sink: Arc<dyn RegulationSink> = Arc::clone(&gas_store) as Arc<dyn RegulationSink>;

    // Shared channel for CurationInput.
    let (curation_inbox_tx, curation_inbox_rx) =
        tokio::sync::mpsc::unbounded_channel::<CurationInput>();

    let consent_store = ConsentStore::from_driver(Arc::clone(&shared_driver));
    let consent_manager = Arc::new(
        ConsentManager::new(
            Arc::new(consent_store) as Arc<dyn hkask_types::consent_port::ConsentPort>
        )
        .with_event_sink(Arc::clone(&reg_event_sink)),
    );

    let escalation_queue = Arc::new(
        EscalationQueue::from_driver(Arc::clone(&shared_driver)).map_err(|e| {
            ServiceError::Domain {
                domain: DomainKind::Curator,
                kind: ErrorKind::ServiceUnavailable,
                source: None,
                message: e.to_string(),
            }
        })?,
    );

    let goal_sink: Arc<dyn RegulationSink> =
        Arc::new(RegulationArchive::from_driver(Arc::clone(&shared_driver)));
    let goal_repo = Arc::new(
        SqliteGoalRepository::from_driver(Arc::clone(&shared_driver)).with_telemetry(goal_sink),
    );

    let sovereignty_boundary_store =
        SovereigntyBoundaryStore::from_driver(Arc::clone(&shared_driver));

    let user_store = Arc::new(std::sync::Mutex::new(UserStore::from_driver(Arc::clone(
        &shared_driver,
    ))));
    {
        let _guard = user_store.lock().map_err(|_| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: hkask_types::InfrastructureError::LockPoisoned.to_string(),
        })?;
    }

    // Regulation runtime
    let ledger_runtime = Arc::new(RwLock::new(RegulationLedger::with_threshold(
        config.reg_threshold,
    )));

    // Reset variety counters for fresh session — prevents stale deficit
    // values from prior sessions from triggering false algedonic alerts.
    {
        let ledger = ledger_runtime.read().await;
        ledger.reset_variety().await;
    }

    // Seam watcher — non-fatal if inventory unavailable.
    let seam_watcher: Arc<RwLock<Option<SeamWatcher>>> = {
        let ledger = ledger_runtime.read().await;
        if let Some(watcher) = SeamWatcher::load() {
            watcher.register_domains(&ledger).await;
            let summary = watcher.summary();
            tracing::info!(
                target: "bootstrap",
                crates = %summary.crate_count,
                coverage_pct = %summary.coverage_pct,
                total_items = %summary.total_items,
                "Seam watcher initialized — watching the public seam"
            );
            Arc::new(RwLock::new(Some(watcher)))
        } else {
            tracing::info!(
                target: "bootstrap",
                "Seam watcher skipped — no inventory available (non-fatal)"
            );
            Arc::new(RwLock::new(None))
        }
    };

    // Spawn periodic seam drift check (background watcher).
    seam_monitor::spawn_seam_drift_check(&seam_watcher, &ledger_runtime, &reg_event_sink);

    Ok(Foundation {
        db,
        curation_inbox_tx,
        curation_inbox_rx: Some(curation_inbox_rx),
        consent_manager,
        escalation_queue,
        goal_repo,
        sovereignty_boundary_store,
        user_store,
        ledger_runtime,
        seam_watcher,
        reg_event_sink: reg_event_sink.clone(),
        gas_event_store,
        regulation_store: gas_store,
        alert_email_sink: None,
    })
}
