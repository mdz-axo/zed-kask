#![forbid(unsafe_code)]
//! hKask Memory — Semantic and episodic memory pipelines
//!
//! **Recall deduplication** runs at recall time in `recall_dedup` (BLAKE3 hash
//! over canonical entity-attribute-value content, first-seen-wins). There is
//! no shared rendering layer: each consuming surface (chat service, MCP server,
//! HTTP API, TUI) joins and serializes recalled memories in the shape its own
//! consumer needs. See ADR-060 for the decision and rationale.

pub(crate) mod bayesian; // Loop 2b (semantic confidence combination)
pub mod chat_turn; // Typed projection of chat episode content
pub mod consolidation; // Episodic → Semantic bridge
pub mod consolidation_service;
pub mod episodic; // Loop 2a
pub mod episodic_loop;
pub mod error;
pub mod ports;
pub mod recall_dedup;
pub mod salience;
pub mod semantic; // Loop 2b
pub mod semantic_loop;

pub use chat_turn::ChatTurn;
pub use consolidation::ConsolidationBridge;
pub use consolidation_service::ConsolidationService;
pub use episodic::{EpisodicMemory, EpisodicMemoryError};
pub use episodic_loop::EpisodicLoop;
pub use error::MemoryPortError;
pub use ports::{
    EpisodicStoragePort, RecallRequest, RecalledEpisode, RecalledSemantic, SemanticStoragePort,
    StorageRequest,
};
pub use semantic::{SemanticMemory, SemanticMemoryError};
pub use semantic_loop::SemanticLoop;

// ── Canonical span namespace (hoisted from 5 per-call .expect() sites) ──
//
// `SpanNamespace::try_from` is not `const fn` (it validates against the
// runtime canonical-namespace registry). This `LazyLock` computes the
// canonical `reg.memory.encode` namespace once and caches it. All memory
// modules reuse `MEMORY_ENCODE_SPAN` instead of `.expect()`-ing at each
// call site. The `.expect` here is the single legitimate site — it pins
// the invariant that `reg.memory.encode` is registered.

use std::sync::LazyLock;

pub(crate) static MEMORY_ENCODE_SPAN: LazyLock<hkask_types::event::SpanNamespace> =
    LazyLock::new(|| {
        hkask_types::event::SpanNamespace::try_from(hkask_types::regulation::RegulationSpan::MemoryEncode)
            .expect("reg.memory.encode is in CANONICAL_NAMESPACES")
    });
