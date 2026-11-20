//! hKask Condenser — Deterministic compression algorithms (in-process, zero LLM calls)

use super::types::*;

/// Emit `"...\n"` markers between non-consecutive lines in `indices`.
fn join_with_ellipsis(lines: &[&str], indices: &[usize]) -> String {
    let mut result = String::new();
    let mut last_idx: Option<usize> = None;
    for &idx in indices {
        if let Some(li) = last_idx
            && idx > li + 1
        {
            result.push_str("...\n");
        }
        result.push_str(lines[idx]);
        result.push('\n');
        last_idx = Some(idx);
    }
    result.trim_end().to_string()
}

/// Inputs shorter than this are passed through uncompressed.
///
/// Terminal output arrives fenced (` ```\n<payload>\n``` `), so even a
/// one-line command is three lines. At the `normal` profile the line budget
/// for such an input rounds to a single line, and the head/tail split then
/// elides the payload in favour of an ellipsis — the agent reads `...` where
/// the command's output should be. An output this short is already minimal:
/// compressing it saves no meaningful tokens while risking the loss of
/// everything the tool reported.
const MIN_COMPRESSIBLE_LINES: usize = 8;

/// Compute the line budget for compression given the input size and profile.
///
/// Returns `(budget, is_passthrough)` — if `is_passthrough`, the algorithm
/// should return the input unchanged.
pub(crate) fn compute_budget(lines: usize, profile: Profile) -> (usize, bool) {
    if lines < MIN_COMPRESSIBLE_LINES {
        // Report the input's own line count as the budget so the caller's
        // invariants (`budget <= lines`, `passthrough == budget >= lines`)
        // hold without a special case.
        return (lines, true);
    }
    let max_lines = profile.max_lines().unwrap_or(usize::MAX);
    let target_lines = ((lines as f64) * profile.retention_pct()).max(1.0).round() as usize;
    let budget = target_lines.min(max_lines).min(lines);
    (budget, budget >= lines)
}

/// Collect the input's lines and compute the profile budget — the shared
/// prologue of the compress implementations. Returns `None` on passthrough
/// (the profile's budget covers every line): the caller returns the input
/// unchanged.
fn budgeted_lines(input: &str, profile: Profile) -> Option<(Vec<&str>, usize)> {
    let lines: Vec<&str> = input.lines().collect();
    let (budget, passthrough) = compute_budget(lines.len(), profile);
    if passthrough {
        return None;
    }
    Some((lines, budget))
}

pub(crate) trait CondenserAlgorithm: Send + Sync {
    fn name(&self) -> &str;

    /// Compress the input and return (compressed_content, health_signals).
    /// `ontology_anchor` provides domain context for saliency weighting (P8.1).
    /// Health signals are empty when the algorithm performed within expected bounds.
    ///
    /// The default body carries the shared prologue — collect the input's
    /// lines, compute the profile budget, and return the input unchanged on
    /// passthrough — then delegates the budgeted body to
    /// [`Self::compress_budgeted`].
    fn compress(
        &self,
        input: &str,
        profile: Profile,
        _category: ContextCategory,
        ontology_anchor: Option<&OntologyAnchor>,
    ) -> (String, Vec<CondenserHealthSignal>) {
        let Some((lines, budget)) = budgeted_lines(input, profile) else {
            return (input.to_string(), vec![]);
        };
        self.compress_budgeted(input, &lines, budget, ontology_anchor)
    }

    /// The budgeted compression body — `lines` are the input's lines and
    /// `budget` is the profile's line budget. The passthrough return is
    /// handled by the [`Self::compress`] default; implementations select
    /// from `lines` within `budget`.
    fn compress_budgeted(
        &self,
        input: &str,
        lines: &[&str],
        budget: usize,
        ontology_anchor: Option<&OntologyAnchor>,
    ) -> (String, Vec<CondenserHealthSignal>);
}

pub struct RtkStyleAlgorithm;

impl CondenserAlgorithm for RtkStyleAlgorithm {
    fn name(&self) -> &str {
        "rtk_style"
    }

    fn compress_budgeted(
        &self,
        input: &str,
        lines: &[&str],
        budget: usize,
        ontology_anchor: Option<&OntologyAnchor>,
    ) -> (String, Vec<CondenserHealthSignal>) {
        // Ontology-aware head/tail split: denser-anchored content gets more
        // tail (summary/conclusion often carries the key measured values)
        let density_factor = ontology_anchor.map(|a| a.density_factor()).unwrap_or(1.0);
        let head_ratio = (0.3 / density_factor).clamp(0.15, 0.5);
        // Retain at least one head line. `(budget as f64 * head_ratio) as usize`
        // truncates to zero for any budget below 1/head_ratio (≈4 at the 0.3
        // nominal ratio, ≈7 at the 0.15 clamp floor), and a zero head replaces
        // the beginning of the output with an ellipsis — the whole payload is
        // dropped whenever the budget is small relative to the line count.
        let head_count = ((budget as f64 * head_ratio) as usize).max(1).min(budget);
        let tail_count = budget.saturating_sub(head_count);

        let mut filtered: Vec<&str> = Vec::new();
        for line in lines.iter().take(head_count) {
            filtered.push(*line);
        }

        // Emit the separator only when tail lines actually follow it — a
        // zero-line tail would otherwise leave a trailing, meaningless
        // ellipsis after the head.
        let tail_start = lines.len().saturating_sub(tail_count);
        if tail_start > head_count && tail_start < lines.len() {
            filtered.push("...");
            for line in lines.iter().skip(tail_start) {
                filtered.push(*line);
            }
        }

        let result = filtered.join("\n");
        let health = if result.len() > input.len() {
            vec![CondenserHealthSignal {
                algorithm: "rtk_style".into(),
                signal_type: "negative_compression".into(),
                detail: format!(
                    "Compressed {}B > original {}B — bounds violation",
                    result.len(),
                    input.len()
                ),
                zero_score_count: None,
                budget_requested: None,
                budget_filled: None,
            }]
        } else {
            vec![]
        };
        (result, health)
    }
}

pub(crate) struct WordRankAlgorithm;

impl WordRankAlgorithm {
    fn compute_word_frequencies(lines: &[&str]) -> std::collections::HashMap<String, f64> {
        let words: Vec<&str> = lines.iter().flat_map(|l| l.split_whitespace()).collect();
        crate::saliency::word_frequencies(&words)
    }

    /// Score a single line: TF-IDF average + structural bonus + ontology anchoring.
    fn line_score(
        line: &str,
        freq: &std::collections::HashMap<String, f64>,
        anchor: Option<&OntologyAnchor>,
    ) -> f64 {
        let words: Vec<&str> = line.split_whitespace().filter(|w| w.len() > 2).collect();
        if words.is_empty() {
            return 0.0;
        }
        let tf_sum: f64 = words
            .iter()
            .map(|w| freq.get(&w.to_lowercase()).copied().unwrap_or(0.0))
            .sum();

        let structural_bonus =
            if line.contains("error") || line.contains("Error") || line.contains("ERROR") {
                2.0
            } else if line.contains("warning") || line.contains("Warning") {
                1.0
            } else if line.starts_with('#') || line.starts_with("##") {
                0.5
            } else if line.starts_with('-') || line.starts_with('*') {
                0.2
            } else {
                0.0
            };

        // Domain-aware ontology bonus (P8.1) — layered on top of TF-IDF
        tf_sum / words.len() as f64 + structural_bonus + domain_saliency(line, anchor)
    }
}

