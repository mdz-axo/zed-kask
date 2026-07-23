//! Loop System — Bootstrap and lifecycle for the 6-loop model
//
//! Manages loop registration, tick scheduling, and lifecycle.
//! Inter-loop communication uses direct `tokio::mpsc` channels.
//!
//! **Authority DAG:** Curation → Cybernetics → {Inference, Episodic, Semantic}
//! Snapshot loop runs independently on its own schedule.

use hkask_regulation::types::loops::LoopId;
use hkask_regulation::types::loops::RegulationLoop;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;

/// Default tick interval for the Inference loop (500ms).
pub const INFERENCE_TICK_MS: u64 = 500;

/// Default tick interval for the Episodic memory loop (5s).
pub const EPISODIC_TICK_SECS: u64 = 5;

/// Default tick interval for the Semantic memory loop (5s).
pub const SEMANTIC_TICK_SECS: u64 = 5;

/// Default tick interval for the Cybernetics loop (2s).
pub const CYBERNETICS_TICK_SECS: u64 = 2;

/// Default tick interval for the Curation loop (10s).
pub const CURATION_TICK_SECS: u64 = 10;

/// Default tick interval for the Snapshot loop (60s).
pub const SNAPSHOT_TICK_SECS: u64 = 60;

/// Fallback tick interval for unregistered loops (1s).
pub const DEFAULT_FALLBACK_TICK_SECS: u64 = 1;

/// expect: "The system regulates agent behavior through cybernetic feedback"
/// \[P8\] Motivating: Semantic Grounding — LoopId names the regulatory loops
/// pre:  `loop_id` is one of `Inference`, `Episodic`, `Semantic`,
///       `Curation`, `Cybernetics`, or `Snapshot`.
/// post: Returns the default tick `Duration` for the given loop:
///       Inference → 500ms, Episodic → 5s, Semantic → 5s, Cybernetics → 2s,
///       Curation → 10s, Snapshot → 60s.
#[must_use]
pub fn default_tick_interval(loop_id: LoopId) -> Duration {
    match loop_id {
        LoopId::Inference => Duration::from_millis(INFERENCE_TICK_MS),
        LoopId::Episodic => Duration::from_secs(EPISODIC_TICK_SECS),
        LoopId::Semantic => Duration::from_secs(SEMANTIC_TICK_SECS),
        LoopId::Cybernetics => Duration::from_secs(CYBERNETICS_TICK_SECS),
        LoopId::Curation => Duration::from_secs(CURATION_TICK_SECS),
        LoopId::Snapshot => Duration::from_secs(SNAPSHOT_TICK_SECS),
        LoopId::StorageGuard => Duration::from_secs(300), // every 5 minutes
        LoopId::McpServerGuard => Duration::from_secs(60), // every minute
    }
}

/// Authority DAG tick order: meta-loops first, then domain loops.
/// Curation → Cybernetics → Inference → Episodic → Semantic
/// Snapshot runs independently.
pub const AUTHORITY_ORDER: [LoopId; 6] = [
    LoopId::Curation,
    LoopId::Cybernetics,
    LoopId::Inference,
    LoopId::Episodic,
    LoopId::Semantic,
    LoopId::Snapshot,
];

/// Multiple loops may share a `LoopId` (e.g., Episodic + Semantic both register
/// as `Memory`). They are ticked in registration order within the same ID.
type LoopRegistry = Arc<RwLock<HashMap<LoopId, Vec<Arc<dyn RegulationLoop>>>>>;

/// Loop System — manages loop registration, tick scheduling, and lifecycle.
///
/// Inter-loop communication uses direct `tokio::mpsc` channels wired during
/// `AgentService::build()`. This struct handles only registration and ticking.
pub struct LoopScheduler {
    /// All registered loops keyed by LoopId. Vec supports multiple loops per ID.
    loops: LoopRegistry,
    /// Cancellation token for graceful shutdown
    cancel: tokio_util::sync::CancellationToken,
    /// Per-loop tick intervals (keyed by LoopId)
    tick_intervals: HashMap<LoopId, Duration>,
}

impl LoopScheduler {
    /// Create a new LoopScheduler.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — LoopScheduler orchestrates sense-act cycles
    /// \[P5\] Constraining: Essentialism — minimal registry + cancellation token
    /// pre:  (none).
    /// post: Returns a `LoopScheduler` with an empty loop registry, a fresh
    ///       cancellation token, and default tick intervals for all four
    ///       loop IDs.
    pub fn new() -> Self {
        Self {
            loops: Arc::new(RwLock::new(HashMap::new())),
            cancel: CancellationToken::new(),
            tick_intervals: [
                LoopId::Inference,
                LoopId::Episodic,
                LoopId::Semantic,
                LoopId::Cybernetics,
                LoopId::Curation,
                LoopId::Snapshot,
            ]
            .into_iter()
            .map(|id| (id, default_tick_interval(id)))
            .collect(),
        }
    }

