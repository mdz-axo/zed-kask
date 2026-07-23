//! Formatting helpers for metacognition output.

use hkask_types::regulation::LedgerHealth;

pub(super) fn format_health_status(h: &LedgerHealth) -> String {
    if h.healthy {
        format!(
            "Healthy (deficit={}, warnings={})",
            h.overall_deficit, h.warning_count
        )
    } else {
        format!(
            "Degraded (deficit={}, critical={}, warnings={})",
            h.overall_deficit, h.critical_count, h.warning_count
        )
    }
}
