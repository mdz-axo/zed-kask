//! MCP tool-error classification for the training server's domain errors.
//!
//! `McpToolError` is a flat `{ kind, message, details }` type with no `source`
//! field, so only the wire-level *kind* can be preserved, not the error chain.
//! Mapping every variant of a domain error to `McpToolError::internal` (or any
//! single kind) mis-classifies `NotFound` / `Unavailable` / auth errors as
//! `Internal`. Each `map_*` fn below classifies per variant instead.

use crate::adapter::AdapterStoreError;
use crate::adapters::JobStoreError;
use crate::dataset::DatasetError;
use crate::huggingface::TrainingArtifactError;
use crate::providers::types::HostProviderError;
use hkask_mcp_server::server::McpToolError;

/// Classify an `AdapterStoreError` into the MCP wire-level `McpToolError` kind.
pub fn map_adapter_store_error(e: AdapterStoreError) -> McpToolError {
    let message = e.to_string();
    match e {
        AdapterStoreError::NotFound(_) | AdapterStoreError::ExpertiseNotFound(_) => {
            McpToolError::not_found(message)
        }
        AdapterStoreError::InvalidState(_) => McpToolError::failed_precondition(message),
        AdapterStoreError::ChecksumMismatch { .. }
        | AdapterStoreError::Database(_)
        | AdapterStoreError::Infra(_)
        | AdapterStoreError::Serialization(_) => McpToolError::internal(message),
    }
}

/// Classify a `HostProviderError` into the MCP wire-level `McpToolError` kind.
pub fn map_host_provider_error(e: HostProviderError) -> McpToolError {
    let message = e.to_string();
    match e {
        HostProviderError::Unavailable(_) => McpToolError::unavailable(message),
        HostProviderError::InvalidConfig(_) | HostProviderError::DatasetError(_) => {
            McpToolError::invalid_argument(message)
        }
        HostProviderError::JobFailed(_) | HostProviderError::Backend(_) => {
            McpToolError::internal(message)
        }
    }
}

/// Classify a `TrainingArtifactError` into the MCP wire-level `McpToolError` kind.
pub fn map_training_artifact_error(e: TrainingArtifactError) -> McpToolError {
    let message = e.to_string();
    match e {
        TrainingArtifactError::InvalidConfiguration(_) => {
            McpToolError::failed_precondition(message)
        }
        TrainingArtifactError::Upload(_)
        | TrainingArtifactError::Retrieval(_)
        | TrainingArtifactError::InvalidManifest(_) => McpToolError::internal(message),
    }
}

/// Classify a `std::io::Error` from a tool-level filesystem operation.
///
/// A missing file is `not_found` and a permission failure is
/// `permission_denied`; other I/O kinds stay `internal`.
pub fn map_fs_error(context: &str, e: std::io::Error) -> McpToolError {
    let message = format!("{context}: {e}");
    match e.kind() {
        std::io::ErrorKind::NotFound => McpToolError::not_found(message),
        std::io::ErrorKind::PermissionDenied => McpToolError::permission_denied(message),
        _ => McpToolError::internal(message),
    }
}

/// Classify a `DatasetError` into the MCP wire-level `McpToolError` kind.
///
/// `Io`/`Cache` are infrastructure failures (internal); `UnsupportedFormat` /
/// `Validation` / `Empty` are user-input problems (invalid_argument).
pub fn map_dataset_error(e: DatasetError) -> McpToolError {
    let message = e.to_string();
    match e {
        DatasetError::UnsupportedFormat(_)
        | DatasetError::Validation { .. }
        | DatasetError::Empty => McpToolError::invalid_argument(message),
        DatasetError::Io(_) | DatasetError::Cache(_) => McpToolError::internal(message),
    }
}

/// Classify a `JobStoreError` from a job-persistence operation into the MCP
/// wire-level `McpToolError` kind. `Storage` (SQLite failure) is `unavailable`
/// (transient infra — the operator can retry); `Serialization` is `internal`.
pub fn map_job_store_error(e: JobStoreError) -> McpToolError {
    let message = e.to_string();
    match e {
        JobStoreError::Storage(_) => McpToolError::unavailable(message),
        JobStoreError::Serialization(_) => McpToolError::internal(message),
    }
}

/// Classify a `SemanticMemoryError` from a semantic-memory query into the MCP
/// wire-level `McpToolError` kind: `NotFound` variants → `not_found`,
/// infrastructure → per-variant via the shared `map_infra_error`, domain
/// contract violations (`InvalidVisibility`, `HasPerspective`) →
/// `invalid_argument`, missing centroid embeddings → `not_found`, remaining
/// embedding failures → `internal`.
pub fn map_semantic_memory_error(e: hkask_memory::SemanticMemoryError) -> McpToolError {
    use hkask_memory::SemanticMemoryError;
    let message = e.to_string();
    match e {
        SemanticMemoryError::HMem(hkask_storage::HMemError::NotFound(_)) => {
            McpToolError::not_found(message)
        }
        SemanticMemoryError::HMem(hkask_storage::HMemError::Infra(ref infra)) => {
            hkask_mcp_server::map_infra_error(infra, "semantic memory query")
        }
        SemanticMemoryError::Embedding(hkask_storage::EmbeddingError::NotFound(_)) => {
            McpToolError::not_found(message)
        }
        SemanticMemoryError::Embedding(hkask_storage::EmbeddingError::Infrastructure(
            ref infra,
        )) => hkask_mcp_server::map_infra_error(infra, "semantic memory query"),
        SemanticMemoryError::InvalidVisibility(_) | SemanticMemoryError::HasPerspective => {
            McpToolError::invalid_argument(message)
        }
        SemanticMemoryError::NoEmbeddingsForCentroid(_) => McpToolError::not_found(message),
        SemanticMemoryError::Embedding(_) => McpToolError::internal(message),
    }
}
