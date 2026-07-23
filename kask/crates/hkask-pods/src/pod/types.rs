//! Pod value types — PodLifecycleState, PodID, PodKind, template types

pub use hkask_types::PodID;
use serde::{Deserialize, Serialize};

/// Pod tier — determines isolation model and filename convention.
///
/// - Curator: singleton system daemon, owns SemanticIndex, Regulation aggregation
/// - UserPod: per-user sovereign pod (1:1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PodKind {
    Curator,
    #[default]
    UserPod,
}

impl std::fmt::Display for PodKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PodKind::Curator => write!(f, "curator"),
            PodKind::UserPod => write!(f, "userpod"),
        }
    }
}

/// Pod lifecycle state machine.
///
/// Simplified for the 1:1 persistent-pod model: a pod is created `Active`
/// (A2A-reachable, inference available). When the user logs out or the
/// account goes inactive, the pod `Sleep`s (storage-at-rest, no compute,
/// no A2A reachability). Logging back in wakes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PodLifecycleState {
    /// Pod is running — A2A-reachable, inference available.
    Active,
    /// Pod is sleeping — storage-at-rest, no compute, no A2A reachability.
    /// User is logged out or account is inactive-but-not-cancelled.
    Sleeping,
}

impl PodLifecycleState {
    /// Whether a transition from `self` to `next` is legal.
    ///
    /// The lifecycle is bidirectional: `Active ↔ Sleeping`.
    /// Re-stating the current state is a no-op and always permitted.
    ///
    /// expect: "Agent interactions are gated by OCAP boundaries"
    /// \[P4\] Motivating: Clear Boundaries — lifecycle state machine enforces transitions
    /// pre:  `self` and `next` are valid `PodLifecycleState` variants.
    /// post: Returns `true` if `self == next` (idempotent) or if the
    ///       transition is `Active ↔ Sleeping`; `false` otherwise.
    pub fn can_transition_to(&self, next: PodLifecycleState) -> bool {
        if *self == next {
            return true;
        }
        matches!(
            (self, next),
            (PodLifecycleState::Active, PodLifecycleState::Sleeping)
                | (PodLifecycleState::Sleeping, PodLifecycleState::Active)
        )
    }
}

impl std::fmt::Display for PodLifecycleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PodLifecycleState::Active => write!(f, "active"),
            PodLifecycleState::Sleeping => write!(f, "sleeping"),
        }
    }
}

// ── Communication Accommodation Theory (CAT) — curator convergence posture ─────
// (Curator-only; userpods have no persona. Retained for the curator daemon's
// Matrix engagement posture.)

/// Communication posture — governs whether and how the curator engages via Matrix.
///
/// Grounded in Communication Accommodation Theory (Giles): convergence is the
/// single dimension along which the curator decides to speak or remain silent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunicationPosture {
    /// Convergence bias: 0.0 (silent, divergent) to 1.0 (fully convergent).
    /// Default: 0.5 — balanced, responds to direct engagement.
    #[serde(default = "default_convergence_bias")]
    pub convergence_bias: f64,

    /// Core traits never compromised by accommodation (consistency anchor).
    #[serde(default)]
    pub invariant_traits: Vec<String>,
}

fn default_convergence_bias() -> f64 {
    0.5
}

impl Default for CommunicationPosture {
    fn default() -> Self {
        Self {
            convergence_bias: 0.5,
            invariant_traits: vec![],
        }
    }
}
