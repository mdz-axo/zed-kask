//! Evidence scoring — the deterministic model behind `evaluate_evidence`.
//!
//! G3 contract (inherited from the tool): no LLM relay — every number in
//! this module is pure arithmetic over the artifact fields. The scoring
//! model IS the weight table (`DEFAULT_PROFILE`): no weight literal lives
//! in a code path.
//!
//! Corroboration is duplicate-aware (the substrate's lesson):
//! distinct-domain counting cannot see syndication — one press release on
//! five domains scored as five corroborations. Content-bearing artifacts
//! are clustered by 4-word shingle similarity (Jaccard ≥
//! `SHINGLE_JACCARD_THRESHOLD`, the corpus QA-grounding granularity);
//! corroboration counts clusters, not domains. Content-less artifacts
//! fall back to domain counting, and the signal basis says so — never
//! silently.

use crate::research::types::EvaluateArtifact;

/// The components of the evidence score. Exhaustive by design: the weight
/// table and the per-artifact signal trail are keyed by this enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EvidenceComponent {
    Base,
    Corroboration,
    Recency,
    Content,
}

impl EvidenceComponent {
    /// Stable wire name for the signal trail.
    pub(crate) fn name(self) -> &'static str {
        match self {
            EvidenceComponent::Base => "base",
            EvidenceComponent::Corroboration => "corroboration",
            EvidenceComponent::Recency => "recency",
            EvidenceComponent::Content => "content",
        }
    }
}

/// The scoring model IS this table. No weight literal lives in a code path.
/// Invariant (unit-test-enforced): weights sum to 1.0; all ≥ 0; each
/// component appears exactly once.
pub(crate) const DEFAULT_PROFILE: [(EvidenceComponent, f64); 4] = [
    (EvidenceComponent::Base, 0.3),
    (EvidenceComponent::Corroboration, 0.3),
    (EvidenceComponent::Recency, 0.2),
    (EvidenceComponent::Content, 0.2),
];

/// Named substitution profiles for the sensitivity report. Private: only
/// the sensitivity computation consumes them — the tool scores with
/// `DEFAULT_PROFILE`. Each sums to 1.0 by construction (unit-test-enforced
/// alongside `DEFAULT_PROFILE`).
const CORROBORATION_HEAVY_PROFILE: [(EvidenceComponent, f64); 4] = [
    (EvidenceComponent::Base, 0.3),
    (EvidenceComponent::Corroboration, 0.4),
    (EvidenceComponent::Recency, 0.1),
    (EvidenceComponent::Content, 0.2),
];
const RECENCY_HEAVY_PROFILE: [(EvidenceComponent, f64); 4] = [
    (EvidenceComponent::Base, 0.3),
    (EvidenceComponent::Corroboration, 0.1),
    (EvidenceComponent::Recency, 0.4),
    (EvidenceComponent::Content, 0.2),
];

/// 4-word shingle Jaccard threshold at which two content-bearing artifacts
/// are the same story (syndication), not independent evidence. The
/// 4-word-sequence granularity is the corpus QA-grounding precedent; the
/// threshold is pinned under unit test.
pub(crate) const SHINGLE_JACCARD_THRESHOLD: f64 = 0.5;
const SHINGLE_SIZE: usize = 4;

/// One component's contribution to one artifact's score. `basis` is the
/// why-string: it states how the earned value was determined, so a reader
/// never has to reverse-engineer the arithmetic.
pub(crate) struct ComponentSignal {
    pub(crate) component: EvidenceComponent,
    pub(crate) earned: f64,
    pub(crate) basis: String,
}

/// Per-artifact deterministic evidence scores under one weight profile.
pub(crate) struct ArtifactScore {
    pub(crate) confidence: f64,
    pub(crate) signals: Vec<ComponentSignal>,
    /// Independent evidence units in the set (0 for an unsourced artifact —
    /// no source, no corroboration).
    pub(crate) corroboration_count: usize,
    pub(crate) published_age_days: Option<i64>,
}

/// Sensitivity of the artifact ordering to the weight model, measured by
/// substituting the named profiles and comparing orderings. `NotEvaluable`
/// is surfaced, never silently reported as stable (degradation contract).
#[derive(Debug)]
pub(crate) enum SensitivityStatus {
    /// The confidence ordering is identical under every named profile.
    Stable { profiles_evaluated: usize },
    /// A named profile's substitution changed the ordering; `driver` is
    /// the component that profile emphasizes.
    Unstable { driver: EvidenceComponent },
    /// Fewer than 2 artifacts, or every artifact scores equally under
    /// every profile — no ordering whose stability would mean anything.
    NotEvaluable { reason: String },
}

