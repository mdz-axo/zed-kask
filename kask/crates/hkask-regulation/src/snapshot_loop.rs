//! SnapshotLoop — Cybernetic loop for scheduled CAS snapshots
//!
//! Implements the RegulationLoop (sense → compare → compute → act) cycle to
//! take snapshots of CAS repositories based on their retention policies.
//!
//! Per-repo policies determine when a snapshot is needed:
//! - If the repo's policy is disabled, no snapshot is taken
//! - If the time since the last snapshot exceeds the policy interval, a snapshot is due
//! - If no previous snapshot exists, one is always taken
//!
//! State tracking uses `parking_lot::RwLock` for interior mutability so
//! the `RegulationLoop` trait's `&self` methods can update snapshot timestamps
//! after successful snapshot actions.

use crate::types::loops::{
    ActionType, Deviation, LoopId, RegulationLoop, RegulatoryAction, RegulatoryActionParams,
    Signal, SignalMetric,
};
use hkask_types::git_cas::{
    CasRetentionPolicy, CasRetentionTier, CommitHash, GitCASPort, RepoId, RepoSnapshotPolicy,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Configuration for the SnapshotLoop.
#[derive(Debug, Clone)]
pub struct SnapshotLoopConfig {
    /// Per-repo snapshot policies. Defaults are used for repos not listed.
    pub repo_policies: Vec<RepoSnapshotPolicy>,
    /// Global default CAS retention policy.
    pub default_policy: CasRetentionPolicy,
}

impl Default for SnapshotLoopConfig {
    fn default() -> Self {
        Self {
            repo_policies: RepoId::all()
                .iter()
                .map(|id| RepoSnapshotPolicy::default_for(id.clone()))
                .collect(),
            default_policy: CasRetentionPolicy::default(),
        }
    }
}

/// The last known snapshot timestamp per repo.
#[derive(Debug, Clone, Default)]
struct SnapshotState {
    /// Instant of the last successful snapshot for this repo.
    last_snapshot: Option<Instant>,
    /// Commit hash of the last successful snapshot.
    last_commit: Option<CommitHash>,
}

/// Cybernetic loop that takes scheduled snapshots of CAS repositories.
///
/// Implements the sense → compare → compute → act cycle:
/// - **Sense**: Check time elapsed since last snapshot per repo
/// - **Compare**: Detect deviations from CasRetentionPolicy intervals
/// - **Compute**: Produce snapshot actions for repos that need them
/// - **Act**: Call `snapshot()` on the GitCASPort for each due repo
///
/// Interior mutability via `parking_lot::RwLock<Hash`Map<String, SnapshotState>`>`
/// allows `act()` to record successful snapshot timestamps despite the
/// `&self` signature required by `RegulationLoop`.
pub struct SnapshotLoop {
    port: Arc<dyn GitCASPort>,
    config: SnapshotLoopConfig,
    /// Per-repo snapshot state, keyed by `RepoId::dir_name()`.
    /// Uses `RwLock` for interior mutability — `act()` writes, `sense()` reads.
    state: Arc<parking_lot::RwLock<HashMap<String, SnapshotState>>>,
}

impl SnapshotLoop {
    /// Create a new SnapshotLoop with the given CAS port and default config.
    pub fn new(port: Arc<dyn GitCASPort>) -> Self {
        Self {
            port,
            config: SnapshotLoopConfig::default(),
            state: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }

    /// Create a SnapshotLoop with custom configuration.
    pub fn with_config(port: Arc<dyn GitCASPort>, config: SnapshotLoopConfig) -> Self {
        Self {
            port,
            config,
            state: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }

    /// Get the policy for a specific repo, falling back to default.
    fn policy_for(&self, repo: &RepoId) -> RepoSnapshotPolicy {
        self.config
            .repo_policies
            .iter()
            .find(|p| &p.repo == repo)
            .cloned()
            .unwrap_or_else(|| RepoSnapshotPolicy::default_for(repo.clone()))
    }

    /// Determine which retention tier applies given the elapsed seconds.
    fn applicable_tier(
        policy: &CasRetentionPolicy,
        elapsed_secs: u64,
    ) -> Option<&CasRetentionTier> {
        for tier in &policy.tiers {
            if elapsed_secs <= tier.max_age_secs {
                return Some(tier);
            }
        }
        // Elapsed exceeds all tier max-age thresholds → use the last (forever) tier
        policy.tiers.last()
    }

    /// Check if a repo needs a snapshot based on its policy and time since last snapshot.
    fn needs_snapshot(&self, repo: &RepoId) -> bool {
        let policy = self.policy_for(repo);
        if !policy.enabled {
            return false;
        }
        let retention = policy.effective_policy();
        let state = self.state.read();

        match state.get(repo.dir_name()).and_then(|s| s.last_snapshot) {
            Some(instant) => {
                let elapsed = instant.elapsed().as_secs();
                let tier = match Self::applicable_tier(&retention, elapsed) {
                    Some(t) => t,
                    None => return true, // No tier = no constraint = snapshot needed
                };
                elapsed >= tier.interval_secs
            }
            None => true, // No previous snapshot — always take one
        }
    }

    /// Record a successful snapshot for a repo.
    fn record_snapshot(&self, repo: &RepoId, commit: CommitHash) {
        let mut state = self.state.write();
        let entry = state.entry(repo.dir_name().to_string()).or_default();
        entry.last_snapshot = Some(Instant::now());
        entry.last_commit = Some(commit);
    }
}

#[async_trait::async_trait]
impl RegulationLoop for SnapshotLoop {
    fn id(&self) -> LoopId {
        LoopId::Snapshot
    }

    /// Sense: measure time since last snapshot per repo.
    async fn sense(&self) -> Vec<Signal> {
        let mut signals = Vec::new();
        let state = self.state.read();

        for repo in RepoId::all() {
            let policy = self.policy_for(repo);
            if !policy.enabled {
                continue;
            }
            let retention = policy.effective_policy();

            let elapsed_secs = state
                .get(repo.dir_name())
                .and_then(|s| s.last_snapshot)
                .map(|instant| instant.elapsed().as_secs())
                .unwrap_or(u64::MAX);

            // The applicable interval for this elapsed duration
            let interval = match Self::applicable_tier(&retention, elapsed_secs) {
                Some(tier) => tier.interval_secs as f64,
                None => 0.0, // No tier → snapshot immediately
            };

            // Signal: elapsed time vs. expected interval
            // If elapsed >= interval, we're "above set-point" (snapshot is due)
            signals.push(Signal::new(
                LoopId::Snapshot,
                SignalMetric::SnapshotInterval,
                elapsed_secs as f64,
                interval,
            ));
        }

        drop(state);
        signals
    }

    /// Compare: detect repos where snapshot interval has been exceeded.
    async fn compare(&self, signals: &[Signal]) -> Vec<Deviation> {
        // Only propagate signals where snapshot is due (elapsed >= interval)
        signals
            .iter()
            .filter(|s| {
                s.metric == SignalMetric::SnapshotInterval
                    && s.value >= s.set_point
                    && s.set_point > 0.0
            })
            .filter_map(Deviation::from_signal)
            .collect()
    }

    /// Compute: produce a single Calibrate action for snapshot scheduling.
    async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
        if deviations.is_empty() {
            return Vec::new();
        }
        // One action covers all due repos — act() iterates them
        vec![RegulatoryAction::new(
            LoopId::Snapshot,
            ActionType::Calibrate,
            RegulatoryActionParams::reason("snapshot_due"),
        )]
    }

    /// Act: take snapshots for repos that need them.
    async fn act(&self, actions: &[RegulatoryAction]) {
        if actions.is_empty() {
            return;
        }
        for repo in RepoId::all() {
            if self.needs_snapshot(repo) {
                match self.port.snapshot(repo, "scheduled snapshot").await {
                    Ok(commit) => {
                        tracing::info!(
                            target: "hkask.snapshot_loop",
                            repo = repo.dir_name(),
                            "Scheduled snapshot taken"
                        );
                        self.record_snapshot(repo, commit);
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "hkask.snapshot_loop",
                            repo = repo.dir_name(),
                            error = %e,
                            "Scheduled snapshot failed"
                        );
                    }
                }
            }
        }
    }
}
