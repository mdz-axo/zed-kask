#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Ontology bridge — the single source of truth for ontology vocabulary and
//! the dual-axis domain-selection logic in hKask.
//!
//! Eight ontologies: two universal axes, one upper
//! ontology, and five domain supplements.
//!
//! Two universal axes (P5.4):
//! - **State axis** — Dublin Core + BIBO + CiTO (`dc_bibo`): the "what is this"
//!   noun dimension. Always available; every artifact carries a state identity.
//! - **Process axis** — PKO (`pko`): the "how did this come to be" verb dimension.
//!   Always available; every artifact carries a process identity.
//!
//! Upper ontology (universal fallback):
//! - **SUMO** (`sumo`): the Suggested Upper Merged Ontology. The general-purpose
//!   fallback for domains that don't map to a specific supplement. Provides
//!   the foundational categories (Entity, Process, Object, Agent, Relation)
//!   that all domain supplements specialize.
//!
//! Domain supplements (P8.1) — layered on top where the universal axes aren't
//! specific enough for a domain:
//! - **FIBO** (`fibo`): financial / company analysis.
//! - **ESO** (`eso`): scientific reasoning, hypotheses, evidence.
//! - **GOLEM** (`golem`): literature, narrative, persona.
//! - **ML-Schema** (`mlschema`): machine-learning experiments.
//! - **SDMX** (`sdmx`): statistical data exchange (FRED, DBnomics, World Bank).
//!
//! The domain-selection logic (`axis`) maps a domain hint to its axis
//! anchoring: state axis is always Dublin Core; process axis is the domain
//! ontology when one applies, PKO otherwise. Unknown domains fall back to
//! SUMO (the upper ontology) rather than the bare 5W1H core, so they get
//! formal categorization beyond the interrogative ground. The invariant: one
//! axis is always DC or PKO, so every artifact has a common mapping in process
//! or state space regardless of domain.
//!
//! Architectural invariant (user directive 2026-08-05): ontologies are domain
//! maps; MCP servers are functional-area maps; these are orthogonal. No
//! ontology vocabulary lives inside an MCP server. Every server that does
//! tagging depends on this crate.
//!
//! References:
//! - 5W1H: Kipling's "six honest serving-men"; the foundational journalism/
//!   investigation interrogative framework.
//! - Dublin Core: <https://www.dublincore.org/specifications/dublin-core/dcmi-terms/>
//! - BIBO: <https://www.dublincore.org/specifications/bibo/>
//! - CiTO: <https://sparontologies.github.io/cito/current/cito.html>
//! - PKO: Carriero et al. (2025, arXiv:2503.20634)
//! - SUMO: <https://github.com/ontologyportal/sumo> — Pease, A. (2010).
//!   Ontology: A Practical Guide. Articulate Software Press.
//! - FIBO: <https://spec.edmcouncil.org/fibo/>
//! - ESO: <https://w3id.org/eso/>
//! - GOLEM: <https://w3id.org/golem/>
//! - ML-Schema: <https://www.w3.org/community/ml-schema/>
//! - SDMX: <https://sdmx.org/> (ISO 17369)

pub mod axis;
pub mod dc_bibo;
pub mod eso;
pub mod fibo;
pub mod golem;
pub mod ml_schema;
pub mod pko;
pub mod sdmx;
pub mod sumo;

// Re-export the universal-axis type aliases at the crate root for ergonomic
// access (`hkask_bridge_ontology::DcConcept`, `::PkoConcept`).
pub use dc_bibo::DcConcept;
pub use pko::PkoConcept;
