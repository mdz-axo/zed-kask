#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! Ontology bridge — the single source of truth for ontology vocabulary and
//! the dual-axis domain-selection logic in hKask.
//!
//! Nine ontologies: two universal axes, one universal core, one upper
//! ontology, and five domain supplements.
//!
//! Universal core:
//! - **5W1H** (`five_w_one_h`): the six interrogative pronouns (Who/What/
//!   When/Where/Why/How). The universal ground — every artifact answers at
//!   least one interrogative. Maps to the state axis (Who/What/When/Where →
//!   Dublin Core) and the process axis (Why/How → PKO).
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
//! - **OMC** (`omc`): media production.
//! - **ML-Schema** (`mlschema`): machine-learning experiments.
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
//! - OMC: <https://movielabs.com/ontology-for-media-creation/>
//! - ML-Schema: <https://www.w3.org/community/ml-schema/>

pub mod axis;
pub mod dc_bibo;
pub mod eso;
pub mod fibo;
pub mod five_w_one_h;
pub mod golem;
pub mod mlschema;
pub mod omc;
pub mod pko;
pub mod sumo;

// Re-export the universal-axis type aliases at the crate root for ergonomic
// access (`hkask_bridge_ontology::DcConcept`, `::PkoConcept`).
pub use dc_bibo::DcConcept;
pub use pko::PkoConcept;

/// The unified ontology → explain-tool dispatch (the "I" pattern —
/// ontology-bounded affordances). Every widget that has an "Explain"
/// affordance calls this single function instead of each reimplementing
/// its own ontology-specific dispatch.
///
/// The dispatch is driven by the concept URI prefix (the ontology
/// namespace). Each ontology contributes its own match arm:
/// - `omc:Scene` / `omc:Asset` → `gallery_analyze` (media scene/asset)
/// - `omc:*` → `describe_image` (media vision fallback)
/// - `fibo:*` → `research_search` (financial research)
/// - `pko:*` → `kanban_task_list` (process step inspection)
/// - `dcterms:*` / `dublin-core` → `research_search` (general research)
/// - empty / unknown → `research_search` (the general fallback)
///
/// Widgets that already have a domain-specific explain tool (e.g. the
/// scenarios widget's rung dispatch) don't call this — they dispatch by
/// pipeline position, not by ontology concept. This function is for widgets
/// that dispatch *because* of the ontology tag.
pub fn explain_tool_for(ontology: &str) -> &'static str {
    if ontology.starts_with("omc:") {
        return omc::explain_tool_for(ontology);
    }
    if ontology.starts_with("fibo:") || ontology == "dublin-core" {
        return "research_search";
    }
    if ontology.starts_with("pko:") {
        return "kanban_task_list";
    }
    if ontology.starts_with("dcterms:") {
        return "research_search";
    }
    // Empty or unknown — the general fallback.
    "research_search"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_tool_for_omc_concepts() {
        assert_eq!(explain_tool_for("omc:Scene"), "gallery_analyze");
        assert_eq!(explain_tool_for("omc:Asset"), "gallery_analyze");
        assert_eq!(explain_tool_for("omc:CreativeWork"), "describe_image");
        assert_eq!(explain_tool_for("omc:Version"), "describe_image");
    }

    #[test]
    fn explain_tool_for_fibo_concepts() {
        assert_eq!(explain_tool_for("fibo:Corporation"), "research_search");
        assert_eq!(explain_tool_for("fibo:Portfolio"), "research_search");
        assert_eq!(
            explain_tool_for("fibo:TransactionLedger"),
            "research_search"
        );
    }

    #[test]
    fn explain_tool_for_pko_concepts() {
        assert_eq!(explain_tool_for("pko:Step"), "kanban_task_list");
        assert_eq!(explain_tool_for("pko:Procedure"), "kanban_task_list");
        assert_eq!(explain_tool_for("pko:ChangeOfStatus"), "kanban_task_list");
    }

    #[test]
    fn explain_tool_for_dublin_core() {
        assert_eq!(explain_tool_for("dcterms:Dataset"), "research_search");
        assert_eq!(explain_tool_for("dublin-core"), "research_search");
    }

    #[test]
    fn explain_tool_for_empty_and_unknown() {
        assert_eq!(explain_tool_for(""), "research_search");
        assert_eq!(explain_tool_for("unknown:Thing"), "research_search");
    }
}
