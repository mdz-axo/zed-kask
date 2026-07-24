//! Health check endpoint — Kubernetes readiness probe.
//!
//! Verifies:
//! - Database is reachable (real SQL query via UserStore)
//! - Matrix/Conduit is reachable (HTTP to Conduit's versions endpoint)
//! - Data volume disk usage (warn if >80%, critical if >95%)
//!
//! Returns 503 with a JSON body listing failures if DB or Conduit checks fail.
//! Disk space warnings do NOT fail the probe — they're informational signals
//! for the Regulation autonomous storage guard loop.

use crate::ApiState;
use axum::extract::State;
use axum::response::{IntoResponse, Json};
use serde::Serialize;

#[derive(Serialize)]
struct HealthResponse {
    healthy: bool,
    db: bool,
    conduit: bool,
    /// Percentage of /data volume used (0–100)
    disk_usage_pct: Option<u8>,
    /// Disk space status: "ok", "warn", or "critical"
    disk_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// GET /health — readiness probe. Checks DB + Matrix connectivity + disk space.
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "All critical checks passed"),
        (status = 503, description = "One or more critical checks failed"),
    ),
)]
pub async fn health_check(State(state): State<ApiState>) -> impl IntoResponse {
    let db_ok = check_db(&state);
    let conduit_ok = check_conduit().await;
    let (disk_usage_pct, disk_status) = check_disk();

    // Only DB and Conduit failures make the pod "not ready."
    // Disk space is informational — the Regulation storage guard loop handles it.
    let healthy = db_ok && conduit_ok;

    let error = if !healthy {
        let mut parts = Vec::new();
        if !db_ok {
            parts.push("database unreachable");
        }
        if !conduit_ok {
            parts.push("conduit unreachable");
        }
        Some(parts.join("; "))
    } else {
        None
    };

    let body = HealthResponse {
        healthy,
        db: db_ok,
        conduit: conduit_ok,
        disk_usage_pct,
        disk_status,
        error,
    };

    let status = if healthy {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(body)).into_response()
}

fn check_db(state: &ApiState) -> bool {
    state.agent_service.storage().list_userpods().is_ok()
}

async fn check_conduit() -> bool {
    let url = match std::env::var("HKASK_MATRIX_URL") {
        Ok(u) => format!("{u}/_matrix/client/versions"),
        Err(_) => return false,
    };
    match reqwest::get(&url).await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Check disk usage on the data directory.
///
/// Returns (usage_percentage_or_none, status_string).
/// Does NOT fail the health probe — disk pressure is handled by the
/// Regulation autonomous storage guard loop.
fn check_disk() -> (Option<u8>, String) {
    let dir = std::env::var("HKASK_DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let path = std::path::Path::new(&dir);
    if !path.exists() {
        return (None, "unknown".to_string());
    }
    let mut used_bytes: u64 = 0;
    walk_dir_safe(path, &mut used_bytes, 0, &mut 0);
    let pvc_capacity: u64 = std::env::var("HKASK_PVC_CAPACITY_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20 * 1024 * 1024 * 1024);
    let pct = ((used_bytes as f64 / pvc_capacity as f64) * 100.0) as u8;
    let pct = pct.min(100);
    if pct > 95 {
        (Some(pct), "critical".to_string())
    } else if pct > 80 {
        (Some(pct), "warn".to_string())
    } else {
        (Some(pct), "ok".to_string())
    }
}

/// Walk directory tree with depth/file-count bounds to prevent
/// unbounded traversal on corrupted/symlink-loop filesystems.
fn walk_dir_safe(path: &std::path::Path, total: &mut u64, depth: usize, file_count: &mut usize) {
    const MAX_DEPTH: usize = 10;
    const MAX_FILES: usize = 100_000;
    if depth > MAX_DEPTH || *file_count > MAX_FILES {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            *file_count += 1;
            if *file_count > MAX_FILES {
                return;
            }
            if let Ok(meta) = entry.metadata() {
                if meta.is_symlink() {
                    continue;
                }
                *total += meta.len();
                if meta.is_dir() {
                    walk_dir_safe(&entry.path(), total, depth + 1, file_count);
                }
            }
        }
    }
}
