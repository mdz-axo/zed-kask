//! Property layer for the event-tree marginalization math
//! (`kask/docs/reference/testing-protocol.md` layer 2).
//!
//! The spec is the documented marginalization contract
//! (`src/superforecast/math.rs`, surfaced verbatim in the `scenario_quantify`
//! framework note): per-group full joint marginalization **under parent
//! independence**, noisy-OR combination across disjoint dependency groups,
//! and an all-parents-true joint product. Each property states a falsifiable
//! hypothesis with its declared input domain; a shrunk counterexample is a
//! finding to report, never a signal to weaken the property.
//!
//! Two oracles, deliberately different computational paths from the
//! implementation:
//!
//! 1. **Global joint enumeration** — the CPTs induce a full joint
//!    distribution over all 2^n truth assignments
//!    (P(assignment) = Π_i P_i(x_i | parents(x_i))); marginalizing the
//!    enumerated table is the "full joint-table marginalization" the tool
//!    advertises. The server marginalizes each group over its parents'
//!    *marginals*, which is exact **only when the parent-independence
//!    assumption holds** — i.e. every node's parents have disjoint
//!    ancestries. The first run of the enumeration property produced a
//!    genuine counterexample where two parents of one node were correlated
//!    (one an ancestor of the other): the server returned the documented
//!    independence approximation, the enumeration returned the true joint.
//!    That approximation is the spec (`types.rs`: "Parent probabilities are
//!    assumed independent during marginalization"), so the enumeration
//!    oracle's domain is constrained to disjoint-ancestry parent sets, and
//!    the approximation itself is pinned by a hand-built example test below.
//! 2. **The documented noisy-OR rule, re-implemented here** — for arbitrary
//!    trees (correlated parents included), the oracle restates the
//!    published per-group formula and noisy-OR combination independently of
//!    the implementation.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use proptest::prelude::*;

use crate::superforecast::build_event_tree;
use crate::types::{EventDependency, ScenarioEvent, ScenarioType, TimeHorizon};

// ── Generator ───────────────────────────────────────────────────────────────

/// A hand-usable event constructor with the non-structural fields pinned.
fn event(id: &str, probability: f64, depends_on: Vec<EventDependency>) -> ScenarioEvent {
    ScenarioEvent {
        id: id.to_string(),
        name: format!("Event {id}"),
        question: format!("Does event {id} occur?"),
        deadline: NaiveDate::from_ymd_opt(2027, 12, 31).expect("valid static date"),
        time_horizon: TimeHorizon::Strategic,
        scenario_type: ScenarioType::CompanyAnalysis,
        subject: "property-tests".to_string(),
        probability,
        basis: None,
        depends_on,
        sub_questions: Vec::new(),
        base_rate: None,
        reference_class: None,
        brier_score: None,
        update_count: 0,
    }
}