    /// Customize the tick interval for a specific loop.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — configurable tick interval per loop
    /// \[P7\] Constraining: Evolutionary Architecture — intervals emerge from operational need
    /// pre:  `loop_id` is a valid `LoopId`; `interval` is a positive
    ///       `Duration`.
    /// post: Returns `self` with the tick interval for `loop_id` updated
    ///       to `interval`.
    pub fn with_tick_interval(mut self, loop_id: LoopId, interval: Duration) -> Self {
        self.tick_intervals.insert(loop_id, interval);
        self
    }

    /// Register a loop with the system.
    ///
    /// Adds the loop to the registry so it can be ticked by `start()` or `tick()`.
    /// Multiple loops may share the same `LoopId`.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — register loop instances under LoopId
    /// pre:  `loop_instance` is a valid `Arc<dyn RegulationLoop>`.
    /// post: The loop is added to the registry under its `LoopId`;
    ///       logs the registration at info level.
    pub async fn register_loop(&self, loop_instance: Arc<dyn RegulationLoop>) {
        let id = loop_instance.id();
        let mut loops = self.loops.write().await;
        loops.entry(id).or_default().push(loop_instance);
        info!(
            target: "loop_system",
            loop_id = %id,
            total = loops.get(&id).map(|v| v.len()).unwrap_or(0),
            "Registered loop"
        );
    }

    /// Get the cancellation token for external cancellation.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — cancellation token stops all loops
    /// pre:  (none — accessor).
    /// post: Returns a clone of the inner `CancellationToken`.
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Start all loop tick tasks.
    ///
    /// Spawns per-loop tick tasks — each registered loop runs its
    /// `sense → compare → compute → act` cycle on a timer.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — spawn tokio tasks for each loop
    /// pre:  Loops have been registered via `register_loop`.
    /// post: Spawns a tokio task per loop instance; each task ticks
    ///       at its configured interval until cancelled.
    pub async fn start(&self) {
        let cancel = self.cancel.clone();

        let loops_map = self.loops.read().await.clone();
        for (id, loop_instances) in loops_map {
            let cancel = cancel.clone();
            let tick_interval = self
                .tick_intervals
                .get(&id)
                .copied()
                .unwrap_or(Duration::from_secs(DEFAULT_FALLBACK_TICK_SECS));

            for loop_instance in loop_instances {
                let cancel = cancel.clone();
                tokio::spawn(async move {
                    info!(
                        target: "loop_system",
                        loop_id = %id,
                        tick_interval_ms = tick_interval.as_millis() as u64,
                        "Loop tick task started"
                    );
                    let mut interval = tokio::time::interval(tick_interval);
                    loop {
                        tokio::select! {
                            _ = interval.tick() => {
                                loop_instance.tick().await;
                            }
                            _ = cancel.cancelled() => {
                                info!(
                                    target: "loop_system",
                                    loop_id = %id,
                                    "Loop tick task cancelled"
                                );
                                break;
                            }
                        }
                    }
                });
            }
        }

        info!(
            target: "loop_system",
            "LoopScheduler started with per-loop tick intervals"
        );
    }

    /// Run a single regulation cycle across all loops in authority order.
    ///
    /// Authority DAG: Curation → Cybernetics → {Inference, Memory}
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — single sense-compare-compute-act tick
    /// pre:  Loops have been registered.
    /// post: Each registered loop is ticked once in authority order;
    ///       unregistered loop IDs are silently skipped.
    pub async fn tick(&self) {
        for loop_id in AUTHORITY_ORDER {
            let loops = self.loops.read().await;
            if let Some(instances) = loops.get(&loop_id) {
                for loop_instance in instances {
                    loop_instance.tick().await;
                }
            }
        }
    }

    /// Run multiple regulation cycles.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — run multiple ticks sequentially
    /// pre:  `max_ticks` > 0.
    /// post: Calls `tick()` `max_ticks` times sequentially; logs each
    ///       completed tick at debug level.
    pub async fn tick_n(&self, max_ticks: usize) {
        for i in 0..max_ticks {
            self.tick().await;
            tracing::debug!(
                target: "loop_system",
                tick = i + 1,
                max = max_ticks,
                "Tick cycle completed"
            );
        }
    }

    /// Signal all loop tasks to stop.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — idempotent stop signal
    /// pre:  (none — idempotent).
    /// post: The cancellation token is triggered; all spawned tick tasks
    ///       will terminate on their next `select!` iteration.
    pub fn shutdown(&self) {
        info!(target: "loop_system", "LoopScheduler shutting down");
        self.cancel.cancel();
    }

    /// Total number of loop instances across all IDs.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P8\] Motivating: Semantic Grounding — count of registered loop instances
    /// pre:  (none).
    /// post: Returns the sum of `Vec::len()` across all entries in the
    ///       loop registry.
    pub async fn registered_count(&self) -> usize {
        self.loops.read().await.values().map(|v| v.len()).sum()
    }

    /// Get the IDs of all registered loops.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P8\] Motivating: Semantic Grounding — list registered loop IDs
    /// pre:  (none).
    /// post: Returns a `Vec<LoopId>` containing all keys currently in
    ///       the loop registry.
    pub async fn registered_loop_ids(&self) -> Vec<LoopId> {
        self.loops.read().await.keys().copied().collect()
    }
}

impl Default for LoopScheduler {
    fn default() -> Self {
        Self::new()
    }
}
