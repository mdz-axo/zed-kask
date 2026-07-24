//! Port error types for type-safe error handling
//!
//! Each port has its own error type to provide specific error information
//! and enable proper error handling at call sites.

use thiserror::Error;

/// Shared core error variants used across multiple agent domains.
///
/// Consolidates `Infra(#[from] InfrastructureError)`, `NoSnapshot`,
/// and `A2A` delegation.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Infrastructure failure (DB, IO, serialization, etc.)
    #[error(transparent)]
    Infra(#[from] hkask_types::InfrastructureError),

    /// No health snapshot available for metacognition cycle
    #[error("No snapshot available for metacognition cycle")]
    NoSnapshot,

    /// A2A protocol failure
    #[error(transparent)]
    A2A(#[from] crate::a2a::A2AError),
}

impl From<rusqlite::Error> for CoreError {
    fn from(e: rusqlite::Error) -> Self {
        CoreError::Infra(hkask_types::InfrastructureError::database(e.to_string()))
    }
}

/// Memory storage errors
///
/// Composes from `CoreError` for infrastructure transport-layer failures
/// and adds a `CapabilityDenied` variant for OCAP visibility/perspective constraints.
///
/// Matches the pattern used by `ConsentError`, `GoalRepositoryError`,
/// and other domain error types.
#[derive(Debug, Error)]
pub enum MemoryError {
    #[error(transparent)]
    Core(#[from] CoreError),

    #[error("Capability denied: {action} on {resource}")]
    CapabilityDenied { resource: String, action: String },
}

impl From<hkask_types::InfrastructureError> for MemoryError {
    fn from(e: hkask_types::InfrastructureError) -> Self {
        MemoryError::Core(CoreError::Infra(e))
    }
}

impl From<hkask_storage::DatabaseError> for MemoryError {
    fn from(e: hkask_storage::DatabaseError) -> Self {
        MemoryError::Core(CoreError::Infra(
            hkask_types::InfrastructureError::database(e.to_string()),
        ))
    }
}

#[cfg(feature = "sql")]
impl From<hkask_memory::EpisodicMemoryError> for MemoryError {
    fn from(e: hkask_memory::EpisodicMemoryError) -> Self {
        match e {
            hkask_memory::EpisodicMemoryError::HMem(inner) => inner.into(),
            hkask_memory::EpisodicMemoryError::InvalidVisibility(msg) => {
                MemoryError::CapabilityDenied {
                    resource: "episodic_memory".into(),
                    action: msg,
                }
            }
            hkask_memory::EpisodicMemoryError::MissingPerspective => {
                MemoryError::CapabilityDenied {
                    resource: "episodic_memory".into(),
                    action: "requires a perspective (agent WebID)".into(),
                }
            }
        }
    }
}

impl From<hkask_memory::SemanticMemoryError> for MemoryError {
    fn from(e: hkask_memory::SemanticMemoryError) -> Self {
        match e {
            hkask_memory::SemanticMemoryError::HMem(inner) => inner.into(),
            hkask_memory::SemanticMemoryError::Embedding(inner) => inner.into(),
            hkask_memory::SemanticMemoryError::InvalidVisibility(msg) => {
                MemoryError::CapabilityDenied { resource: "semantic_memory".into(), action: msg }
            }
            hkask_memory::SemanticMemoryError::NoEmbeddingsForCentroid(msg) => {
                MemoryError::Core(CoreError::Infra(hkask_types::InfrastructureError::NotFound(msg)))
            }
            hkask_memory::SemanticMemoryError::HasPerspective => MemoryError::CapabilityDenied {
                resource: "semantic_memory".into(),
                action: "requires no perspective (use consolidation bridge for episodic→semantic promotion)".into(),
            },
        }
    }
}

impl From<hkask_storage::HMemError> for MemoryError {
    fn from(e: hkask_storage::HMemError) -> Self {
        match e {
            hkask_storage::HMemError::Infra(inner) => MemoryError::Core(CoreError::Infra(inner)),
            hkask_storage::HMemError::NotFound(nf) => {
                MemoryError::Core(CoreError::Infra(hkask_types::InfrastructureError::from(nf)))
            }
        }
    }
}

impl From<hkask_storage::EmbeddingError> for MemoryError {
    fn from(e: hkask_storage::EmbeddingError) -> Self {
        match e {
            hkask_storage::EmbeddingError::Infrastructure(inner) => {
                MemoryError::Core(CoreError::Infra(inner))
            }
            hkask_storage::EmbeddingError::NotFound(nf) => MemoryError::Core(CoreError::Infra(
                hkask_types::InfrastructureError::NotFound(nf.to_string()),
            )),
            hkask_storage::EmbeddingError::DimensionMismatch { .. } => {
                MemoryError::Core(CoreError::Infra(
                    hkask_types::InfrastructureError::Serialization(e.to_string()),
                ))
            }
            hkask_storage::EmbeddingError::Storage(_) => MemoryError::Core(CoreError::Infra(
                hkask_types::InfrastructureError::database(e.to_string()),
            )),
            hkask_storage::EmbeddingError::Decode(msg) => MemoryError::Core(CoreError::Infra(
                hkask_types::InfrastructureError::Serialization(msg),
            )),
        }
    }
}

// ── MemoryError → MemoryPortError conversion ─────────────────────────────
//
// Adapters in hkask-agents use MemoryError internally. The port traits
// (now in hkask-memory) return MemoryPortError. This From impl lets `?`
// auto-convert at the adapter boundary (ADR-042).

impl From<MemoryError> for hkask_memory::MemoryPortError {
    fn from(e: MemoryError) -> Self {
        match e {
            MemoryError::Core(core) => hkask_memory::MemoryPortError::Storage(core.to_string()),
            MemoryError::CapabilityDenied { resource, action } => {
                hkask_memory::MemoryPortError::CapabilityDenied { resource, action }
            }
        }
    }
}

impl From<hkask_memory::MemoryPortError> for MemoryError {
    fn from(e: hkask_memory::MemoryPortError) -> Self {
        match e {
            hkask_memory::MemoryPortError::Storage(msg) => MemoryError::Core(CoreError::Infra(
                hkask_types::InfrastructureError::database(msg),
            )),
            hkask_memory::MemoryPortError::CapabilityDenied { resource, action } => {
                MemoryError::CapabilityDenied { resource, action }
            }
        }
    }
}
