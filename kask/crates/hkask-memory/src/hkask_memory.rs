#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask Memory — one unified memory store, ontology-discriminated.
//!
//! The ontology blob on each h_mem carries dual-axis anchoring
//! (PKO process axis + DC state axis). A process-anchored h_mem
//! (e.g., a chat turn) carries PKO procedure/step; a state-anchored h_mem
//! (e.g., a fact) carries DC type/subject. Both are unified h_mems —
//! there is no episodic/semantic type distinction. All h_mems are stored,
//! recalled, and queried through the single [`MemoryStore`].
//!
//! **Flow:** chat stream → chunks → each chunk tagged with both the best-fit
//! state axis (Dublin Core) and the best-fit process axis (PKO). The
//! `HMemOntology` blob is the discriminator; the `perspective` field is
//! provenance (who wrote the memory), not a type classifier.
//!
//! **Recall deduplication** runs at recall time in `recall_dedup` (BLAKE3 hash
//! over canonical entity-attribute-value content, first-seen-wins). There is
//! no shared rendering layer: each consuming surface (chat service, MCP server,
//! HTTP API, TUI) joins and serializes recalled memories in the shape its own
//! consumer needs. See ADR-060 for the decision and rationale.

pub(crate) mod bayesian; // Loop 2b (semantic confidence combination)
pub mod consolidation_service; // Memory consolidator (perspective-bound → shared)
pub mod memory_store; // Unified store (ontology-discriminated)
pub mod recall_dedup;
pub mod salience;
pub mod text_chunking; // Pure chunking helpers (no store access)

pub use consolidation_service::MemoryConsolidator;
pub use memory_store::CentroidResult;
pub use memory_store::{MemoryStore, MemoryStoreError};
pub use text_chunking::{chunk_text, strip_gutenberg_headers};

pub use bayesian::combine_confidences;

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
        hkask_types::event::SpanNamespace::try_from(
            hkask_types::regulation::RegulationSpan::MemoryEncode,
        )
        .expect("reg.memory.encode is in CANONICAL_NAMESPACES")
    });
