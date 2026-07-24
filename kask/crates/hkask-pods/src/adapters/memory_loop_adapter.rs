//! Memory Loop Adapter — routes through hkask-memory's domain logic
//
//! Wraps `EpisodicMemory` and `SemanticMemory` so that pods get
//! domain-logic-enriched storage (dedup, Bayesian confidence decay,
//! temporal attention weighting) through the loop membrane.

use crate::error::MemoryError;
use hkask_capability::{DelegationToken, require_read_access, require_write_access};
use hkask_memory::MemoryPortError;
use hkask_memory::{EpisodicMemory, SemanticMemory};
use hkask_memory::{
    EpisodicStoragePort, RecallRequest, RecalledEpisode, RecalledSemantic, SemanticStoragePort,
    StorageRequest,
};
use hkask_regulation::ExperienceClassification;
use hkask_storage::{EmbeddingStore, HMem, HMemStore};
use hkask_types::Confidence;
use std::sync::Arc;

// ── Template Method helpers (P2.4) ──────────────────────────────────────

/// Convert a `HMem` into a typed `RecalledEpisode`.
///
/// Used by `recall_episodic` to produce domain-typed DTOs instead of
/// untyped `serde_json::Value`. `recall_semantic` uses
/// `triple_to_recalled_semantic` (separate type, no perspective).
fn h_mem_to_recalled_episode(t: HMem) -> RecalledEpisode {
    RecalledEpisode {
        id: t.id.to_string(),
        entity: t.entity,
        attribute: t.attribute,
        value: t.value,
        confidence: t.confidence,
        perspective: t.access.perspective,
        visibility: t.access.visibility,
        observed_at: t.observed_at.to_rfc3339(),
        dimension: t.dimension,
    }
}

/// Convert a `HMem` into a typed `RecalledSemantic`.
///
/// Used by `recall_semantic` to produce domain-typed DTOs instead of
/// untyped `serde_json::Value`. Semantic h_mems are perspective-free,
/// so this struct omits the `perspective` field that `RecalledEpisode` carries.
fn h_mem_to_recalled_semantic(t: HMem) -> RecalledSemantic {
    RecalledSemantic {
        id: t.id.to_string(),
        entity: t.entity,
        attribute: t.attribute,
        value: t.value,
        confidence: t.confidence,
        visibility: t.access.visibility,
        observed_at: t.observed_at.to_rfc3339(),
        dimension: t.dimension,
    }
}

/// Build a `HMem` from a `StorageRequest`.
///
/// The `StorageRequest` capture-common-parameters (P2.4/P1.5), and this
/// helper converts it into the `HMem` value object that the storage layer
/// expects — eliminating per-method inline construction.
fn request_to_h_mem(req: &StorageRequest) -> HMem {
    let mut h_mem = HMem::new(
        &req.entity,
        &req.attribute,
        req.value.clone(),
        req.access.owner_webid,
    )
    .with_visibility(req.access.visibility)
    .with_confidence(req.confidence);
    if let Some(p) = req.access.perspective {
        h_mem = h_mem.with_perspective(p);
    }
    if let Some(d) = req.dimension {
        h_mem = h_mem.with_dimension(d);
    }
    h_mem
}

/// Verify write access to the given store type.
///
/// Wraps `require_write_access` with `MemoryError::CapabilityDenied`
/// mapping, eliminating the `.map_err()` repetition across store methods.
fn check_write_access(token: &DelegationToken, store_type: &str) -> Result<(), MemoryPortError> {
    require_write_access(token, store_type).map_err(|e| MemoryPortError::CapabilityDenied {
        resource: store_type.to_string(),
        action: e.to_string(),
    })
}

/// Verify read access to the given store type.
///
/// Wraps `require_read_access` with `MemoryPortError::CapabilityDenied`
/// mapping, eliminating the `.map_err()` repetition across recall methods.
fn check_read_access(token: &DelegationToken, store_type: &str) -> Result<(), MemoryPortError> {
    require_read_access(token, store_type).map_err(|e| MemoryPortError::CapabilityDenied {
        resource: store_type.to_string(),
        action: e.to_string(),
    })
}