impl CondenserAlgorithm for WordRankAlgorithm {
    fn name(&self) -> &str {
        "word_rank"
    }

    fn compress_budgeted(
        &self,
        _input: &str,
        lines: &[&str],
        budget: usize,
        ontology_anchor: Option<&OntologyAnchor>,
    ) -> (String, Vec<CondenserHealthSignal>) {
        let freq = Self::compute_word_frequencies(lines);

        let mut scored: Vec<(usize, f64, &str)> = lines
            .iter()
            .enumerate()
            .map(|(i, line)| (i, Self::line_score(line, &freq, ontology_anchor), *line))
            .collect();

        let zero_count = scored.iter().filter(|(_, s, _)| *s == 0.0).count();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut selected_indices: Vec<usize> =
            scored.into_iter().take(budget).map(|(i, _, _)| i).collect();
        selected_indices.sort_unstable();

        let result = join_with_ellipsis(lines, &selected_indices);
        let health = if zero_count > lines.len() / 2 {
            vec![CondenserHealthSignal {
                algorithm: "word_rank".into(),
                signal_type: "low_signal".into(),
                detail: format!(
                    "{} of {} lines scored 0.0 — content had no usable signal to rank by",
                    zero_count,
                    lines.len()
                ),
                zero_score_count: Some(zero_count),
                budget_requested: None,
                budget_filled: None,
            }]
        } else {
            vec![]
        };
        (result, health)
    }
}

/// Domain saliency: score a line against an ontology anchor using graph proximity.
///
/// Returns 0.0 (unrelated) up to ~1.0 (strong domain match). Combines direct
/// keyword recognition with graph adjacency from the ontology concept graph (P5.4).
pub(crate) fn domain_saliency(line: &str, anchor: Option<&OntologyAnchor>) -> f64 {
    let direct = match anchor {
        Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Sumo,
            ..
        }) => {
            if line.contains("memory")
                || line.contains("recall")
                || line.contains("encoding")
                || line.contains("salience")
                || line.contains("consolidation")
                || line.contains("forgetting")
                || line.contains("chunking")
            {
                0.4
            } else {
                0.0
            }
        }
        Some(OntologyAnchor::DualAxis {
            axis: OntologyAxis::Pko,
            ..
        }) => {
            if line.contains("status")
                || line.contains("verify")
                || line.contains("execution")
                || line.contains("step")
            {
                0.3
            } else if line.contains("error") || line.contains("issue") {
                0.4
            } else {
                0.0
            }
        }
        Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Golem,
            ..
        }) if line.contains("character")
            || line.contains("narrative")
            || line.contains("scene")
            || line.contains("event") =>
        {
            0.3
        }
        Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::MlSchema,
            ..
        }) if line.contains("accuracy")
            || line.contains("loss")
            || line.contains("epoch")
            || line.contains("learning_rate")
            || line.contains("batch")
            || line.contains("evaluation")
            || line.chars().any(|c| c.is_ascii_digit()) =>
        {
            0.3
        }
        _ => 0.0,
    };

    let graph_bonus = match anchor {
        Some(a) => graph_adjacency_bonus(line, a),
        None => 0.0,
    };

    direct + graph_bonus
}

/// Score a line for concepts adjacent to the anchor in the published
/// `hkask-bridge-ontology` relation graph (1-hop outgoing neighbors).
/// Returns 0.15 per matched neighbor label, capped at 0.5.
fn graph_adjacency_bonus(line: &str, anchor: &OntologyAnchor) -> f64 {
    let concept = match anchor {
        OntologyAnchor::Core => return 0.0,
        OntologyAnchor::DualAxis { concept, .. }
        | OntologyAnchor::DomainSupplement { concept, .. } => concept.as_str(),
    };
    let traversal = hkask_bridge_ontology::ontology_graph::graph().traverse(concept, None, 1);
    if traversal.edges.is_empty() {
        return 0.0;
    }
    let lower = line.to_lowercase();
    let mut bonus = 0.0_f64;
    let mut seen = std::collections::HashSet::new();
    for edge in &traversal.edges {
        if let Some(term) = hkask_bridge_ontology::published::get(&edge.to) {
            let candidates = std::iter::once(term.name).chain(term.labels.iter().copied());
            for label in candidates {
                let needle = label.to_ascii_lowercase();
                if needle.len() >= 3 && seen.insert(needle.clone()) && lower.contains(&needle) {
                    bonus += 0.15;
                    if bonus >= 0.5 {
                        return 0.5;
                    }
                }
            }
        }
    }
    bonus.min(0.5)
}

/// Greedy Maximal Marginal Relevance (MMR, Carbonell & Goldstein, SIGIR 1998)
/// line selector balancing query-lead relevance, lexical novelty against
/// already-selected lines, and line brevity.
pub struct FlashrankAlgorithm;

impl FlashrankAlgorithm {
    fn brevity_score(line: &str) -> f64 {
        let len = line.len() as f64;
        if len == 0.0 {
            return 0.0;
        }
        1.0 / (1.0 + len / 100.0)
    }
}

impl CondenserAlgorithm for FlashrankAlgorithm {
    fn name(&self) -> &str {
        "flashrank"
    }

