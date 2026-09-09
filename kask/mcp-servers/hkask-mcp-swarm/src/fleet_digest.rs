//! The local fleet's shape, in a fixed number of lines — the prompt tier of
//! fermi's meta-agent fleet-awareness pattern
//! (`docs/architecture/META_AGENT_FLEET_AWARENESS.md`), ported to the local
//! registry.
//!
//! # The problem this exists to prevent
//!
//! fermi's incident: asked which model a free-tier creature would use for
//! `biotech_analyst`, the navigator answered three specific claims — all
//! false, all one local lookup from being checked. It could not have
//! answered correctly: its sources were a one-line prose digest per agent
//! (no model facts) and a listing tool (no model facts either). Fabrication
//! was the only way to answer at all.
//!
//! The fix is three tiers: the **prompt** carries the fleet's *shape* (a
//! derived, fixed-size digest), the **tools** serve the *territory*
//! (`swarm_get_local_agent` for per-agent facts, `swarm_who_answers_local`
//! for cohorts), and delegation routes to cluster experts. zed-kask already
//! has the tool tier's index (`swarm_list_local_agents`) and detail
//! (`swarm_get_local_agent`); this module is the map, plus the cohort
//! reading the tools do not give.
//!
//! # The invariant
//!
//! **This digest names categories and counts, never individual agents.**
//! Adding a hundred agents moves the counts and leaves the row count almost
//! unchanged — O(structure), not O(members). The natural edit ("and here
//! are the members") silently restores the O(n) prompt this exists to
//! remove, so [`tests::the_digest_names_no_individual_agent`] defends it.
//!
//! # The two counterweights that must ship with a thin map
//!
//! 1. **Name what the reader does not know** ([`WHAT_YOU_DO_NOT_KNOW`]) — a
//!    digest that only says what *is* known invites the model to fill the
//!    rest from memory.
//! 2. **Make staleness self-detectable** ([`Digest::describes`]) — the digest
//!    carries the fleet size it was built from; a tool reporting a different
//!    count means the map is stale and the reader can say so instead of
//!    answering from it.

use crate::local_registry::LocalAgentCard;
use std::collections::BTreeMap;

/// Cap on digest rows per axis. The digest is a fixed-size artifact; an
/// axis that would exceed this is the wrong axis (its cardinality tracks
/// membership, not structure — fermi's `skills` axis failed exactly this
/// test at 366 distinct values over 102 agents).
pub const MAP_ROWS: usize = 12;

/// Above this share of the corpus, a shared `accepts` label stops narrowing
/// anything — it describes how the platform is *called*, not what any agent
/// is *for*. fermi's argument for a tenth: `query` sat at 24% (a convention),
/// `workspace-state` at 8% (a real cohort), and the gap between those two is
/// the whole distinction.
pub const UNIVERSAL_SHARE: f64 = 0.10;

/// The three-state reading of one `accepts` label. A count alone is true
/// and useless — `query` accepted by 24 of 102 excludes nothing. Only the
/// classification says whether a label narrows the fleet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Substitutes {
    /// One agent answers to this label. "Only this one" is a real answer,
    /// not a missing one.
    Bespoke,
    /// A real set of interchangeable answerers.
    Cohort(usize),
    /// So many agents accept it that the label is the calling convention.
    Universal(usize),
}

impl Substitutes {
    /// The wire word for the reading.
    pub fn as_str(&self) -> &'static str {
        match self {
            Substitutes::Bespoke => "bespoke",
            Substitutes::Cohort(_) => "cohort",
            Substitutes::Universal(_) => "universal",
        }
    }

    /// The count of accepting agents, whichever state.
    pub fn count(&self) -> usize {
        match self {
            Substitutes::Bespoke => 1,
            Substitutes::Cohort(count) | Substitutes::Universal(count) => *count,
        }
    }
}

/// Classify one label from the number of agents accepting it.
pub fn substitutes(accepting: usize, corpus: usize) -> Substitutes {
    if accepting <= 1 {
        return Substitutes::Bespoke;
    }
    if corpus > 0 && (accepting as f64) / (corpus as f64) > UNIVERSAL_SHARE {
        return Substitutes::Universal(accepting);
    }
    Substitutes::Cohort(accepting)
}

/// The derived fleet map. Categories and counts only — never member names.
#[derive(Debug, Clone, PartialEq)]
pub struct Digest {
    /// The fleet size this digest was built from — the staleness anchor.
    /// A tool reporting a different live count means this map is stale.
    pub fleet_size: usize,
    /// `agent_type` → count. The reliable spine: a curated vocabulary whose
    /// cardinality tracks structure, not membership.
    pub types: Vec<(String, usize)>,
    /// `accepts` label → (count, reading). The "who answers what" map.
    pub cohorts: Vec<(String, usize, Substitutes)>,
}