/// Store a h_mem via a backend, with capability check and entity return.
///
/// Extracts the common pattern shared by `store_episodic`, `store_semantic`,
/// and `store_episodic_classified`: check write access, convert request to
/// h_mem, delegate to the storage backend, return the entity string.
///
/// The `store_fn` closure receives the constructed `HMem` and is expected
/// to call the appropriate backend's `store` method, converting its error
/// type to `MemoryError` via the `?` operator (which relies on existing
/// `From` impls).
fn store_via<E>(
    request: StorageRequest,
    token: &DelegationToken,
    store_type: &str,
    store_fn: impl FnOnce(HMem) -> Result<(), E>,
) -> Result<String, MemoryPortError>
where
    MemoryPortError: From<E>,
{
    check_write_access(token, store_type)?;
    let entity = request.entity.clone();
    store_fn(request_to_h_mem(&request))?;
    Ok(entity)
}

/// Memory Loop Forwarder — wraps EpisodicMemory and SemanticMemory
///
/// Forwards pod storage requests through `hkask-memory`'s domain
/// logic (dedup, Bayesian confidence decay, temporal attention weighting)
/// instead of directly hitting `HMemStore`.
pub struct MemoryLoopForwarder {
    episodic: Arc<EpisodicMemory>,
    semantic: Arc<SemanticMemory>,
}

impl MemoryLoopForwarder {
    /// Create a new adapter wrapping EpisodicMemory and SemanticMemory.
    ///
    /// expect: "The system loads and adapts agent registries for generative use"
    /// \[P3\] Motivating: Generative Space — MemoryLoopForwarder wires episodic + semantic
    /// pre:  `episodic` is a valid `EpisodicMemory`; `semantic` is a valid
    ///       `SemanticMemory`.
    /// post: Returns a `MemoryLoopForwarder` holding both memory instances.
    pub fn new(episodic: Arc<EpisodicMemory>, semantic: Arc<SemanticMemory>) -> Self {
        Self { episodic, semantic }
    }

    /// Create from a single DatabaseDriver — unified provider for all stores.
    ///
    /// Both `episodic_store` and `semantic_store` wrap the same backing driver
    /// (cloned `Arc`); they are two Rust-side handles to one logical h_mem store,
    /// not two separate stores. Two bindings are required because each is moved
    /// into its respective memory constructor.
    pub fn from_driver(
        driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver>,
    ) -> Result<Self, MemoryError> {
        let episodic_store = HMemStore::from_driver(Arc::clone(&driver));
        let episodic = Arc::new(EpisodicMemory::new(episodic_store));
        let semantic_store = HMemStore::from_driver(Arc::clone(&driver));
        let embedding_store = EmbeddingStore::from_driver(driver, 1024);
        let semantic = Arc::new(SemanticMemory::new(semantic_store, embedding_store));
        Ok(Self::new(episodic, semantic))
    }
}

// Episodic Storage Port — routed through EpisodicMemory

impl EpisodicStoragePort for MemoryLoopForwarder {
    fn store_episodic(
        &self,
        request: StorageRequest,
        token: &DelegationToken,
    ) -> Result<String, MemoryPortError> {
        store_via(request, token, "episodic", |h_mem| {
            self.episodic.store(h_mem)
        })
    }

    fn recall_episodic(
        &self,
        request: &RecallRequest,
    ) -> Result<Vec<RecalledEpisode>, MemoryPortError> {
        check_read_access(&request.token, "episodic")?;

        // P4.1: Replaced `.expect(...)` with a typed error. Episodic memory
        // is owner-scoped (OCAP), so a missing `perspective` is a capability
        // constraint violation, not a panic-worthy condition.
        let owner = request
            .perspective
            .ok_or_else(|| MemoryPortError::CapabilityDenied {
                resource: "episodic_memory".into(),
                action: "recall requires a perspective (owner WebID)".into(),
            })?;

        // Route through EpisodicMemory's deduped+decayed query
        let h_mems = self.episodic.query_for_deduped(&request.query, owner)?;

        let results: Vec<RecalledEpisode> =
            h_mems.into_iter().map(h_mem_to_recalled_episode).collect();

        tracing::debug!(
            target: "hkask.memory.episodic",
            query = %request.query,
            owner = %owner,
            results = results.len(),
            "Episodic recall (via loop membrane)"
        );

        Ok(results)
    }

