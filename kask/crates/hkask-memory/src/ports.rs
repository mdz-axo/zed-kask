//! Memory Storage Ports — Episodic and Semantic boundaries
//!
//! Episodic (private, agent-scoped) and semantic (shared, public) access patterns.
//!
//! # Canonical Home
//!
//! These port traits live in `hkask-memory` — their natural domain. Under the
//! **promotion rule** (see ADR-042), a port trait lives in the domain crate that
//! first consumes it. When a second consumer needs it, the trait is promoted to
//! a shared crate. These traits have two consumers (`hkask-agents`,
//! `hkask-services-context`) and so belong here in `hkask-memory`, not in any
//! individual consumer.
//!
//! # OCAP Discipline
//!
//! - `EpisodicStoragePort` — store/recall episodic h_mems (private, agent-scoped)
//!   Only the owning agent can store or read their own episodic h_mems.
//! - `SemanticStoragePort` — store/recall semantic h_mems (shared, public)
//!   Any agent with a capability token can read semantic h_mems.
//!   Only agents with consolidation capability can store semantic h_mems.

use crate::error::MemoryPortError;
use hkask_capability::DelegationToken;
use hkask_regulation::ExperienceClassification;
use hkask_types::visibility::AccessControl;
use hkask_types::{Confidence, Dimension, Visibility, WebID};
use serde_json::Value;

// ── Request value objects ───────────────────────────────────────────────

/// Capture-common-parameters struct for memory store operations.
///
/// Groups the fields that every store call shares (entity, attribute, value,
/// access, confidence) so that `store_episodic`, `store_episodic_classified`,
/// and `store_semantic` accept a single request object instead of a flat
/// parameter list.
#[derive(Debug, Clone)]
pub struct StorageRequest {
    pub entity: String,
    pub attribute: String,
    pub value: Value,
    pub confidence: Confidence,
    pub access: AccessControl,
    pub dimension: Option<Dimension>,
}

impl StorageRequest {
    pub fn new(
        entity: impl Into<String>,
        attribute: impl Into<String>,
        value: Value,
        confidence: Confidence,
        access: AccessControl,
    ) -> Self {
        Self {
            entity: entity.into(),
            attribute: attribute.into(),
            value,
            confidence,
            access,
            dimension: None,
        }
    }

    pub fn episodic(
        entity: impl Into<String>,
        attribute: impl Into<String>,
        value: Value,
        confidence: Confidence,
        producer_webid: WebID,
    ) -> Self {
        Self::new(
            entity,
            attribute,
            value,
            confidence,
            AccessControl::episodic(producer_webid, producer_webid),
        )
    }

    pub fn semantic(
        entity: impl Into<String>,
        attribute: impl Into<String>,
        value: Value,
        confidence: Confidence,
        producer_webid: WebID,
    ) -> Self {
        Self::new(
            entity,
            attribute,
            value,
            confidence,
            AccessControl::semantic(producer_webid),
        )
    }

    pub fn classified_episodic(
        entity: impl Into<String>,
        attribute: impl Into<String>,
        value: Value,
        classification: ExperienceClassification,
        confidence_override: Option<Confidence>,
        producer_webid: WebID,
    ) -> Self {
        let confidence = confidence_override
            .unwrap_or_else(|| Confidence::new(classification.default_confidence()));
        Self::episodic(entity, attribute, value, confidence, producer_webid)
    }
}

/// Capture-common-parameters struct for memory recall operations.
#[derive(Debug, Clone)]
pub struct RecallRequest {
    pub query: String,
    pub perspective: Option<WebID>,
    pub token: DelegationToken,
}

impl RecallRequest {
    pub fn episodic(query: impl Into<String>, owner: WebID, token: DelegationToken) -> Self {
        Self {
            query: query.into(),
            perspective: Some(owner),
            token,
        }
    }

    pub fn semantic(query: impl Into<String>, token: DelegationToken) -> Self {
        Self {
            query: query.into(),
            perspective: None,
            token,
        }
    }
}

// ── Response DTOs ────────────────────────────────────────────────────────