/// Build the digest from the live registry. Pure over its input — the tool
/// layer passes the registry's cards, tests pass fixtures.
pub fn digest(cards: &[LocalAgentCard]) -> Digest {
    let fleet_size = cards.len();

    let mut type_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut label_counts: BTreeMap<String, usize> = BTreeMap::new();
    for card in cards {
        *type_counts.entry(card.agent_type.clone()).or_default() += 1;
        for label in &card.accepts {
            *label_counts.entry(label.clone()).or_default() += 1;
        }
    }

    let mut types: Vec<(String, usize)> = type_counts.into_iter().collect();
    types.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    types.truncate(MAP_ROWS);

    let mut cohorts: Vec<(String, usize, Substitutes)> = label_counts
        .into_iter()
        .map(|(label, count)| {
            let reading = substitutes(count, fleet_size);
            (label, count, reading)
        })
        .collect();
    // Narrowing labels first — a cohort of 2 is more informative than a
    // universal of 24. Bespoke labels carry the "only this one" answer.
    cohorts.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    cohorts.truncate(MAP_ROWS);

    Digest {
        fleet_size,
        types,
        cohorts,
    }
}

/// What the digest deliberately does not carry — the honesty counterweight.
/// Per-agent facts (model, prompt, ports, stats) come from
/// `swarm_get_local_agent`; a digest that only says what *is* known invites
/// the reader to fill the rest from memory.
pub const WHAT_YOU_DO_NOT_KNOW: &str = "You know the fleet's shape, not per-agent \
facts. Models, prompts, ports, and execution stats come from \
swarm_get_local_agent — do not state them from memory. If a tool reports a \
different fleet size than this map carries, the map is stale: say so rather \
than answering from it.";

impl Digest {
    /// Render the digest as the prompt-tier text: the staleness anchor
    /// first, the map in the middle, [`WHAT_YOU_DO_NOT_KNOW`] last.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "The local fleet: {} agents. This map was built from that count.\n\n",
            self.fleet_size
        ));
        out.push_str("By type:\n");
        for (agent_type, count) in &self.types {
            out.push_str(&format!("  {agent_type}  {count}\n"));
        }
        out.push_str("\nBy accepted ask (who answers what):\n");
        for (label, count, reading) in &self.cohorts {
            out.push_str(&format!(
                "  {label}  {count} of {}  {}\n",
                self.fleet_size,
                reading.as_str()
            ));
        }
        out.push_str(&format!("\n{WHAT_YOU_DO_NOT_KNOW}\n"));
        out
    }

    /// The staleness check: does this digest still describe a fleet of
    /// `live_count` agents? A meta agent holding a stale map can say so
    /// instead of answering from it.
    pub fn describes(&self, live_count: usize) -> bool {
        self.fleet_size == live_count
    }

    /// The structured form the tool returns (the rendered text rides
    /// alongside it for prompt injection).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "fleet_size": self.fleet_size,
            "types": self.types.iter().map(|(agent_type, count)| {
                serde_json::json!({ "type": agent_type, "count": count })
            }).collect::<Vec<_>>(),
            "cohorts": self.cohorts.iter().map(|(label, count, reading)| {
                serde_json::json!({
                    "label": label,
                    "count": count,
                    "share": if self.fleet_size > 0 {
                        (*count as f64) / (self.fleet_size as f64)
                    } else { 0.0 },
                    "reading": reading.as_str(),
                })
            }).collect::<Vec<_>>(),
            "what_you_do_not_know": WHAT_YOU_DO_NOT_KNOW,
        })
    }
}

/// Who else answers the same ask — the cohort query behind
/// `swarm_who_answers_local`. Unlike the digest, this DOES name agents:
/// it is a tool-tier answer to a specific question, not a prompt-tier map.
#[derive(Debug, Clone, PartialEq)]
pub struct Answerers {
    /// The `accepts` label, as declared.
    pub question: String,
    /// Agent ids, sorted, so the set reads the same on every call.
    pub agents: Vec<String>,
    /// The three-state reading of the cohort size.
    pub reading: Substitutes,
}