/// Deterministically carve a stream of (probability, structure-byte) pairs
/// into a valid event tree: `n_events` events, each event's parents drawn
/// from events with a strictly lower index (acyclic by construction, so
/// index order is a valid topological order), disjoint across groups
/// (validation rejects overlap), conditionals sized 2^k. The stream wraps
/// when exhausted — repeated draws stay valid, only less varied.
///
/// `single_group` limits every dependent event to one dependency group
/// (the global-enumeration domain). `independent_parents` additionally
/// enforces that each event's parents have pairwise-disjoint ancestries —
/// the condition under which the documented parent-independence
/// marginalization is exact (see the module doc).
fn carve_tree(
    n_events: usize,
    stream: &[(f64, u8)],
    single_group: bool,
    independent_parents: bool,
) -> Vec<ScenarioEvent> {
    let mut cursor = 0usize;
    let take = |stream: &[(f64, u8)], cursor: &mut usize| -> (f64, u8) {
        let pair = stream[*cursor % stream.len()];
        *cursor += 1;
        pair
    };

    let mut events: Vec<ScenarioEvent> = Vec::with_capacity(n_events);
    // ancestry[i] = {i} ∪ ancestors of i — used to enforce the
    // parent-independence precondition when requested.
    let mut ancestry: Vec<HashSet<usize>> = Vec::with_capacity(n_events);

    for i in 0..n_events {
        let (probability, structure) = take(stream, &mut cursor);
        let mut depends_on: Vec<EventDependency> = Vec::new();
        let mut used_parents: HashSet<usize> = HashSet::new();

        // Event 0 is always a root; later events are roots when the structure
        // byte says so, else they carry 1-2 disjoint dependency groups.
        let group_count = if single_group {
            1
        } else {
            1 + (structure % 2) as usize
        };
        if i > 0 && structure % 4 != 0 {
            for _ in 0..group_count {
                let (_, group_structure) = take(stream, &mut cursor);
                let wanted = 1 + ((group_structure >> 1) % 2) as usize; // 1-2 parents
                let mut parents: Vec<usize> = Vec::new();
                let mut chosen_ancestry: HashSet<usize> = HashSet::new();
                let mut candidate = (group_structure as usize) % i;
                let mut tries = 0;
                while parents.len() < wanted && tries < i {
                    tries += 1;
                    let inadmissible = used_parents.contains(&candidate)
                        || (independent_parents
                            && !ancestry[candidate].is_disjoint(&chosen_ancestry));
                    if !inadmissible {
                        parents.push(candidate);
                        used_parents.insert(candidate);
                        chosen_ancestry.extend(ancestry[candidate].iter().copied());
                    }
                    candidate = (candidate + 3) % i;
                }
                if parents.is_empty() {
                    break; // no admissible parent left; no room for this group
                }
                let table_len = 1usize << parents.len();
                let mut conditionals = Vec::with_capacity(table_len);
                for _ in 0..table_len {
                    let (conditional, _) = take(stream, &mut cursor);
                    conditionals.push(conditional);
                }
                depends_on.push(EventDependency {
                    parent_event_ids: parents.iter().map(|&p| format!("e{p}")).collect(),
                    conditionals,
                });
            }
        }

        let mut own = HashSet::from([i]);
        for dep in &depends_on {
            for parent_id in &dep.parent_event_ids {
                let parent_index: usize = parent_id
                    .trim_start_matches('e')
                    .parse()
                    .expect("generated parent id");
                own.extend(ancestry[parent_index].iter().copied());
            }
        }
        ancestry.push(own);

        events.push(event(&format!("e{i}"), probability, depends_on));
    }
    events
}

// ── Oracle 1: global joint enumeration ─────────────────────────────────────

/// Marginalize the enumerated global joint distribution of a single-group
/// tree. `P(assignment) = Π_i P_i(x_i | parents(x_i))` with roots
/// independent. Returns (per-event marginals in event order,
/// P(all events true)). Exact for any tree; equal to the server's
/// per-node-marginal computation only when parent independence holds
/// (disjoint-ancestry parent sets — enforced by the generator for the
/// property that uses this oracle).
fn enumerate_joint(events: &[ScenarioEvent]) -> (Vec<f64>, f64) {
    let n = events.len();
    let index: HashMap<&str, usize> = events
        .iter()
        .enumerate()
        .map(|(i, event)| (event.id.as_str(), i))
        .collect();
    let mut marginals = vec![0.0f64; n];
    let mut joint_all_true = 0.0f64;

    for state in 0..(1usize << n) {
        let mut state_probability = 1.0f64;
        for (i, event) in events.iter().enumerate() {
            let occurs = (state >> i) & 1 == 1;
            let p_true = if event.depends_on.is_empty() {
                event.probability
            } else {
                // Single-group domain: exactly one dependency per event.
                let dep = &event.depends_on[0];
                let mut assignment = 0usize;
                for (j, parent_id) in dep.parent_event_ids.iter().enumerate() {
                    let parent_pos = *index
                        .get(parent_id.as_str())
                        .expect("generated parents exist");
                    if (state >> parent_pos) & 1 == 1 {
                        assignment |= 1 << j;
                    }
                }
                dep.conditionals[assignment]
            };
            state_probability *= if occurs { p_true } else { 1.0 - p_true };
        }
        for i in 0..n {
            if (state >> i) & 1 == 1 {
                marginals[i] += state_probability;
            }
        }
        if state == (1usize << n) - 1 {
            joint_all_true = state_probability;
        }
    }
    (marginals, joint_all_true)
}

// ── Oracle 2: the documented noisy-OR rule, re-implemented ─────────────────

