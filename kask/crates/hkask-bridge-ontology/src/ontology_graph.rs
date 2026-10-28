//! Read-only relation graph over resolved ontology concepts.
//!
//! Edges carry their authority: derived constituent edges come from the
//! recorded operator rulings; published edges (direct parents and inverse
//! properties) come from the pinned source file that states them. This is not
//! an instance-fact graph or an OWL reasoner: no transitive or inferred edge is
//! ever added. A path proves only the stated relations between concept IDs.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::{derived, published, term_resolution::resolve_term};

pub const MAX_HOPS: u8 = 4;
const MAX_VISITS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    HasConstituent,
    InverseOf,
    /// The source directly states the parent: SUMO `subclass`, `instance`,
    /// `subrelation`, `subAttribute`; schema.org `subTypeOf`, `subPropertyOf`,
    /// or an enumeration member's type.
    HasParent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationEdge {
    pub from: String,
    pub relation: Relation,
    pub to: String,
    /// Published page or recorded operator ruling; never a model-supplied URI.
    pub authority: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraversalStatus {
    Neighbors,
    PathFound,
    NoSupportedPath,
    CoarseAnchor,
    InvalidQuery,
    BudgetExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraversalResult {
    pub from: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub status: TraversalStatus,
    /// Outgoing edges for a neighbor query, or ordered edges of the shortest
    /// supported directed path for a path query. Never inferred transitive edges.
    pub edges: Vec<RelationEdge>,
    pub visited_nodes: usize,
    pub max_hops: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug)]
pub struct OntologyGraph {
    outgoing: HashMap<String, Vec<RelationEdge>>,
}

impl OntologyGraph {
    fn from_edges(mut edges: Vec<RelationEdge>) -> Self {
        // Insertion order must not change the chosen equal-length path.
        edges.sort_by(|a, b| {
            (&a.from, &a.to, a.relation, &a.authority).cmp(&(
                &b.from,
                &b.to,
                b.relation,
                &b.authority,
            ))
        });
        edges.dedup();
        let mut outgoing: HashMap<String, Vec<RelationEdge>> = HashMap::new();
        for edge in edges {
            outgoing.entry(edge.from.clone()).or_default().push(edge);
        }
        Self { outgoing }
    }

    fn build() -> Self {
        let mut edges = Vec::new();
        for concept in derived::DERIVED_CONCEPTS {
            for constituent in concept.constituents {
                let resolved = resolve_term(constituent);
                // All unknown terms share the coarse 5W1H anchor. Joining
                // them as graph nodes would manufacture paths between them.
                if resolved.tier == "core" || resolved.concept == concept.term {
                    continue;
                }
                edges.push(RelationEdge {
                    from: concept.term.to_string(),
                    relation: Relation::HasConstituent,
                    to: resolved.concept,
                    authority: concept.authority.to_string(),
                });
            }
        }
        // Relations BETWEEN published vocabulary terms, never instance facts.
        // Each edge is asserted by the source row of its `from` term.
        for term in published::terms() {
            for (relation, targets) in [
                (Relation::HasParent, &term.parents),
                (Relation::InverseOf, &term.inverse_of),
            ] {
                for target in targets {
                    edges.push(RelationEdge {
                        from: term.concept.to_string(),
                        relation,
                        to: (*target).to_string(),
                        authority: term.source.to_string(),
                    });
                }
            }
        }
        Self::from_edges(edges)
    }

    /// Return outgoing relations or the shortest directed path, with explicit
    /// absence and budget states. BFS is non-recursive and deterministic.
    pub fn traverse(
        &self,
        from_term: &str,
        to_term: Option<&str>,
        max_hops: u8,
    ) -> TraversalResult {
        let from = resolve_term(from_term);
        let to = to_term.map(resolve_term);
        let mut result = TraversalResult {
            from: from.concept.clone(),
            to: to.as_ref().map(|target| target.concept.clone()),
            status: TraversalStatus::NoSupportedPath,
            edges: Vec::new(),
            visited_nodes: 0,
            max_hops,
            note: None,
        };
        if !(1..=MAX_HOPS).contains(&max_hops) || (to.is_none() && max_hops != 1) {
            result.status = TraversalStatus::InvalidQuery;
            result.note = Some(format!(
                "max_hops must be 1..={MAX_HOPS} for a path, and 1 for neighbors"
            ));
            return result;
        }
        if from.tier == "core" || to.as_ref().is_some_and(|target| target.tier == "core") {
            result.status = TraversalStatus::CoarseAnchor;
            result.note = Some("A core-rung anchor has no distinct concept identity for graph traversal; request a ruling instead of inferring an edge".to_string());
            return result;
        }
        let start = from.concept;
        let Some(target) = to.map(|resolved| resolved.concept) else {
            result.visited_nodes = 1;
            let edges = self
                .outgoing
                .get(&start)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if edges.len() > MAX_VISITS {
                result.status = TraversalStatus::BudgetExhausted;
                result.note = Some(
                    "Outgoing edge budget exhausted; neighbor set is not complete".to_string(),
                );
                return result;
            }
            result.status = TraversalStatus::Neighbors;
            result.edges = edges.to_vec();
            return result;
        };
        if start == target {
            result.note = Some("Identical anchors do not establish a relation".to_string());
            return result;
        }

        let mut queue = VecDeque::from([(start.clone(), 0_u8)]);
        let mut seen = HashSet::from([start]);
        let mut parents: HashMap<String, RelationEdge> = HashMap::new();
        while let Some((node, depth)) = queue.pop_front() {
            if depth == max_hops {
                continue;
            }
            if let Some(edges) = self.outgoing.get(&node) {
                for edge in edges {
                    if seen.contains(&edge.to) {
                        continue;
                    }
                    if seen.len() == MAX_VISITS {
                        result.status = TraversalStatus::BudgetExhausted;
                        result.visited_nodes = seen.len();
                        result.note = Some("Traversal visit budget exhausted; absence of a path is not established".to_string());
                        return result;
                    }
                    seen.insert(edge.to.clone());
                    parents.insert(edge.to.clone(), edge.clone());
                    if edge.to == target {
                        let mut cursor = target;
                        while let Some(previous) = parents.get(&cursor) {
                            result.edges.push(previous.clone());
                            cursor = previous.from.clone();
                        }
                        result.edges.reverse();
                        result.status = TraversalStatus::PathFound;
                        result.visited_nodes = seen.len();
                        return result;
                    }
                    queue.push_back((edge.to.clone(), depth + 1));
                }
            }
        }
        result.visited_nodes = seen.len();
        result.note = Some("No supported directed path within the searched graph and hop bound; this does not prove the relation false".to_string());
        result
    }
}

static GRAPH: OnceLock<OntologyGraph> = OnceLock::new();

pub fn graph() -> &'static OntologyGraph {
    GRAPH.get_or_init(OntologyGraph::build)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_two_hop_path_carries_each_distinct_ruling() {
        let path = graph().traverse("sustainable growth rate", Some("net margin"), 2);
        assert_eq!(path.status, TraversalStatus::PathFound);
        assert_eq!(path.edges.len(), 2);
        assert_eq!(path.edges[0].from, "sustainable_growth_rate");
        assert_eq!(path.edges[0].to, "return_on_equity");
        assert_eq!(path.edges[1].to, "net_margin");
        for edge in path.edges {
            assert_eq!(edge.relation, Relation::HasConstituent);
            assert!(edge.authority.contains("operator ruling"));
        }
        // Pin the whole reviewed constituent set, not only one selected path:
        // a wrongly added edge with the same authority must fail this test.
        for (term, expected) in [
            (
                "sustainable growth rate",
                &["retention", "return_on_equity"][..],
            ),
            (
                "return on equity",
                &["asset_turnover", "equity_multiplier", "net_margin"][..],
            ),
        ] {
            let actual: Vec<_> = graph()
                .traverse(term, None, 1)
                .edges
                .into_iter()
                .map(|edge| edge.to)
                .collect();
            assert_eq!(actual, expected, "incorrect constituent edge for {term}");
        }
    }

    /// expect: every published edge is stated by its source row and cites
    /// that source; inverse properties run in both published directions.
    #[test]
    fn published_relation_is_source_pinned_and_directional() {
        let forward = graph().traverse("schema:hasPart", None, 1);
        assert_eq!(forward.status, TraversalStatus::Neighbors);
        let inverse: Vec<_> = forward
            .edges
            .iter()
            .filter(|edge| edge.relation == Relation::InverseOf)
            .collect();
        assert_eq!(inverse.len(), 1, "{:?}", forward.edges);
        assert_eq!(inverse[0].to, "schema:isPartOf");
        for edge in &forward.edges {
            let source = published::get(&edge.from).expect("edge source is published");
            assert_eq!(edge.authority, source.source);
            let stated = match edge.relation {
                Relation::InverseOf => &source.inverse_of,
                Relation::HasParent => &source.parents,
                Relation::HasConstituent => panic!("no ruling edge on a published term"),
            };
            assert!(stated.contains(&edge.to.as_str()), "unstated edge {edge:?}");
        }
        let reverse = graph().traverse("schema:isPartOf", Some("schema:hasPart"), 1);
        assert_eq!(reverse.status, TraversalStatus::PathFound);
        assert!(
            reverse.edges[0]
                .authority
                .contains("schemaorg-all-https-properties.csv")
        );
    }

    /// expect: published parents are traversable, one stated hop at a time,
    /// never as an inferred shortcut.
    #[test]
    fn published_parents_are_traversable_without_inferred_edges() {
        let neighbors = graph().traverse("Game", None, 1);
        let parents: Vec<_> = neighbors
            .edges
            .iter()
            .map(|edge| edge.to.as_str())
            .collect();
        assert!(parents.contains(&"sumo:Contest"), "{parents:?}");
        assert!(
            neighbors
                .edges
                .iter()
                .all(|edge| edge.relation == Relation::HasParent)
        );
        let path = graph().traverse("Game", Some("SocialInteraction"), 2);
        assert_eq!(path.status, TraversalStatus::PathFound, "{path:?}");
        assert_eq!(
            path.edges.len(),
            2,
            "Game -> Contest -> SocialInteraction, no shortcut"
        );
        assert!(
            path.edges
                .iter()
                .all(|edge| edge.authority.starts_with("Merge.kif"))
        );
    }

    #[test]
    fn absent_edge_and_too_shallow_search_cannot_support_a_conclusion() {
        let shallow = graph().traverse("sustainable growth rate", Some("net margin"), 1);
        assert_eq!(shallow.status, TraversalStatus::NoSupportedPath);
        assert!(shallow.edges.is_empty());
        let removed = OntologyGraph::from_edges(vec![RelationEdge {
            from: "sustainable_growth_rate".into(),
            to: "return_on_equity".into(),
            relation: Relation::HasConstituent,
            authority: "operator ruling 2026-09-10".into(),
        }]);
        let path = removed.traverse("sustainable growth rate", Some("net margin"), 2);
        assert_eq!(path.status, TraversalStatus::NoSupportedPath);
        assert!(path.edges.is_empty());
    }

    #[test]
    fn core_terms_never_connect_via_the_shared_core_anchor() {
        let path = graph().traverse("zephyr coefficient", Some("another unknown"), 2);
        assert_eq!(path.status, TraversalStatus::CoarseAnchor);
        assert!(path.edges.is_empty());
        let neighbors = graph().traverse("net margin", None, 1);
        assert!(neighbors.edges.iter().all(|edge| edge.to != "5w1h_core"));
    }

    #[test]
    fn bfs_is_directed_stable_and_reports_budget_exhaustion() {
        let edges = vec![
            ("sustainable_growth_rate", "schema:hasPart"),
            ("sustainable_growth_rate", "return_on_equity"),
            ("schema:hasPart", "net_margin"),
            ("return_on_equity", "net_margin"),
        ];
        let make = |ordered: Vec<(&str, &str)>| {
            OntologyGraph::from_edges(
                ordered
                    .into_iter()
                    .map(|(from, to)| RelationEdge {
                        from: from.into(),
                        to: to.into(),
                        relation: Relation::HasConstituent,
                        authority: "test".into(),
                    })
                    .collect(),
            )
        };
        let first = make(edges.clone());
        let second = make(edges.into_iter().rev().collect());
        let path = first.traverse("sustainable growth rate", Some("net margin"), 2);
        assert_eq!(path.status, TraversalStatus::PathFound);
        assert_eq!(path.edges[0].to, "return_on_equity");
        assert_eq!(
            path,
            second.traverse("sustainable growth rate", Some("net margin"), 2)
        );
        assert_eq!(
            first
                .traverse("net margin", Some("sustainable growth rate"), 2)
                .status,
            TraversalStatus::NoSupportedPath
        );
        let oversized = OntologyGraph::from_edges(
            (0..=MAX_VISITS)
                .map(|n| RelationEdge {
                    from: "schema:hasPart".to_string(),
                    to: format!("node_{n}"),
                    relation: Relation::InverseOf,
                    authority: "test".to_string(),
                })
                .collect(),
        );
        let exhausted = oversized.traverse("schema:hasPart", Some("schema:isPartOf"), 2);
        assert_eq!(exhausted.status, TraversalStatus::BudgetExhausted);
        assert_eq!(exhausted.visited_nodes, MAX_VISITS);
        assert!(exhausted.edges.is_empty());
        let neighbors = oversized.traverse("schema:hasPart", None, 1);
        assert_eq!(neighbors.status, TraversalStatus::BudgetExhausted);
        assert!(neighbors.edges.is_empty());
        assert_eq!(
            graph()
                .traverse("schema:hasPart", Some("schema:isPartOf"), 0)
                .status,
            TraversalStatus::InvalidQuery
        );
        assert_eq!(
            graph()
                .traverse("schema:hasPart", Some("schema:isPartOf"), MAX_HOPS + 1)
                .status,
            TraversalStatus::InvalidQuery
        );
        assert_eq!(
            graph()
                .traverse("schema:isPartOf", Some("net margin"), 2)
                .status,
            TraversalStatus::NoSupportedPath
        );
    }
}
