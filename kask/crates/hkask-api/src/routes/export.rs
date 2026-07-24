//! Export routes — sovereignty archive creation and migration for P1 data portability.
//!
//! # REQ: P1-deploy-backup-export — P1 User Sovereignty: export/upload encrypted h_mem archive.
//! expect: "I can export and upload my encrypted h_mem archive for data portability"
//!
//! `POST /api/v1/export/create` — generate encrypted sovereignty archive.
//! `POST /api/v1/export/upload` — upload archive for server migration.

use axum::{Extension, Json, extract::State, http::StatusCode, response::Response};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing;
use utoipa::ToSchema;

use crate::ApiState;
use crate::middleware::AuthContext;
use hkask_storage::{BackupArchive, HMemStore, MigrationReceipt};

/// Acquire a short-lived `HMemStore` bound to the shared user-store driver.
///
/// Both export handlers need an `HMemStore` to read/write h_mems. The canonical
/// store is the `UserStore` held under `state.agent_service.storage().users`.
/// Each handler takes a brief lock, clones the `Arc<dyn DatabaseDriver>`, and
/// constructs a transient `HMemStore` that is dropped at handler exit. The
/// underlying driver (and its connection pool) persists because it is `Arc`-shared.
///
/// pre:  `state.agent_service.storage().users` is initialized
/// post: returns an `HMemStore` bound to the same driver as the user store
fn h_mem_store_from_state(state: &ApiState) -> Result<HMemStore, (StatusCode, String)> {
    let user_store = state.agent_service.storage().users.clone();
    let store = user_store.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lock error: {e}"),
        )
    })?;
    Ok(HMemStore::from_driver(Arc::clone(store.driver())))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ExportRequest {
    pub passphrase: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExportResponse {
    pub archive_path: String,
    pub triple_count: u64,
    pub bytes: u64,
    pub duration_ms: u64,
}

/// POST /api/v1/export/create
#[utoipa::path(
    post,
    path = "/api/v1/export/create",
    tag = "export",
    request_body = ExportRequest,
    responses(
        (status = 200, description = "Export archive created successfully", body = ExportResponse),
        (status = 400, description = "Bad request — passphrase too short or empty archive"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub async fn export_create(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<ExportRequest>,
) -> Result<Json<ExportResponse>, (StatusCode, String)> {
    let start = Instant::now();
    if req.passphrase.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Passphrase must be at least 8 characters".to_string(),
        ));
    }
    let webid = auth.webid;
    let export_dir = PathBuf::from("/var/lib/hkask/exports").join(webid.to_string());
    std::fs::create_dir_all(&export_dir).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create export directory: {e}"),
        )
    })?;
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let archive_path = export_dir.join(format!("{timestamp}.db"));
    let h_mem_store = h_mem_store_from_state(&state)?;
    let domain = std::env::var("HKASK_DOMAIN").unwrap_or_else(|_| "localhost".to_string());
    let archive = BackupArchive::create(
        archive_path.clone(),
        &req.passphrase,
        &h_mem_store,
        &webid,
        &domain,
    )
    .map_err(|e| {
        let status = if matches!(e, hkask_storage::ArchiveError::Empty) {
            StatusCode::BAD_REQUEST
        } else {
            tracing::error!(target: "hkask.api.export", error = %e, "Failed to create archive");
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, format!("Archive creation failed: {e}"))
    })?;
    let triple_count = archive
        .triple_count()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?
        as u64;
    let bytes = std::fs::metadata(archive.path())
        .map(|m| m.len())
        .unwrap_or(0);
    let duration_ms = start.elapsed().as_millis() as u64;
    tracing::info!(target = "reg.backup.export", webid = %webid, triple_count = triple_count, bytes = bytes, duration_ms = duration_ms, "REG");
    Ok(Json(ExportResponse {
        archive_path: archive.path().to_string_lossy().to_string(),
        triple_count,
        bytes,
        duration_ms,
    }))
}

// ── Upload ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
pub struct UploadRequest {
    pub archive_base64: String,
    pub passphrase: String,
}

/// POST /api/v1/export/upload
///
/// expect: "My API access is scoped to my sovereignty boundaries"
#[utoipa::path(
    post,
    path = "/api/v1/export/upload",
    tag = "export",
    request_body = UploadRequest,
    responses(
        (status = 200, description = "Archive uploaded and imported successfully", body = MigrationReceipt),
        (status = 400, description = "Bad request — invalid passphrase or corrupted archive"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub async fn export_upload(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<UploadRequest>,
) -> Result<Json<MigrationReceipt>, (StatusCode, String)> {
    let start = Instant::now();
    if req.passphrase.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Passphrase must be at least 8 characters".to_string(),
        ));
    }
    let archive_bytes = base64::engine::general_purpose::STANDARD
        .decode(&req.archive_base64)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid base64: {e}")))?;
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("hkask-upload-{}.db", uuid::Uuid::new_v4()));
    std::fs::write(&tmp_path, &archive_bytes).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write temp file: {e}"),
        )
    })?;
    let archive = BackupArchive::open(tmp_path.clone(), &req.passphrase).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        let msg = if e.to_string().to_lowercase().contains("sqlite")
            || e.to_string().contains("not a database")
        {
            "Wrong passphrase or corrupted archive file".to_string()
        } else {
            format!("Failed to open archive: {e}")
        };
        (StatusCode::BAD_REQUEST, msg)
    })?;
    let h_mem_store = h_mem_store_from_state(&state)?;
    let webid = auth.webid;
    let receipt = archive.restore_into(&h_mem_store, &webid).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Import failed: {e}"),
        )
    })?;
    let duration_ms = start.elapsed().as_millis() as u64;
    tracing::info!(target = "reg.backup.upload", webid = %webid, triple_count = receipt.triple_count, duration_ms = duration_ms, "REG");
    let _ = std::fs::remove_file(&tmp_path);
    Ok(Json(receipt))
}

/// Build the export router.
pub fn export_router() -> utoipa_axum::router::OpenApiRouter<ApiState> {
    use utoipa_axum::router::OpenApiRouter;
    use utoipa_axum::routes;
    OpenApiRouter::new()
        .routes(routes!(export_create))
        .routes(routes!(export_upload))
        .route(
            "/api/v1/export/download",
            axum::routing::get(export_download),
        )
}

/// GET /api/v1/export/download — download the latest export archive for the authenticated user.
///
/// expect: "My API access is scoped to my sovereignty boundaries"
pub async fn export_download(
    State(_state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Response, (StatusCode, String)> {
    let webid = auth.webid;
    let export_dir = PathBuf::from("/var/lib/hkask/exports").join(webid.to_string());

    // Find the latest archive
    let mut entries: Vec<_> = std::fs::read_dir(&export_dir)
        .map_err(|_| (StatusCode::NOT_FOUND, "No exports found".to_string()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "db"))
        .collect();
    entries.sort_by_key(|e| std::fs::metadata(e.path()).and_then(|m| m.modified()).ok());
    entries.reverse();

    let latest = entries
        .first()
        .ok_or((StatusCode::NOT_FOUND, "No exports found".to_string()))?;
    let bytes = std::fs::read(latest.path()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to read archive: {e}"),
        )
    })?;

    let filename = latest.file_name().to_string_lossy().to_string();
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/octet-stream")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(axum::body::Body::from(bytes))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}