/// The published marginalization rule stated independently of the
/// implementation: per group, P_g(E) = Σ_a P(E|a) · Π_i P(p_i)^{a_i} ·
/// (1−P(p_i))^{1−a_i} over the group's parents' resolved marginals (bit j
/// of a ↔ parent j); groups combined by noisy-OR
/// P(E) = 1 − Π_g (1 − P_g(E)); joint = Π per-event factor (root marginal,
/// else noisy-OR of conditionals[last] = P_g(E | all parents true)).
/// Event-index order is a valid topological order by construction.
fn documented_rule(events: &[ScenarioEvent]) -> (HashMap<String, f64>, f64) {
    let mut resolved: HashMap<String, f64> = HashMap::new();
    for event in events {
        if event.depends_on.is_empty() {
            resolved.insert(event.id.clone(), event.probability);
            continue;
        }
        let mut survival = 1.0f64;
        for dep in &event.depends_on {
            let parent_probs: Vec<f64> = dep
                .parent_event_ids
                .iter()
                .map(|pid| {
                    *resolved
                        .get(pid)
                        .expect("parents resolved first by index order")
                })
                .collect();
            let mut group_marginal = 0.0f64;
            for assignment in 0..(1usize << dep.parent_event_ids.len()) {
                let mut weight = 1.0f64;
                for (j, &parent_prob) in parent_probs.iter().enumerate() {
                    weight *= if (assignment >> j) & 1 == 1 {
                        parent_prob
                    } else {
                        1.0 - parent_prob
                    };
                }
                group_marginal += dep.conditionals[assignment] * weight;
            }
            survival *= 1.0 - group_marginal;
        }
        resolved.insert(event.id.clone(), (1.0 - survival).clamp(0.0, 1.0));
    }

    let mut joint = 1.0f64;
    for event in events {
        let factor = if event.depends_on.is_empty() {
            *resolved.get(&event.id).expect("resolved above")
        } else {
            let mut survival = 1.0f64;
            for dep in &event.depends_on {
                let all_true = *dep.conditionals.last().expect("2^k table is nonempty");
                survival *= 1.0 - all_true;
            }
            1.0 - survival
        };
        joint *= factor;
    }
    (resolved, joint)
}

/// Server-resolved marginals keyed by event id, from a built tree.
fn server_marginals(tree: &crate::types::EventTree) -> HashMap<&str, f64> {
    tree.nodes
        .iter()
        .map(|node| (node.event.id.as_str(), node.marginal_probability))
        .collect()
}