/// Compute the answerers for one label. Pure over its input.
pub fn answerers(cards: &[LocalAgentCard], label: &str) -> Answerers {
    let mut agents: Vec<String> = cards
        .iter()
        .filter(|card| card.accepts.iter().any(|accepts| accepts == label))
        .map(|card| card.agent_id.clone())
        .collect();
    agents.sort();
    let reading = substitutes(agents.len(), cards.len());
    Answerers {
        question: label.to_string(),
        agents,
        reading,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_registry::{
        LocalAgentCapabilities, LocalAgentDependencies, LocalWorkflowTemplate,
    };

    /// A minimal card — the digest only reads `agent_id`, `agent_type`, and
    /// `accepts`, so the fixture stays minimal.
    fn card(agent_id: &str, agent_type: &str, accepts: &[&str]) -> LocalAgentCard {
        LocalAgentCard {
            agent_id: agent_id.to_string(),
            agent_type: agent_type.to_string(),
            description: String::new(),
            display_name: String::new(),
            accepts: accepts.iter().map(|label| label.to_string()).collect(),
            produces: vec![],
            dependencies: LocalAgentDependencies::default(),
            capabilities: LocalAgentCapabilities::default(),
            cloud_swarm_id: None,
            tags: vec![],
            visibility: String::new(),
            sample_queries: vec![],
            valence: None,
            version: String::new(),
            workflow_template: None::<LocalWorkflowTemplate>,
        }
    }

    /// A fleet shaped to exercise every reading: 10 agents, `query`
    /// accepted by 3 (30% — universal), `analysis` by 2 (cohort),
    /// `genome-query` by 1 (bespoke).
    fn fleet() -> Vec<LocalAgentCard> {
        vec![
            card("alpha", "analyst", &["query", "analysis"]),
            card("beta", "analyst", &["query", "analysis"]),
            card("gamma", "analyst", &["query"]),
            card("delta", "critic", &["query", "genome-query"]),
            card("epsilon", "critic", &["text"]),
            card("zeta", "writer", &["text"]),
            card("eta", "writer", &["text"]),
            card("theta", "writer", &["text"]),
            card("iota", "planner", &["text"]),
            card("kappa", "planner", &["text"]),
        ]
    }

    /// THE invariant: the digest names categories and counts, never
    /// individual agents. The natural edit — "and here are the members" —
    /// silently restores the O(n) prompt this module exists to remove.
    #[test]
    fn the_digest_names_no_individual_agent() {
        let cards = fleet();
        let digest = digest(&cards);
        let rendered = digest.render();
        for card in &cards {
            assert!(
                !rendered.contains(&card.agent_id),
                "the digest must not name individual agents — found '{}'",
                card.agent_id
            );
        }
        // The structured form carries the same invariant.
        let structured = digest.to_json().to_string();
        for card in &cards {
            assert!(
                !structured.contains(&format!("\"{}\"", card.agent_id)),
                "the structured digest must not name individual agents — found '{}'",
                card.agent_id
            );
        }
    }

    #[test]
    fn the_digest_is_o_structure_not_o_members() {
        // Adding agents of EXISTING types and labels moves counts, not rows.
        let mut small = fleet();
        let digest_small = digest(&small);
        for index in 0..20 {
            small.push(card(&format!("extra-{index}"), "writer", &["text"]));
        }
        let digest_big = digest(&small);
        assert_eq!(
            digest_big.types.len(),
            digest_small.types.len(),
            "same type vocabulary — the type rows did not grow"
        );
        assert!(digest_big.fleet_size > digest_small.fleet_size);
    }

    #[test]
    fn a_label_above_the_universal_share_reads_universal() {
        // 4 of 10 accept `query` — 40% > 10%: the calling convention.
        let cards = fleet();
        let answer = answerers(&cards, "query");
        assert_eq!(answer.reading, Substitutes::Universal(4));
    }

    #[test]
    fn a_small_shared_label_reads_cohort() {
        // The cohort band is (1, 10%] of the corpus. With the 10-agent
        // fixture, 2 of 10 is 20% — universal — so grow the corpus to 20:
        // 2 of 20 is exactly 10%, not above it, which is a cohort.
        let mut cards = fleet();
        for index in 0..10 {
            cards.push(card(&format!("filler-{index}"), "writer", &["text"]));
        }
        let answer = answerers(&cards, "analysis");
        assert_eq!(answer.reading, Substitutes::Cohort(2));
    }

    #[test]
    fn one_answerer_is_bespoke_and_that_is_an_answer() {
        let cards = fleet();
        let answer = answerers(&cards, "genome-query");
        assert_eq!(answer.reading, Substitutes::Bespoke);
        assert_eq!(answer.agents, vec!["delta".to_string()]);
    }

    #[test]
    fn the_digest_carries_the_staleness_anchor() {
        let cards = fleet();
        let digest = digest(&cards);
        assert!(digest.describes(cards.len()));
        assert!(!digest.describes(cards.len() + 1));
        // The rendered map states the fleet size it was built from.
        assert!(digest.render().contains("10 agents"));
    }

    #[test]
    fn the_render_carries_what_you_do_not_know() {
        // A digest that only says what IS known invites the model to fill
        // the rest from memory — the counterweight is load-bearing.
        let digest = digest(&fleet());
        let rendered = digest.render();
        assert!(rendered.contains(WHAT_YOU_DO_NOT_KNOW));
        assert!(rendered.contains("swarm_get_local_agent"));
    }

    #[test]
    fn answerers_names_agents_because_it_is_a_tool_answer() {
        // The digest must not name agents; a specific cohort query must —
        // that is the map/territory split, held by both sides.
        let cards = fleet();
        let answer = answerers(&cards, "analysis");
        assert_eq!(answer.agents, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn an_unknown_label_answers_empty_bespoke() {
        let cards = fleet();
        let answer = answerers(&cards, "nonexistent-ask");
        assert!(answer.agents.is_empty());
        assert_eq!(answer.reading, Substitutes::Bespoke);
    }
}
