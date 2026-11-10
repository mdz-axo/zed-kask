//! Shared integration-test helpers for the hkask-private-inference tests.
//!
//! The fleet-discovery setup (seed the briefs, discover one model's brief
//! through the production path) is repeated by the live test files; one
//! helper, not one copy per file (the ratchet's duplication gate caught
//! the repeated block).

use tempfile::tempdir;

use hkask_private_inference::local_leg::fleet;

/// Discover one model's brief through the production fleet path (seed +
/// discover — never a CWD-relative file read).
pub fn discover_brief(
    model_id: &str,
) -> hkask_private_inference::local_leg::model_brief::ModelBrief {
    let briefs_dir = tempdir().expect("briefs dir");
    fleet::seed_briefs(briefs_dir.path()).expect("the seed set writes");
    let models_root = tempdir().expect("models root");
    fleet::discover_model(briefs_dir.path(), models_root.path(), model_id)
        .expect("the discovered fleet carries the model brief")
        .brief
}