/// One content cluster: content-bearing artifacts whose 4-word shingles
/// are similar enough (Jaccard ≥ `SHINGLE_JACCARD_THRESHOLD`, transitively
/// closed) that they are one story, not independent corroboration — a
/// syndicated story is visible here, not merely discounted. `domains` are
/// the distinct source domains of the cluster's sourced members;
/// `artifact_urls` lists every member.
pub(crate) struct ContentCluster {
    pub(crate) domains: Vec<String>,
    pub(crate) artifact_urls: Vec<String>,
}

/// Set-level report for one `evaluate_evidence` call.
pub(crate) struct EvidenceReport {
    pub(crate) artifacts: Vec<ArtifactScore>,
    /// Distinct source domains across the set — the raw domain signal,
    /// kept visible alongside the cluster count.
    pub(crate) distinct_domains: usize,
    /// Artifacts that carry a source at all.
    pub(crate) sourced_count: usize,
    /// Content clusters among the content-bearing artifacts.
    pub(crate) content_clusters: Vec<ContentCluster>,
    pub(crate) sensitivity: SensitivityStatus,
    /// How duplication was detected: "shingles" (deterministic Tier 1,
    /// always on). Commit 6 may upgrade this to "semantic" when the
    /// embedding tier is requested and available.
    pub(crate) duplication_mode: &'static str,
}

/// Weight of one component in a profile. Total for the named profiles
/// (completeness is unit-test-enforced); the 0.0 arm is unreachable for a
/// valid table.
fn profile_weight(profile: &[(EvidenceComponent, f64); 4], component: EvidenceComponent) -> f64 {
    profile
        .iter()
        .find(|(candidate, _)| *candidate == component)
        .map(|(_, weight)| *weight)
        .unwrap_or(0.0)
}

/// Profile-independent per-artifact facts. Precomputed once so the scoring
/// profile and every sensitivity profile share one arithmetic path.
struct RawAchievements {
    sourced: bool,
    /// 1.0 fresh (≤365 days), 0.5 stale, 0.0 missing or unparseable.
    recency_multiplier: f64,
    published_age_days: Option<i64>,
    /// Recency basis — profile-independent (only the earned weight varies).
    recency_basis: String,
    content: bool,
}

fn raw_achievements(artifact: &EvaluateArtifact) -> RawAchievements {
    let (multiplier, age, basis) = recency_parts(artifact.published.as_deref());
    RawAchievements {
        sourced: artifact.source.is_some(),
        recency_multiplier: multiplier,
        published_age_days: age,
        recency_basis: basis,
        content: artifact.content.is_some(),
    }
}

/// Recency multiplier, parsed age, and basis from an optional published
/// date. Fresh (≤365 days) earns the full weight; older earns half (a
/// verifiable date still beats none); missing or unparseable earns 0 —
/// and the basis says which, so no dual-meaning field survives.
fn recency_parts(published: Option<&str>) -> (f64, Option<i64>, String) {
    let Some(published) = published else {
        return (0.0, None, "no published field".to_string());
    };
    match parse_age_days(published) {
        Some(age) if age <= 365 => (
            1.0,
            Some(age),
            format!("published {age} days ago (within 365 days)"),
        ),
        Some(age) => (
            0.5,
            Some(age),
            format!("published {age} days ago (older than 365 days — half weight)"),
        ),
        None => (
            0.0,
            None,
            "published field present but unparseable as RFC 3339 or YYYY-MM-DD".to_string(),
        ),
    }
}

/// Age in days of a published-date string (RFC 3339 or `YYYY-MM-DD`),
/// relative to now. None when neither format parses — an unparseable date
/// carries no recency signal.
fn parse_age_days(published: &str) -> Option<i64> {
    let now = chrono::Utc::now();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(published) {
        return Some((now - dt.with_timezone(&chrono::Utc)).num_days());
    }
    let date = chrono::NaiveDate::parse_from_str(published, "%Y-%m-%d").ok()?;
    Some((now.date_naive() - date).num_days())
}

/// The corroboration basis string: how the set's units were counted for
/// this artifact.
fn corroboration_basis(raw: &RawAchievements, units: usize) -> String {
    if !raw.sourced {
        return "no source — no corroboration".to_string();
    }
    if raw.content {
        format!("{units} independent evidence units (content-clustered)")
    } else {
        format!(
            "{units} independent evidence units (domain-counted — no content, duplication not checkable)"
        )
    }
}

