#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Ontology bridge — the single source of truth for ontology vocabulary and
//! the dual-axis domain-selection logic in hKask.
//!
//! Full, pinned published vocabularies are compiled into `published` by
//! `build.rs`; the modules below offer named constants, not term inventories.
//! FIBO includes the publisher's Q2 Release maturity set, not Provisional.
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
//! - **SEPIO** (`sepio`): scientific evidence and provenance — evidence,
//!   support, dispute, contradiction, confidence.
//! - **GOLEM** (`golem`): literature, narrative, persona; includes pinned
//!   CIDOC-CRM and LRMoo reuse. Unlicensed DLP terms are not emitted.
//! - **ML-Schema** (`mlschema`): machine-learning experiments.
//! - **RDF Data Cube** (`data_cube`): published `qb:` terms for cube-shaped
//!   statistics. The former local `sdmx:` identifiers were removed.
//! - **MovieLabs OMC** (`omc`): media production workflows (capture → post → distribution).
//!
//! Pipeline vocabularies (assertion extraction):
//! - **schema.org** (`schema_org`): named predicates from its complete
//!   published release for corpus assertion extraction.
//! - **RDF 1.1** (`rdf`): the pipeline uses `rdf:type`; complete RDF/RDFS
//!   namespace documents are pinned in `sources/rdf-11/`.
//!
//! The domain-selection logic (`axis`) maps a domain hint to its axis
//! anchoring: state axis is always Dublin Core; process axis is the domain
//! ontology when one applies, PKO otherwise. Unknown domains fall back to
//! SUMO (the upper ontology) rather than the bare 5W1H core, so they get
//! formal categorization beyond the interrogative ground. The invariant: one
//! axis is always DC or PKO, so every artifact has a common mapping in process
//! or state space regardless of domain.
//!
//! ## The fallback ladder (P8.3)
//!
//! Ontology anchoring is a scope-broadening walk, never a single pick.
//! When a concept has no fit in the narrowest applicable ontology, the
//! anchor falls to progressively broader scopes until one fits:
//!
//! 1. **Domain supplement** — the domain's specific ontology, when the
//!    concept exists in its published vocabulary. Never force a concept
//!    into an ontology that has no place for it in its graph.
//! 2. **Derived concepts** (term resolution only) — recorded compositions
//!    over anchored constituents (`derived`), each carrying its identity
//!    and its authority citation. Operator rulings become durable anchors
//!    on this rung. Applies when resolving a TERM's meaning (the
//!    `onto_anchor` tool); artifact anchoring (`select_ontology_anchor`)
//!    skips this rung.
//! 3. **Upper ontology** — the full SUMO distribution (tier `upper`).
//! 4. **General vocabulary** — schema.org, after SUMO.
//! 5. **State-axis senses** — published Dublin Core, BIBO and CiTO terms,
//!    after the generalist rungs (artifact-axis dispatch is separate).
//! 6. **Interrogative ground** — the 5W1H core: the guaranteed final rung.
//! Published resolutions carry the source's own definition when supplied;
//! an absent definition is never filled with a private gloss.
//!
//! The invariant: **nothing is ever untagged.** SUMO and the 5W1H core
//! exist precisely so the ladder always terminates on a real anchor.
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
//! - SEPIO: https://github.com/monarch-initiative/SEPIO-ontology
//! - GOLEM: Pianzola et al. (GOLEM Lab, 2024). <https://ontology.golemlab.eu/>
//! - ML-Schema: <https://www.w3.org/community/ml-schema/>
//! - RDF Data Cube: <https://www.w3.org/TR/vocab-data-cube/>

pub mod axis;
pub mod data_cube;
pub mod dc_bibo;
pub mod derived;
pub mod fibo;
pub mod golem;
pub mod ml_schema;
pub mod omc;
pub mod ontology_graph;
pub mod pko;
pub mod published;
#[cfg(test)]
mod published_sources;
pub mod rdf;
pub mod schema_org;

pub mod sepio;
pub mod sumo;
pub mod term_resolution;

// Re-export the universal-axis type aliases at the crate root for ergonomic
// access (`hkask_bridge_ontology::DcConcept`, `::PkoConcept`).
pub use dc_bibo::DcConcept;
pub use pko::PkoConcept;
