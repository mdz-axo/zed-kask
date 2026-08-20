//! BundleManifest type system — skill manifests for hKask
//!
//! Re-export facade. Submodules organized by concern:
//! - `manifest`: BundleManifest, BundleManifestStep, ManifestCategory, OnFailureConfig
//! - `config`: ConvergenceConfig, ErrorHandlingConfig, BundleLedgerConfig, BundleAuditConfig

pub mod config;
pub mod manifest;

pub use config::*;
pub use manifest::{
    BundleManifest, BundleManifestStep, MAX_CONCURRENCY, ManifestCategory, OnFailureConfig,
};
