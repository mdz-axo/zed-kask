//! StorageGuard Loop — Autonomous disk space management (Loop 7)
//!
//! Implements the RegulationLoop (sense → compare → compute → act) cycle to
//! monitor disk usage on the /data volume and take corrective action.
//!
//! Extracted from hkask-regulation to separate disk space management from
//! cybernetic regulation. Depends on hkask-regulation only for the Loop trait.
//!
//! ## Guardrail Contract
//!
//! - **Sense:** Measure disk usage percentage on the data directory
//! - **Compare:** Detect deviations from configurable thresholds (warn 80%, critical 95%)
//! - **Compute:** At warn level → log Regulation span. At critical level → produce Prune action.
//! - **Act:** Prune old export archives. If pruning is insufficient, escalate to Curator.
//! - **Verify:** Re-check after dampener cooldown (5 min). If still critical → escalate.
//!
//! ## P2 Affirmative Consent
//!
//! Pruning is pre-authorized by the user via configuration:
//! `kask config set reg.autonomous.prune_exports true`
//!
//! Exports are sovereignty artifacts. Pruning without consent would violate P1.
//! The loop checks `prune_exports_enabled` before acting.

use hkask_regulation::RegulationLoop;
use hkask_types::loops::{
    ActionType, Deviation, LoopId, RegulatoryAction, RegulatoryActionParams, Signal, SignalMetric,
};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Configuration for the StorageGuard loop.
pub struct StorageGuardConfig {
    /// Enable autonomous export pruning. Default: false (P2 consent required).
    pub prune_exports_enabled: Arc<AtomicBool>,
    /// Disk usage percentage threshold for warning (Regulation span only).
    pub warn_threshold_pct: f64,
    /// Disk usage percentage threshold for critical action.
    pub critical_threshold_pct: f64,
    /// Age threshold for pruning exports (delete archives older than this).
    pub prune_older_than_days: u64,
    /// Dampener cooldown between prune actions (prevents thrashing).
    pub prune_cooldown: Duration,
    /// Data directory to monitor.
    pub data_dir: String,
    /// PVC capacity in bytes (from HKASK_PVC_CAPACITY_BYTES env, default 20Gi).
    pub pvc_capacity_bytes: u64,
}

impl Default for StorageGuardConfig {
    fn default() -> Self {
        let pvc_capacity_bytes = std::env::var("HKASK_PVC_CAPACITY_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20 * 1024 * 1024 * 1024);
        Self {
            prune_exports_enabled: Arc::new(AtomicBool::new(false)),
            warn_threshold_pct: 80.0,
            critical_threshold_pct: 95.0,
            prune_older_than_days: 7,
            prune_cooldown: Duration::from_secs(300),
            data_dir: "/data".to_string(),
            pvc_capacity_bytes,
        }
    }
}

/// Autonomous disk space guard loop.
///
/// Monitors the /data PVC usage and prunes old export archives when the
/// volume approaches capacity. If pruning is insufficient, escalates to
/// the Curator for human intervention.
pub struct StorageGuardLoop {
    config: StorageGuardConfig,
    /// Last time a prune action was taken (cooldown timer).
    last_prune: parking_lot::Mutex<Option<Instant>>,
}

impl StorageGuardLoop {
    /// Create a new StorageGuard loop with the given configuration.
    pub fn new(config: StorageGuardConfig) -> Self {
        Self {
            config,
            last_prune: parking_lot::Mutex::new(None),
        }
    }

    /// Check if enough time has passed since the last prune action.
    fn prune_cooldown_elapsed(&self) -> bool {
        let last = self.last_prune.lock();
        match *last {
            Some(t) => t.elapsed() >= self.config.prune_cooldown,
            None => true,
        }
    }

    /// Record that a prune action was just taken.
    fn record_prune(&self) {
        *self.last_prune.lock() = Some(Instant::now());
    }

