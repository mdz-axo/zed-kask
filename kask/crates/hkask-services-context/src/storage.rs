//! Storage context — template registry, goal repository,
//! user store, sovereignty boundaries, and wallet store.
//!
//! Extracted from `AgentService` as part of the strangler-fig decomposition.

use hkask_identity::UserPod;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_storage::goals::SqliteGoalRepository;
use hkask_storage::user_store::UserStore;
use hkask_storage::{SovereigntyBoundaryStore, WalletStore};
use hkask_templates::SqliteRegistry;
use hkask_types::WebID;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Consolidated storage context — all persistent stores in one place.
pub struct StorageContext {
    pub registry: Arc<Mutex<SqliteRegistry>>,
    pub goals: Arc<SqliteGoalRepository>,
    pub users: Arc<std::sync::Mutex<UserStore>>,
    pub sovereignty: SovereigntyBoundaryStore,
    pub wallet: Option<Arc<WalletStore>>,
}

impl StorageContext {
    pub fn new(
        registry: Arc<Mutex<SqliteRegistry>>,
        goals: Arc<SqliteGoalRepository>,
        users: Arc<std::sync::Mutex<UserStore>>,
        sovereignty: SovereigntyBoundaryStore,
        wallet: Option<Arc<WalletStore>>,
    ) -> Self {
        Self {
            registry,
            goals,
            users,
            sovereignty,
            wallet,
        }
    }

    /// Find a userpod by name.
    ///
    /// Returns `Ok(None)` if no userpod with the given name exists.
    #[must_use = "result must be used"]
    pub fn find_userpod_by_name(&self, name: &str) -> Result<Option<UserPod>, ServiceError> {
        let store = self.users.lock().map_err(|_| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: "User store lock poisoned".into(),
        })?;
        store.get_userpod(name).map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: Some(Box::new(e)),
            message: format!("Failed to look up userpod '{name}'"),
        })
    }

    /// Find a user by WebID.
    ///
    /// Returns `Ok(None)` if no userpod with the given WebID exists.
    #[must_use = "result must be used"]
    pub fn find_user_by_webid(&self, webid: &WebID) -> Result<Option<UserPod>, ServiceError> {
        let store = self.users.lock().map_err(|_| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: "User store lock poisoned".into(),
        })?;
        store
            .get_userpod_by_webid(webid)
            .map_err(|e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: Some(Box::new(e)),
                message: format!("Failed to look up user by WebID '{webid}'"),
            })
    }

    /// List all userpods across all users.
    ///
    /// Returns `Ok(Vec<UserPod>)` ordered by creation time.
    #[must_use = "result must be used"]
    pub fn list_userpods(&self) -> Result<Vec<UserPod>, ServiceError> {
        let store = self.users.lock().map_err(|_| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: "User store lock poisoned".into(),
        })?;
        store.list_userpods().map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: Some(Box::new(e)),
            message: "Failed to list userpods".into(),
        })
    }
}
