//! Memory port error — the error boundary for memory storage port traits.
//!
//! Lives in `hkask-memory` alongside the port traits. Adapters in other crates
//! return this type from port impls. The adapters' internal error types
//! (e.g., `MemoryError` in `hkask-agents`) implement `From` conversions
//! into this type so that `?` propagation works across the boundary.

use thiserror::Error;

/// Errors that memory storage port implementations can produce.
///
/// Two variants mirror the two failure modes of the OCAP-guarded
/// memory boundary:
///
/// - **Storage** — infrastructure failure (DB, IO, serialization)
/// - **CapabilityDenied** — OCAP visibility/perspective constraint violation
#[derive(Debug, Error)]
pub enum MemoryPortError {
    /// Infrastructure failure during storage or recall.
    #[error("{0}")]
    Storage(String),

    /// OCAP capability denied — missing read/write permission.
    #[error("Capability denied: {action} on {resource}")]
    CapabilityDenied { resource: String, action: String },
}

// ── Conversions from hkask-memory domain error types ────────────────────

impl From<crate::EpisodicMemoryError> for MemoryPortError {
    fn from(e: crate::EpisodicMemoryError) -> Self {
        MemoryPortError::Storage(e.to_string())
    }
}

impl From<crate::SemanticMemoryError> for MemoryPortError {
    fn from(e: crate::SemanticMemoryError) -> Self {
        MemoryPortError::Storage(e.to_string())
    }
}
