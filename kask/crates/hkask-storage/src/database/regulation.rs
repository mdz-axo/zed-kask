//! Regulation span helpers for storage observability.
//!
//! Emits tracing events on `target: "reg.storage"` so the Regulation regulator
//! can observe query latency, error rates, and throughput per table.

/// Extract a table name from a SQL statement.
///
/// Returns the first identifier after FROM, INSERT INTO, UPDATE,
/// DELETE FROM, or INTO. Returns "unknown" if no table can be identified.
pub(crate) fn extract_table(sql: &str) -> &str {
    let upper = sql.to_uppercase();
    for keyword in &["FROM", "INSERT INTO", "UPDATE", "DELETE FROM", "INTO"] {
        if let Some(pos) = upper.find(keyword) {
            let rest = &sql[pos + keyword.len()..].trim();
            return rest.split_whitespace().next().unwrap_or("unknown");
        }
    }
    "unknown"
}

/// Emit a Regulation span for a completed storage operation.
///
/// The regulator consumes these events on `target: "reg.storage"` to
/// track latency distributions, error rates, and throughput per table.
pub(crate) fn emit_storage_span(
    operation: &str,
    table: &str,
    duration_us: u64,
    rows: usize,
    error: bool,
) {
    tracing::info!(
        target: "reg.storage",
        operation = operation,
        table = table,
        duration_us = duration_us,
        rows = rows,
        error = error,
        "Storage operation completed"
    );
}