/// The per-artifact signal trail under one profile. Confidence is the sum
/// of the earned values, capped at 1.0 — the signals are the single source
/// of truth for the composite.
fn artifact_signals(
    raw: &RawAchievements,
    corroboration_count: usize,
    profile: &[(EvidenceComponent, f64); 4],
) -> Vec<ComponentSignal> {
    let corroboration_multiplier = (corroboration_count.min(3) as f64) / 3.0;
    vec![
        ComponentSignal {
            component: EvidenceComponent::Base,
            earned: profile_weight(profile, EvidenceComponent::Base),
            basis: "constant base weight".to_string(),
        },
        ComponentSignal {
            component: EvidenceComponent::Corroboration,
            earned: corroboration_multiplier
                * profile_weight(profile, EvidenceComponent::Corroboration),
            basis: corroboration_basis(raw, corroboration_count),
        },
        ComponentSignal {
            component: EvidenceComponent::Recency,
            earned: raw.recency_multiplier * profile_weight(profile, EvidenceComponent::Recency),
            basis: raw.recency_basis.clone(),
        },
        ComponentSignal {
            component: EvidenceComponent::Content,
            earned: if raw.content {
                profile_weight(profile, EvidenceComponent::Content)
            } else {
                0.0
            },
            basis: if raw.content {
                "content present".to_string()
            } else {
                "no content field".to_string()
            },
        },
    ]
}

/// Confidence of one artifact under one profile — the signal earnings
/// summed and capped. Sensitivity reuses this so the substituted profiles
/// and the scored profile share one arithmetic path.
fn confidence_of(
    raw: &RawAchievements,
    corroboration_count: usize,
    profile: &[(EvidenceComponent, f64); 4],
) -> f64 {
    artifact_signals(raw, corroboration_count, profile)
        .iter()
        .map(|signal| signal.earned)
        .sum::<f64>()
        .min(1.0)
}

/// Score an evidence set deterministically (not an LLM relay — G3 contract).
///
/// Corroboration counts independent evidence units, not raw domains:
/// content-bearing artifacts are clustered by shingle similarity (one
/// syndicated story = one unit), content-less sourced artifacts contribute
/// their distinct domains. The composite cannot saturate on a single
/// artifact: one fresh complete single-unit artifact scores 0.8.
pub(crate) fn score_evidence_set(
    artifacts: &[EvaluateArtifact],
    profile: &[(EvidenceComponent, f64); 4],
) -> EvidenceReport {
    let distinct_domains: usize = artifacts
        .iter()
        .filter_map(|a| a.source.as_deref())
        .collect::<std::collections::HashSet<_>>()
        .len();
    let sourced_count: usize = artifacts.iter().filter(|a| a.source.is_some()).count();

    let clusters = cluster_content(artifacts);
    let units = corroboration_units(artifacts, &clusters);

    let raws: Vec<RawAchievements> = artifacts.iter().map(raw_achievements).collect();

    let scored = raws
        .iter()
        .map(|raw| {
            let corroboration_count = corroboration_count_of(raw, units);
            let signals = artifact_signals(raw, corroboration_count, profile);
            let confidence = signals
                .iter()
                .map(|signal| signal.earned)
                .sum::<f64>()
                .min(1.0);
            ArtifactScore {
                confidence,
                signals,
                corroboration_count,
                published_age_days: raw.published_age_days,
            }
        })
        .collect();

    EvidenceReport {
        artifacts: scored,
        distinct_domains,
        sourced_count,
        content_clusters: content_clusters_of(artifacts, &clusters),
        sensitivity: sensitivity_status(&raws, units),
        duplication_mode: "shingles",
    }
}

// ── Tier-1 duplication detection (deterministic shingles) ──

/// The 4-word shingles of a content string, lowercased and
/// whitespace-normalized (the corpus QA-grounding granularity). Content
/// shorter than one shingle has an empty set — it cannot be compared.
fn shingle_set(content: &str) -> std::collections::HashSet<String> {
    let lowered = content.to_lowercase();
    let words: Vec<&str> = lowered.split_whitespace().collect();
    if words.len() < SHINGLE_SIZE {
        return std::collections::HashSet::new();
    }
    words
        .windows(SHINGLE_SIZE)
        .map(|window| window.join(" "))
        .collect()
}

/// Jaccard similarity of two shingle sets: |∩| / |∪|. An empty union
/// (both contents shorter than one shingle) is an explicit 0.0 — the
/// 0/0 division would be NaN, and `NaN >= threshold` is silently false.
fn shingle_jaccard(
    left: &std::collections::HashSet<String>,
    right: &std::collections::HashSet<String>,
) -> f64 {
    let union_size = left.union(right).count();
    if union_size == 0 {
        return 0.0;
    }
    left.intersection(right).count() as f64 / union_size as f64
}

