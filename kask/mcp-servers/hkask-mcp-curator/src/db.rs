//! Curator MCP database adapter over `hkask_memory::{CuratorStoreSet, SelfHealingStore, FederatedSourceCache}`.

use std::path::PathBuf;
use std::sync::Arc;

use hkask_mcp_server::server::{McpToolError, resolve_db_passphrase};
use hkask_memory::{
    AdmittedSourceSet, CuratorOpenConfig, CuratorOpenFailure, CuratorStoreSet,
    FederatedSourceCache, SelfHealingStore, default_federated_manifest_path,
    open_curator_store_set, parse_memory_life_days_value, resolve_curator_db_path,
};

/// The curator's sovereign stores (`RegulationArchive` + `MemoryStore`), shared
/// over a single SQLCipher driver in `hkask_memory`.
pub type CuratorStores = CuratorStoreSet;

/// MCP-surface handle over the curator's sovereign `curator.db`, optional
/// split `curator-private.db` (D100), and federated external-source cache.
/// Maps pure `hkask_memory` open failures onto `McpToolError`.
pub struct CuratorDb {
    stores: SelfHealingStore<CuratorStores, McpToolError>,
    private_stores: Option<SelfHealingStore<CuratorStores, McpToolError>>,
    pub(crate) passphrase: Option<String>,
    federated_manifest_path: PathBuf,
    federated_sources: FederatedSourceCache,
}

fn sibling_private_db_path(db_path: &str) -> Option<String> {
    if db_path == ":memory:" || db_path.trim().is_empty() {
        return None;
    }
    std::path::Path::new(db_path).parent().map(|parent| {
        parent
            .join("curator-private.db")
            .to_string_lossy()
            .into_owned()
    })
}

fn build_self_healing_stores(
    db_path: String,
    passphrase: Option<String>,
    memory_life_days: f64,
    label: &str,
) -> SelfHealingStore<CuratorStores, McpToolError> {
    let heal_enabled = passphrase.is_some();
    let (initial_stores, initial_outage) = open_curator_stores(
        Some(db_path.as_str()),
        passphrase.as_deref(),
        memory_life_days,
    );
    if initial_stores.is_down() {
        if heal_enabled {
            tracing::error!(
                target: "hkask.mcp.curator",
                db_path = %db_path,
                store = label,
                "Curator DB unavailable — both curator stores (regulation \
                 archive, memory) are down. Every tool call re-attempts the open \
                 (self-healing); check that no other process holds the \
                 SQLCipher lock and that HKASK_DB_PASSPHRASE matches the keychain."
            );
        } else {
            tracing::error!(
                target: "hkask.mcp.curator",
                db_path = %db_path,
                store = label,
                "Curator DB unavailable and HKASK_DB_PASSPHRASE is not \
                 set — curator stores will stay down until the server is \
                 restarted with the passphrase configured. Set \
                 HKASK_DB_PASSPHRASE (keychain-provisioned) and relaunch."
            );
        }
    }
    let reopen_path = db_path.clone();
    let reopen_passphrase = passphrase;
    SelfHealingStore::new(
        db_path,
        initial_stores,
        initial_outage,
        heal_enabled,
        CuratorStoreSet::is_down,
        move || {
            open_curator_stores(
                Some(reopen_path.as_str()),
                reopen_passphrase.as_deref(),
                memory_life_days,
            )
        },
    )
}

impl CuratorDb {
    pub(crate) fn from_context(ctx: &hkask_mcp_server::server::ServerContext) -> Self {
        let db_path = resolve_curator_db_path();
        let passphrase = match resolve_db_passphrase(&ctx.credentials) {
            Ok(passphrase) => Some(passphrase),
            Err(error) => {
                tracing::warn!(
                    target: "hkask.mcp.curator",
                    %error,
                    "Curator stores stay down — store-backed tools return permission_denied until relaunch with HKASK_DB_PASSPHRASE resolvable"
                );
                None
            }
        };
        let memory_life_days = memory_life_days_from_env();
        let private_stores = sibling_private_db_path(&db_path).map(|priv_path| {
            build_self_healing_stores(
                priv_path,
                passphrase.clone(),
                memory_life_days,
                "curator-private",
            )
        });
        let stores =
            build_self_healing_stores(db_path, passphrase.clone(), memory_life_days, "curator");
        Self {
            stores,
            private_stores,
            passphrase,
            federated_manifest_path: default_federated_manifest_path(),
            federated_sources: FederatedSourceCache::new(),
        }
    }

