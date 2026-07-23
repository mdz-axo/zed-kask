//! HMem domain types — the bitemporal h_mem data model.
//!
//! Moved from `hkask-storage::hmem` so that hKask crates can use `HMem` and
//! `HMemError` without depending on the storage crate (which pulls in
//! `rusqlite`/`sqlite-vec`, conflicting with zed's pinned `libsqlite3-sys`).
//! The `HMemStore` adapter (SQL over `StorageDriver`) lives in `hkask-memory`.

use crate::visibility::{AccessControl, Confidence, Dimension, Visibility};
use crate::{HMemId, InfrastructureError, NotFound, WebID};
use chrono::{DateTime, Utc};
use serde_json::Value;
use thiserror::Error;

// ── HMemError ───────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum HMemError {
    #[error(transparent)]
    Infra(#[from] InfrastructureError),
    #[error("{0}")]
    NotFound(NotFound),
}

impl From<NotFound> for HMemError {
    fn from(nf: NotFound) -> Self {
        HMemError::NotFound(nf)
    }
}

impl From<crate::DbError> for HMemError {
    fn from(e: crate::DbError) -> Self {
        HMemError::Infra(InfrastructureError::from(e))
    }
}

impl From<serde_json::Error> for HMemError {
    fn from(e: serde_json::Error) -> Self {
        HMemError::Infra(InfrastructureError::from(e))
    }
}

// ── HMem ────────────────────────────────────────────────────────────────────

/// Bitemporal h_mem — entity/attribute/value with observed_at timestamp.
#[derive(Debug, Clone)]
pub struct HMem {
    pub id: HMemId,
    pub entity: String,
    pub attribute: String,
    pub value: Value,
    /// When this memory was formed (observation timestamp).
    pub observed_at: DateTime<Utc>,
    pub confidence: Confidence,
    pub access: AccessControl,
    /// Last time this h_mem was recalled. Starts at creation time.
    /// Updated on each recall — resets the decay clock.
    pub recalled_at: DateTime<Utc>,
    /// 5W1H dimension — which curator ontology category this h_mem belongs to.
    /// Maps to `OntologyAnchor::Core` (universal ground). None = unclassified.
    pub dimension: Option<Dimension>,
}

impl HMem {
    /// Create a new HMem with required fields.
    pub fn new(entity: &str, attribute: &str, value: Value, owner_webid: WebID) -> Self {
        let now = Utc::now();
        Self {
            id: HMemId::new(),
            entity: entity.to_string(),
            attribute: attribute.to_string(),
            value,
            observed_at: now,
            confidence: Confidence::full(),
            access: AccessControl::new(owner_webid),
            recalled_at: now,
            dimension: None,
        }
    }

    pub fn with_confidence(mut self, c: impl Into<Confidence>) -> Self {
        self.confidence = c.into();
        self
    }

    pub fn with_perspective(mut self, p: WebID) -> Self {
        self.access = self.access.with_perspective(p);
        self
    }

    pub fn with_visibility(mut self, v: Visibility) -> Self {
        self.access = self.access.with_visibility(v);
        self
    }

    pub fn with_dimension(mut self, d: Dimension) -> Self {
        self.dimension = Some(d);
        self
    }

    pub fn is_episodic(&self) -> bool {
        self.access.is_episodic()
    }

    pub fn is_semantic(&self) -> bool {
        self.access.is_semantic()
    }
}