/// Cluster the content-bearing artifacts by shingle similarity (Jaccard
/// ≥ `SHINGLE_JACCARD_THRESHOLD`), transitively closed — syndication chains
/// through intermediates. Returns groups of artifact indices, ordered by
/// first member.
fn cluster_content(artifacts: &[EvaluateArtifact]) -> Vec<Vec<usize>> {
    let content_indices: Vec<usize> = artifacts
        .iter()
        .enumerate()
        .filter(|(_, artifact)| artifact.content.is_some())
        .map(|(index, _)| index)
        .collect();
    let shingles: Vec<std::collections::HashSet<String>> = content_indices
        .iter()
        .map(|&index| shingle_set(artifacts[index].content.as_deref().unwrap_or_default()))
        .collect();

    // Union-find over positions in `content_indices`, path-halving find.
    let mut parent: Vec<usize> = (0..content_indices.len()).collect();
    fn find(parent: &mut [usize], mut node: usize) -> usize {
        while parent[node] != node {
            let grandparent = parent[parent[node]];
            parent[node] = grandparent;
            node = grandparent;
        }
        node
    }
    for left in 0..content_indices.len() {
        for right in (left + 1)..content_indices.len() {
            if shingle_jaccard(&shingles[left], &shingles[right]) >= SHINGLE_JACCARD_THRESHOLD {
                let root_left = find(&mut parent, left);
                let root_right = find(&mut parent, right);
                if root_left != root_right {
                    parent[root_right] = root_left;
                }
            }
        }
    }

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group_of_root: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for position in 0..content_indices.len() {
        let root = find(&mut parent, position);
        let group_index = *group_of_root.entry(root).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group_index].push(content_indices[position]);
    }
    // Members are pushed in ascending artifact-index order, so each
    // group's first element is its smallest index — sort clusters by it.
    groups.sort_by_key(|group| group[0]);
    groups
}

/// Independent evidence units in the set: content clusters containing at
/// least one sourced artifact (an unsourced paste is not evidence), plus
/// the distinct domains of content-less sourced artifacts whose domain is
/// not already carried by a cluster (same publisher, no new unit).
fn corroboration_units(artifacts: &[EvaluateArtifact], clusters: &[Vec<usize>]) -> usize {
    let mut cluster_domains: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut units = 0usize;
    for group in clusters {
        let has_sourced = group.iter().any(|&index| artifacts[index].source.is_some());
        if has_sourced {
            units += 1;
        }
        for &index in group {
            if let Some(domain) = artifacts[index].source.as_deref() {
                cluster_domains.insert(domain);
            }
        }
    }
    let mut uncovered_domains: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for artifact in artifacts {
        if artifact.content.is_none()
            && let Some(domain) = artifact.source.as_deref()
            && !cluster_domains.contains(domain)
        {
            uncovered_domains.insert(domain);
        }
    }
    units + uncovered_domains.len()
}

