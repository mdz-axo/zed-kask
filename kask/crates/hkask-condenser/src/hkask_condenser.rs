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
//! - **`algorithms`** — Four compression algorithms:
//!   - `structured_json_array` — AST-preserving homogeneous JSON object-array elision retaining head/tail boundaries, numeric extrema, and explicit error/rare-null outliers with `_kask_elided_count` markers
//!   - `rtk_style` — head/tail preservation with ontology-aware split ratios
//!   - `word_rank` — TF-IDF bag-of-words compression with structural bonus and ontology anchoring
//!   - `flashrank` — greedy marginal-utility selection under profile line-retention limits
//!     The `select_ontology_anchor` function (re-exported from `hkask-bridge-ontology`
//!     via `types`) maps tool names to the 3-tier ontology hierarchy without
//!     wire-protocol overhead — every MCP server links against the same bridge
//!     crates.
//!     `domain_saliency` scores text relevance against an ontology anchor using
//!     published graph proximity from `hkask-bridge-ontology` — used by
//!     `WordRankAlgorithm` internally.
//! - **`types`** — Domain types: `OntologyAnchor` (3-tier classification),
//!   `OntologyAxis` (Pko/DcBibo), `OntologyNamespace` (Sepio/Golem/Sumo/
//!   MlSchema), compression profiles, health signals.
//! - **`engine`** — `CondenserEngine` owns profile state and compression
//!   dispatch. Exhaustively maps context categories to algorithms and derives
//!   ontology anchors from tool names internally.
//! - **`spillover`** — `D100`-isolated, `BLAKE3` content-addressed in-memory
//!   `SpilloverVault` (`kask://spillover/<blake3>`) with single-line JSON
//!   pretty-projection, bounded LRU eviction, and paginated slice reads.
//!
//! This crate provides the domain primitives consumed by:
//! - `kask_bridge` (`BridgeThreadCondenser` — the runtime tool-result
//!   compression path wired into the agent turn loop)
//! - `crates/agent` (`SpilloverVault` + `read_file` bounded slice resolution
//!   over `kask://spillover/<blake3>` handles)

pub(crate) mod algorithms;
pub mod engine;
pub(crate) mod saliency;
pub mod spillover;
pub mod types;
