#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask Memory — one unified memory store, ontology-discriminated.
//!
//! The episodic/semantic distinction is carried by the `HMemOntology` blob on
//! each h_mem (P5.4 dual-axis anchoring), not by separate store structs. A
//! semantic fact anchors to the state axis (Dublin Core + BIBO: `dc_type`,
//! `dc_subject`, `dc_source`); an episodic experience anchors to the process
//! axis (PKO: `pko_procedure`, `pko_step`). Both are stored, recalled, and
//! queried through the single [`MemoryStore`].
//!
//! **Flow:** chat stream → chunks → each chunk tagged with both the best-fit
//! state axis (Dublin Core) and the best-fit process axis (PKO). The
//! `HMemOntology` blob is the discriminator; the `perspective` field is
//! provenance (who wrote the memory), not a semantic classifier.
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
pub use memory_store::{MemoryStore, MemoryStoreError};
pub use memory_store::CentroidResult;
pub use text_chunking::{chunk_text, strip_gutenberg_headers};

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
