#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Ontology bridge — the single source of truth for ontology vocabulary and
//! the dual-axis domain-selection logic in hKask.
//!
//! Two universal axes (P5.4) plus domain supplements (P8.1):
//!
//! - **State axis** — Dublin Core + BIBO + CiTO (`dc_bibo`): the "what is this"
//!   noun dimension. Always available; every artifact carries a state identity.
//! - **Process axis** — PKO (`pko`): the "how did this come to be" verb dimension.
//!   Always available; every artifact carries a process identity.
//! - **Domain supplements** — FIBO, ESO, GOLEM, OMC, ML-Schema: layered on top
//!   where the universal axes aren't specific enough for a domain. These are
//!   supplements, not alternatives to the dual-axis core.
//!
//! The domain-selection logic (`axis`) maps a domain hint to its axis
//! anchoring: state axis is always Dublin Core; process axis is the domain
//! ontology when one applies, PKO otherwise. The invariant: one axis is
//! always DC or PKO, so every artifact has a common mapping in process or
//! state space regardless of domain.
//!
//! Architectural invariant (user directive 2026-08-05): ontologies are domain
//! maps; MCP servers are functional-area maps; these are orthogonal. No
//! ontology vocabulary lives inside an MCP server. Every server that does
//! tagging depends on this crate.
//!
//! References:
//! - Dublin Core: <https://www.dublincore.org/specifications/dublin-core/dcmi-terms/>
//! - BIBO: <https://www.dublincore.org/specifications/bibo/>
//! - CiTO: <https://sparontologies.github.io/cito/current/cito.html>
//! - PKO: Carriero et al. (2025, arXiv:2503.20634)
//! - FIBO: <https://spec.edmcouncil.org/fibo/>
//! - ESO: <https://w3id.org/eso/>
//! - GOLEM: <https://w3id.org/golem/>
//! - OMC: <https://movielabs.com/ontology-for-media-creation/>
//! - ML-Schema: <https://www.w3.org/community/ml-schema/>

pub mod axis;
pub mod dc_bibo;
pub mod eso;
pub mod fibo;
pub mod golem;
pub mod mlschema;
pub mod omc;
pub mod pko;

// Re-export the universal-axis type aliases at the crate root for ergonomic
// access (`hkask_bridge_ontology::DcConcept`, `::PkoConcept`).
pub use dc_bibo::DcConcept;
pub use pko::PkoConcept;
