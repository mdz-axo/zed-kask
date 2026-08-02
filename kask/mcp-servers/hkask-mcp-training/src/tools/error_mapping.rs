//! MCP tool-error classification for the training server's domain errors.
//!
//! `McpToolError` is a flat `{ kind, message, details }` type with no `source`
//! field, so only the wire-level *kind* can be preserved, not the error chain.
//! Mapping every variant of a domain error to `McpToolError::internal` (or any
//! single kind) mis-classifies `NotFound` / `Unavailable` / auth errors as
//! `Internal`. Each `map_*` fn below classifies per variant instead.

use crate::adapter::AdapterStoreError;
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
        TrainingArtifactError::InvalidConfiguration => McpToolError::failed_precondition(message),
        TrainingArtifactError::Upload
        | TrainingArtifactError::Retrieval
        | TrainingArtifactError::InvalidManifest => McpToolError::internal(message),
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