/// Typed DTO for recalled episodic h_mems.
///
/// Carries `perspective: Option<WebID>` — the owning agent's identity. This
/// field is load-bearing for OCAP: episodic memory is perspective-bound (only
/// the owning agent can read their own episodic h_mems). `RecalledSemantic`
/// omits this field because semantic memory is shared (any agent with a
/// capability token can read). The two types are deliberately separate rather
/// than one type with an optional perspective: merging them would let callers
/// accidentally ignore the perspective field on what should be episodic data,
/// weakening the OCAP boundary at the type level. See ADR-042 for the port
/// promotion rule that placed these traits in `hkask-memory`.
#[derive(Debug, Clone)]
pub struct RecalledEpisode {
    pub id: String,
    pub entity: String,
    pub attribute: String,
    pub value: Value,
    pub confidence: Confidence,
    pub perspective: Option<WebID>,
    pub visibility: Visibility,
    pub observed_at: String,
    pub dimension: Option<Dimension>,
}

/// Typed DTO for recalled semantic h_mems.
///
/// Omits `perspective` (which `RecalledEpisode` carries) because semantic
/// memory is perspective-free and shared across agents. The absence of this
/// field is the type-level signal that no OCAP perspective check is needed
/// for semantic recall. See the doc on `RecalledEpisode` for the rationale
/// behind keeping the two types separate.
#[derive(Debug, Clone)]
pub struct RecalledSemantic {
    pub id: String,
    pub entity: String,
    pub attribute: String,
    pub value: Value,
    pub confidence: Confidence,
    pub visibility: Visibility,
    pub observed_at: String,
    pub dimension: Option<Dimension>,
}

// ── Port traits ──────────────────────────────────────────────────────────

/// Port trait for episodic memory storage operations.
///
/// Episodic memory is private to the owning agent.
pub trait EpisodicStoragePort: Send + Sync {
    /// Store an episodic memory for the owning agent.
    ///
    /// expect: The system provides durable, queryable episodic memory with perspective-bound access control
    /// pre: token must be valid for the perspective identified in the request's access control
    /// post: returns the unique identifier of the stored episode
    fn store_episodic(
        &self,
        request: StorageRequest,
        token: &DelegationToken,
    ) -> Result<String, MemoryPortError>;

    /// Recall episodic memories matching the query from the requester's perspective.
    ///
    /// expect: The system provides durable, queryable episodic memory with perspective-bound access control
    fn recall_episodic(
        &self,
        request: &RecallRequest,
    ) -> Result<Vec<RecalledEpisode>, MemoryPortError>;

    /// Return the current episodic storage usage for a given perspective.
    ///
    /// expect: The system provides durable, queryable episodic memory with perspective-bound access control
    fn episodic_storage_usage(&self, perspective: &WebID) -> Result<usize, MemoryPortError>;

    /// Return the episodic storage budget ceiling.
    ///
    /// expect: The system provides durable, queryable episodic memory with perspective-bound access control
    fn episodic_storage_budget(&self) -> usize;

    /// Store an episodic memory with experience classification.
    ///
    /// expect: The system provides durable, queryable episodic memory with perspective-bound access control
    /// pre: token must be valid for the perspective identified in the request's access control
    /// post: returns the unique identifier of the stored classified episode
    fn store_episodic_classified(
        &self,
        request: StorageRequest,
        classification: ExperienceClassification,
        confidence_override: Option<Confidence>,
        token: &DelegationToken,
    ) -> Result<String, MemoryPortError>;
}

/// Port trait for semantic memory storage operations.
///
/// Semantic memory is shared across agents.
pub trait SemanticStoragePort: Send + Sync {
    /// Store a semantic memory for shared, cross-agent access.
    ///
    /// expect: The system provides shared semantic memory with deduplication and confidence-weighted recall
    /// pre: token must carry consolidation capability
    /// post: returns the unique identifier of the stored semantic memory
    fn store_semantic(
        &self,
        request: StorageRequest,
        token: &DelegationToken,
    ) -> Result<String, MemoryPortError>;

    /// Recall semantic memories matching the query.
    ///
    /// expect: The system provides shared semantic memory with deduplication and confidence-weighted recall
    fn recall_semantic(
        &self,
        request: &RecallRequest,
    ) -> Result<Vec<RecalledSemantic>, MemoryPortError>;

    /// Return the current semantic storage usage for a given entity.
    ///
    /// expect: The system provides shared semantic memory with deduplication and confidence-weighted recall
    fn semantic_storage_usage(&self, entity: &str) -> Result<usize, MemoryPortError>;

    /// Search for semantically similar memories by vector proximity.
    ///
    /// expect: The system provides shared semantic memory with deduplication and confidence-weighted recall
    fn search_similar(
        &self,
        _query_vector: &[f32],
        _limit: usize,
    ) -> Result<Vec<RecalledSemantic>, MemoryPortError> {
        Ok(Vec::new())
    }
}
