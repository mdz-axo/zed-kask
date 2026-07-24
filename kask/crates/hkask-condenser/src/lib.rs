#![forbid(unsafe_code)]
//! hKask Condenser — Domain logic for context condensation
//!
//! Pure domain crate: compression algorithms, ontology-aware saliency
//! weighting (P5.4/P8.1), engine state management, prompt formatting, and
//! output construction. No MCP, no HTTP, no async.
//!
//! ## Architecture
//!
//! - **`algorithms`** — Three compression algorithms:
//!   - `rtk_style` — head/tail preservation with ontology-aware split ratios
//!   - `word_rank` — TF-IDF bag-of-words compression with structural bonus and ontology anchoring
//!   - `flashrank` — greedy marginal-utility selection under token budget
//!     The `derive_ontology_anchor` function maps tool names to the 3-tier ontology
//!     hierarchy without wire-protocol overhead — every MCP server links against the
//!     same bridge crates.
//!     `domain_saliency` is a public free function for scoring text relevance against
//!     an ontology anchor using graph proximity — reusable by communication gates
//!     and other callers independent of the compression pipeline.
//! - **`ontology_graph`** — A lightweight cross-domain concept relationship
//!   index (FIBO, CogAT, GOLEM, ML-Schema, OMC, PKO, DC+BIBO). Built once
//!   at startup via `OnceLock`, zero dependencies, no reasoners. Used as a
//!   saliency multiplier — lines containing concepts adjacent to the anchor
//!   concept (e.g., "market_capitalization" when anchored to a FIBO corporation)
//!   receive bonus scores.
//! - **`types`** — Domain types: `OntologyAnchor` (3-tier classification),
//!   `OntologyAxis` (Pko/DcBibo), `OntologyNamespace` (Fibo/Golem/Cogat/
//!   MlSchema/Omc), compression profiles, health signals, `CompressionRecord`
//!   (per-compression observation for learning), `CompressionHistoryStats`.
//! - **`engine`** — `CondenserEngine` owns profile state, compression dispatch,
//!   and compression history (bounded ring buffer of `CompressionRecord`).
//!   After 10+ observations per category, `recommend_algorithm()` returns the
//!   best-performing algorithm — `compress()` auto-selects it (learning).
//!   `suggest_profile()` recommends a more aggressive profile when health
//!   checks flag degradation. Derives ontology anchors from tool names internally.
//! - **`inference`** — Prompt formatting and token estimation for
//!   LLM-assisted thread summarization.
//!
//! This crate provides the domain primitives consumed by:
//! - `hkask-services-chat` (ChatService::condense_history — two-phase auto-condense:
//!   CPU pre-compress + LLM summarize)
//! - `hkask-mcp-condenser` (MCP server — thin wrapper exposing tools)

pub mod algorithms;
pub mod engine;
pub mod inference;
pub mod ontology_graph;
pub mod saliency;
pub mod types;

pub use inference::{
    SUMMARY_SYSTEM_PROMPT, approx_token_count, build_summarization_prompt, build_summary_output,
    format_conversation_text,
};
