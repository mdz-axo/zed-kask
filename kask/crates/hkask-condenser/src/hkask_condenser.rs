#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
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
//!   index (FIBO, SUMO, GOLEM, ML-Schema, PKO, DC+BIBO). Built once
//!   at startup via `OnceLock`, zero dependencies, no reasoners. Used as a
//!   saliency multiplier — lines containing concepts adjacent to the anchor
//!   concept (e.g., "market_capitalization" when anchored to a FIBO corporation)
//!   receive bonus scores.
//! - **`types`** — Domain types: `OntologyAnchor` (3-tier classification),
//!   `OntologyAxis` (Pko/DcBibo), `OntologyNamespace` (Fibo/Golem/Sumo/
//!   MlSchema), compression profiles, health signals.
//! - **`engine`** — `CondenserEngine` owns profile state and compression
//!   dispatch. Selects an algorithm per compression via the static
//!   `default_for()` mapping. Derives ontology anchors from tool names
//!   internally.
//!
//! This crate provides the domain primitives consumed by:
//! - `kask_bridge` (`BridgeThreadCondenser` — the runtime tool-result
//!   compression path wired into the agent turn loop)
//! - `kask_bridge::BridgeThreadCondenser` (in-process thread condensation)

pub mod algorithms;
pub mod engine;
pub mod ontology_graph;
pub mod saliency;
pub mod types;
