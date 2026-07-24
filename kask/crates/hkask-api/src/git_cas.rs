//! Git CAS adapter bundle (P2.2).
//!
//! Extracted from `ApiState` construction to keep CAS initialization self-contained
//! and to surface the `expect("Failed to create GixCasAdapter")` failure as a
//! typed `ApiError::Internal` rather than a panic at startup (P4.1).

use std::path::PathBuf;
use std::sync::Arc;

use crate::error::ApiError;

/// Git CAS adapter bundle (P2.2).
///
/// Extracted from `ApiState` construction to keep CAS initialization self-contained
/// and to surface the `expect("Failed to create GixCasAdapter")` failure as a
/// typed `ApiError::Internal` rather than a panic at startup (P4.1).
pub(crate) struct GitCasBundle {
    /// TemplateCrateLoader — disk-based template loading.
    pub template_adapter: Arc<hkask_templates::TemplateCrateLoader>,
    /// Trait-object `GitCASPort` (hexagonal boundary) used by stores.
    pub git_cas_port: Arc<dyn hkask_types::git_cas::GitCASPort>,
    /// Concrete `GixCasAdapter` for admin operations (resolve_ref, diff)
    /// that are not part of the backup contract.
    pub gix_cas: Arc<hkask_git_cas::GixCasAdapter>,
}

/// Initialize the Git CAS adapter and the trait-object port.
///
/// `git_cas` writes to a fixed on-disk directory; `git_cas_port` resolves
/// from `GIX_*` env vars when present and falls back to the same directory.
///
/// P4.1: Returns `Result<GitCasBundle, ApiError>` so CAS initialization
/// failures surface as typed errors instead of panics. The hard-coded
/// `/tmp/hkask-templates` fallback directory is the documented invariant
/// of this function — if even that cannot be created, returning
/// `ApiError::Internal` is the correct (non-panicking) failure mode.
pub(crate) fn init_git_cas() -> Result<GitCasBundle, ApiError> {
    let template_adapter = Arc::new(hkask_templates::TemplateCrateLoader::from_path(
        PathBuf::from("/tmp/hkask-templates"),
    ));
    let fallback_path = PathBuf::from("/tmp/hkask-templates");
    let gix_cas = Arc::new(
        hkask_git_cas::GixCasAdapter::from_env()
            .or_else(|_| hkask_git_cas::GixCasAdapter::new(fallback_path))
            .map_err(|e| ApiError::Internal {
                message: format!("Failed to create GixCasAdapter: {e}"),
            })?,
    );
    let git_cas_port: Arc<dyn hkask_types::git_cas::GitCASPort> = gix_cas.clone();
    Ok(GitCasBundle {
        template_adapter,
        git_cas_port,
        gix_cas,
    })
}