    /// Open the stores at an explicit path — used by production bootstrap and
    /// isolated tests without mutating process environment variables.
    #[doc(hidden)]
    pub fn open_at(
        db_path: String,
        passphrase: Option<String>,
        federated_manifest_path: PathBuf,
        memory_life_days: f64,
    ) -> Self {
        let stores =
            build_self_healing_stores(db_path, passphrase.clone(), memory_life_days, "curator");
        Self {
            stores,
            private_stores: None,
            passphrase,
            federated_manifest_path,
            federated_sources: FederatedSourceCache::new(),
        }
    }

    #[doc(hidden)]
    pub fn from_stores(stores: CuratorStores) -> Self {
        Self {
            stores: SelfHealingStore::disabled(stores, CuratorStoreSet::is_down),
            private_stores: None,
            passphrase: None,
            federated_manifest_path: PathBuf::new(),
            federated_sources: FederatedSourceCache::new(),
        }
    }

    #[doc(hidden)]
    pub fn from_split_stores(public_stores: CuratorStores, private_stores: CuratorStores) -> Self {
        Self {
            stores: SelfHealingStore::disabled(public_stores, CuratorStoreSet::is_down),
            private_stores: Some(SelfHealingStore::disabled(
                private_stores,
                CuratorStoreSet::is_down,
            )),
            passphrase: None,
            federated_manifest_path: PathBuf::new(),
            federated_sources: FederatedSourceCache::new(),
        }
    }

    #[doc(hidden)]
    pub fn from_stores_with_federated_manifest(
        stores: CuratorStores,
        manifest_path: PathBuf,
        passphrase: String,
    ) -> Self {
        Self {
            stores: SelfHealingStore::disabled(stores, CuratorStoreSet::is_down),
            private_stores: None,
            passphrase: Some(passphrase),
            federated_manifest_path: manifest_path,
            federated_sources: FederatedSourceCache::new(),
        }
    }

    pub(crate) fn federated_sources(&self) -> Arc<AdmittedSourceSet> {
        self.federated_sources
            .admit_all(&self.federated_manifest_path, self.passphrase.as_deref())
    }

    /// Read the current store set, attempting a throttled re-open when the DB is down.
    pub(crate) fn get(&self) -> CuratorStores {
        self.stores.get()
    }

    /// Read the optional split private store set (`curator-private.db`), when configured.
    pub(crate) fn private_get(&self) -> Option<CuratorStores> {
        self.private_stores.as_ref().map(SelfHealingStore::get)
    }

    /// Store set routed by provenance (prefers `curator-private.db` when private and configured).
    pub(crate) fn stores_for_provenance(
        &self,
        provenance: hkask_types::InferenceProvenance,
    ) -> CuratorStores {
        if provenance == hkask_types::InferenceProvenance::PrivateInference {
            if let Some(private) = self.private_get() {
                return private;
            }
        }
        self.get()
    }

    /// The curator memory store, or the typed reason it is down.
    pub(crate) fn memory(&self) -> Result<Arc<hkask_memory::MemoryStore>, McpToolError> {
        self.get()
            .memory
            .ok_or_else(|| Self::store_error_for(&self.stores, "curator memory store"))
    }

    /// The optional split `curator-private.db` memory store. When configured,
    /// an outage fails closed (`Err`) rather than falling back to public `curator.db`.
    pub(crate) fn private_memory(
        &self,
    ) -> Result<Option<Arc<hkask_memory::MemoryStore>>, McpToolError> {
        let Some(private) = &self.private_stores else {
            return Ok(None);
        };
        private
            .get()
            .memory
            .map(Some)
            .ok_or_else(|| Self::store_error_for(private, "private curator memory store"))
    }

    /// Write-side memory store routed by provenance: `PrivateInference` writes
    /// to `curator-private.db` whenever split stores are configured (and fails
    /// closed if `curator-private.db` is unavailable), falling through to the
    /// single store only in single-store test fixtures.
    pub(crate) fn write_memory_for_provenance(
        &self,
        provenance: hkask_types::InferenceProvenance,
    ) -> Result<Arc<hkask_memory::MemoryStore>, McpToolError> {
        if provenance == hkask_types::InferenceProvenance::PrivateInference {
            if let Some(private) = self.private_memory()? {
                return Ok(private);
            }
        }
        self.memory()
    }

