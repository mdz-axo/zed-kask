//! Seam drift monitor — periodic public-seam drift detection.

use hkask_regulation::{RegulationLedger, SeamWatcher};
use hkask_types::event::RegulationSink;
use std::sync::Arc;
use tokio::sync::RwLock;

pub(crate) fn spawn_seam_drift_check(
    seam_watcher: &Arc<RwLock<Option<SeamWatcher>>>,
    ledger_runtime: &Arc<RwLock<RegulationLedger>>,
    event_sink: &Arc<dyn RegulationSink>,
) {
    let watcher_lock = Arc::clone(seam_watcher);
    let ledger = Arc::clone(ledger_runtime);
    let sink = Arc::clone(event_sink);

    let interval_secs: u64 = std::env::var("HKASK_SEAM_CHECK_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1800);

    tokio::spawn(async move {
        tracing::info!(
            target: "hkask.architecture.seam",
            interval_secs = %interval_secs,
            "Seam periodic drift check started — watching every {}s",
            interval_secs
        );
        {
            let reg_rt = ledger.read().await;
            let mut guard = watcher_lock.write().await;
            if let Some(ref mut watcher) = *guard {
                let drifts = watcher.check_drift(&reg_rt, &*sink).await;
                if !drifts.is_empty() {
                    tracing::info!(
                        target: "hkask.architecture.seam",
                        drift_count = %drifts.len(),
                        "Initial seam drift check complete"
                    );
                }
            }
        }
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        loop {
            interval.tick().await;
            let reg_rt = ledger.read().await;
            let mut guard = watcher_lock.write().await;
            if let Some(ref mut watcher) = *guard {
                let _ = watcher.refresh();
                let drifts = watcher.check_drift(&reg_rt, &*sink).await;
                if !drifts.is_empty() {
                    let degradations: Vec<_> =
                        drifts.iter().filter(|d| d.delta_pct < 0.0).collect();
                    let improvements: Vec<_> =
                        drifts.iter().filter(|d| d.delta_pct > 0.0).collect();
                    tracing::info!(
                        target: "hkask.architecture.seam",
                        total_drifts = %drifts.len(),
                        degradations = %degradations.len(),
                        improvements = %improvements.len(),
                        "Periodic seam drift check complete"
                    );
                }
            }
        }
    });
}