    fn compress_budgeted(
        &self,
        _input: &str,
        lines: &[&str],
        budget: usize,
        _ontology_anchor: Option<&OntologyAnchor>,
    ) -> (String, Vec<CondenserHealthSignal>) {
        let alpha = 0.4f64;
        let beta = 0.3f64;
        let gamma = 0.3f64;

        // Pre-lowercase lines and lead query terms once so greedy MMR selection
        // performs zero string or HashSet allocations inside the selection loop.
        let lower_lines: Vec<String> = lines.iter().map(|l| l.to_lowercase()).collect();
        let query_terms: Vec<String> = lower_lines
            .iter()
            .take(5)
            .flat_map(|l| {
                l.split_whitespace()
                    .filter(|w| w.len() > 3)
                    .map(str::to_owned)
            })
            .take(20)
            .collect();

        let static_scores: Vec<f64> = lines
            .iter()
            .zip(lower_lines.iter())
            .map(|(raw, lower)| {
                let rel: f64 = query_terms
                    .iter()
                    .map(|term| {
                        if lower.contains(term.as_str()) {
                            1.0
                        } else {
                            0.0
                        }
                    })
                    .sum();
                let brev = Self::brevity_score(raw);
                alpha * rel - gamma * (1.0 - brev)
            })
            .collect();

        let line_unique_words: Vec<Vec<&str>> = lower_lines
            .iter()
            .map(|lower| {
                let mut words: Vec<&str> = lower.split_whitespace().collect();
                words.sort_unstable();
                words.dedup();
                words
            })
            .collect();

        let mut is_selected = vec![false; lines.len()];
        let mut selected_indices: Vec<usize> = Vec::with_capacity(budget);
        let mut selected_word_df: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();

        while selected_indices.len() < budget {
            let selected_count = selected_indices.len();
            let mut best_idx: Option<usize> = None;
            let mut best_score = f64::NEG_INFINITY;

            for i in 0..lines.len() {
                if is_selected[i] {
                    continue;
                }

                let words = &line_unique_words[i];
                let nov = if selected_count == 0 || words.is_empty() {
                    1.0
                } else {
                    let total = selected_count * words.len();
                    let overlap: usize = words
                        .iter()
                        .map(|w| selected_word_df.get(w).copied().unwrap_or(0))
                        .sum();
                    1.0 - (overlap as f64 / total as f64)
                };

                let score = static_scores[i] + beta * nov;
                if score > best_score {
                    best_score = score;
                    best_idx = Some(i);
                }
            }

            match best_idx {
                Some(idx) => {
                    is_selected[idx] = true;
                    selected_indices.push(idx);
                    for &w in &line_unique_words[idx] {
                        *selected_word_df.entry(w).or_insert(0) += 1;
                    }
                }
                None => break,
            }
        }

        let filled = selected_indices.len();
        selected_indices.sort_unstable();
        let result = join_with_ellipsis(lines, &selected_indices);

        let health = if filled < budget {
            vec![CondenserHealthSignal {
                algorithm: "flashrank".into(),
                signal_type: "budget_shortfall".into(),
                detail: format!(
                    "Filled {}/{} budget — all remaining lines had non-positive score",
                    filled, budget
                ),
                zero_score_count: None,
                budget_requested: Some(budget),
                budget_filled: Some(filled),
            }]
        } else {
            vec![]
        };
        (result, health)
    }
}

/// Keyword→category mapping — single source of truth for both exact-token (Phase 1)
/// and substring (Phase 2) matching in [`classify_tool`].
const KEYWORD_CATEGORIES: &[(&[&str], ContextCategory)] = &[
    (
        &[
            "git", "docker", "cargo", "npm", "shell", "bash", "exec", "run", "terminal",
        ],
        ContextCategory::ShellCommand,
    ),
    (&["test", "pytest", "spec"], ContextCategory::TestOutput),
    (&["build", "compile", "make"], ContextCategory::BuildOutput),
    (
        &["chat", "conversation", "message"],
        ContextCategory::ConversationHistory,
    ),
    (&["log", "journal", "trace"], ContextCategory::LogOutput),
    (&["json", "api", "query"], ContextCategory::StructuredData),
    (&["file", "read", "cat"], ContextCategory::FileContents),
];

/// Classify a tool name into a `ContextCategory`.
/// Phase 1: exact token match on `_`/`-`-split parts. Phase 2: substring fallback.
pub fn classify_tool(tool_name: &str) -> ContextCategory {
    let lower = tool_name.to_lowercase();
    let parts: Vec<&str> = lower.split('_').chain(lower.split('-')).collect();

    // Phase 1: exact token match — no false positives
    for part in &parts {
        for (keywords, cat) in KEYWORD_CATEGORIES {
            if keywords.contains(part) {
                return *cat;
            }
        }
    }

    // Phase 2: substring heuristic for compound names without separators.
    // False positives possible ("logistics"→LogOutput) but Phase 2 only fires
    // after Phase 1 fails, so the tool name is already non-standard.
    for (keywords, cat) in KEYWORD_CATEGORIES {
        if keywords.iter().any(|kw| lower.contains(kw)) {
            return *cat;
        }
    }
    ContextCategory::Unknown
}

// ═══════════════════════════════════════════════════════════════════════════
// Structured JSON Homogeneous Object-Array Compression
// ═══════════════════════════════════════════════════════════════════════════

const MIN_JSON_ARRAY_ITEMS: usize = 8;
const MAX_JSON_WALK_DEPTH: usize = 6;
const ELIDED_COUNT_KEY: &str = "_kask_elided_count";

#[derive(Default, Clone, Copy)]
struct DistinctNumbers3 {
    first: Option<f64>,
    second: Option<f64>,
    has_three: bool,
}

impl DistinctNumbers3 {
    fn observe(&mut self, val: f64) {
        if self.has_three {
            return;
        }
        match (self.first, self.second) {
            (None, _) => self.first = Some(val),
            (Some(a), None) => {
                if (val - a).abs() > f64::EPSILON {
                    self.second = Some(val);
                }
            }
            (Some(a), Some(b)) => {
                if (val - a).abs() > f64::EPSILON && (val - b).abs() > f64::EPSILON {
                    self.has_three = true;
                }
            }
        }
    }
}

fn contains_ascii_ignore_case(haystack: &str, needle_lower: &[u8]) -> bool {
    haystack
        .as_bytes()
        .windows(needle_lower.len())
        .any(|w| w.eq_ignore_ascii_case(needle_lower))
}

fn is_non_trivial_anomaly_value(val: &serde_json::Value) -> bool {
    match val {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Number(_)
        | serde_json::Value::Array(_)
        | serde_json::Value::Object(_) => true,
    }
}

fn estimated_object_bytes(obj: &serde_json::Map<String, serde_json::Value>) -> usize {
    let mut approx = 2usize;
    for (k, v) in obj {
        approx += k.len() + 4;
        approx += match v {
            serde_json::Value::Null => 4,
            serde_json::Value::Bool(b) => {
                if *b {
                    4
                } else {
                    5
                }
            }
            serde_json::Value::Number(_) => 8,
            serde_json::Value::String(s) => s.len() + 2,
            serde_json::Value::Array(a) => 2 + a.len() * 16,
            serde_json::Value::Object(m) => 2 + m.len() * 24,
        };
        if approx > 96 {
            break;
        }
    }
    approx
}

fn is_compressible_object_array(arr: &[serde_json::Value]) -> (bool, bool) {
    let mut has_elided_marker = false;
    for item in arr {
        let Some(obj) = item.as_object() else {
            return (false, false);
        };
        if obj.contains_key(ELIDED_COUNT_KEY) {
            has_elided_marker = true;
        }
    }
    if has_elided_marker {
        return (false, true);
    }
    if arr.len() < MIN_JSON_ARRAY_ITEMS {
        return (false, false);
    }
    let Some(first_obj) = arr[0].as_object() else {
        return (false, false);
    };
    if first_obj.is_empty() {
        return (false, false);
    }
    // Verify schema homogeneity: >= 75% of elements share at least one key with arr[0].
    let matching = arr
        .iter()
        .filter(|v| {
            v.as_object()
                .is_some_and(|o| first_obj.keys().any(|k| o.contains_key(k)))
        })
        .count();
    (matching * 4 >= arr.len() * 3, false)
}

