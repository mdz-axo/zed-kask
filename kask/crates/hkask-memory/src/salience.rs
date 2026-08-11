//! Salience scoring for style corpora.
//!
//! Computes passage salience scores (weighted graph degree centrality
//! combining entity tag counts, method coverage, category diversity, and
//! positional significance into a single 0.0-1.0+ score) for budget-gated
//! h_mem storage.
//!
//! Used by `EmbedService` at embed time (budget gating) and by the style
//! synthesizer at query time (salience-parameterized retrieval).

// ── Entity Tagging ────────────────────────────────────────────────────────

/// Tags extracted from a passage by string-matching against declared entities.
#[derive(Debug, Clone, Default)]
pub struct EntityTags {
    pub characters: Vec<String>,
    pub places: Vec<String>,
    pub events: Vec<String>,
    pub concepts: Vec<String>,
    pub methods: Vec<String>,
}

impl EntityTags {
    /// All entity and method names as a single iterator for graph construction.
    ///
    /// expect: "The system scores passage salience to gate h_mem storage budget"
    /// \[P3\] Motivating: Generative Space — flattens entity categories for graph construction
    /// \[P5\] Constraining: Essentialism — minimal iterator over existing vectors
    /// post: returns iterator over all tag strings across all categories
    pub fn all_tags(&self) -> impl Iterator<Item = &str> {
        self.characters
            .iter()
            .map(String::as_str)
            .chain(self.places.iter().map(String::as_str))
            .chain(self.events.iter().map(String::as_str))
            .chain(self.concepts.iter().map(String::as_str))
            .chain(self.methods.iter().map(String::as_str))
    }
}

// ── Salience Score ────────────────────────────────────────────────────────