    fn episodic_storage_usage(
        &self,
        perspective: &hkask_types::WebID,
    ) -> Result<usize, MemoryPortError> {
        let count = self.episodic.storage_usage(perspective)?;

        // F-SYN-013: `reg.memory.budget` is observed by the
        // cybernetics loop's `CyberneticsLoop` (consumes via
        // `tracing-subscriber` layer). The expected consumer is
        // the `hkask-regulation` crate's Regulation runtime, not any in-source
        // subscriber. The consumer boundary is the tracing
        // registry, configured at startup via
        // `hkask_cli::bootstrap::install_tracing_subscriber`.
        tracing::debug!(
            target: "reg.memory.budget",
            perspective = %perspective,
            count = count,
            "Episodic storage usage checked (via loop membrane); consumer: hkask-regulation"
        );

        Ok(count)
    }

    fn episodic_storage_budget(&self) -> usize {
        self.episodic.storage_budget()
    }

    fn store_episodic_classified(
        &self,
        request: StorageRequest,
        classification: ExperienceClassification,
        confidence_override: Option<Confidence>,
        token: &DelegationToken,
    ) -> Result<String, MemoryPortError> {
        // Resolve confidence: override takes precedence, otherwise classification default
        let confidence = confidence_override
            .unwrap_or_else(|| Confidence::new(classification.default_confidence()));

        tracing::info!(
            target: "reg.memory.encode",
            classification = %classification,
            confidence = %confidence,
            entity = %request.entity,
            attribute = %request.attribute,
            "Episodic experience encoded (via loop membrane)"
        );

        store_via(
            StorageRequest {
                confidence,
                ..request
            },
            token,
            "episodic",
            |h_mem| self.episodic.store(h_mem),
        )
    }
}

// Semantic Storage Port — routed through SemanticMemory

impl SemanticStoragePort for MemoryLoopForwarder {
    fn store_semantic(
        &self,
        request: StorageRequest,
        token: &DelegationToken,
    ) -> Result<String, MemoryPortError> {
        store_via(request, token, "semantic", |h_mem| {
            self.semantic.store(h_mem)
        })
    }

    fn recall_semantic(
        &self,
        request: &RecallRequest,
    ) -> Result<Vec<RecalledSemantic>, MemoryPortError> {
        check_read_access(&request.token, "semantic")?;

        // Route through SemanticMemory's deduped query
        let h_mems = self.semantic.query_deduped(&request.query)?;

        let results: Vec<RecalledSemantic> =
            h_mems.into_iter().map(h_mem_to_recalled_semantic).collect();

        tracing::debug!(
            target: "hkask.memory.semantic",
            query = %request.query,
            results = results.len(),
            "Semantic recall (via loop membrane)"
        );

        Ok(results)
    }

    fn semantic_storage_usage(&self, entity: &str) -> Result<usize, MemoryPortError> {
        let count = self.semantic.h_mem_count_for_entity(entity)?;

        tracing::debug!(
            target: "reg.memory.budget",
            entity = %entity,
            count = count,
            "Semantic storage usage checked (via loop membrane)"
        );

        Ok(count)
    }

    fn search_similar(
        &self,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<Vec<RecalledSemantic>, MemoryPortError> {
        let results = self.semantic.search_similar(query_vector, limit)?;
        let mut h_mems = Vec::new();
        for r in results {
            if let Ok(matches) = self.semantic.query_deduped(&r.embedding.entity_ref) {
                for t in matches {
                    h_mems.push(h_mem_to_recalled_semantic(t));
                }
            }
        }
        Ok(h_mems)
    }
}