/// The wire form of the clusters: distinct sorted domains of the sourced
/// members, and every member's URL.
fn content_clusters_of(
    artifacts: &[EvaluateArtifact],
    clusters: &[Vec<usize>],
) -> Vec<ContentCluster> {
    clusters
        .iter()
        .map(|group| {
            let domains: Vec<String> = group
                .iter()
                .filter_map(|&index| artifacts[index].source.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let artifact_urls: Vec<String> = group
                .iter()
                .map(|&index| artifacts[index].url.clone())
                .collect();
            ContentCluster {
                domains,
                artifact_urls,
            }
        })
        .collect()
}

// ── Sensitivity (named-profile substitution) ──

/// A sourced artifact corroborates with the set's units; an unsourced one
/// does not corroborate at all.
fn corroboration_count_of(raw: &RawAchievements, units: usize) -> usize {
    if raw.sourced { units } else { 0 }
}

/// Sensitivity of the confidence ordering to the weight model: substitute
/// the named profiles and compare orderings (stable sort by confidence,
/// ties by input position). The first substituting profile that changes
/// the ordering names the driver; if none does, the ordering is Stable. A
/// set whose artifacts all score equally under every profile has no
/// ordering whose stability would mean anything — NotEvaluable, never a
/// fabricated Stable.
fn sensitivity_status(raws: &[RawAchievements], units: usize) -> SensitivityStatus {
    if raws.len() < 2 {
        return SensitivityStatus::NotEvaluable {
            reason: "fewer than 2 artifacts — no ordering to compare".to_string(),
        };
    }
    let default_confidences: Vec<f64> = raws
        .iter()
        .map(|raw| confidence_of(raw, corroboration_count_of(raw, units), &DEFAULT_PROFILE))
        .collect();
    let corroboration_heavy: Vec<f64> = raws
        .iter()
        .map(|raw| {
            confidence_of(
                raw,
                corroboration_count_of(raw, units),
                &CORROBORATION_HEAVY_PROFILE,
            )
        })
        .collect();
    let recency_heavy: Vec<f64> = raws
        .iter()
        .map(|raw| {
            confidence_of(
                raw,
                corroboration_count_of(raw, units),
                &RECENCY_HEAVY_PROFILE,
            )
        })
        .collect();

    let all_equal = |confidences: &[f64]| {
        confidences
            .iter()
            .all(|value| (value - confidences[0]).abs() < 1e-9)
    };
    if all_equal(&default_confidences)
        && all_equal(&corroboration_heavy)
        && all_equal(&recency_heavy)
    {
        return SensitivityStatus::NotEvaluable {
            reason: "all artifacts score equally under every profile — no ordering to compare"
                .to_string(),
        };
    }

    let default_order = ordering(&default_confidences);
    if ordering(&corroboration_heavy) != default_order {
        return SensitivityStatus::Unstable {
            driver: EvidenceComponent::Corroboration,
        };
    }
    if ordering(&recency_heavy) != default_order {
        return SensitivityStatus::Unstable {
            driver: EvidenceComponent::Recency,
        };
    }
    SensitivityStatus::Stable {
        profiles_evaluated: 3,
    }
}

/// Artifact indices ordered by confidence (descending), ties by input
/// position — a stable ordering, so a tie under one profile is comparable
/// to a strict order under another.
fn ordering(confidences: &[f64]) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..confidences.len()).collect();
    indices.sort_by(|&left, &right| {
        confidences[right]
            .partial_cmp(&confidences[left])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    indices
}

#[cfg(test)]
mod evidence_scoring_tests {
    use super::*;
    use crate::research::types::EvaluateArtifact;

    fn artifact(
        url: &str,
        source: Option<&str>,
        published: Option<&str>,
        content: Option<&str>,
    ) -> EvaluateArtifact {
        EvaluateArtifact {
            url: url.to_string(),
            title: Some("title".to_string()),
            source: source.map(str::to_string),
            published: published.map(str::to_string),
            content: content.map(str::to_string),
        }
    }

    fn today() -> String {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    }

    #[test]
    fn same_domain_duplicates_are_not_independent_corroboration() {
        // Carried from the pre-redesign suite: two same-domain duplicates
        // with identical content are one story — one unit, confidence 0.6
        // (base 0.3 + one unit 0.1 + content 0.2, no date). The redesign
        // additionally makes the duplicate pair VISIBLE as one cluster.
        let arts = vec![
            artifact(
                "https://same.example/1",
                Some("same.example"),
                None,
                Some("the quick brown fox jumps"),
            ),
            artifact(
                "https://same.example/2",
                Some("same.example"),
                None,
                Some("the quick brown fox jumps"),
            ),
        ];
        let report = score_evidence_set(&arts, &DEFAULT_PROFILE);
        assert_eq!(report.distinct_domains, 1);
        assert_eq!(report.sourced_count, 2);
        assert_eq!(report.artifacts[0].corroboration_count, 1);
        assert!((report.artifacts[0].confidence - 0.6).abs() < 1e-9);
        assert_eq!(report.content_clusters.len(), 1);
        assert_eq!(report.content_clusters[0].artifact_urls.len(), 2);
        assert_eq!(report.duplication_mode, "shingles");
    }

    #[test]
    fn single_fresh_complete_artifact_does_not_saturate() {
        // Carried: one fresh single-unit artifact = 0.8, never 1.0. The
        // redesign surfaces NotEvaluable sensitivity for a single artifact
        // instead of fabricating stability.
        let arts = vec![artifact(
            "https://solo.example/1",
            Some("solo.example"),
            Some(&today()),
            Some("unique body text words"),
        )];
        let report = score_evidence_set(&arts, &DEFAULT_PROFILE);
        assert!((report.artifacts[0].confidence - 0.8).abs() < 1e-9);
        assert_eq!(report.artifacts[0].published_age_days, Some(0));
        assert!(matches!(
            report.sensitivity,
            SensitivityStatus::NotEvaluable { .. }
        ));
    }

    #[test]
    fn stale_dated_artifact_scores_below_fresh() {
        // Carried: a 2010 date earns half the recency weight, not the full
        // term the old presence-check paid for any date.
        let fresh = score_evidence_set(
            &[artifact(
                "https://a.example/1",
                Some("a.example"),
                Some(&today()),
                None,
            )],
            &DEFAULT_PROFILE,
        );
        let stale = score_evidence_set(
            &[artifact(
                "https://a.example/1",
                Some("a.example"),
                Some("2010-01-01"),
                None,
            )],
            &DEFAULT_PROFILE,
        );
        assert!(
            stale.artifacts[0].confidence < fresh.artifacts[0].confidence,
            "stale {} must score below fresh {}",
            stale.artifacts[0].confidence,
            fresh.artifacts[0].confidence
        );
        // 0.3 base + 0.1 one unit + 0.1 stale recency
        assert!((stale.artifacts[0].confidence - 0.5).abs() < 1e-9);
    }

    #[test]
    fn undated_unsourced_artifact_scores_base_only() {
        // Carried: nothing known — base 0.3, corroboration 0.
        let report = score_evidence_set(
            &[artifact("https://x.example/1", None, None, None)],
            &DEFAULT_PROFILE,
        );
        assert_eq!(report.artifacts[0].corroboration_count, 0);
        assert!((report.artifacts[0].confidence - 0.3).abs() < 1e-9);
    }

    #[test]
    fn three_distinct_units_earn_the_full_corroboration_term() {
        // Rewritten fixture: the pre-redesign test used IDENTICAL content
        // ("body") across three domains — under the duplicate-aware model
        // that is syndication (one unit), not corroboration. Distinct
        // content across three domains is three units and earns the full
        // term: 0.3 base + 0.3 corroboration + 0.2 content (no date).
        let arts = vec![
            artifact(
                "https://a.example/1",
                Some("a.example"),
                None,
                Some("alpha story body text one"),
            ),
            artifact(
                "https://b.example/1",
                Some("b.example"),
                None,
                Some("beta story body text two"),
            ),
            artifact(
                "https://c.example/1",
                Some("c.example"),
                None,
                Some("gamma story body text three"),
            ),
        ];
        let report = score_evidence_set(&arts, &DEFAULT_PROFILE);
        assert_eq!(report.distinct_domains, 3);
        assert_eq!(report.artifacts[0].corroboration_count, 3);
        assert!((report.artifacts[0].confidence - 0.8).abs() < 1e-9);
        assert_eq!(report.content_clusters.len(), 3);
    }

    #[test]
    fn syndicated_content_across_domains_counts_as_one_unit() {
        // The substrate's lesson, now enforced: one wire story on three
        // domains is ONE evidence unit — distinct-domain counting scored
        // it as three corroborations. The cluster is visible in the
        // report, not merely discounted.
        let arts = vec![
            artifact(
                "https://a.example/1",
                Some("a.example"),
                None,
                Some("wire story body text alpha beta gamma delta"),
            ),
            artifact(
                "https://b.example/1",
                Some("b.example"),
                None,
                Some("wire story body text alpha beta gamma delta"),
            ),
            artifact(
                "https://c.example/1",
                Some("c.example"),
                None,
                Some("wire story body text alpha beta gamma delta"),
            ),
        ];
        let report = score_evidence_set(&arts, &DEFAULT_PROFILE);
        assert_eq!(report.distinct_domains, 3);
        assert_eq!(report.artifacts[0].corroboration_count, 1);
        assert_eq!(report.content_clusters.len(), 1);
        assert_eq!(report.content_clusters[0].domains.len(), 3);
        // 0.3 base + (1/3)·0.3 + 0.2 content, no date
        assert!((report.artifacts[0].confidence - 0.6).abs() < 1e-9);
    }

    #[test]
    fn shingle_jaccard_threshold_is_pinned() {
        // The threshold is a pinned const, not a tuning knob: exactly at
        // 0.5 clusters (≥ is inclusive), strictly below does not, and the
        // empty union (both contents shorter than one shingle) is an
        // explicit 0.0 — never NaN.
        let set = |items: &[&str]| {
            items
                .iter()
                .map(|item| item.to_string())
                .collect::<std::collections::HashSet<String>>()
        };
        let left = set(&["s1", "s2", "s3", "s4"]);
        let at_threshold = set(&["s1", "s2", "s3", "s5", "s6"]); // ∩ 3, ∪ 6 → 0.5
        let below = set(&["s1", "s2", "s5", "s6"]); // ∩ 2, ∪ 6 → 1/3
        assert!((shingle_jaccard(&left, &at_threshold) - 0.5).abs() < 1e-9);
        assert!(shingle_jaccard(&left, &at_threshold) >= SHINGLE_JACCARD_THRESHOLD);
        assert!((shingle_jaccard(&left, &below) - 1.0 / 3.0).abs() < 1e-9);
        assert!(shingle_jaccard(&left, &below) < SHINGLE_JACCARD_THRESHOLD);
        let empty = std::collections::HashSet::new();
        assert_eq!(shingle_jaccard(&empty, &empty), 0.0);
        assert!(shingle_jaccard(&empty, &empty).is_finite());
    }

    #[test]
    fn named_profiles_are_valid_weight_tables() {
        // The model IS the table — every table must be total: sum 1.0,
        // non-negative, each component exactly once.
        for profile in [
            &DEFAULT_PROFILE,
            &CORROBORATION_HEAVY_PROFILE,
            &RECENCY_HEAVY_PROFILE,
        ] {
            let sum: f64 = profile.iter().map(|(_, weight)| weight).sum();
            assert!((sum - 1.0).abs() < 1e-9, "profile sums to {sum}");
            assert!(profile.iter().all(|(_, weight)| *weight >= 0.0));
            for component in [
                EvidenceComponent::Base,
                EvidenceComponent::Corroboration,
                EvidenceComponent::Recency,
                EvidenceComponent::Content,
            ] {
                assert_eq!(
                    profile.iter().filter(|(c, _)| *c == component).count(),
                    1,
                    "component {:?} missing or duplicated",
                    component.name()
                );
            }
        }
    }

    #[test]
    fn sensitivity_stable_when_dominance_survives_substitution() {
        // A (fresh, content, sourced) dominates B (no date, no content,
        // sourced) under every profile: 0.9 vs 0.5 default, 0.8667 vs
        // 0.5667 corroboration-heavy, 0.9667 vs 0.3667 recency-heavy.
        let arts = vec![
            artifact(
                "https://a.example/1",
                Some("a.example"),
                Some(&today()),
                Some("alpha one two three four"),
            ),
            artifact("https://b.example/1", Some("b.example"), None, None),
        ];
        let report = score_evidence_set(&arts, &DEFAULT_PROFILE);
        assert!(matches!(
            report.sensitivity,
            SensitivityStatus::Stable {
                profiles_evaluated: 3
            }
        ));
    }

    #[test]
    fn sensitivity_unstable_when_substitution_flips_ordering() {
        // A (undated, substantive content) ties B (fresh, thin) at 0.7
        // under the default profile; corroboration-heavy keeps A first
        // (0.7667 vs 0.6667); recency-heavy flips to B first (0.5667 vs
        // 0.7667) — the ordering is not robust, and the driver is Recency.
        let arts = vec![
            artifact(
                "https://a.example/1",
                Some("a.example"),
                None,
                Some("alpha one two three four"),
            ),
            artifact(
                "https://b.example/1",
                Some("b.example"),
                Some(&today()),
                None,
            ),
        ];
        let report = score_evidence_set(&arts, &DEFAULT_PROFILE);
        assert!(matches!(
            report.sensitivity,
            SensitivityStatus::Unstable {
                driver: EvidenceComponent::Recency
            }
        ));
    }

    #[test]
    fn sensitivity_not_evaluable_with_fewer_than_two_artifacts() {
        // A single artifact has no ordering to compare — surface it, never
        // fabricate stability.
        let report = score_evidence_set(
            &[artifact(
                "https://a.example/1",
                Some("a.example"),
                None,
                None,
            )],
            &DEFAULT_PROFILE,
        );
        match report.sensitivity {
            SensitivityStatus::NotEvaluable { reason } => {
                assert!(reason.contains("fewer than 2"), "reason: {reason}");
            }
            other => panic!("expected NotEvaluable, got {other:?}"),
        }
    }

    #[test]
    fn sensitivity_not_evaluable_when_all_artifacts_score_equally() {
        // Identical artifacts tie under every profile — a trivially stable
        // ordering that would fabricate robustness. NotEvaluable instead.
        let arts = vec![
            artifact(
                "https://a.example/1",
                Some("a.example"),
                None,
                Some("same words here now"),
            ),
            artifact(
                "https://a.example/2",
                Some("a.example"),
                None,
                Some("same words here now"),
            ),
            artifact(
                "https://a.example/3",
                Some("a.example"),
                None,
                Some("same words here now"),
            ),
        ];
        let report = score_evidence_set(&arts, &DEFAULT_PROFILE);
        match report.sensitivity {
            SensitivityStatus::NotEvaluable { reason } => {
                assert!(reason.contains("equally"), "reason: {reason}");
            }
            other => panic!("expected NotEvaluable, got {other:?}"),
        }
    }

    #[test]
    fn short_content_cannot_cluster_and_never_produces_nan() {
        // Content shorter than one 4-word shingle has an empty shingle
        // set; two such artifacts have an empty union and Jaccard must be
        // an explicit 0.0 (the 0/0 NaN trap), never a silent comparison
        // result. Each short-content artifact is its own unit.
        let arts = vec![
            artifact(
                "https://a.example/1",
                Some("a.example"),
                None,
                Some("two words"),
            ),
            artifact(
                "https://a.example/2",
                Some("a.example"),
                None,
                Some("hi there"),
            ),
        ];
        let report = score_evidence_set(&arts, &DEFAULT_PROFILE);
        assert_eq!(report.artifacts[0].corroboration_count, 2);
        for scored in &report.artifacts {
            assert!(
                scored.confidence.is_finite(),
                "confidence must be finite, got {}",
                scored.confidence
            );
        }
        assert_eq!(report.content_clusters.len(), 2);
    }

    #[test]
    fn unsourced_content_bearing_artifacts_create_no_units() {
        // An unsourced paste joins a cluster by content identity, but a
        // cluster with no sourced member is not evidence: it must not
        // raise anyone's corroboration count.
        let mixed = vec![
            artifact(
                "https://a.example/1",
                Some("a.example"),
                None,
                Some("story words here now please"),
            ),
            artifact(
                "https://paste.example/1",
                None,
                None,
                Some("story words here now please"),
            ),
        ];
        let report = score_evidence_set(&mixed, &DEFAULT_PROFILE);
        assert_eq!(report.artifacts[0].corroboration_count, 1);
        assert_eq!(report.artifacts[1].corroboration_count, 0);
        assert_eq!(report.content_clusters.len(), 1);
        assert_eq!(report.content_clusters[0].artifact_urls.len(), 2);

        let unsourced_only = vec![
            artifact(
                "https://paste.example/1",
                None,
                None,
                Some("story words here now please"),
            ),
            artifact(
                "https://paste.example/2",
                None,
                None,
                Some("story words here now please"),
            ),
        ];
        let report = score_evidence_set(&unsourced_only, &DEFAULT_PROFILE);
        assert_eq!(report.artifacts[0].corroboration_count, 0);
        assert!((report.artifacts[0].confidence - 0.5).abs() < 1e-9); // base + content
    }

    #[test]
    fn content_less_artifacts_fall_back_to_domain_counting() {
        // Without content, duplication cannot be checked — the artifact
        // counts its domain, and the basis SAYS the fallback is in effect.
        let arts = vec![
            artifact(
                "https://a.example/1",
                Some("a.example"),
                None,
                Some("alpha one two three four"),
            ),
            artifact("https://b.example/1", Some("b.example"), None, None),
        ];
        let report = score_evidence_set(&arts, &DEFAULT_PROFILE);
        assert_eq!(report.artifacts[0].corroboration_count, 2);
        let content_basis = report.artifacts[0]
            .signals
            .iter()
            .find(|signal| signal.component == EvidenceComponent::Corroboration)
            .map(|signal| signal.basis.clone())
            .unwrap_or_default();
        assert!(
            content_basis.contains("content-clustered"),
            "basis: {content_basis}"
        );
        let contentless_basis = report.artifacts[1]
            .signals
            .iter()
            .find(|signal| signal.component == EvidenceComponent::Corroboration)
            .map(|signal| signal.basis.clone())
            .unwrap_or_default();
        assert!(
            contentless_basis.contains("domain-counted")
                && contentless_basis.contains("no content"),
            "basis: {contentless_basis}"
        );
    }

    #[test]
    fn unparseable_published_date_carries_no_recency_and_says_so() {
        // The deleted `has_published_date: true` + `published_age_days:
        // null` ambiguity: an unparseable date earns 0 recency and the
        // basis states the parse outcome.
        let report = score_evidence_set(
            &[artifact(
                "https://a.example/1",
                Some("a.example"),
                Some("not a date"),
                None,
            )],
            &DEFAULT_PROFILE,
        );
        let recency = report.artifacts[0]
            .signals
            .iter()
            .find(|signal| signal.component == EvidenceComponent::Recency)
            .unwrap_or_else(|| panic!("recency signal missing"));
        assert!((recency.earned - 0.0).abs() < 1e-9);
        assert!(
            recency.basis.contains("unparseable"),
            "basis: {}",
            recency.basis
        );
        assert_eq!(report.artifacts[0].published_age_days, None);
    }

    #[test]
    fn signal_earnings_sum_to_the_confidence() {
        // The signals are the single source of truth: the composite is
        // exactly their sum (capped), and each earned value matches the
        // profile arithmetic.
        let arts = vec![
            artifact(
                "https://a.example/1",
                Some("a.example"),
                Some(&today()),
                Some("alpha one two three four"),
            ),
            artifact(
                "https://b.example/1",
                Some("b.example"),
                Some("2010-01-01"),
                None,
            ),
            artifact("https://c.example/1", None, None, None),
        ];
        let report = score_evidence_set(&arts, &DEFAULT_PROFILE);
        for scored in &report.artifacts {
            let sum: f64 = scored.signals.iter().map(|signal| signal.earned).sum();
            assert!(
                (scored.confidence - sum.min(1.0)).abs() < 1e-9,
                "confidence {} != sum {sum}",
                scored.confidence
            );
        }
        let base = report.artifacts[0]
            .signals
            .iter()
            .find(|signal| signal.component == EvidenceComponent::Base)
            .map(|signal| signal.earned)
            .unwrap_or_default();
        assert!((base - 0.3).abs() < 1e-9);
    }
}
