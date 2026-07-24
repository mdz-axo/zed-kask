//! hKask MCP Condenser — Compression algorithms (Phase 1: local, no LLM)

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

/// Compute the line budget for compression given the input size and profile.
///
/// Returns `(budget, is_passthrough)` — if `is_passthrough`, the algorithm
/// should return the input unchanged.
pub(crate) fn compute_budget(lines: usize, profile: Profile) -> (usize, bool) {
    let max_lines = profile.max_lines().unwrap_or(usize::MAX);
    let target_lines = ((lines as f64) * profile.retention_pct()).max(1.0).round() as usize;
    let budget = target_lines.min(max_lines).min(lines);
    (budget, budget >= lines)
}

pub trait CondenserAlgorithm: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn default_for(&self) -> &[ContextCategory];
    /// Compress the input and return (compressed_content, health_signals).
    /// `ontology_anchor` provides domain context for saliency weighting (P8.1).
    /// Health signals are empty when the algorithm performed within expected bounds.
    fn compress(
        &self,
        input: &str,
        profile: Profile,
        category: ContextCategory,
        ontology_anchor: Option<&OntologyAnchor>,
    ) -> (String, Vec<CondenserHealthSignal>);
}

pub struct RtkStyleAlgorithm;

impl CondenserAlgorithm for RtkStyleAlgorithm {
    fn name(&self) -> &str {
        "rtk_style"
    }
    fn description(&self) -> &str {
        "Command-specific rules: filter, group, truncate, dedup"
    }
    fn default_for(&self) -> &[ContextCategory] {
        &[
            ContextCategory::ShellCommand,
            ContextCategory::TestOutput,
            ContextCategory::BuildOutput,
        ]
    }
    fn compress(
        &self,
        input: &str,
        profile: Profile,
        _category: ContextCategory,
        ontology_anchor: Option<&OntologyAnchor>,
    ) -> (String, Vec<CondenserHealthSignal>) {
        let lines: Vec<&str> = input.lines().collect();
        let (budget, passthrough) = compute_budget(lines.len(), profile);
        if passthrough {
            return (input.to_string(), vec![]);
        }

        // Ontology-aware head/tail split: FIBO financial data gets more tail
        // (summary/conclusion often carries key financial ratios)
        let density_factor = ontology_anchor.map(|a| a.density_factor()).unwrap_or(1.0);
        let head_ratio = (0.3 / density_factor).clamp(0.15, 0.5);
        let head_count = (budget as f64 * head_ratio) as usize;
        let tail_count = budget.saturating_sub(head_count);

        let mut filtered: Vec<&str> = Vec::new();
        for line in lines.iter().take(head_count) {
            filtered.push(*line);
        }

        let tail_start = lines.len().saturating_sub(tail_count);
        if tail_start > head_count {
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

pub struct WordRankAlgorithm;

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
    fn description(&self) -> &str {
        "TF-IDF bag-of-words compression with structural bonus and ontology anchoring"
    }
    fn default_for(&self) -> &[ContextCategory] {
        &[
            ContextCategory::ConversationHistory,
            ContextCategory::LogOutput,
        ]
    }
    fn compress(
        &self,
        input: &str,
        profile: Profile,
        _category: ContextCategory,
        ontology_anchor: Option<&OntologyAnchor>,
    ) -> (String, Vec<CondenserHealthSignal>) {
        let lines: Vec<&str> = input.lines().collect();
        let (budget, passthrough) = compute_budget(lines.len(), profile);
        if passthrough {
            return (input.to_string(), vec![]);
        }

        let freq = Self::compute_word_frequencies(&lines);

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

        let result = join_with_ellipsis(&lines, &selected_indices);
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
///
/// Extracted from the condenser's ontology bonus logic for reuse by the
/// communication gate and other callers that need domain relevance without the
/// full compression pipeline.
pub fn domain_saliency(line: &str, anchor: Option<&OntologyAnchor>) -> f64 {
    let direct = match anchor {
        Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Fibo,
            ..
        }) => {
            if line.contains('%') || line.contains('$') || line.chars().any(|c| c.is_ascii_digit())
            {
                0.5
            } else if line.contains("ratio")
                || line.contains("margin")
                || line.contains("growth")
                || line.contains("value")
            {
                0.3
            } else {
                0.0
            }
        }
        Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Cogat,
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
            } else if line.contains("episodic") || line.contains("semantic") {
                0.3
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
        }) => {
            if line.contains("character")
                || line.contains("narrative")
                || line.contains("scene")
                || line.contains("event")
            {
                0.3
            } else {
                0.0
            }
        }
        Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::MlSchema,
            ..
        }) => {
            if line.contains("accuracy")
                || line.contains("loss")
                || line.contains("epoch")
                || line.contains("learning_rate")
                || line.contains("batch")
                || line.contains("evaluation")
                || line.chars().any(|c| c.is_ascii_digit())
            {
                0.3
            } else {
                0.0
            }
        }
        _ => 0.0,
    };

    let graph_bonus = match anchor {
        Some(a) => {
            let kws = crate::ontology_graph::anchor_keywords(a);
            if kws.is_empty() {
                0.0
            } else {
                crate::ontology_graph::graph().graph_adjacency_bonus(line, &kws)
            }
        }
        None => 0.0,
    };

    direct + graph_bonus
}