fn compress_object_array_in_place(arr: &mut Vec<serde_json::Value>, profile: Profile) -> bool {
    let n = arr.len();
    if n < MIN_JSON_ARRAY_ITEMS || profile == Profile::Light {
        return false;
    }

    let (head_keep, tail_keep) = match profile {
        Profile::Heavy => (1usize, 1usize),
        Profile::Normal => (2usize, 2usize),
        Profile::Soft => (3usize, 3usize),
        Profile::Light => return false,
    };

    if head_keep + tail_keep >= n {
        return false;
    }

    let mut keep = vec![false; n];
    for idx in 0..head_keep {
        keep[idx] = true;
    }
    for idx in (n - tail_keep)..n {
        keep[idx] = true;
    }

    // Collect distinct scalar/object field names across the array (bounded to 48 keys).
    let mut field_names: Vec<&str> = Vec::with_capacity(16);
    for item in arr.iter() {
        if let Some(obj) = item.as_object() {
            for k in obj.keys() {
                if !field_names.contains(&k.as_str()) && field_names.len() < 48 {
                    field_names.push(k.as_str());
                }
            }
        }
    }

    // Per-field single pass for numeric min/max extrema and rare-null outlier detection.
    for &field in &field_names {
        let mut min_pair: Option<(f64, usize)> = None;
        let mut max_pair: Option<(f64, usize)> = None;
        let mut distinct = DistinctNumbers3::default();
        let mut non_null_count = 0usize;

        for (idx, item) in arr.iter().enumerate() {
            if let Some(obj) = item.as_object()
                && let Some(val) = obj.get(field)
                && !val.is_null()
            {
                non_null_count += 1;
                if let Some(num) = val.as_f64()
                    && num.is_finite()
                {
                    distinct.observe(num);
                    if min_pair.is_none_or(|(m, _)| num < m) {
                        min_pair = Some((num, idx));
                    }
                    if max_pair.is_none_or(|(m, _)| num > m) {
                        max_pair = Some((num, idx));
                    }
                }
            }
        }

        if distinct.has_three {
            if let Some((_, min_idx)) = min_pair {
                keep[min_idx] = true;
            }
            if let Some((_, max_idx)) = max_pair {
                keep[max_idx] = true;
            }
        }

        // Rare-null outlier: >= 80% of items carry a non-null value for `field` and <= 2 are null.
        if non_null_count * 5 >= n * 4 && (n - non_null_count) <= 2 && non_null_count < n {
            for (idx, item) in arr.iter().enumerate() {
                if let Some(obj) = item.as_object()
                    && obj.get(field) == Some(&serde_json::Value::Null)
                {
                    keep[idx] = true;
                }
            }
        }
    }

    // Explicit error / warning / high-critical priority outlier check.
    for (idx, item) in arr.iter().enumerate() {
        let Some(obj) = item.as_object() else {
            continue;
        };
        if obj.get("is_error") == Some(&serde_json::Value::Bool(true)) {
            keep[idx] = true;
            continue;
        }
        if let Some(prio) = obj.get("priority").and_then(|v| v.as_str())
            && (prio.eq_ignore_ascii_case("critical") || prio.eq_ignore_ascii_case("high"))
        {
            keep[idx] = true;
            continue;
        }
        for (k, v) in obj {
            if (contains_ascii_ignore_case(k, b"error")
                || contains_ascii_ignore_case(k, b"warning"))
                && is_non_trivial_anomaly_value(v)
            {
                keep[idx] = true;
                break;
            }
        }
    }

    // Under Soft profile, retain evenly spaced interior samples up to ~40% of the array (cap 30).
    if profile == Profile::Soft {
        let soft_target = ((n as f64 * 0.40).round() as usize).min(30);
        let currently_kept = keep.iter().filter(|&&b| b).count();
        if currently_kept < soft_target {
            let needed = soft_target - currently_kept;
            let step = (n / (needed + 1)).max(1);
            let mut added = 0usize;
            let mut pos = step;
            while pos < n && added < needed {
                if !keep[pos] {
                    keep[pos] = true;
                    added += 1;
                }
                pos += step;
            }
        }
    }

    if keep.iter().all(|&b| b) {
        return false;
    }

    // Coalesce unkept runs into `{"_kask_elided_count": count}` markers.
    let old_items = std::mem::take(arr);
    let mut new_items = Vec::with_capacity(keep.iter().filter(|&&b| b).count() + 4);
    let mut i = 0usize;
    let mut elided_any = false;
    let items_slice = old_items.into_iter().collect::<Vec<_>>();

    while i < n {
        if keep[i] {
            new_items.push(items_slice[i].clone());
            i += 1;
        } else {
            let start = i;
            while i < n && !keep[i] {
                i += 1;
            }
            let run_len = i - start;
            if run_len == 1
                && items_slice[start]
                    .as_object()
                    .is_some_and(|o| estimated_object_bytes(o) <= 64)
            {
                // A single tiny object is ~the same size as the elision marker; keep it intact.
                new_items.push(items_slice[start].clone());
            } else {
                new_items.push(serde_json::json!({ ELIDED_COUNT_KEY: run_len }));
                elided_any = true;
            }
        }
    }

    *arr = new_items;
    elided_any
}

fn walk_json_value(
    val: &mut serde_json::Value,
    profile: Profile,
    depth: usize,
    mutated: &mut bool,
    saw_existing_marker: &mut bool,
) {
    if depth > MAX_JSON_WALK_DEPTH {
        return;
    }
    match val {
        serde_json::Value::Array(arr) => {
            let (compressible, has_marker) = is_compressible_object_array(arr);
            if has_marker {
                *saw_existing_marker = true;
            } else if compressible && compress_object_array_in_place(arr, profile) {
                *mutated = true;
            }
            for child in arr.iter_mut() {
                walk_json_value(child, profile, depth + 1, mutated, saw_existing_marker);
            }
        }
        serde_json::Value::Object(map) => {
            if map.contains_key(ELIDED_COUNT_KEY) {
                *saw_existing_marker = true;
                return;
            }
            for child in map.values_mut() {
                walk_json_value(child, profile, depth + 1, mutated, saw_existing_marker);
            }
        }
        _ => {}
    }
}