/// Example pin (hand-built, not generated): when a node's parents are
/// correlated, the server keeps the documented parent-independence
/// approximation — marginalizing over the parents' *marginals* — rather
/// than the true joint. Here e1 ≡ e0 (conditionals 0, 1), so e2's AND-gate
/// over {e0, e1} returns 0.5·0.5 = 0.25 under independence, not the true
/// joint value 0.5 (both parents true in exactly the states e0 true, which
/// have probability 0.5). Found by this property suite's enumeration
/// oracle on its first run (2026-09-18); pinned so the approximation stays
/// intentional and visible, not "fixed" silently later.
#[test]
fn correlated_parents_keep_the_documented_independence_approximation() {
    let events = vec![
        event("e0", 0.5, vec![]),
        event(
            "e1",
            0.0,
            vec![EventDependency {
                parent_event_ids: vec!["e0".to_string()],
                conditionals: vec![0.0, 1.0], // e1 ≡ e0
            }],
        ),
        event(
            "e2",
            0.0,
            vec![EventDependency {
                parent_event_ids: vec!["e0".to_string(), "e1".to_string()],
                conditionals: vec![0.0, 0.0, 0.0, 1.0], // AND gate
            }],
        ),
    ];
    let tree = build_event_tree(&events).expect("hand-built tree is valid");
    let server = server_marginals(&tree);
    let e2 = *server.get("e2").expect("e2 resolves");

    assert!(
        (e2 - 0.25).abs() < 1e-12,
        "independence value is 0.25, got {e2}"
    );
    assert!(
        (e2 - 0.5).abs() > 0.1,
        "the true-joint value 0.5 must NOT appear: got {e2}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Hypothesis: for single-group trees whose parent sets have disjoint
    /// ancestries (the parent-independence precondition), the server's
    /// per-node marginals and all-events joint equal the brute-force
    /// marginalization of the enumerated global joint distribution
    /// (2^n states) — under that precondition, marginalizing over parent
    /// marginals is exact, so per-node propagation must reproduce "full
    /// joint-table marginalization." Domain: 2-7 events, 1-2 parents per
    /// group, probabilities and conditionals in [0,1], parents strictly
    /// lower-indexed (acyclic), disjoint-ancestry parent sets.
    #[test]
    fn independent_parent_trees_match_global_joint_enumeration(
        n_events in 2usize..=7,
        stream in prop::collection::vec((0.0..=1.0f64, any::<u8>()), 8..=64),
    ) {
        let events = carve_tree(n_events, &stream, true, true);
        let tree = build_event_tree(&events).expect("generated single-group tree is valid");
        let (oracle_marginals, oracle_joint) = enumerate_joint(&events);
        let server = server_marginals(&tree);

        for (i, event) in events.iter().enumerate() {
            let got = *server.get(event.id.as_str()).expect("every event resolves to a node");
            prop_assert!(
                (got - oracle_marginals[i]).abs() < 1e-9,
                "event {} marginal {} != enumerated {}",
                event.id, got, oracle_marginals[i]
            );
        }
        prop_assert!((tree.joint_probability - oracle_joint).abs() < 1e-12);
    }

    /// Hypothesis: for multi-group trees (correlated parents included), the
    /// server's marginals and joint match the documented noisy-OR rule
    /// re-implemented independently — disjoint groups are separate channels
    /// combined per the published formula, and the joint factor per
    /// dependent event is the noisy-OR of the all-parents-true
    /// conditionals. Domain: as above, with 1-2 disjoint dependency groups
    /// per event, ancestries unconstrained.
    #[test]
    fn multi_group_marginals_match_documented_noisy_or_rule(
        n_events in 2usize..=7,
        stream in prop::collection::vec((0.0..=1.0f64, any::<u8>()), 8..=64),
    ) {
        let events = carve_tree(n_events, &stream, false, false);
        let tree = build_event_tree(&events).expect("generated multi-group tree is valid");
        let (oracle_marginals, oracle_joint) = documented_rule(&events);
        let server = server_marginals(&tree);

        for event in &events {
            let got = *server.get(event.id.as_str()).expect("every event resolves to a node");
            let expected = *oracle_marginals.get(&event.id).expect("oracle covers every event");
            prop_assert!(
                (got - expected).abs() < 1e-12,
                "event {} marginal {} != documented rule {}",
                event.id, got, expected
            );
        }
        prop_assert!(
            (tree.joint_probability - oracle_joint).abs() < 1e-12,
            "joint {} != documented rule {}",
            tree.joint_probability, oracle_joint
        );
    }

    /// Hypothesis (invariant): input order is incidental — reversing the
    /// event list yields identical per-event marginals and joint. The
    /// topological sort must absorb input order; only f64 product reordering
    /// may differ. Domain: the unconstrained multi-group generator.
    #[test]
    fn marginals_are_invariant_under_input_permutation(
        n_events in 2usize..=7,
        stream in prop::collection::vec((0.0..=1.0f64, any::<u8>()), 8..=64),
    ) {
        let events = carve_tree(n_events, &stream, false, false);
        let forward = build_event_tree(&events).expect("valid forward");
        let mut reversed = events.clone();
        reversed.reverse();
        let backward = build_event_tree(&reversed).expect("valid reversed");

        let forward_marginals = server_marginals(&forward);
        let backward_marginals = server_marginals(&backward);
        for event in &events {
            let a = *forward_marginals.get(event.id.as_str()).expect("resolves forward");
            let b = *backward_marginals.get(event.id.as_str()).expect("resolves backward");
            prop_assert!((a - b).abs() < 1e-9, "event {} moved {}", event.id, (a - b).abs());
        }
        prop_assert!((forward.joint_probability - backward.joint_probability).abs() < 1e-9);
    }

    /// Hypothesis (invariant): every resolved marginal and the joint stay
    /// finite and inside [0, 1] for any valid generated tree, and root
    /// nodes pass their intrinsic probability through unchanged.
    #[test]
    fn marginals_and_joint_stay_bounded(
        n_events in 2usize..=7,
        stream in prop::collection::vec((0.0..=1.0f64, any::<u8>()), 8..=64),
    ) {
        let events = carve_tree(n_events, &stream, false, false);
        let tree = build_event_tree(&events).expect("generated tree is valid");
        let server = server_marginals(&tree);

        for event in &events {
            let marginal = *server.get(event.id.as_str()).expect("every event resolves");
            prop_assert!(marginal.is_finite());
            prop_assert!((0.0..=1.0).contains(&marginal));
            if event.depends_on.is_empty() {
                prop_assert!((marginal - event.probability).abs() < 1e-12);
            }
        }
        prop_assert!(tree.joint_probability.is_finite());
        prop_assert!((0.0..=1.0).contains(&tree.joint_probability));
    }
}