/// Compute salience scores for all tagged passages using graph centrality.
/// Compute passage salience scores for budget-gated h_mem storage.
///
/// Salience = connectedness × (1 − redundancy):
///
///   connectedness = (one_hop + avg_neighbor_quality) / 2
///   redundancy    = local_clustering_coefficient(sampled_neighbors)
///   salience      = connectedness × (1 − redundancy)
///
/// **one_hop** — degree centrality: fraction of all passages sharing at
/// least one entity with this passage. High = well-connected.
///
/// **avg_neighbor_quality** — mean one_hop score of this passage's
/// neighbors (evenly sampled, max 50). Being connected to well-connected
/// passages boosts this term (eigenvector-like).
///
/// **redundancy** — Watts-Strogatz local clustering coefficient: what
/// fraction of my neighbor pairs are themselves connected? High clustering
/// = I sit in a dense, redundant clique. Low clustering = I bridge
/// otherwise-disconnected communities.
///
/// The multiplicative penalty ensures moderate clustering gets moderate
/// reduction rather than being zeroed out. Only fully interconnected
/// cliques (redundancy=1) get salience=0.
///
/// All expansion steps are capped at 50 sampled neighbors to bound
/// worst-case complexity at O(n × k × d) where k=50, d=average degree.
/// Foundational rules (passages with zero tags) get salience 0.0.
///
/// expect: "The system scores passage salience to gate h_mem storage budget"
/// \[P3\] Motivating: Generative Space — scores passage salience to gate h_mem storage budget
/// \[P9\] Constraining: Homeostatic Self-Regulation — graph centrality bounded by neighbor sampling
/// pre:  all_tags is a slice of EntityTags
/// post: returns `Vec<f32>` with one salience score per passage
/// post: passages with zero tags get salience 0.0
/// post: returns empty Vec for empty input
pub fn compute_salience_batch(all_tags: &[EntityTags]) -> Vec<f32> {
    let n = all_tags.len();
    if n == 0 {
        return Vec::new();
    }

    // Build inverted index: entity_name → set of passage indices
    let mut entity_to_passages: std::collections::HashMap<&str, Vec<usize>> =
        std::collections::HashMap::new();

    for (i, tags) in all_tags.iter().enumerate() {
        for tag in tags.all_tags() {
            entity_to_passages.entry(tag).or_default().push(i);
        }
    }

    // For each passage, compute its neighbor set (union of all entity co-occurrences)
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (i, tags) in all_tags.iter().enumerate() {
        let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
        for tag in tags.all_tags() {
            if let Some(passages) = entity_to_passages.get(tag) {
                for &p in passages {
                    if p != i {
                        seen.insert(p);
                    }
                }
            }
        }
        neighbors[i] = seen.into_iter().collect();
    }

    // One-hop: degree centrality — fraction of passages directly connected
    let n_f = n as f32;
    let one_hop: Vec<f32> = neighbors.iter().map(|nb| nb.len() as f32 / n_f).collect();

    // For each passage, compute connectedness and redundancy via capped sampling.
    // Both expansions capped at 50 neighbors to bound O(n × k × d).
    const MAX_SAMPLE: usize = 50;

    let salience: Vec<f32> = neighbors
        .iter()
        .enumerate()
        .map(|(i, nb)| {
            if nb.is_empty() {
                return 0.0;
            }

            // Evenly sample neighbors (avoids bias toward first neighbors)
            let sample: Vec<usize> = if nb.len() > MAX_SAMPLE {
                let step = nb.len() / MAX_SAMPLE;
                nb.iter().step_by(step.max(1)).copied().collect()
            } else {
                nb.clone()
            };

            // ── avg_neighbor_quality: mean one_hop of sampled neighbors ──
            let avg_nq: f32 = sample.iter().map(|&j| one_hop[j]).sum::<f32>() / sample.len() as f32;

            // ── connectedness = (one_hop + avg_neighbor_quality) / 2 ──
            let connectedness = (one_hop[i] + avg_nq) / 2.0;

            // ── redundancy: local clustering coefficient ──
            // What fraction of sampled neighbor pairs are themselves connected?
            let redundancy = if sample.len() < 2 {
                0.0
            } else {
                // Build hash sets for sampled neighbors for O(1) edge checks
                let sample_sets: Vec<std::collections::HashSet<usize>> = sample
                    .iter()
                    .map(|&j| neighbors[j].iter().copied().collect())
                    .collect();

                let mut edges = 0usize;
                let mut pairs = 0usize;
                for (a_idx, a_set) in sample_sets.iter().enumerate() {
                    for &b in &sample[(a_idx + 1)..] {
                        pairs += 1;
                        if a_set.contains(&b) {
                            edges += 1;
                        }
                    }
                }
                edges as f32 / pairs as f32
            };

            // ── salience = connectedness × (1 − redundancy) ──
            // Multiplicative penalty: moderate clustering → moderate reduction.
            // Only fully interconnected cliques (redundancy=1) get zeroed.
            connectedness * (1.0 - redundancy)
        })
        .collect();

    salience
}

// ── Budget ────────────────────────────────────────────────────────────────

/// HMem budget configuration for gating metadata storage.
///
/// Variant order is load-bearing: serde tries each variant in declaration
/// order on untagged input. Variants with required fields must come before
/// variants whose fields all carry `#[serde(default)]`, otherwise the
/// all-defaulted variant silently matches first and drops the required
/// field. `Absolute` (required `max_triples`) and `Flat` (required
/// `triple_budget_per_100`) come before `PerPage` (all-defaulted) so that
/// `{ max_triples: N }` selects `Absolute` and `{ total_passages, triple_budget_per_100 }`
/// selects `Flat`; `PerPage` is the fallback for `{ per_100_pages }` or empty.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum BudgetConfig {
    /// Absolute hard cap.
    Absolute { max_triples: usize },
    /// Flat config with explicit total passages and rate.
    /// Used by gentle-lovelace and similar mashup styles.
    Flat {
        /// Budget cap on passages (0 = no cap).
        #[serde(default)]
        total_passages: usize,
        /// Triples per 100 page-equivalents.
        triple_budget_per_100: usize,
    },
    /// Budget derived from passage count: `triples_per_100_pages`.
    PerPage {
        #[serde(default = "default_budget_per_100_pages")]
        per_100_pages: usize,
    },
}

fn default_budget_per_100_pages() -> usize {
    3750
}

impl Default for BudgetConfig {
    fn default() -> Self {
        BudgetConfig::PerPage {
            per_100_pages: default_budget_per_100_pages(),
        }
    }
}