    /// Measure disk usage as a percentage of PVC capacity.
    fn measure_disk_usage(&self) -> f64 {
        let path = Path::new(&self.config.data_dir);
        if !path.exists() {
            return 0.0;
        }
        let mut used_bytes: u64 = 0;
        walk_dir(path, &mut used_bytes);
        let pct = (used_bytes as f64 / self.config.pvc_capacity_bytes as f64) * 100.0;
        pct.min(100.0)
    }
}

/// Recursively walk a directory tree, summing file sizes.
/// Limited to max_depth levels and max_files entries to prevent
/// unbounded traversal on corrupted filesystems.
pub fn walk_dir(path: &Path, total: &mut u64) {
    walk_dir_bounded(path, total, 0, 10, &mut 0, 100_000);
}

fn walk_dir_bounded(
    path: &Path,
    total: &mut u64,
    depth: usize,
    max_depth: usize,
    file_count: &mut usize,
    max_files: usize,
) {
    if depth > max_depth || *file_count > max_files {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            *file_count += 1;
            if *file_count > max_files {
                return;
            }
            if let Ok(meta) = entry.metadata() {
                // Skip symlinks — they can create cycles
                if meta.is_symlink() {
                    continue;
                }
                *total += meta.len();
                if meta.is_dir() {
                    walk_dir_bounded(
                        &entry.path(),
                        total,
                        depth + 1,
                        max_depth,
                        file_count,
                        max_files,
                    );
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl RegulationLoop for StorageGuardLoop {
    fn id(&self) -> LoopId {
        LoopId::StorageGuard
    }

    /// Sense: measure disk usage on /data volume.
    async fn sense(&self) -> Vec<Signal> {
        let pct = self.measure_disk_usage();
        vec![
            Signal::new(
                LoopId::StorageGuard,
                SignalMetric::DiskUsagePct,
                pct,
                self.config.warn_threshold_pct,
            ),
            Signal::new(
                LoopId::StorageGuard,
                SignalMetric::DiskUsagePct,
                pct,
                self.config.critical_threshold_pct,
            ),
        ]
    }

    /// Compare: detect if disk usage exceeds warn or critical thresholds.
    async fn compare(&self, signals: &[Signal]) -> Vec<Deviation> {
        signals
            .iter()
            .filter(|s| s.metric == SignalMetric::DiskUsagePct && s.value >= s.set_point)
            .filter_map(Deviation::from_signal)
            .collect()
    }

    /// Compute: produce Prune action if critical, Notify if warn.
    async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
        if deviations.is_empty() {
            return Vec::new();
        }

        // Find the most severe deviation (highest set-point exceeded)
        let worst = deviations
            .iter()
            .max_by(|a, b| a.signal.set_point.partial_cmp(&b.signal.set_point).unwrap());

        let Some(dev) = worst else {
            return Vec::new();
        };

        let _severity = if dev.signal.set_point >= self.config.critical_threshold_pct {
            "critical"
        } else {
            "warn"
        };

        let action_type = if dev.signal.set_point >= self.config.critical_threshold_pct
            && self.config.prune_exports_enabled.load(Ordering::Relaxed)
        {
            ActionType::Prune
        } else {
            ActionType::Notify
        };

        vec![RegulatoryAction::new(
            LoopId::StorageGuard,
            action_type,
            RegulatoryActionParams::reason("disk_usage_exceeded"),
        )]
    }

    /// Act: if Prune, delete export archives older than the configured threshold.
    /// If Notify, log a Regulation span (the Cybernetics loop handles escalation).
    async fn act(&self, actions: &[RegulatoryAction]) {
        for action in actions {
            if action.action_type == ActionType::Prune {
                if !self.prune_cooldown_elapsed() {
                    tracing::info!(
                        target: "hkask.storage_guard",
                        "Prune action throttled by dampener"
                    );
                    continue;
                }

                let older_than = self.config.prune_older_than_days;
                let cutoff = std::time::SystemTime::now()
                    - std::time::Duration::from_secs(older_than * 86400);

                let exports_dir = Path::new(&self.config.data_dir).join("exports");
                if !exports_dir.exists() {
                    tracing::info!(
                        target: "hkask.storage_guard",
                        "No exports directory at {:?} — nothing to prune",
                        exports_dir
                    );
                    continue;
                }

                let mut pruned_count: u64 = 0;
                let mut pruned_bytes: u64 = 0;

                if let Ok(entries) = std::fs::read_dir(&exports_dir) {
                    for entry in entries.flatten() {
                        if let Ok(meta) = entry.metadata()
                            && let Ok(modified) = meta.modified()
                            && modified < cutoff
                        {
                            let path = entry.path();
                            let size = meta.len();
                            if meta.is_dir() {
                                let mut dir_size: u64 = 0;
                                walk_dir(&path, &mut dir_size);
                                if std::fs::remove_dir_all(&path).is_ok() {
                                    pruned_count += 1;
                                    pruned_bytes += size + dir_size;
                                }
                            } else if std::fs::remove_file(&path).is_ok() {
                                pruned_count += 1;
                                pruned_bytes += size;
                            }
                        }
                    }
                }

                self.record_prune();

                if pruned_count > 0 {
                    tracing::info!(
                        target: "hkask.storage_guard",
                        pruned_count = pruned_count,
                        pruned_bytes = pruned_bytes,
                        "Pruned old export archives"
                    );
                } else {
                    tracing::warn!(
                        target: "hkask.storage_guard",
                        "Disk at {:.1}% but no exports to prune — escalation needed",
                        self.measure_disk_usage()
                    );
                }
            } else if action.action_type == ActionType::Notify {
                tracing::info!(
                    target: "hkask.storage_guard",
                    pct = self.measure_disk_usage(),
                    "Disk usage warning — notifying operator"
                );
            }
        }
    }
}
