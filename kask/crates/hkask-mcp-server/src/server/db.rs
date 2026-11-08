//! Server-database bootstrap (dual-axis review Batch C): the shared
//! open-or-repair path every SQLCipher-backed built-in server runs — D28
//! path resolution with the parent self-heal, the one passphrase through
//! the canonical 2-tier chain, open+repair, and the labeled SQLite
//! driver. The caller keeps its failure posture: startup-refused
//! (`Infrastructure`) or degraded-to-empty (the curator's deliberate
//! posture) — never a silent empty-key open.

use std::collections::HashMap;
use std::sync::Arc;

use super::credentials::resolve_db_passphrase;
use super::error::McpToolError;
use hkask_storage::database::driver::DatabaseDriver;

/// An opened server database: the path it was opened at, the handle the
/// pool is cached on, and the labeled driver every store layer builds on.
pub struct ServerDatabase {
    /// The path the database was opened at (the env override or the D28
    /// per-agent default) — the labeled driver and diagnostics carry it.
    pub path: String,
    /// The open database handle (`sqlite_pool()` serves cached pools from
    /// it, so later store layers can fetch the pool cheaply).
    pub database: hkask_storage::Database,
    /// The SQLite driver labeled with the open path.
    pub driver: Arc<dyn DatabaseDriver>,
}

/// Resolve a server DB path: the env override, else the caller-supplied
/// D28 relative default resolved under the per-agent data dir, with the
/// parent-directory self-heal (D28: every write path `create_dir_all`s its
/// parent — a missing directory would surface as a missing-directory
/// error instead of the real DB error).
#[must_use = "result must be used"]
pub fn resolve_server_db_path(env_var: &str, default_relative: &std::path::Path) -> String {
    std::env::var(env_var).unwrap_or_else(|_| {
        let default_path = hkask_types::agent_paths::resolve_under_data_dir(default_relative);
        if let Some(Err(error)) = default_path.parent().map(std::fs::create_dir_all) {
            tracing::warn!(
                target: "hkask.mcp.db",
                path = %default_path.display(),
                %error,
                "Failed to create the default server DB directory \
                 — the subsequent DB open will surface the failure"
            );
        }
        tracing::info!(
            target: "hkask.mcp.db",
            path = %default_path.display(),
            "Using default per-agent server database"
        );
        default_path.to_string_lossy().to_string()
    })
}

/// Open a server database: the parent self-heal, the one passphrase
/// through the canonical 2-tier chain (ctx.credentials → env →
/// keychain), open-or-repair, the pool, and the labeled SQLite driver.
/// The error is the canonical `McpToolError` — the caller maps it to its
/// failure posture, never a silent empty-key open.
pub fn open_server_db(
    db_path: &str,
    credentials: &HashMap<String, String>,
) -> Result<ServerDatabase, McpToolError> {
    let path = std::path::Path::new(db_path);
    if let Some(Err(error)) = path.parent().map(std::fs::create_dir_all) {
        tracing::warn!(
            target: "hkask.mcp.db",
            path = %path.display(),
            %error,
            "Failed to create the server DB directory \
             — the subsequent DB open will surface the failure"
        );
    }
    let passphrase = resolve_db_passphrase(credentials)?;
    let database = hkask_storage::open_or_repair(db_path, &passphrase)
        .map_err(|error| McpToolError::internal(format!("open {}: {error}", path.display())))?;
    let pool = database
        .sqlite_pool()
        .map_err(|error| McpToolError::internal(format!("sqlite pool: {error}")))?;
    let driver: Arc<dyn DatabaseDriver> = Arc::new(
        hkask_storage::database::sqlite::SqliteDriver::new_labeled(pool, db_path),
    );
    Ok(ServerDatabase {
        path: db_path.to_string(),
        database,
        driver,
    })
}

/// The shared `HKASK_DB_PASSPHRASE` credential requirement every
/// SQLCipher-backed server declares (byte-identical literals across the
/// servers before the extraction — dual-axis review Batch C).
#[must_use = "result must be used"]
pub fn db_passphrase_requirement() -> crate::CredentialRequirement {
    crate::CredentialRequirement::required(
        "HKASK_DB_PASSPHRASE",
        "SQLCipher encryption passphrase (resolved via hkask keystore chain when not set)",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bootstrap's seam test (ra-strangle: one test per service
    /// operation, at the new seam): open under an allowed root, repair,
    /// label, and hand back the pool and driver.
    #[test]
    fn open_server_db_opens_repairs_and_labels() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/c-bootstrap-probe");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("probe.db").to_string_lossy().to_string();
        let credentials = HashMap::from([(
            "HKASK_DB_PASSPHRASE".to_string(),
            "c-bootstrap-key".to_string(),
        )]);
        let server_db = open_server_db(&path, &credentials).expect("open");
        assert_eq!(server_db.path, path);
        assert!(
            server_db
                .database
                .sqlite_pool()
                .expect("the pool is served from the cached handle")
                .max_size()
                > 0
        );
        // The parent self-heal: a second open on a fresh subdirectory
        // creates it instead of a missing-directory error.
        let nested = dir.join("nested/deeper").join("probe.db");
        let nested_path = nested.to_string_lossy().to_string();
        let reopened = open_server_db(&nested_path, &credentials).expect("nested open");
        assert_eq!(reopened.path, nested_path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