impl BudgetConfig {
    /// Compute the absolute h_mem budget from the config and passage count.
    ///
    /// For `Flat`: budget = (effective_pages / 250) × triple_budget_per_100.
    /// `total_passages` caps the passage count (0 = no cap, use actual count).
    /// For `PerPage`: budget = (passage_count / 250) × per_100_pages.
    /// The constant 250 assumes ~250 passages ≈ 100 pages.
    ///
    /// expect: "The system scores passage salience to gate h_mem storage budget"
    /// \[P3\] Motivating: Generative Space — resolves passage count into absolute h_mem budget
    /// \[P9\] Constraining: Homeostatic Self-Regulation — budget caps generative storage growth
    /// pre:  passage_count ≥ 0
    /// post: returns computed absolute h_mem budget
    /// post: Flat variant caps at total_passages if set and smaller
    pub fn resolve(&self, passage_count: usize) -> usize {
        match self {
            BudgetConfig::Flat {
                total_passages,
                triple_budget_per_100,
            } => {
                let effective = if *total_passages > 0 && *total_passages < passage_count {
                    *total_passages
                } else {
                    passage_count
                };
                let pages_equivalent = (effective as f32 / 250.0).max(1.0);
                (pages_equivalent * *triple_budget_per_100 as f32).ceil() as usize
            }
            BudgetConfig::PerPage { per_100_pages } => {
                let pages_equivalent = (passage_count as f32 / 250.0).max(1.0);
                (pages_equivalent * *per_100_pages as f32).ceil() as usize
            }
            BudgetConfig::Absolute { max_triples } => *max_triples,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salience_zero_for_empty_tags() {
        let tags = vec![EntityTags::default()];
        let scores = compute_salience_batch(&tags);
        assert_eq!(scores.len(), 1);
        assert!((scores[0] - 0.0).abs() < 0.01);
    }

    #[test]
    fn salience_increases_with_shared_entities() {
        // Three passages: two share "Jake", one isolated
        let tags = vec![
            EntityTags {
                characters: vec!["Jake".into()],
                ..Default::default()
            },
            EntityTags {
                characters: vec!["Jake".into(), "Brett".into()],
                ..Default::default()
            },
            EntityTags {
                concepts: vec!["rain".into()],
                ..Default::default()
            },
        ];
        let scores = compute_salience_batch(&tags);
        assert!(scores[0] > 0.0, "passage 0 shares Jake with passage 1");
        assert!(scores[1] > 0.0, "passage 1 shares Jake with passage 0");
        assert!((scores[2] - 0.0).abs() < 0.01, "passage 2 isolated");
    }

    #[test]
    fn clustering_zero_when_neighbors_disconnected() {
        // Three passages each with a unique entity — no shared entities
        // between neighbors, so clustering coefficient = 0.
        // Salience should equal connectedness (no redundancy penalty).
        let tags = vec![
            EntityTags {
                characters: vec!["A".into()],
                ..Default::default()
            },
            EntityTags {
                characters: vec!["A".into(), "B".into()],
                ..Default::default()
            },
            EntityTags {
                characters: vec!["B".into()],
                ..Default::default()
            },
            EntityTags::default(),
        ];
        let scores = compute_salience_batch(&tags);
        // Passage 1 (bridge: shares A with 0, B with 2) should have
        // clustering=0 since 0 and 2 don't share entities.
        // Its salience should be >0 (connectedness with no penalty).
        assert!(
            scores[1] > 0.0,
            "bridge passage should have positive salience"
        );
        // Passage 0 and 2 each have one neighbor (1), so |sample|=1 → clustering=0
        assert!(scores[0] > 0.0);
        assert!(scores[2] > 0.0);
        // Passage 3 isolated
        assert!((scores[3] - 0.0).abs() < 0.01);
    }

    #[test]
    fn bridge_scores_higher_than_dense_clique() {
        // Four passages: A and B share entity X. C and D share entity Y.
        // Passage B also shares Y — making it a bridge between the two clusters.
        // Passage A is in a dense clique with B (they share X, and B shares Y with C/D
        // but A doesn't — so A's only neighbor is B, |sample|=1, clustering=0).
        //
        // Actually construct a clearer case:
        // A: shares X with B, C.  B: shares X with A, C.  C: shares X with A, B.
        // All three form a triangle → high clustering for all.
        // D: shares Y with E.  E: shares Y with D, and also X with A,B,C (bridge).
        // E connects the X-clique to D → E should score higher than clique members.
        let tags = vec![
            // A: X only (clique member)
            EntityTags {
                characters: vec!["X".into()],
                ..Default::default()
            },
            // B: X only (clique member)
            EntityTags {
                characters: vec!["X".into()],
                ..Default::default()
            },
            // C: X only (clique member)
            EntityTags {
                characters: vec!["X".into()],
                ..Default::default()
            },
            // D: Y only (peripheral)
            EntityTags {
                characters: vec!["Y".into()],
                ..Default::default()
            },
            // E: X + Y (bridge between X-clique and D)
            EntityTags {
                characters: vec!["X".into(), "Y".into()],
                ..Default::default()
            },
        ];
        let scores = compute_salience_batch(&tags);
        // E (bridge) should outscore A/B/C (clique members) because
        // E's neighbors include D (who is NOT connected to A/B/C),
        // giving E lower clustering than the pure X-clique members.
        assert!(
            scores[4] > scores[0],
            "bridge E should outscore clique member A"
        );
        assert!(scores[4] > 0.0, "bridge should have positive salience");
        // D (peripheral, one neighbor E) should have some salience
        assert!(
            scores[3] > 0.0,
            "peripheral touching bridge should have salience"
        );
    }

    #[test]
    fn methods_participate_in_graph() {
        let tags = vec![
            EntityTags {
                methods: vec!["iceberg_theory".into()],
                ..Default::default()
            },
            EntityTags {
                methods: vec!["iceberg_theory".into()],
                ..Default::default()
            },
        ];
        let scores = compute_salience_batch(&tags);
        assert!(scores[0] > 0.0);
        assert!(scores[1] > 0.0);
    }

    #[test]
    fn budget_per_page_resolve() {
        let budget = BudgetConfig::PerPage {
            per_100_pages: 3750,
        };
        // 250 passages ≈ 100 pages
        assert_eq!(budget.resolve(250), 3750);
        // 500 passages ≈ 200 pages
        assert_eq!(budget.resolve(500), 7500);
        // Tiny corpus: minimum 1 page-equivalent
        assert!(budget.resolve(10) >= 3750);
    }

    #[test]
    fn budget_absolute() {
        let budget = BudgetConfig::Absolute { max_triples: 10000 };
        assert_eq!(budget.resolve(5000), 10000);
    }

    // ── Property-based tests (Wave 2) ─────────────────────────────────────

    use proptest::prelude::*;

    /// Strategy: generate random EntityTags with controlled entity sets.
    fn arbitrary_entity_tags() -> BoxedStrategy<EntityTags> {
        prop::collection::vec(proptest::arbitrary::any::<String>(), 0..5)
            .prop_map(|chars| EntityTags {
                characters: chars.into_iter().filter(|s| !s.is_empty()).collect(),
                places: vec![],
                events: vec![],
                concepts: vec![],
                methods: vec![],
            })
            .boxed()
    }

    // All salience scores are in [0.0, 1.0] and function never panics.
    proptest! {
        #[test]
        fn salience_scores_in_valid_range(
            tags in prop::collection::vec(arbitrary_entity_tags(), 0..20),
        ) {
            let result = std::panic::catch_unwind(|| {
                compute_salience_batch(&tags)
            });
            prop_assert!(result.is_ok(), "compute_salience_batch panicked");
            let scores = result.unwrap();
            for (i, score) in scores.iter().enumerate() {
                prop_assert!(*score >= 0.0 && *score <= 1.0,
                    "score[{}] = {} out of [0.0, 1.0] range", i, score);
            }
        }
    }

    // Passages with no entity tags always score zero.
    proptest! {
        #[test]
        fn empty_tags_produce_zero_salience(
            mut tags in prop::collection::vec(arbitrary_entity_tags(), 1..10),
        ) {
            // Add an empty-tag passage
            tags.push(EntityTags::default());
            let scores = compute_salience_batch(&tags);
            let last = scores.last().unwrap();
            prop_assert_eq!(*last, 0.0f32,
                "empty-tag passage should score 0.0, got {}", last);
        }
    }
}
