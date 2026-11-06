//! Shared cfg(test) fixtures for media_panel's test modules — one canonical
//! copy (the kask_bridge `test_support` pattern): `mod tests` and
//! `mod viewer_layout_tests` in `media_viewer.rs` are sibling modules, so a
//! fixture defined in either is invisible to the other.

use hkask_mcp_media::types::JobRecord;

/// A job record with explicit identity, op, and timing — the L11 shape needs
/// controlled `created_at` ordering (a deep job older than the page's newer
/// completions).
pub(crate) fn scheduled_job(id: &str, op: &str, status: &str, created_at: &str) -> JobRecord {
    JobRecord {
        id: id.into(),
        op: op.into(),
        status: status.into(),
        created_at: created_at.into(),
        completed_at: None,
        result: None,
        error: None,
        source: None,
    }
}