/// Attempt AST-preserving structured JSON compression.
///
/// Returns `Some((compressed_json, health_signals))` whenever `input` parses as
/// valid JSON (`{...}` or `[...]`), ensuring multi-line JSON never falls back
/// to line-splicing `join_with_ellipsis`. Homogeneous `>= 8`-item object arrays
/// elide interior routine items into `{"_kask_elided_count": N}` markers while
/// smaller multi-line JSON documents receive lossless whitespace minification.
pub(crate) fn try_compress_structured_json_array(
    input: &str,
    profile: Profile,
) -> Option<(String, Vec<CondenserHealthSignal>)> {
    let trimmed = input.trim();
    if trimmed.len() < 2 {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let looks_like_json = (bytes.first() == Some(&b'{') && bytes.last() == Some(&b'}'))
        || (bytes.first() == Some(&b'[') && bytes.last() == Some(&b']'));
    if !looks_like_json {
        return None;
    }

    let mut root: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    if profile == Profile::Light {
        return Some((input.to_string(), vec![]));
    }

    let mut mutated = false;
    let mut saw_existing_marker = false;
    walk_json_value(
        &mut root,
        profile,
        0,
        &mut mutated,
        &mut saw_existing_marker,
    );

    if !mutated {
        if saw_existing_marker || !input.contains('\n') {
            return Some((input.to_string(), vec![]));
        }
        let minified = serde_json::to_string(&root).ok()?;
        if minified.len() < input.len() {
            return Some((minified, vec![]));
        }
        return Some((input.to_string(), vec![]));
    }

    let serialized = serde_json::to_string(&root).ok()?;
    if serialized.len() >= input.len() {
        return Some((input.to_string(), vec![]));
    }
    Some((serialized, vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_adjacency_bonus_returns_zero_for_core_anchor() {
        assert_eq!(
            graph_adjacency_bonus("any line with process and entity", &OntologyAnchor::Core),
            0.0
        );
    }

    #[test]
    fn graph_adjacency_bonus_uses_bridge_ontology_neighbors() {
        let anchor = OntologyAnchor::DualAxis {
            axis: crate::types::OntologyAxis::DcBibo,
            concept: hkask_bridge_ontology::dc_bibo::ARTICLE.to_string(),
        };
        let unrelated = graph_adjacency_bonus("zzz unrelated tokens only", &anchor);
        assert_eq!(unrelated, 0.0);

        let traversal = hkask_bridge_ontology::ontology_graph::graph().traverse(
            hkask_bridge_ontology::dc_bibo::ARTICLE,
            None,
            1,
        );
        if let Some(first_label) = traversal
            .edges
            .iter()
            .filter_map(|e| hkask_bridge_ontology::published::get(&e.to))
            .map(|t| t.name)
            .find(|name| name.len() >= 3)
        {
            let matched = graph_adjacency_bonus(
                &format!("This line mentions {first_label} explicitly"),
                &anchor,
            );
            assert!(
                matched >= 0.15 && matched <= 0.5,
                "expected bounded positive bonus for neighbor {first_label}, got {matched}"
            );
        }
    }

    #[test]
    fn compute_budget_passthrough_when_within_profile() {
        let (budget, passthrough) = compute_budget(10, Profile::Light);
        assert!(passthrough);
        assert_eq!(budget, 10);
    }

    #[test]
    fn compute_budget_respects_max_lines_cap() {
        let (budget, passthrough) = compute_budget(1000, Profile::Heavy);
        assert!(!passthrough);
        assert_eq!(budget, 30);
    }

    // Note: retention_pct is applied first, then capped. Below the
    // compressible floor the input passes through with its own line count.
    #[test]
    fn compute_budget_never_exceeds_input() {
        let (budget, passthrough) = compute_budget(5, Profile::Normal);
        assert_eq!(budget, 5);
        assert!(passthrough, "below the floor the input is not compressed");
    }

    // Above the floor, the retention budget applies as before.
    #[test]
    fn compute_budget_applies_retention_above_the_floor() {
        let (budget, passthrough) = compute_budget(20, Profile::Normal);
        assert_eq!(budget, 4);
        assert!(!passthrough);
    }

    #[test]
    fn compute_budget_single_line() {
        let (budget, passthrough) = compute_budget(1, Profile::Heavy);
        assert!(passthrough);
        assert_eq!(budget, 1);
    }

    #[test]
    fn compute_budget_zero_lines() {
        let (budget, passthrough) = compute_budget(0, Profile::Heavy);
        assert!(passthrough);
        assert_eq!(budget, 0);
    }

    // Note: Phase 1 exact-token matching checks split parts in order. "npm" matches
    // ShellCommand before "build" is reached. "cargo_test" matches ShellCommand via "cargo".
    #[test]
    fn classify_tool_exact_match() {
        assert_eq!(classify_tool("git_status"), ContextCategory::ShellCommand);
        assert_eq!(classify_tool("docker_run"), ContextCategory::ShellCommand);
        // "npm_build" → "npm" matches ShellCommand before "build" → BuildOutput
        assert_eq!(classify_tool("npm_build"), ContextCategory::ShellCommand);
        // "cargo_test" → "cargo" matches ShellCommand before "test" → TestOutput
        assert_eq!(classify_tool("cargo_test"), ContextCategory::ShellCommand);
    }

    // Note: Phase 2 substring matching can produce false positives on short keywords (e.g., "run"
    // matches "testrunner", overriding the intended TestOutput classification).
    #[test]
    fn classify_tool_substring_fallback() {
        // "buildsystem" — no exact match, Phase 2: "build" → BuildOutput
        assert_eq!(classify_tool("buildsystem"), ContextCategory::BuildOutput);
        // "testrunner" contains "run" (ShellCommand) before Phase 2 reaches TestOutput
        assert_eq!(classify_tool("testrunner"), ContextCategory::ShellCommand);
        // "jsonparser" contains "json" → StructuredData
        assert_eq!(classify_tool("jsonparser"), ContextCategory::StructuredData);
    }

    #[test]
    fn classify_tool_unknown() {
        assert_eq!(classify_tool("unknown_tool"), ContextCategory::Unknown);
        assert_eq!(classify_tool(""), ContextCategory::Unknown);
        assert_eq!(classify_tool("xyz"), ContextCategory::Unknown);
    }

    // Note: Both hyphen and underscore splits yield the same token set. First match wins.
    #[test]
    fn classify_tool_hyphen_vs_underscore() {
        assert_eq!(classify_tool("cargo-test"), ContextCategory::ShellCommand);
        assert_eq!(classify_tool("cargo_test"), ContextCategory::ShellCommand);
        assert_eq!(classify_tool("npm-build"), ContextCategory::ShellCommand);
        assert_eq!(classify_tool("npm_build"), ContextCategory::ShellCommand);
    }

    #[test]
    fn rtk_style_compression_within_budget() {
        let input = (0..200)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let algo = RtkStyleAlgorithm;
        let (result, health) =
            algo.compress(&input, Profile::Normal, ContextCategory::ShellCommand, None);
        let result_lines = result.lines().count();
        assert!(
            result_lines <= 80,
            "result has {} lines, expected <= 80",
            result_lines
        );
        assert!(
            result.len() <= input.len(),
            "compressed output larger than input"
        );
        assert!(
            health.is_empty(),
            "no health signals expected for normal compression"
        );
    }

    #[test]
    fn rtk_style_preserves_head_tail_structure() {
        let input = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10";
        let algo = RtkStyleAlgorithm;
        let (result, _) =
            algo.compress(input, Profile::Normal, ContextCategory::ShellCommand, None);
        assert!(result.contains("line1"));
        assert!(result.contains("line10"));
        assert!(result.contains("..."));
    }

    #[test]
    fn rtk_style_passthrough_small_input() {
        let input = "line1\nline2\nline3";
        let algo = RtkStyleAlgorithm;
        let (result, health) =
            algo.compress(input, Profile::Light, ContextCategory::ShellCommand, None);
        assert_eq!(result, input);
        assert!(health.is_empty());
    }

    // Regression: a short terminal result must reach the agent intact.
    //
    // Terminal output arrives fenced, so a one-line command is three lines
    // (` ``` `, payload, ` ``` `). At the `normal` profile this once produced a
    // one-line budget whose head/tail split elided the payload, so the agent
    // read a bare `...` instead of the command's output. Observed live: every
    // short command (`echo`, `ls`, `git status`) returned `...` to the agent.
    #[test]
    fn rtk_style_preserves_short_fenced_terminal_output() {
        let input = "```\nhello\n```";
        let algo = RtkStyleAlgorithm;
        for profile in [
            Profile::Heavy,
            Profile::Normal,
            Profile::Soft,
            Profile::Light,
        ] {
            let (result, _) = algo.compress(input, profile, ContextCategory::ShellCommand, None);
            assert_eq!(
                result, input,
                "{profile:?}: short terminal output must pass through verbatim"
            );
            assert!(
                result.contains("hello"),
                "{profile:?}: payload lost — agent would read only the ellipsis"
            );
        }
    }

    // Regression: a small budget must never elide the whole head.
    //
    // `(budget as f64 * head_ratio) as usize` truncates to 0 for any budget
    // below 1/head_ratio, and a zero head replaces the beginning of the output
    // with an ellipsis. Above the passthrough floor the head must still carry
    // at least one real line.
    #[test]
    fn rtk_style_small_budget_keeps_a_head_line() {
        let input = (0..8)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let algo = RtkStyleAlgorithm;
        let (result, _) =
            algo.compress(&input, Profile::Heavy, ContextCategory::ShellCommand, None);
        let first = result.lines().next().unwrap_or_default();
        assert_ne!(first, "...", "head was elided entirely: {result}");
        assert_eq!(first, "line0", "head line not preserved: {result}");
        assert!(
            !result.starts_with("..."),
            "output opens with an ellipsis: {result}"
        );
    }

    #[test]
    fn word_rank_preserves_error_lines() {
        let input = "info: ok\ninfo: ok\ninfo: ok\nerror: critical failure\ninfo: ok\ninfo: ok";
        let algo = WordRankAlgorithm;
        let (result, _) = algo.compress(input, Profile::Heavy, ContextCategory::LogOutput, None);
        assert!(
            result.contains("error"),
            "error line not preserved: {}",
            result
        );
    }

    #[test]
    fn word_rank_low_signal_when_no_content() {
        let input = "a\na\na\na\na\na\na\na\na\na";
        let algo = WordRankAlgorithm;
        let (_, health) = algo.compress(input, Profile::Heavy, ContextCategory::Unknown, None);
        assert!(!health.is_empty(), "expected low_signal health signal");
        assert_eq!(health[0].signal_type, "low_signal");
    }

    #[test]
    fn flashrank_selects_within_budget() {
        let input = (0..100)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let algo = FlashrankAlgorithm;
        let (result, health) =
            algo.compress(&input, Profile::Heavy, ContextCategory::FileContents, None);
        let result_lines = result.lines().count();
        assert!(
            result_lines <= 30,
            "flashrank exceeded budget: {} lines",
            result_lines
        );
        assert!(result.len() <= input.len());
        assert!(health.is_empty());
    }

    // Note: 3 lines with Heavy profile (10% retention, max 30) → budget = 1. Flashrank
    // fills 1 out of 1 → no shortfall. Budget_shortfall only when budget > available lines.
    #[test]
    fn flashrank_budget_shortfall() {
        let input = "line1\nline2\nline3";
        let algo = FlashrankAlgorithm;
        let (_, health) = algo.compress(input, Profile::Heavy, ContextCategory::FileContents, None);
        // 3 lines → budget = min(ceil(3*0.10), 30) = 1 → fills 1 → no shortfall
        assert!(
            health.is_empty(),
            "expected no budget_shortfall with budget=1 from 3 lines"
        );
    }

    // ── End-to-end ontology-aware compression tests (P8.1) ───────────────

    /// SUMO-anchored structured data preserves key numeric measurements.
    #[test]
    fn sumo_anchor_preserves_structured_metrics() {
        let input = concat!(
            "Benchmark Profile: p95 latency\n",
            "Run: 2026-10-18\n",
            "p95 latency: 142.0 ms\n",
            "p50 latency: 61.5 ms\n",
            "Throughput change: 5.2%\n",
            "Peak memory: 102 MB\n",
            "Error rate: 0.45%\n",
            "This is a general description of the run's conditions.\n",
            "The harness was run on a Framework laptop in 2026.\n",
            "It measures request handling under load.\n",
            "The comparison baseline is the prior release.\n",
            "Operator commentary: we expect continued improvement in latency.\n",
        );
        let algo = WordRankAlgorithm;
        let anchor = Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Sumo,
            concept: hkask_bridge_ontology::sumo::PROCESS.into(),
        });
        let (result, _) = algo.compress(
            input,
            Profile::Soft,
            ContextCategory::StructuredData,
            anchor.as_ref(),
        );
        // The SUMO anchor's graph bonus should prioritize lines carrying
        // process/measurement vocabulary.
        // Soft profile (60% retention): enough budget for multiple metric lines
        assert!(
            result.contains("p95 latency"),
            "measurement not preserved: {result}"
        );
        let has_numbers = result.contains("p50 latency")
            || result.contains("Throughput change")
            || result.contains("Peak memory");
        assert!(
            has_numbers,
            "at least one additional measurement should be preserved: {result}"
        );
        let result_lines: Vec<&str> = result.lines().collect();
        assert!(result_lines.len() <= 30, "result exceeds budget");
    }

    /// SUMO-anchored text preserves memory/cognitive keywords.
    #[test]
    fn sumo_anchor_preserves_memory_keywords() {
        let input = concat!(
            "Memory Operation Report\n",
            "Encoding completed successfully for all events.\n",
            "Memory consolidation is now in progress.\n",
            "Salience ranking identified 3 high-priority memories.\n",
            "Cued recall returned 7 matching contexts.\n",
            "General system health check passed.\n",
            "Routine maintenance scheduled for tomorrow.\n",
            "Disk usage is at 42% capacity.\n",
            "Network latency is within normal bounds.\n",
        );
        let algo = WordRankAlgorithm;
        let anchor = Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Sumo,
            concept: hkask_bridge_ontology::sumo::PROCESS.into(),
        });
        let (result, _) = algo.compress(
            input,
            Profile::Soft,
            ContextCategory::LogOutput,
            anchor.as_ref(),
        );
        // SUMO anchor should prioritize lines with cognitive/memory keywords
        // Soft profile (60% retention): enough budget for multiple lines
        let preserved_keywords = result.contains("Encoding")
            || result.contains("Salience")
            || result.contains("Cued recall")
            || result.contains("memory");
        assert!(
            preserved_keywords,
            "at least one cognitive/memory keyword should be preserved: {result}"
        );
        // Consolidation/memory line should be present
        assert!(
            result.contains("consolidation") || result.contains("memory"),
            "core memory concept not preserved: {result}"
        );
    }

    /// Core anchor (no domain) produces baseline results — no bonus applied.
    #[test]
    fn core_anchor_no_domain_bonus() {
        let input = concat!(
            "System Log\n",
            "error: database connection timeout\n",
            "info: service restarted successfully\n",
            "info: 42 requests processed\n",
            "debug: cache hit ratio 0.87\n",
            "info: user session created\n",
            "warning: approaching rate limit\n",
            "error: downstream service unavailable\n",
            "info: health check passed\n",
            "debug: garbage collection completed\n",
        );
        let algo = WordRankAlgorithm;
        // Core anchor — no domain bonus, but error structural bonus (2.0) and warning bonus (1.0) apply
        let (result, _) = algo.compress(
            input,
            Profile::Soft,
            ContextCategory::LogOutput,
            Some(OntologyAnchor::Core).as_ref(),
        );
        // Errors (2.0 structural bonus) and warnings (1.0 bonus) always preserved regardless of anchor
        assert!(
            result.contains("error"),
            "error lines must be preserved even with core anchor"
        );
        assert!(
            result.contains("warning"),
            "warning lines must be preserved: {result}"
        );
        // No domain-specific bonuses expected with core anchor
        assert!(result.lines().count() <= 30);
    }

    /// Test that `select_ontology_anchor` + compress produces correct anchors end-to-end.
    #[test]
    fn derive_and_compress_with_correct_anchor() {
        // Simulate the engine's workflow: tool_name → derive anchor → compress
        let tool_name = "corpus_chunk";
        let anchor = select_ontology_anchor(tool_name);
        assert_eq!(
            anchor,
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Golem,
                concept: hkask_bridge_ontology::golem::WORK.to_string()
            }
        );

        let input = "The character enters the scene, and the narrative turns on that event.";
        let algo = WordRankAlgorithm;
        let (result, _) = algo.compress(
            input,
            Profile::Normal,
            ContextCategory::StructuredData,
            Some(&anchor),
        );
        // With the GOLEM narrative anchor, narrative vocabulary is high-signal.
        // Normal profile (20% retention): short enough to pass through
        assert!(!result.is_empty());
        // The result should include the narrative terms
        // (with Normal profile on short input, it may pass through entirely)
        let has_narrative = result.contains("character")
            || result.contains("scene")
            || result.contains("narrative");
        assert!(has_narrative, "narrative content not preserved: {result}");
    }

    // ── Property-based tests (Wave 2) ─────────────────────────────────────

    use proptest::prelude::*;
    use proptest::sample::select;

    /// Strategy: generate a non-empty string with varied content.
    fn arbitrary_input() -> BoxedStrategy<String> {
        proptest::arbitrary::any::<String>()
            .prop_filter("must be non-empty", |s| !s.is_empty())
            .boxed()
    }

    /// Strategy: any compression profile.
    fn profile_strategy() -> BoxedStrategy<Profile> {
        select(&[
            Profile::Heavy,
            Profile::Normal,
            Profile::Soft,
            Profile::Light,
        ])
        .boxed()
    }

    /// Strategy: any context category.
    fn category_strategy() -> BoxedStrategy<ContextCategory> {
        select(&[
            ContextCategory::ShellCommand,
            ContextCategory::TestOutput,
            ContextCategory::BuildOutput,
            ContextCategory::FileContents,
            ContextCategory::ConversationHistory,
            ContextCategory::StructuredData,
            ContextCategory::LogOutput,
            ContextCategory::Unknown,
        ])
        .boxed()
    }

    // For any input, re-compressing the output produces the same result.
    proptest! {
        #[test]
        fn compression_is_idempotent(
            input in arbitrary_input(),
            profile in profile_strategy(),
            category in category_strategy(),
        ) {
            let algo = RtkStyleAlgorithm;
            let (first, _) = algo.compress(&input, profile, category, None);
            let (second, _) = algo.compress(&first, profile, category, None);
            let first_len = first.len();
            let second_len = second.len();
            prop_assert_eq!(first, second,
                "re-compression changed output: first={} bytes, second={} bytes",
                first_len, second_len);
        }
    }

    // Compression never produces output larger than input.
    proptest! {
        #[test]
        fn compression_never_expands(
            input in arbitrary_input(),
            profile in profile_strategy(),
            category in category_strategy(),
        ) {
            let algo = RtkStyleAlgorithm;
            let (compressed, _) = algo.compress(&input, profile, category, None);
            prop_assert!(compressed.len() <= input.len(),
                "compressed {} > original {}", compressed.len(), input.len());
        }
    }

    // Flashrank's greedy marginal-utility selection works on any content type — it must never
    // expand input even when given arbitrary Unknown-category content.
    proptest! {
        #[test]
        fn flashrank_fallback_never_expands(
            input in arbitrary_input(),
            profile in profile_strategy(),
        ) {
            let algo = FlashrankAlgorithm;
            let (compressed, _) =
                algo.compress(&input, profile, ContextCategory::Unknown, None);
            prop_assert!(compressed.len() <= input.len(),
                "flashrank fallback expanded: compressed {} > original {}", compressed.len(), input.len());
        }
    }

    // compute_budget: budget never exceeds input, respects max_lines cap,
    // and passthrough flag is consistent. Subsumes the 5 hardcoded compute_budget_* tests.
    proptest! {
        #[test]
        fn compute_budget_invariants(
            lines in 0usize..10_000usize,
            profile in profile_strategy(),
        ) {
            let (budget, passthrough) = compute_budget(lines, profile);

            prop_assert!(budget <= lines,
                "budget exceeds input: budget={}, lines={}", budget, lines);

            if let Some(max) = profile.max_lines() {
                prop_assert!(budget <= max,
                    "budget exceeds max_lines: budget={}, max={}, profile={:?}", budget, max, profile);
            }

            prop_assert_eq!(passthrough, budget >= lines,
                "passthrough flag inconsistent: budget={}, lines={}, passthrough={}",
                budget, lines, passthrough);
        }
    }

    #[test]
    fn structured_json_array_preserves_parse_validity_boundaries_extrema_and_outliers() {
        use crate::engine::CondenserEngine;
        use serde_json::{Value, json};

        let mut rows = Vec::with_capacity(50);
        for idx in 0..50usize {
            let val = if idx == 17 {
                -5.548 // GFC trough
            } else if idx == 33 {
                8.979 // CPI peak
            } else {
                2.0 + (idx % 5) as f64 * 0.25
            };
            let coord = if idx == 25 {
                Value::Null // rare-null territory outlier
            } else {
                json!(48.0 + idx as f64 * 0.1)
            };
            let err = if idx == 41 {
                json!("alignment drift detected")
            } else {
                Value::Null
            };
            rows.push(json!({
                "idx": idx,
                "date": format!("2000-{:02}-01", (idx % 12) + 1),
                "value": val,
                "latitude": coord,
                "alignment_error": err,
            }));
        }

        let envelope = json!({
            "content": {
                "series_id": "MACRO_TEST",
                "returned": 50,
                "observations": rows,
                "ontology": {
                    "pko_procedure": hkask_bridge_ontology::pko::STEP_EXECUTION,
                    "dc_type": hkask_bridge_ontology::dc_bibo::DATASET
                }
            },
            "duration_ms": 42,
            "usage": null
        });

        let mut engine = CondenserEngine::new();
        for (format_name, serialized) in [
            ("compact", serde_json::to_string(&envelope).unwrap()),
            ("pretty", serde_json::to_string_pretty(&envelope).unwrap()),
        ] {
            let res = engine.compress("fred_get_observations", &serialized, None);
            assert_eq!(res.algorithm, "structured_json_array", "{format_name}");
            assert_eq!(res.category, "structured_data", "{format_name}");
            assert!(
                !res.content.contains("\n...\n"),
                "{format_name} must never emit bare ellipsis lines"
            );
            assert!(
                res.reduction_pct >= 55.0,
                "{format_name} reduction {:.2}% below 55%",
                res.reduction_pct
            );

            let parsed: Value = serde_json::from_str(&res.content)
                .unwrap_or_else(|e| panic!("{format_name} invalid JSON: {e}"));
            assert_eq!(parsed["content"]["series_id"], "MACRO_TEST");
            assert_eq!(parsed["content"]["returned"], 50);

            let arr = parsed["content"]["observations"]
                .as_array()
                .expect("observations array");
            let mut kept_indices = Vec::new();
            let mut elided_sum = 0usize;
            for entry in arr {
                if let Some(elided) = entry.get("_kask_elided_count").and_then(|v| v.as_u64()) {
                    elided_sum += elided as usize;
                } else if let Some(idx) = entry.get("idx").and_then(|v| v.as_u64()) {
                    kept_indices.push(idx as usize);
                }
            }

            // Conservation of cardinality: kept records + elided counts == 50.
            assert_eq!(
                kept_indices.len() + elided_sum,
                50,
                "{format_name} cardinality mismatch"
            );
            for required_idx in [0usize, 1, 17, 25, 33, 41, 48, 49] {
                assert!(
                    kept_indices.contains(&required_idx),
                    "{format_name} lost required boundary/extrema/outlier index {required_idx}: {kept_indices:?}"
                );
            }
        }
    }

    #[test]
    fn structured_json_array_skips_primitive_arrays_and_non_structured_overrides() {
        use crate::engine::CondenserEngine;
        use serde_json::json;

        let primitive_tags = json!({
            "id": "evt_1",
            "tags": (0..25).map(|i| vec!["r".to_string(), format!("wss://relay-{i}.example")]).collect::<Vec<_>>()
        })
        .to_string();

        let mut engine = CondenserEngine::new();
        let res = engine.compress("nostr_relay_query", &primitive_tags, None);
        assert_eq!(res.algorithm, "structured_json_array");
        assert_eq!(res.content, primitive_tags);
        assert!(!res.content.contains("_kask_elided_count"));

        let obj_array = json!({
            "items": (0..20).map(|i| json!({"id": i, "text": "payload_entry_for_shell_override"})).collect::<Vec<_>>()
        })
        .to_string();
        let override_res = engine.compress(
            "bash_execute",
            &obj_array,
            Some(ContextCategory::ShellCommand),
        );
        assert_eq!(override_res.algorithm, "rtk_style");
    }

    #[test]
    fn structured_json_array_64kib_latency_within_budget() {
        use crate::engine::CondenserEngine;
        use serde_json::{Value, json};
        use std::time::Instant;

        let items: Vec<Value> = (0..1050)
            .map(|i| {
                json!({
                    "date": format!("2010-{:02}-{:02}", (i % 12) + 1, (i % 28) + 1),
                    "country": "United States",
                    "value": if i == 300 { -12.4 } else if i == 700 { 19.8 } else { (i % 10) as f64 * 0.3 }
                })
            })
            .collect();
        let payload = json!({
            "content": {
                "series_id": "LATENCY_64KIB",
                "observations": items
            },
            "duration_ms": 15,
            "usage": null
        })
        .to_string();
        assert!(
            payload.len() >= 65_536,
            "expected >= 64 KiB payload, got {} bytes",
            payload.len()
        );

        let mut engine = CondenserEngine::new();
        let mut samples_ms = Vec::with_capacity(30);
        for _ in 0..30 {
            let t0 = Instant::now();
            let out = engine.compress("fred_get_observations", &payload, None);
            samples_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(out.algorithm, "structured_json_array");
            assert!(out.reduction_pct >= 80.0);
            assert!(serde_json::from_str::<Value>(&out.content).is_ok());
        }
        samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p95_ms = samples_ms[(samples_ms.len() * 95 / 100).min(samples_ms.len() - 1)];
        let ceiling_ms = if cfg!(debug_assertions) { 25.0 } else { 1.5 };
        assert!(
            p95_ms <= ceiling_ms,
            "64 KiB p95 latency {p95_ms:.3} ms exceeded {ceiling_ms:.1} ms ceiling"
        );
    }

    proptest! {
        #[test]
        fn structured_json_array_is_idempotent_and_never_expands(
            vals in proptest::collection::vec(-1000i64..1000i64, 0..60),
            pretty in proptest::bool::ANY,
            profile in profile_strategy(),
        ) {
            use crate::engine::CondenserEngine;
            use serde_json::{Value, json};

            let items: Vec<Value> = vals
                .iter()
                .enumerate()
                .map(|(idx, &v)| {
                    json!({
                        "idx": idx,
                        "label": format!("item_{idx}_payload"),
                        "value": v,
                        "warning": if v < -900 { Value::String("extreme_low".into()) } else { Value::Null }
                    })
                })
                .collect();
            let doc = json!({
                "content": {
                    "count": items.len(),
                    "records": items
                }
            });
            let input = if pretty {
                serde_json::to_string_pretty(&doc).unwrap()
            } else {
                serde_json::to_string(&doc).unwrap()
            };

            let mut engine = CondenserEngine::new();
            engine.set_profile(profile);

            let first = engine.compress("wb_get_observations", &input, None);
            let second = engine.compress("wb_get_observations", &first.content, None);

            prop_assert!(
                first.content.len() <= input.len(),
                "expanded: {} > {}",
                first.content.len(),
                input.len()
            );
            prop_assert_eq!(
                &first.content,
                &second.content,
                "not idempotent under profile {:?}",
                profile
            );
            prop_assert!(!first.content.contains("\n...\n"));
            let parsed: Value = serde_json::from_str(&first.content)
                .expect("compressed JSON must remain valid JSON");
            if let Some(arr) = parsed["content"]["records"].as_array() {
                let mut total = 0usize;
                for item in arr {
                    if let Some(c) = item.get("_kask_elided_count").and_then(|x| x.as_u64()) {
                        total += c as usize;
                    } else {
                        total += 1;
                    }
                }
                prop_assert_eq!(total, vals.len(), "cardinality not conserved");
            }
        }
    }
}