    /// The Regulation archive, or the typed reason it is down.
    pub(crate) fn regulation_store(
        &self,
    ) -> Result<Arc<hkask_storage::RegulationArchive>, McpToolError> {
        self.get()
            .regulation_store
            .ok_or_else(|| Self::store_error_for(&self.stores, "Regulation archive"))
    }

    /// Why a store is down, when one is: the recorded open failure.
    pub(crate) fn outage_reason(&self) -> Option<String> {
        self.stores.outage().map(|error| error.message)
    }

    fn store_error_for(
        handle: &SelfHealingStore<CuratorStores, McpToolError>,
        store: &str,
    ) -> McpToolError {
        match handle.outage() {
            Some(outage) => McpToolError::new(
                outage.kind,
                format!("{store} is not available: {}", outage.message),
            ),
            None => McpToolError::unavailable(format!(
                "{store} is not available: the curator database is not open"
            )),
        }
    }
}

/// Resolve the memory decay constant from `HKASK_MEMORY_LIFE_DAYS`.
pub(crate) fn memory_life_days_from_env() -> f64 {
    match std::env::var("HKASK_MEMORY_LIFE_DAYS") {
        Ok(raw) => parse_memory_life_days_value(&raw),
        Err(_) => hkask_memory::MemoryStore::default_memory_life_days(),
    }
}

/// Map a domain [`CuratorOpenFailure`] onto the canonical MCP tool error contract.
fn map_open_failure(failure: CuratorOpenFailure) -> McpToolError {
    match failure {
        CuratorOpenFailure::MissingDbPath => {
            McpToolError::internal("the curator database path was not resolved")
        }
        CuratorOpenFailure::MissingPassphrase => McpToolError::permission_denied(
            "HKASK_DB_PASSPHRASE did not resolve from the environment or the keychain; \
             relaunch the server with it set",
        ),
        CuratorOpenFailure::DatabaseOpen(hkask_storage::DatabaseError::PassphraseMismatch(err)) => {
            let error = hkask_storage::DatabaseError::PassphraseMismatch(err);
            McpToolError::permission_denied(format!(
                "the curator database did not open: {error}. HKASK_DB_PASSPHRASE (environment or \
                 keychain) does not match this database"
            ))
        }
        CuratorOpenFailure::DatabaseOpen(hkask_storage::DatabaseError::MaintenanceLease {
            path,
            reason,
        }) => {
            let error = hkask_storage::DatabaseError::MaintenanceLease { path, reason };
            McpToolError::unavailable(format!(
                "the curator database is under maintenance: {error}"
            ))
        }
        CuratorOpenFailure::DatabaseOpen(other) => {
            hkask_mcp_server::server::map_database_error(other, "the curator database did not open")
        }
        CuratorOpenFailure::PartialInit(reason) => {
            McpToolError::internal(format!("{reason}; restart the server after fixing it"))
        }
    }
}

pub(crate) fn open_curator_stores(
    db_path: Option<&str>,
    passphrase: Option<&str>,
    memory_life_days: f64,
) -> (CuratorStores, Option<McpToolError>) {
    let config = CuratorOpenConfig {
        embedding_dim: hkask_storage::embedding_dim(),
        memory_life_days,
        allow_degraded_embeddings: true,
        journal_open_span: false,
    };
    let (stores, failure) = open_curator_store_set(db_path, passphrase, config);
    (stores, failure.map(map_open_failure))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: "The `kask.memory.memory_life_days` setting governs the store's
    /// actual decay — not just the regulation sensor that reports it." [P1]
    #[test]
    fn open_curator_stores_applies_configured_memory_life_days() {
        let directory = tempfile::tempdir().expect("tempdir");
        let db_path = directory.path().join("curator.db");
        let db_path = db_path.to_str().expect("UTF-8 path");
        hkask_storage::open_or_repair(db_path, "test-passphrase").expect("create db");

        let (stores, outage) = open_curator_stores(Some(db_path), Some("test-passphrase"), 30.0);
        assert!(outage.is_none(), "a clean open records no outage");
        let memory = stores.memory.expect("memory store");
        assert_eq!(
            memory.memory_life_days(),
            30.0,
            "the configured decay constant must reach the store that actually decays"
        );
    }
}