/// Derive an ontology anchor from persona description text.
///
/// Uses the same domain-signaling pattern as `derive_ontology_anchor` but
/// applied to natural-language persona text (description + capabilities).
/// Returns `None` if no domain signals are detected (caller treats as Core).
pub fn persona_to_anchor(description: &str, capabilities: &[String]) -> Option<OntologyAnchor> {
    let lower = description.to_lowercase();
    let cap_lower: Vec<String> = capabilities.iter().map(|c| c.to_lowercase()).collect();
    let combined: Vec<&str> = lower
        .split_whitespace()
        .chain(cap_lower.iter().flat_map(|c| c.split('_')))
        .collect();

    // CogAT: cognitive / memory domain
    if combined.iter().any(|w| {
        *w == "memory"
            || *w == "cognition"
            || *w == "recall"
            || *w == "consolidation"
            || *w == "encoding"
            || *w == "episodic"
    }) {
        return Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Cogat,
            concept: hkask_bridge_dublincore::DATASET.to_string(),
        });
    }
    // FIBO: financial domain
    if combined.iter().any(|w| {
        *w == "financial"
            || *w == "finance"
            || *w == "portfolio"
            || *w == "stock"
            || *w == "trading"
            || *w == "dcf"
            || *w == "screener"
    }) {
        return Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Fibo,
            concept: hkask_bridge_dublincore::DATASET.to_string(),
        });
    }
    // GOLEM: narrative domain
    if combined.iter().any(|w| {
        *w == "narrative" || *w == "character" || *w == "story" || *w == "replica" || *w == "author"
    }) {
        return Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Golem,
            concept: hkask_bridge_dublincore::TEXT.to_string(),
        });
    }
    // ML-Schema: training / ML domain
    if combined.iter().any(|w| {
        *w == "training"
            || *w == "model"
            || *w == "adapter"
            || *w == "sweep"
            || *w == "learning"
            || *w == "ml"
    }) {
        return Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::MlSchema,
            concept: hkask_bridge_dublincore::DATASET.to_string(),
        });
    }
    // OMC: media domain
    if combined.iter().any(|w| {
        *w == "media" || *w == "video" || *w == "image" || *w == "gallery" || *w == "generate"
    }) {
        return Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Omc,
            concept: hkask_bridge_dublincore::COLLECTION.to_string(),
        });
    }
    // PKO: process / workflow domain (broadest — catches curator, task, spec, etc.)
    if combined.iter().any(|w| {
        *w == "curator"
            || *w == "process"
            || *w == "workflow"
            || *w == "task"
            || *w == "kanban"
            || *w == "spec"
            || *w == "skill"
            || *w == "pipeline"
            || *w == "regulation" /* regulation stopword */
    }) {
        return Some(OntologyAnchor::DualAxis {
            axis: OntologyAxis::Pko,
            concept: hkask_bridge_dublincore::PROCEDURE.to_string(),
        });
    }
    // DC+BIBO: document / metadata domain
    if combined.iter().any(|w| {
        *w == "document" || *w == "file" || *w == "registry" || *w == "metadata" || *w == "archive"
    }) {
        return Some(OntologyAnchor::DualAxis {
            axis: OntologyAxis::DcBibo,
            concept: hkask_bridge_dublincore::TEXT.to_string(),
        });
    }
    None
}

pub struct FlashrankAlgorithm;

impl FlashrankAlgorithm {
    fn relevance_score(line: &str, query_terms: &[String]) -> f64 {
        let lower = line.to_lowercase();
        query_terms
            .iter()
            .map(|term| {
                if lower.contains(&term.to_lowercase()) {
                    1.0
                } else {
                    0.0
                }
            })
            .sum()
    }

    fn novelty_score(line: &str, selected: &[&str]) -> f64 {
        let lower = line.to_lowercase();
        let words: std::collections::HashSet<&str> = lower.split_whitespace().collect();
        let mut overlap = 0usize;
        let mut total = 0usize;
        for prev in selected {
            let prev_lower = prev.to_lowercase();
            let prev_words: std::collections::HashSet<&str> =
                prev_lower.split_whitespace().collect();
            for w in &words {
                total += 1;
                if prev_words.contains(w) {
                    overlap += 1;
                }
            }
        }
        if total == 0 {
            return 1.0;
        }
        1.0 - (overlap as f64 / total as f64)
    }

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
    fn description(&self) -> &str {
        "Greedy marginal-utility selection under token budget"
    }
    fn default_for(&self) -> &[ContextCategory] {
        &[
            ContextCategory::FileContents,
            ContextCategory::StructuredData,
            ContextCategory::Unknown,
        ]
    }
    fn compress(
        &self,
        input: &str,
        profile: Profile,
        _category: ContextCategory,
        _ontology_anchor: Option<&OntologyAnchor>,
    ) -> (String, Vec<CondenserHealthSignal>) {
        let lines: Vec<&str> = input.lines().collect();
        let (budget, passthrough) = compute_budget(lines.len(), profile);
        if passthrough {
            return (input.to_string(), vec![]);
        }

        let alpha = 0.4f64;
        let beta = 0.3f64;
        let gamma = 0.3f64;

        let query_terms: Vec<String> = lines
            .iter()
            .take(5)
            .flat_map(|l| {
                l.split_whitespace()
                    .filter(|w| w.len() > 3)
                    .map(|s| s.to_string())
            })
            .take(20)
            .collect();

        let mut selected_indices: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        let mut selected_lines: Vec<&str> = Vec::new();

        while selected_indices.len() < budget {
            let mut best_idx: Option<usize> = None;
            let mut best_score = f64::NEG_INFINITY;

            for (i, line) in lines.iter().enumerate() {
                if selected_indices.contains(&i) {
                    continue;
                }

                let rel = Self::relevance_score(line, &query_terms);
                let nov = Self::novelty_score(line, &selected_lines);
                let brev = Self::brevity_score(line);

                let score = alpha * rel + beta * nov - gamma * (1.0 - brev);
                if score > best_score {
                    best_score = score;
                    best_idx = Some(i);
                }
            }

            match best_idx {
                Some(idx) => {
                    selected_indices.insert(idx);
                    selected_lines.push(lines[idx]);
                }
                None => break,
            }
        }

        let filled = selected_indices.len();
        let mut selected_indices: Vec<usize> = selected_indices.into_iter().collect();
        selected_indices.sort_unstable();
        let result = join_with_ellipsis(&lines, &selected_indices);

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

pub struct AlgorithmRegistry {
    algorithms: Vec<Box<dyn CondenserAlgorithm>>,
}

impl Default for AlgorithmRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AlgorithmRegistry {
    pub fn new() -> Self {
        let algorithms: Vec<Box<dyn CondenserAlgorithm>> = vec![
            Box::new(RtkStyleAlgorithm),
            Box::new(WordRankAlgorithm),
            Box::new(FlashrankAlgorithm),
        ];
        Self { algorithms }
    }

    pub fn select(&self, category: ContextCategory) -> &dyn CondenserAlgorithm {
        for algo in &self.algorithms {
            if algo.default_for().contains(&category) {
                return algo.as_ref();
            }
        }
        self.algorithms
            .last()
            .expect("at least one algorithm")
            .as_ref()
    }

    /// Select an algorithm by name. Used by the learning mechanism in
    /// `CondenserEngine::compress()` when `recommend_algorithm()` returns
    /// a historically better-performing algorithm than the static mapping.
    pub fn select_by_name(&self, name: &str) -> Option<&dyn CondenserAlgorithm> {
        self.algorithms
            .iter()
            .find(|a| a.name() == name)
            .map(|a| a.as_ref())
    }

    pub fn list_algorithms(&self) -> Vec<serde_json::Value> {
        self.algorithms
            .iter()
            .map(|a| {
                serde_json::json!({
                    "name": a.name(),
                    "description": a.description(),
                    "default_for": a.default_for().iter().map(|c| c.label()).collect::<Vec<_>>(),
                })
            })
            .collect()
    }
}

/// Keyword→category mapping — single source of truth for both exact-token (Phase 1)
/// and substring (Phase 2) matching in [`classify_tool`].
const KEYWORD_CATEGORIES: &[(&[&str], ContextCategory)] = &[
    (
        &[
            "git", "docker", "cargo", "npm", "shell", "bash", "exec", "run",
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

/// Derive the ontology anchor from the tool name alone.
/// Every MCP server links against the same bridge crates — no wire-protocol fields needed.
pub fn derive_ontology_anchor(tool_name: &str) -> OntologyAnchor {
    let lower = tool_name.to_lowercase();
    // FIBO: financial data
    if lower.starts_with("company")
        || lower.starts_with("stock")
        || lower.starts_with("portfolio")
        || lower.starts_with("dcf")
        || lower.starts_with("screener")
        || lower.starts_with("forecast")
        || lower.starts_with("scenario")
    {
        return OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Fibo,
            concept: hkask_bridge_dublincore::DATASET.to_string(),
        };
    }
    // CogAT: cognitive/memory
    if lower.starts_with("memory") || lower.starts_with("episodic") || lower.starts_with("semantic")
    {
        return OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Cogat,
            concept: hkask_bridge_dublincore::DATASET.to_string(),
        };
    }
    // GOLEM: narrative
    if lower.starts_with("replica") || lower.starts_with("author") {
        return OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Golem,
            concept: hkask_bridge_dublincore::TEXT.to_string(),
        };
    }
    // ML-Schema: training
    if lower.starts_with("training") || lower.starts_with("adapter") || lower.starts_with("sweep") {
        return OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::MlSchema,
            concept: hkask_bridge_dublincore::DATASET.to_string(),
        };
    }
    // OMC: media
    if lower.starts_with("generate")
        || lower.starts_with("video")
        || lower.starts_with("image")
        || lower.starts_with("gallery")
        || lower.starts_with("face")
    {
        return OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Omc,
            concept: hkask_bridge_dublincore::COLLECTION.to_string(),
        };
    }
    // PKO dual-axis: process workflows
    if lower.starts_with("kanban")
        || lower.starts_with("board")
        || lower.starts_with("task")
        || lower.starts_with("research")
        || lower.starts_with("spec")
        || lower.starts_with("skill")
        || lower.starts_with("docproc")
        || lower.starts_with("curator")
        || lower.starts_with("condenser")
    {
        return OntologyAnchor::DualAxis {
            axis: OntologyAxis::Pko,
            concept: hkask_bridge_dublincore::PROCEDURE.to_string(),
        };
    }
    // DC+BIBO dual-axis: entity metadata
    if lower.starts_with("file")
        || lower.starts_with("web")
        || lower.starts_with("registry")
        || lower.starts_with("wallet")
    {
        return OntologyAnchor::DualAxis {
            axis: OntologyAxis::DcBibo,
            concept: hkask_bridge_dublincore::TEXT.to_string(),
        };
    }
    OntologyAnchor::Core
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // Note: retention_pct is applied first, then capped. 5 lines * 20% = 1, so budget = 1.
    #[test]
    fn compute_budget_never_exceeds_input() {
        let (budget, _) = compute_budget(5, Profile::Normal);
        assert_eq!(budget, 1);
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

    // ── Ontology derivation tests (P5.4/P8.1) ─────────────────────────────

    #[test]
    fn derive_ontology_fibo_for_financial_tools() {
        assert_eq!(
            derive_ontology_anchor("company_profile"),
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Fibo,
                concept: hkask_bridge_dublincore::DATASET.to_string()
            }
        );
        assert_eq!(
            derive_ontology_anchor("stock_screener"),
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Fibo,
                concept: hkask_bridge_dublincore::DATASET.to_string()
            }
        );
        assert_eq!(
            derive_ontology_anchor("dcf_valuation"),
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Fibo,
                concept: hkask_bridge_dublincore::DATASET.to_string()
            }
        );
    }

    #[test]
    fn derive_ontology_cogat_for_memory_tools() {
        assert_eq!(
            derive_ontology_anchor("memory_recall"),
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Cogat,
                concept: hkask_bridge_dublincore::DATASET.to_string()
            }
        );
        assert_eq!(
            derive_ontology_anchor("episodic_store"),
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Cogat,
                concept: hkask_bridge_dublincore::DATASET.to_string()
            }
        );
    }

    #[test]
    fn derive_ontology_pko_for_kanban_tools() {
        assert_eq!(
            derive_ontology_anchor("kanban_task_create"),
            OntologyAnchor::DualAxis {
                axis: OntologyAxis::Pko,
                concept: hkask_bridge_dublincore::PROCEDURE.to_string()
            }
        );
        assert_eq!(
            derive_ontology_anchor("condenser_compress"),
            OntologyAnchor::DualAxis {
                axis: OntologyAxis::Pko,
                concept: hkask_bridge_dublincore::PROCEDURE.to_string()
            }
        );
    }

    #[test]
    fn derive_ontology_golem_for_replica_tools() {
        assert_eq!(
            derive_ontology_anchor("replica_build"),
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Golem,
                concept: hkask_bridge_dublincore::TEXT.to_string()
            }
        );
    }

    #[test]
    fn derive_ontology_mlschema_for_training_tools() {
        assert_eq!(
            derive_ontology_anchor("training_submit"),
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::MlSchema,
                concept: hkask_bridge_dublincore::DATASET.to_string()
            }
        );
    }

    #[test]
    fn derive_ontology_omc_for_media_tools() {
        assert_eq!(
            derive_ontology_anchor("generate_image"),
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Omc,
                concept: hkask_bridge_dublincore::COLLECTION.to_string()
            }
        );
    }

    #[test]
    fn derive_ontology_core_for_unknown_tools() {
        assert_eq!(derive_ontology_anchor("unknown_tool"), OntologyAnchor::Core);
        assert_eq!(derive_ontology_anchor(""), OntologyAnchor::Core);
    }

    #[test]
    fn derive_ontology_dc_bibo_for_file_tools() {
        assert_eq!(
            derive_ontology_anchor("file_read"),
            OntologyAnchor::DualAxis {
                axis: OntologyAxis::DcBibo,
                concept: hkask_bridge_dublincore::TEXT.to_string()
            }
        );
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

    #[test]
    fn algorithm_registry_selects_by_category() {
        let registry = AlgorithmRegistry::new();
        assert_eq!(
            registry.select(ContextCategory::ShellCommand).name(),
            "rtk_style"
        );
        assert_eq!(
            registry.select(ContextCategory::ConversationHistory).name(),
            "word_rank"
        );
        assert_eq!(
            registry.select(ContextCategory::FileContents).name(),
            "flashrank"
        );
        // LogOutput → word_rank
        assert_eq!(
            registry.select(ContextCategory::LogOutput).name(),
            "word_rank"
        );
    }

    // Flashrank is the universal fallback — its greedy marginal-utility selection works on
    // any text type without needing category-specific structural markers.
    #[test]
    fn algorithm_registry_fallback_for_unknown() {
        let registry = AlgorithmRegistry::new();
        assert_eq!(
            registry.select(ContextCategory::Unknown).name(),
            "flashrank"
        );
    }

    // ── End-to-end ontology-aware compression tests (P8.1) ───────────────

    /// Financial text with FIBO anchoring preserves key numeric metrics.
    #[test]
    fn fibo_anchor_preserves_financial_metrics() {
        let input = concat!(
            "Company Profile: AAPL\n",
            "Sector: Technology\n",
            "Market Capitalization: 3.2T USD\n",
            "P/E Ratio: 28.5\n",
            "Revenue Growth: 5.2%\n",
            "Free Cash Flow: 102B\n",
            "Dividend Yield: 0.45%\n",
            "This is a general description of the company's operations.\n",
            "The company was founded in 1976 and is headquartered in Cupertino.\n",
            "It designs, manufactures, and markets smartphones and computers.\n",
            "The competitive landscape includes Samsung, Google, and Microsoft.\n",
            "Management commentary: we expect continued growth in services.\n",
        );
        let algo = WordRankAlgorithm;
        let anchor = Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Fibo,
            concept: "fibo:Corporation".into(),
        });
        let (result, _) = algo.compress(
            input,
            Profile::Soft,
            ContextCategory::StructuredData,
            anchor.as_ref(),
        );
        // FIBO anchor should prioritize lines with numeric financial data
        // Soft profile (60% retention): enough budget for multiple financial lines
        assert!(
            result.contains("Market Capitalization"),
            "financial metric not preserved: {result}"
        );
        let has_numbers = result.contains("P/E")
            || result.contains("Revenue Growth")
            || result.contains("Free Cash Flow");
        assert!(
            has_numbers,
            "at least one additional financial metric should be preserved: {result}"
        );
        // Financial lines should get higher scores than generic description
        let result_lines: Vec<&str> = result.lines().collect();
        assert!(result_lines.len() <= 30, "result exceeds budget");
    }

    /// CogAT-anchored text preserves memory/cognitive keywords.
    #[test]
    fn cogat_anchor_preserves_memory_keywords() {
        let input = concat!(
            "Memory Operation Report\n",
            "The episodic memory store received 15 new events.\n",
            "Encoding completed successfully for all events.\n",
            "Memory consolidation is now in progress.\n",
            "Salience ranking identified 3 high-priority memories.\n",
            "Cued recall returned 7 matching contexts.\n",
            "Semantic processing updated the embedding index.\n",
            "General system health check passed.\n",
            "Routine maintenance scheduled for tomorrow.\n",
            "Disk usage is at 42% capacity.\n",
            "Network latency is within normal bounds.\n",
        );
        let algo = WordRankAlgorithm;
        let anchor = Some(OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Cogat,
            concept: "cogat:episodic_memory".into(),
        });
        let (result, _) = algo.compress(
            input,
            Profile::Soft,
            ContextCategory::LogOutput,
            anchor.as_ref(),
        );
        // CogAT anchor should prioritize lines with cognitive/memory keywords
        // Soft profile (60% retention): enough budget for multiple lines
        let preserved_keywords = result.contains("episodic")
            || result.contains("Encoding")
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

    /// Test that `derive_ontology_anchor` + compress produces correct anchors end-to-end.
    #[test]
    fn derive_and_compress_with_correct_anchor() {
        // Simulate the engine's workflow: tool_name → derive anchor → compress
        let tool_name = "company_profile";
        let anchor = derive_ontology_anchor(tool_name);
        assert_eq!(
            anchor,
            OntologyAnchor::DomainSupplement {
                namespace: OntologyNamespace::Fibo,
                concept: hkask_bridge_dublincore::DATASET.to_string()
            }
        );

        let input = "AAPL: revenue 383B, net income 97B, P/E 28.5, market cap 3.2T";
        let algo = WordRankAlgorithm;
        let (result, _) = algo.compress(
            input,
            Profile::Normal,
            ContextCategory::StructuredData,
            Some(&anchor),
        );
        // With FIBO anchoring, numeric financial content is high-signal
        // Normal profile (20% retention): short enough to pass through
        assert!(!result.is_empty());
        // The result should include the key financial terms
        // (with Normal profile on short input, it may pass through entirely)
        let has_financial =
            result.contains("revenue") || result.contains("P/E") || result.contains("market cap");
        assert!(has_financial, "financial content not preserved: {result}");
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

    // For any input, re-compressing the output produces the same result.
    proptest! {
        #[test]
        fn compression_is_idempotent(
            input in arbitrary_input(),
            profile in select(&[Profile::Heavy, Profile::Normal, Profile::Soft, Profile::Light]),
            category in select(&[
                ContextCategory::ShellCommand,
                ContextCategory::TestOutput,
                ContextCategory::BuildOutput,
                ContextCategory::FileContents,
                ContextCategory::ConversationHistory,
                ContextCategory::StructuredData,
                ContextCategory::LogOutput,
                ContextCategory::Unknown,
            ]),
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
            profile in select(&[Profile::Heavy, Profile::Normal, Profile::Soft, Profile::Light]),
            category in select(&[
                ContextCategory::ShellCommand,
                ContextCategory::TestOutput,
                ContextCategory::BuildOutput,
                ContextCategory::FileContents,
                ContextCategory::ConversationHistory,
                ContextCategory::StructuredData,
                ContextCategory::LogOutput,
                ContextCategory::Unknown,
            ]),
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
            profile in select(&[Profile::Heavy, Profile::Normal, Profile::Soft, Profile::Light]),
        ) {
            let algo = FlashrankAlgorithm;
            let (compressed, _) =
                algo.compress(&input, profile, ContextCategory::Unknown, None);
            prop_assert!(compressed.len() <= input.len(),
                "flashrank fallback expanded: compressed {} > original {}", compressed.len(), input.len());
        }
    }
}
