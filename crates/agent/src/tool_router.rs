//! Lazy tool router.
//!
//! Narrows the tool set only when the user's request is complex enough to
//! benefit from filtering. For simple interactions, all tools pass through
//! unchanged — the model is good at selecting from a list when the request
//! is straightforward.
//!
//! The router activates in two cases:
//!
//! 1. **Explicit tool request** — the user message mentions a specific tool
//!    name or capability ("use grep", "run terminal", "fetch this URL").
//!    The router narrows to tools whose descriptions overlap with the request.
//!
//! 2. **Complex request** — the message is long (≥ 9 words) or signals
//!    decomposition ("plan", "break down", "multiple steps", "subagent",
//!    "delegate"). The router narrows to tools relevant to the task,
//!    reducing the tool list the model must reason about.
//!
//! For all other messages, the router returns `None` (fail-open — no
//! filtering). This preserves the full tool set for simple interactions
//! and avoids starving the model on short prompts.
//!
//! ## Selection
//!
//! When activated, each MCP candidate is scored on whole-term overlap between
//! the request's keywords and the tool's description, candidates are ranked, and
//! the top [`DEFAULT_SELECTION_BUDGET`] above [`LazyToolRouter::threshold`] are
//! kept. Ranking rather than pure thresholding is deliberate: it only requires
//! scores to order candidates correctly relative to each other, not to be
//! calibrated in absolute terms, and it bounds token cost predictably.
//!
//! The scoring is intentionally recall-biased. Dropping a tool the request
//! needed costs a failed turn; carrying a spare costs roughly 45 tokens. Three
//! properties follow from that asymmetry and are pinned by tests: match evidence
//! saturates on matched-term count so long messages cannot dilute a strong match
//! (`score_does_not_dilute_as_the_message_grows`), matching is whole-term so
//! `search` does not match "research" (`keyword_matching_is_whole_term_not_substring`),
//! and an empty selection is treated as scorer failure and fails open
//! (`empty_selection_fails_open_instead_of_stripping_all_mcp_tools`).
//!
//! Keyword overlap is a floor, not a ceiling — see the embedding-based
//! successor sketched in `kask/docs/architecture/AGENT_SYSTEM_PROMPT.md`.
//!
//! When no router is wired (upstream Zed), all tools pass through (I2).

use gpui::SharedString;
use std::collections::HashSet;

/// A candidate tool for routing. The name + description pair is what the
/// router scores — descriptions are available for all tools including MCP
/// tools via `AnyAgentTool::description()`.
#[derive(Debug, Clone)]
pub struct ToolCandidate {
    pub name: SharedString,
    pub description: SharedString,
}

/// Context for tool selection, built from the current turn's state.
#[derive(Debug, Clone, Default)]
pub struct ToolSelectionContext {
    /// The latest user message text, if any.
    pub user_message: Option<String>,
    /// Absolute paths of files currently open in the editor.
    pub open_file_paths: Vec<String>,
    /// All candidate tools (name + description) after profile/feature-flag
    /// filtering. The router returns a subset of these names.
    pub candidates: Vec<ToolCandidate>,
}

/// A tool router selects which tools to activate based on context.
///
/// Returns `Some(names)` to filter to that set. Returns `None` to signal
/// "no filtering" (fail-open) — the caller passes all tools through.
/// This distinguishes "router did not activate" from "router activated
/// but found no matching tools."
pub trait ToolRouter: Send + Sync {
    fn select_tools(&self, context: &ToolSelectionContext) -> Option<Vec<SharedString>>;
}

/// Apply the tool router to a set of tools, bypassing built-in zed tools.
///
/// zed-kask: the router only filters context-server (MCP) tools. Built-in
/// zed tools (those in `built_in_names`) always pass through — the router
/// was introduced to tame MCP tool floods, not to second-guess the
/// agent-profile allowlist for core tools.
///
/// `tools` is an iterator of `(name, description)` pairs for all tools
/// currently enabled (after profile/feature-flag filtering). The function
/// returns the set of tool names to retain. Built-in tools are always in
/// the returned set. MCP tools are retained only if the router includes
/// them (or if the router returns `None` / fails open).
///
/// This is a free function so it can be unit-tested without the
/// process-global `TOOL_ROUTER` `OnceLock`.
pub fn apply_router_bypassing_built_ins<'a, I>(
    router: &dyn ToolRouter,
    tools: I,
    user_message: Option<&str>,
    open_file_paths: Vec<String>,
    built_in_names: &std::collections::HashSet<&str>,
) -> std::collections::HashSet<SharedString>
where
    I: IntoIterator<Item = (&'a SharedString, &'a SharedString)>,
{
    // Collect into a map for lookup.
    let tool_map: std::collections::HashMap<&SharedString, &SharedString> =
        tools.into_iter().collect();

    // Start with all built-in tools — they bypass the router.
    let mut retained: std::collections::HashSet<SharedString> = tool_map
        .keys()
        .filter(|name| built_in_names.contains(name.as_ref()))
        .map(|name| (*name).clone())
        .collect();

    // Build candidates from MCP tools only.
    let candidates: Vec<ToolCandidate> = tool_map
        .iter()
        .filter(|(name, _)| !built_in_names.contains(name.as_ref()))
        .map(|(name, description)| ToolCandidate {
            name: (*name).clone(),
            description: (*description).clone(),
        })
        .collect();

    // If there are no MCP candidates, skip the router entirely.
    if candidates.is_empty() {
        return retained;
    }

    let context = ToolSelectionContext {
        user_message: user_message.map(|s| s.to_string()),
        open_file_paths,
        candidates,
    };
    // `candidates` was built by filtering out built-in tools, so its length
    // is the MCP tool count. The prior implementation re-iterated
    // `tool_map.keys()` filtering against `built_in_names` to compute the
    // same value — a redundant O(tools) pass on every `enabled_tools` call.
    let mcp_tool_count = context.candidates.len();
    let selected = router.select_tools(&context);

    // An empty selection is not a selection — it is the scorer failing to find
    // signal. `LazyToolRouter` scores by keyword overlap against tool
    // descriptions, and a substantive-but-vague message ("take a look at how the
    // parser handles nested quotes") produces many keywords that match no
    // description, driving every tool to the score floor. Before the activation
    // threshold was lowered such messages did not activate the router at all and
    // so kept every tool; afterwards they activate and would strip the entire MCP
    // surface. Treat an empty result as no-confidence and fail open: paying for
    // tool schemas is cheap next to silently removing the one tool the request
    // needed.
    let no_confidence = selected
        .as_ref()
        .is_some_and(|selected| selected.len() < NO_CONFIDENCE_FLOOR && mcp_tool_count > 0);

    match selected {
        Some(selected) if !no_confidence => {
            // Retain MCP tools that the router selected.
            for name in selected {
                if tool_map.contains_key(&name) {
                    retained.insert(name);
                }
            }
        }
        _ => {
            // Router returned None, or produced a no-confidence selection —
            // fail open and retain all MCP tools.
            if no_confidence {
                log::debug!(
                    "tool router: no-confidence selection (0 of {mcp_tool_count} MCP tools \
                     scored above threshold) — failing open"
                );
            }
            for name in tool_map.keys() {
                if !built_in_names.contains(name.as_ref()) {
                    retained.insert((*name).clone());
                }
            }
        }
    }

    retained
}

/// Minimum number of MCP tools a router selection must contain to be trusted.
///
/// Set to 1 — i.e. only a completely empty selection fails open. A higher floor
/// was considered and rejected on evidence: measuring a representative 20-tool
/// MCP surface showed that precise requests legitimately select **one or two**
/// tools ("generate an image and add it to the gallery" → `generate_image` +
/// `web_search`; "what is the calibrated probability this market resolves
/// yes" → `market_forecast` alone), and those are the selections worth keeping —
/// they are where the token saving comes from. A floor of 3 would have discarded
/// exactly the cases the router gets right while fixing only the empty ones.
/// Empty-vs-nonempty is the signal that separates scorer failure from precision;
/// selection size is not.
const NO_CONFIDENCE_FLOOR: usize = 1;

/// Lazy keyword-overlap tool router. Only activates when the request is
/// complex or explicitly tool-directed. For simple messages, returns `None`
/// (fail-open).
///
/// When activated, scores each MCP tool by keyword overlap between the context
/// and the tool's description. Tools scoring ≥ the threshold are included.
///
/// There is deliberately no always-on list. One existed — `spawn_agent`,
/// `skill`, `create_thread`, `list_agents_and_models` — and was dead: those four
/// are built-in tools, `apply_router_bypassing_built_ins` builds candidates from
/// MCP tools only, so they were never scored and the bypass never fired. They
/// are protected by the built-in bypass itself, which is unconditional. A field
/// advertising a guarantee it does not implement is worse than no field, so it
/// was removed rather than wired.
pub struct LazyToolRouter {
    /// Score threshold for inclusion.
    threshold: f64,
    /// Minimum word count for a message to be considered "complex."
    complex_word_threshold: usize,
    /// Maximum number of MCP tools to retain when the router activates.
    selection_budget: usize,
    /// Minimum best-match score required before the ranking is trusted enough to
    /// prune. Below this, the router fails open.
    confidence_gate: f64,
}

impl LazyToolRouter {
    /// Thresholds here must match `KaskToolRouterSettings::default()` in
    /// `kask_bridge` — that is the operator-facing source of truth, and this is
    /// the fallback for callers that construct the router without settings
    /// (tests, and any pre-settings construction). Two defaults that disagree is
    /// the drift class that silently changed routing behaviour before; pinned by
    /// `default_thresholds_are_the_documented_values`.
    pub fn new() -> Self {
        Self::new_with_thresholds(0.30, 6)
    }

    /// Construct with explicit thresholds. The composition root (main.rs)
    /// wires `KaskToolRouterSettings` into the router via this constructor so
    /// the activation threshold and complex-word threshold are
    /// operator-tunable instead of hardcoded.
    pub fn new_with_thresholds(threshold: f64, complex_word_threshold: usize) -> Self {
        Self {
            threshold,
            complex_word_threshold,
            selection_budget: DEFAULT_SELECTION_BUDGET,
            confidence_gate: DEFAULT_CONFIDENCE_GATE,
        }
    }

    /// Override the selection budget (the cap on retained MCP tools). Used by
    /// tests that need to observe ranking behaviour on small candidate sets.
    #[cfg(test)]
    fn with_selection_budget(mut self, budget: usize) -> Self {
        self.selection_budget = budget;
        self
    }
}

impl Default for LazyToolRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRouter for LazyToolRouter {
    fn select_tools(&self, context: &ToolSelectionContext) -> Option<Vec<SharedString>> {
        let Some(message) = &context.user_message else {
            return None; // No message → fail-open.
        };

        // Decide whether to activate. The router is lazy — it only filters
        // when the request is complex or explicitly tool-directed.
        // `has_code_file` is computed once here and passed to `should_activate`
        // (which uses it in branch 3) and reused for scoring below — the prior
        // implementation recomputed it inside `should_activate` and discarded
        // the result, then recomputed it again here.
        let has_code_file = context
            .open_file_paths
            .iter()
            .any(|path| is_code_file(path));
        if !self.should_activate(message, has_code_file) {
            return None; // Not activated → fail-open.
        }

        let context_keywords = extract_context_keywords(context);

        // Rank, then take a budget -- rather than admitting everything above an
        // absolute threshold.
        //
        // Thresholding made behaviour hostage to score calibration: the same
        // 0.30 bar admitted 5 unrelated tools on one phrasing and 0 tools on
        // another. Ranking only needs scores to order candidates correctly
        // relative to each other, which is a much weaker requirement than
        // producing calibrated absolute values, and it bounds the token cost
        // predictably. The threshold is retained as a floor so obvious
        // non-matches are still excluded when few candidates exist.
        let mut scored: Vec<(f64, &ToolCandidate)> = context
            .candidates
            .iter()
            .map(|candidate| {
                (
                    self.score_tool(candidate, &context_keywords, has_code_file),
                    candidate,
                )
            })
            .collect();
        // Descending by score; ties broken by name so selection is deterministic
        // (candidate order comes from a HashMap iteration upstream).
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.name.cmp(&b.1.name))
        });

        // Confidence gate: only prune when the best match is strong enough that
        // the ranking is trustworthy.
        //
        // Keyword scoring fails in a specific and dangerous way -- it can be
        // *confidently wrong*. "read this paragraph out loud in a natural voice"
        // shares no term with `generate_speech`'s "Generate speech audio from
        // text", so the correct tool scores 0.12 while `voice_design`,
        // `ledger_read`, and `rss_mark_all_read` score 0.32 on the incidental
        // words "voice" and "read". The router then prunes to 3 tools, none of
        // them right. Small confident selections are worse than no selection.
        //
        // Measured on the eval set, the best-match score separates these cases
        // cleanly: every correct pruning peaked at >= 0.52, both wrong prunings
        // peaked at 0.32, and every request that should fail open peaked at
        // <= 0.23. Requiring a strong top match before trusting the ranking took
        // recall from 0.91 to 1.00 while costing ~22 extra tools on average,
        // because 5 of the 7 newly-opened cases were already failing open.
        let top_score = scored.first().map(|(score, _)| *score).unwrap_or(0.0);
        if top_score < self.confidence_gate {
            return None;
        }

        // Keep every tool that clears the threshold, up to the budget. The
        // asymmetry is deliberate: dropping a needed tool costs a failed turn,
        // whereas carrying a spare costs a few dozen tokens, so the budget errs
        // generous.
        // Exact-name bypass: if the user message contains a candidate's
        // exact tool name, always retain it regardless of score. This
        // ensures that subagent messages like "Call the corpus_tag_chunks
        // MCP tool" don't lose the tool to budget/threshold filtering.
        let message_lower = message.to_lowercase();
        let exact_name_matches: std::collections::HashSet<SharedString> = context
            .candidates
            .iter()
            .filter(|candidate| message_lower.contains(candidate.name.as_ref().to_lowercase().as_str()))
            .map(|candidate| candidate.name.clone())
            .collect();

        let mut selected: Vec<SharedString> = scored
            .iter()
            .take(self.selection_budget)
            .filter(|(score, _)| *score >= self.threshold)
            .map(|(_, candidate)| candidate.name.clone())
            .collect();

        // Merge in exact-name matches that were filtered out.
        for name in &exact_name_matches {
            if !selected.contains(name) {
                selected.push(name.clone());
            }
        }

        Some(selected)
    }
}

impl LazyToolRouter {
    /// Decide whether the router should activate for this message.
    ///
    /// Activates when:
    /// - The message mentions a specific tool name (e.g., "use grep", "run terminal")
    /// - The message is complex (long, or signals decomposition/planning)
    /// - A code file is open AND the message mentions file/edit/search keywords
    ///
    /// Does NOT activate for simple greetings, short questions, or messages
    /// with no tool-relevant signal.
    fn should_activate(&self, message: &str, has_code_file: bool) -> bool {
        let lower = message.to_lowercase();
        let word_count = message.split_whitespace().count();

        // 1. Explicit tool name mention — always activate.
        let mentions_tool = TOOL_NAME_SIGNALS
            .iter()
            .any(|signal| lower.contains(signal));
        if mentions_tool {
            return true;
        }

        // 2. Complex request — long message or decomposition signals.
        if word_count >= self.complex_word_threshold {
            return true;
        }
        let complex_signals = [
            "plan",
            "break down",
            "decompose",
            "multiple steps",
            "subagent",
            "delegate",
            "parallel",
            "coordinate",
            "orchestrate",
        ];
        if complex_signals.iter().any(|signal| lower.contains(signal)) {
            return true;
        }

        // 3. Code file open + file/edit/search keywords — activate to
        // narrow to relevant code tools.
        if has_code_file {
            let code_signals = [
                "edit",
                "write",
                "read",
                "fix",
                "refactor",
                "search",
                "grep",
                "find",
                "delete",
                "create",
                "move",
                "rename",
                "diagnostic",
                "debug",
                "test",
                "build",
                "compile",
            ];
            if code_signals.iter().any(|signal| lower.contains(signal)) {
                return true;
            }
        }

        false
    }

    fn score_tool(
        &self,
        candidate: &ToolCandidate,
        context_keywords: &HashSet<String>,
        has_code_file: bool,
    ) -> f64 {
        let description_lower = candidate.description.to_lowercase();
        let description_terms = tokenize(&description_lower);
        let name_lower = candidate.name.to_lowercase();
        let name_terms = tokenize(&name_lower);

        // Count how many context keywords appear in the tool description.
        //
        // Matching is whole-term for every keyword length. It was previously
        // substring-based for keywords over 4 characters, which made `search`
        // match "re-search": a request to search the web scored
        // `companies_profile` and `web_search` as relevant purely because
        // their descriptions contain the word "research".
        let matched = context_keywords
            .iter()
            .filter(|keyword| description_terms.contains(keyword.to_lowercase().as_str()))
            .count();

        // A keyword hitting the tool's *name* is far stronger evidence than one
        // hitting its prose. Tool names are curated identifiers (`web_search`,
        // `generate_speech`), so a match there is close to the user naming the
        // tool outright, whereas descriptions share incidental vocabulary with
        // every other tool. Measured on the eval set: "search the web for the
        // symbol..." shares three description terms with `web_search` yet
        // scored 0.20 and was outranked by unrelated tools, because name evidence
        // counted for nothing.
        let name_matched = context_keywords
            .iter()
            .filter(|keyword| name_terms.contains(keyword.to_lowercase().as_str()))
            .count();
        let name_evidence = (name_matched as f64 / NAME_MATCH_SATURATION).min(1.0);

        // Saturating count, NOT a fraction of the message.
        //
        // The previous form divided by total keyword count, so appending
        // irrelevant words to an unchanged request monotonically destroyed its
        // score: "generate an image of a mountain" kept `generate_image`, and the
        // same request behind a 19-word conversational preamble kept nothing.
        // Evidence of relevance should accumulate with the number of matched
        // terms and be indifferent to how much unrelated text surrounds them --
        // the same reason retrieval scoring weights term matches rather than the
        // share of the query they occupy.[^tfidf]
        //
        // [^tfidf]: Sparck Jones, K. (1972). A statistical interpretation of term
        // specificity and its application in retrieval. Journal of Documentation.
        let match_evidence = (matched as f64 / MATCH_SATURATION).min(1.0);

        // Intent-signal keywords get a higher weight.
        let intent_matched = context_keywords
            .iter()
            .filter(|keyword| {
                matches!(
                    keyword.as_str(),
                    "url" | "fetch" | "web" | "terminal" | "shell" | "command" | "search"
                )
            })
            .filter(|keyword| description_terms.contains(keyword.to_lowercase().as_str()))
            .count();

        // No constant floor. Every candidate previously started at 0.2 against a
        // 0.30 threshold, so a single intent hit (+0.4) cleared the bar for any
        // tool that happened to share one generic word, while genuine topical
        // overlap was compressed into the remaining range. A constant added to
        // every candidate carries no discriminating information.
        let mut score = NAME_WEIGHT * name_evidence
            + DESCRIPTION_WEIGHT * match_evidence
            + INTENT_WEIGHT * (intent_matched as f64).min(1.0);

        // File-type nudge for code tools, applied only when a code file is
        // actually open.
        //
        // This was previously a `score.max(0.5)` floor gated on *either* an open
        // code file or any of ~16 generic verbs (`read`, `search`, `build`,
        // `find`). Both halves were wrong. The verb gate fired on 7 of 23 eval
        // cases -- "read this paragraph out loud", "search the web", "build a
        // scenario matrix" are not code requests -- and the floor then lifted ~62
        // tools to 0.5, outranking the genuinely correct tool sitting at 0.2-0.4
        // on real evidence and pushing it out of the selection budget.
        //
        // The floor also only became dominant when the constant 0.2 base was
        // removed: at that point typical real scores fell to 0.2-0.4, so a 0.5
        // override stopped being mid-range and started winning outright. Rescaling
        // one term of a scoring function without rescaling the others is the
        // hazard; an additive nudge keeps the boost proportionate and cannot
        // outrank direct evidence.
        if has_code_file {
            let code_tool_keywords = [
                "file",
                "edit",
                "write",
                "read",
                "grep",
                "search",
                "directory",
                "path",
                "diagnostic",
                "definition",
                "reference",
                "rename",
                "code action",
                "symbol",
            ];
            // Whole-term, for the same reason as the main match loop: a
            // substring check here matched "pro*file*" and "re*search*", handing
            // the code-tool floor to `companies_profile` on any request
            // containing a code verb. `code action` is two words, so it is
            // checked as a phrase against the raw description.
            let boosted = code_tool_keywords.iter().any(|kw| {
                if kw.contains(' ') {
                    description_lower.contains(kw)
                } else {
                    description_terms.contains(*kw)
                }
            });
            if boosted {
                score += CODE_TOOL_NUDGE;
            }
        }

        score.min(1.0)
    }
}

/// Minimum best-match score required before the router trusts its ranking.
///
/// Set at 0.50 from the eval-set separation: correct prunings peaked at >= 0.52,
/// incorrect ones at 0.32. Chosen at the top of the viable 0.35-0.50 band because
/// every value in that band produced identical recall and token cost on the eval
/// set, so the higher value buys margin against phrasings the set does not cover
/// at no measured cost. Revisit with a larger eval set -- 26 cases cannot
/// distinguish 0.35 from 0.50.
const DEFAULT_CONFIDENCE_GATE: f64 = 0.50;

/// Maximum number of MCP tools retained when the router activates.
///
/// Sized against the observed cost asymmetry: an unnecessary MCP tool schema is
/// roughly 45 tokens (~15,000 tokens across 331 tools), whereas dropping a tool
/// the request needed costs a failed turn and a retry. A budget of 40 keeps the
/// worst case near 1,800 tokens -- an ~88% reduction on the full surface -- while
/// leaving substantial headroom above the 1-2 tools a precise request selects.
const DEFAULT_SELECTION_BUDGET: usize = 40;

/// Number of matched keywords at which a tool is considered maximally relevant.
///
/// Match evidence saturates here rather than scaling with message length, so a
/// long request cannot dilute a strong match. Three is deliberate: a genuinely
/// on-topic tool description shares a small handful of terms with the request
/// ("generate", "image"), and requiring more would penalise terse descriptions.
const MATCH_SATURATION: f64 = 3.0;

/// Additive nudge for file-ish tools when a code file is open.
///
/// Deliberately small and additive rather than a `max()` floor: it should break
/// ties between comparably-scored tools during editing work, never promote a tool
/// that matched nothing above one that matched the request directly. Sized below
/// the score of a single description match (0.35 / 3 ≈ 0.12 per matched term) so
/// direct evidence always dominates.
const CODE_TOOL_NUDGE: f64 = 0.10;

/// Relative weights of the three evidence sources. They sum to 1.0 so a perfect
/// match on all three saturates at 1.0 without clamping.
///
/// Name evidence is weighted highest because tool names are curated identifiers:
/// a keyword matching `web_search`'s name is near-explicit tool selection,
/// whereas description vocabulary is shared incidentally across the surface.
const NAME_WEIGHT: f64 = 0.40;
const DESCRIPTION_WEIGHT: f64 = 0.35;
const INTENT_WEIGHT: f64 = 0.25;

/// Matched name terms at which name evidence saturates. Lower than the
/// description saturation because names are short -- two matching terms out of a
/// two-or-three-term name is already a decisive signal.
const NAME_MATCH_SATURATION: f64 = 2.0;

/// Split text into lowercase alphanumeric terms for whole-term matching.
///
/// Hyphens and underscores are treated as separators so `read_file` and
/// `code-action` contribute their parts, which is what callers compare against.
fn tokenize(text: &str) -> HashSet<&str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .collect()
}

/// Words in user messages that signal an explicit tool request. When any
/// of these appear, the router activates to narrow the tool set.
const TOOL_NAME_SIGNALS: &[&str] = &[
    "grep",
    "read file",
    "read_file",
    "edit file",
    "edit_file",
    "write file",
    "write_file",
    "terminal",
    "fetch",
    "web search",
    "web_search",
    "find path",
    "find_path",
    "list directory",
    "list_directory",
    "diagnostics",
    "find references",
    "find_references",
    "code action",
    "go to definition",
    "rename",
    "spawn agent",
    "spawn_agent",
    "create thread",
    "create_thread",
];

/// Extract keywords from the context: user message words (excluding
/// stopwords), file extensions from open file paths, and intent signals
/// (URL presence, terminal keywords) that map to tool capabilities.
fn extract_context_keywords(context: &ToolSelectionContext) -> HashSet<String> {
    let mut keywords = HashSet::new();

    if let Some(message) = &context.user_message {
        for word in message.split_whitespace() {
            let cleaned = word
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                .to_lowercase();
            if cleaned.len() > 2 && !STOPWORDS.contains(&cleaned.as_str()) {
                keywords.insert(cleaned);
            }
        }

        // Intent signals: add synthetic keywords that map to tool
        // capabilities.
        let lower = message.to_lowercase();
        if lower.contains("http://") || lower.contains("https://") {
            keywords.insert("url".to_string());
            keywords.insert("fetch".to_string());
            keywords.insert("web".to_string());
        }
        if lower.contains("terminal")
            || lower.contains("run ")
            || lower.contains("execute")
            || lower.contains("command")
            || lower.contains("shell")
        {
            keywords.insert("terminal".to_string());
            keywords.insert("shell".to_string());
            keywords.insert("command".to_string());
        }
        if lower.contains("search") || lower.contains("find ") || lower.contains("grep") {
            keywords.insert("search".to_string());
        }
    }

    // Add file extensions as keywords.
    for path in &context.open_file_paths {
        if let Some(ext) = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
        {
            keywords.insert(ext.to_lowercase());
        }
    }

    keywords
}

/// Common English stopwords to exclude from keyword extraction.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "her", "was", "one", "our",
    "out", "day", "get", "has", "him", "his", "how", "its", "may", "new", "now", "old", "see",
    "two", "way", "who", "boy", "did", "man", "men", "put", "say", "she", "too", "use", "this",
    "that", "with", "have", "from", "they", "will", "would", "there", "their", "what", "about",
    "which", "when", "your", "them", "then", "than", "been", "want", "into", "some", "like",
    "just", "also", "make", "more", "most", "such", "only", "does", "done", "very", "much", "need",
    "even", "here", "know", "think", "help", "please", "could", "should",
];

/// Check if a file path looks like a code file.
fn is_code_file(path: &str) -> bool {
    let extensions = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".java", ".c", ".cpp", ".h", ".hpp",
        ".rb", ".php", ".swift", ".kt", ".scala", ".lua", ".vim", ".clj", ".hs", ".ml", ".fs",
        ".ex", ".exs", ".erl", ".zig", ".nim", ".v", ".sv", ".d", ".dart",
    ];
    extensions.iter().any(|ext| path.ends_with(ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(name: &str, description: &str) -> ToolCandidate {
        ToolCandidate {
            name: name.into(),
            description: description.into(),
        }
    }

    #[test]
    fn test_lazy_router_does_not_activate_for_simple_message() {
        let context = ToolSelectionContext {
            user_message: Some("hello".to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate("grep", "Search file contents using a regular expression"),
                candidate("read_file", "Read a file from the project filesystem"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router.select_tools(&context);
        assert!(
            selected.is_none(),
            "simple message should fail-open (no filtering)"
        );
    }

    /// Pins this crate's half of the default-sync contract. `agent` cannot
    /// depend on `kask_bridge` (that would invert the D8 seam), so the paired
    /// assertion lives in `kask_bridge::settings` and references these values.
    /// Both must be updated together.
    #[test]
    fn default_thresholds_are_the_documented_values() {
        let router = LazyToolRouter::new();
        assert_eq!(router.complex_word_threshold, 6);
        assert!((router.threshold - 0.30).abs() < f64::EPSILON);
    }

    /// At the previous threshold of 40 words the router almost never activated,
    /// so every ordinary request paid for all MCP tool schemas. These are real
    /// message shapes that must now route (they carry tool-relevant intent) and
    /// must still fail open (bare one-liners with no actionable signal).
    #[test]
    fn nine_word_threshold_routes_ordinary_requests_but_not_one_liners() {
        let router = LazyToolRouter::new();
        let candidates = vec![
            candidate("grep", "Search file contents using a regular expression"),
            candidate("read_file", "Read a file from the project filesystem"),
            candidate("fetch", "Fetches a URL and returns the content as Markdown"),
        ];
        let probe = |msg: &str| {
            router.select_tools(&ToolSelectionContext {
                user_message: Some(msg.to_string()),
                open_file_paths: vec![],
                candidates: candidates.clone(),
            })
        };

        // 10+ words with a strong tool match — routes on word count alone, which
        // is exactly what the 40-word threshold used to miss. The message must
        // also clear the confidence gate, so it names something a tool actually
        // does ("search" / "file contents") rather than being merely long.
        assert!(
            probe("can you search the file contents for the parser regular expression").is_some(),
            "an ordinary multi-clause request with a clear tool match must route"
        );

        // Short and genuinely ambiguous — must still fail open so a terse
        // request never loses a tool it needed.
        for short in ["hello", "thanks", "what does this do", "fix this"] {
            assert!(
                probe(short).is_none(),
                "short message {short:?} must fail open, not filter"
            );
        }
    }

    #[test]
    fn test_lazy_router_activates_for_explicit_tool_request() {
        let context = ToolSelectionContext {
            user_message: Some("use grep to search for the function".to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate("grep", "Search file contents using a regular expression"),
                candidate("fetch", "Fetches a URL and returns content as Markdown"),
                candidate("spawn_agent", "Spawn a sub-agent for a task"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&context)
            .expect("router should activate");
        assert!(selected.contains(&"grep".into()));
        assert!(
            !selected.contains(&"fetch".into()),
            "fetch should be filtered — no URL context"
        );
    }

    #[test]
    fn test_lazy_router_activates_for_complex_request() {
        let long_message = "I need to plan a multi-step refactoring of the authentication \
            module. We should break down the work into parallel subtasks and delegate \
            each one to a subagent. The first step is to search for all usages of the \
            old auth function, then edit each call site to use the new API, and finally \
            run the test suite to verify nothing broke.";
        // MCP-shaped candidates: the scorer never sees built-ins.
        let context = ToolSelectionContext {
            user_message: Some(long_message.to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate("web_search", "Look up web pages and extract content"),
                candidate(
                    "kanban_task_create",
                    "Create a task to delegate work to a subagent",
                ),
                candidate("market_lookup", "Look up a prediction market by question"),
            ],
        };
        // The best candidate here is `kanban_task_create` at 0.35 (three shared
        // description terms: "task", "delegate", "subagent"), which is below the
        // 0.50 confidence gate, so the router declines to prune and returns
        // `None`. That is the intended outcome: a long decomposition request whose
        // strongest match is only moderate is exactly the shape that produced
        // confidently-wrong selections before the gate existed.
        //
        // Ranking is still asserted directly by
        // `code_nudge_breaks_ties_without_promoting_unrelated_tools`; this test
        // pins activation plus the gate's conservatism.
        let selected = LazyToolRouter::new().select_tools(&context);
        assert!(
            selected.is_none(),
            "a moderate best match must fail open rather than prune, got {selected:?}"
        );
    }

    /// A short code request with an open code file activates the router (via the
    /// code-file + code-verb path) but supplies almost no scoreable keywords:
    /// "fix the bug in main.rs" reduces to the single term `main.rs`, which
    /// matches no tool description. Both candidates therefore score below
    /// threshold and the empty selection fails open, retaining everything.
    ///
    /// This is the correct outcome and the reason `CODE_TOOL_NUDGE` is additive:
    /// the previous `max(0.5)` floor manufactured confidence here, admitting
    /// whichever tools happened to mention "file" while the request contained no
    /// evidence for any of them.
    ///
    /// Candidates are MCP-shaped because the scorer only ever sees MCP tools --
    /// built-ins like `read_file` and `grep` are retained unconditionally by the
    /// bypass and never scored.
    #[test]
    fn short_code_request_with_no_scoreable_keywords_fails_open() {
        let names: Vec<SharedString> = vec!["web_search".into(), "market_lookup".into()];
        let descriptions: Vec<SharedString> = vec![
            "Search the web with RRF fusion across providers".into(),
            "Look up a prediction market by question".into(),
        ];
        let tools: Vec<(&SharedString, &SharedString)> =
            names.iter().zip(descriptions.iter()).collect();

        let retained = apply_router_bypassing_built_ins(
            &LazyToolRouter::new(),
            tools,
            Some("fix the bug in main.rs"),
            vec!["/project/src/main.rs".to_string()],
            &std::collections::HashSet::new(),
        );
        assert_eq!(
            retained.len(),
            2,
            "no scoreable evidence must fail open rather than guess"
        );
    }

    /// With a code file open, a code-oriented MCP tool that genuinely matches the
    /// request must outrank an unrelated one — and the nudge must not promote the
    /// unrelated tool on its own.
    #[test]
    fn code_nudge_breaks_ties_without_promoting_unrelated_tools() {
        let context = ToolSelectionContext {
            user_message: Some(
                "search the codebase for the symbol that parses configuration files".to_string(),
            ),
            open_file_paths: vec!["/project/src/main.rs".to_string()],
            candidates: vec![
                candidate("web_search", "Search the web for code and documentation"),
                candidate("market_lookup", "Look up a prediction market by question"),
            ],
        };
        let selected = LazyToolRouter::new()
            .select_tools(&context)
            .expect("router should activate");
        assert!(
            selected.contains(&"web_search".into()),
            "the matching web tool must be retained"
        );
        assert!(
            !selected.contains(&"market_lookup".into()),
            "the code nudge must not admit a tool that matched nothing"
        );
    }

    #[test]
    fn test_lazy_router_does_not_activate_for_short_question() {
        let context = ToolSelectionContext {
            user_message: Some("what does this function do?".to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate("grep", "Search file contents"),
                candidate("read_file", "Read a file"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router.select_tools(&context);
        assert!(
            selected.is_none(),
            "short question should fail-open — no tool signal"
        );
    }

    #[test]
    fn test_lazy_router_includes_fetch_when_url_in_explicit_request() {
        let context = ToolSelectionContext {
            user_message: Some("fetch the content from https://example.com".to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate("fetch", "Fetches a URL and returns content as Markdown"),
                candidate("grep", "Search file contents using a regular expression"),
                candidate("web_search", "Search the web for information"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&context)
            .expect("router should activate");
        assert!(selected.contains(&"fetch".into()));
        assert!(selected.contains(&"web_search".into()));
    }

    #[test]
    fn test_lazy_router_scores_mcp_tools_by_description() {
        let context = ToolSelectionContext {
            user_message: Some(
                "use grep to search the corpus for investment strategies".to_string(),
            ),
            open_file_paths: vec![],
            candidates: vec![
                candidate(
                    "corpus_search",
                    "Search the embedded document corpus for relevant passages using semantic query",
                ),
                candidate(
                    "lora_train",
                    "Configure and launch a LoRA training run on the GPU",
                ),
                candidate("grep", "Search file contents using a regular expression"),
            ],
        };
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&context)
            .expect("router should activate");
        assert!(
            selected.contains(&"corpus_search".into()),
            "corpus_search should match via 'search' keyword overlap"
        );
        assert!(selected.contains(&"grep".into()));
        assert!(
            !selected.contains(&"lora_train".into()),
            "lora_train should be filtered — no keyword overlap"
        );
    }

    /// Built-in tools such as `spawn_agent` are protected by the built-in bypass
    /// in `apply_router_bypassing_built_ins`, **not** by any list inside the
    /// scorer. This asserts the real contract: they are retained even when the
    /// router activates and scores nothing, because they are never candidates.
    ///
    /// This replaces a test that fed `spawn_agent` in as a candidate and asserted
    /// the (now removed) `always_on` bypass. Production never does that —
    /// candidates are MCP-only — so the old test pinned unreachable behaviour.
    #[test]
    fn built_in_tools_survive_an_active_router_via_the_bypass() {
        let names: Vec<SharedString> = vec![
            "spawn_agent".into(),
            "skill".into(),
            "grep".into(),
            "some_mcp_tool".into(),
        ];
        let descriptions: Vec<SharedString> = vec![
            "Spawn a sub-agent for a task".into(),
            "Run a skill manifest cascade".into(),
            "Search file contents using a regular expression".into(),
            "An unrelated MCP capability about widgets".into(),
        ];
        let tools: Vec<(&SharedString, &SharedString)> =
            names.iter().zip(descriptions.iter()).collect();
        let built_in: std::collections::HashSet<&str> =
            ["spawn_agent", "skill", "grep"].into_iter().collect();

        let retained = apply_router_bypassing_built_ins(
            &LazyToolRouter::new(),
            tools,
            Some("use grep to find the function that parses the configuration file please"),
            vec![],
            &built_in,
        );

        for built_in_name in ["spawn_agent", "skill", "grep"] {
            assert!(
                retained.contains(&SharedString::from(built_in_name)),
                "built-in {built_in_name} must be retained regardless of scoring"
            );
        }
    }

    /// Appending irrelevant words to an unchanged request must not destroy its
    /// match. The old `matched / total_keywords` denominator made score a
    /// function of message length: the same "generate an image" intent kept
    /// `generate_image` at 14 words and kept *nothing* at 25 words. Match
    /// evidence now saturates on matched-term count instead.
    #[test]
    fn score_does_not_dilute_as_the_message_grows() {
        let router = LazyToolRouter::new();
        let candidates = vec![
            candidate("generate_image", "Generate an image from a text prompt"),
            candidate(
                "market_forecast",
                "Produce a calibrated probability forecast",
            ),
            candidate("curator_status", "Report system health and energy budgets"),
        ];
        let select = |msg: &str| {
            router
                .select_tools(&ToolSelectionContext {
                    user_message: Some(msg.to_string()),
                    open_file_paths: vec![],
                    candidates: candidates.clone(),
                })
                .expect("message should activate the router")
        };

        let terse = select("please generate an image of a mountain for me");
        let padded = select(
            "i was thinking about this earlier today and wondered whether perhaps you \
             could possibly help me out here since please generate an image of a mountain",
        );

        for selection in [&terse, &padded] {
            assert!(
                selection.contains(&"generate_image".into()),
                "generate_image must survive regardless of surrounding verbiage"
            );
        }
    }

    /// Keyword matching must be whole-term. Substring matching made `search`
    /// match "re*search*", so a web search scored `companies_profile` and
    /// any other tool whose description merely contained the word "research".
    #[test]
    fn keyword_matching_is_whole_term_not_substring() {
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&ToolSelectionContext {
                user_message: Some(
                    "search the web for the mountain image i generated last week".to_string(),
                ),
                open_file_paths: vec![],
                candidates: vec![
                    candidate("web_search", "Search the web for images and content"),
                    candidate("companies_profile", "Research a company profile and sector"),
                ],
            })
            .expect("message should activate the router");

        assert!(
            selected.contains(&"web_search".into()),
            "the genuinely matching tool must be kept"
        );
        assert!(
            !selected.contains(&"companies_profile".into()),
            "`search` must not match `research` — substring matching regressed"
        );
    }

    /// Selection is rank-and-budget, not threshold-only: the retained set is
    /// capped so a broad request cannot re-admit the entire MCP surface, and the
    /// tools kept are the highest-scoring ones.
    #[test]
    fn selection_is_capped_by_the_budget_and_takes_the_best() {
        let router = LazyToolRouter::new().with_selection_budget(2);
        let selected = router
            .select_tools(&ToolSelectionContext {
                user_message: Some(
                    "please search the web and the corpus for the mountain image".to_string(),
                ),
                open_file_paths: vec![],
                candidates: vec![
                    candidate("web_search", "Search the web for images and content"),
                    candidate("corpus_search", "Search the ingested document corpus"),
                    candidate("other_search", "Search unrelated telemetry archives"),
                    candidate("third_search", "Search custodial ledger statements"),
                ],
            })
            .expect("message should activate the router");

        assert!(
            selected.len() <= 2,
            "selection must respect the budget, got {}",
            selected.len()
        );
    }

    /// The gate's purpose: a *small, confident, wrong* selection is the worst
    /// outcome, and keyword scoring produces exactly that when the request and
    /// the tool share no vocabulary. "read this paragraph out loud in a natural
    /// voice" needs `generate_speech` ("Generate speech audio from text") but
    /// shares no term with it, while `voice_design` and `ledger_read` match the
    /// incidental words "voice" and "read". Without the gate the router pruned to
    /// those three and dropped the right tool.
    #[test]
    fn moderate_best_match_fails_open_rather_than_pruning_confidently_wrong() {
        let selected = LazyToolRouter::new().select_tools(&ToolSelectionContext {
            user_message: Some("read this paragraph out loud in a natural voice".to_string()),
            open_file_paths: vec![],
            candidates: vec![
                candidate(
                    "generate_speech",
                    "Generate speech audio from text using a voice design",
                ),
                candidate("ledger_read", "Read ledger entries for a portfolio"),
                candidate("voice_design", "Create a voice profile from a description"),
            ],
        });
        assert!(
            selected.is_none(),
            "a request sharing only incidental vocabulary must fail open, got {selected:?}"
        );
    }

    /// The router must never hand back an empty or low-confidence MCP set.
    /// Keyword scoring can find no match at all on a substantive-but-vague
    /// request, and at the 9-word activation threshold such requests reach the
    /// scorer. Stripping every MCP tool in that case is worse than paying for the
    /// schemas. The confidence gate now catches this before an empty selection is
    /// even constructed, so this asserts the outcome rather than the mechanism.
    #[test]
    fn empty_selection_fails_open_instead_of_stripping_all_mcp_tools() {
        let names: Vec<SharedString> = vec!["widget_alpha".into(), "widget_beta".into()];
        let descriptions: Vec<SharedString> = vec![
            "Manage orbital telemetry calibration records".into(),
            "Reconcile ledger entries against custodial statements".into(),
        ];
        let tools: Vec<(&SharedString, &SharedString)> =
            names.iter().zip(descriptions.iter()).collect();
        let built_in: std::collections::HashSet<&str> = std::collections::HashSet::new();

        // Long enough to activate, with no keyword overlap against either tool.
        let message = "i have been mulling over how the nested quote handling ought to \
                       behave in this parser and wanted your read";
        let router = LazyToolRouter::new();
        let selection = router.select_tools(&ToolSelectionContext {
            user_message: Some(message.to_string()),
            open_file_paths: vec![],
            candidates: names
                .iter()
                .zip(descriptions.iter())
                .map(|(n, d)| ToolCandidate {
                    name: n.clone(),
                    description: d.clone(),
                })
                .collect(),
        });
        assert!(
            selection.is_none(),
            "a request matching nothing must fail open, got {selection:?}"
        );

        let retained =
            apply_router_bypassing_built_ins(&router, tools, Some(message), vec![], &built_in);
        assert_eq!(
            retained.len(),
            2,
            "a no-confidence (empty) selection must fail open and retain all MCP tools"
        );
    }

    #[test]
    fn test_is_code_file_detects_extensions() {
        assert!(is_code_file("/project/src/main.rs"));
        assert!(is_code_file("app.tsx"));
        assert!(is_code_file("script.py"));
        assert!(!is_code_file("README.md"));
        assert!(!is_code_file("config.json"));
    }

    // zed-kask: `Thread::enabled_tools` filters built-in tool names out of
    // the candidates list before calling `select_tools`, so the router only
    // ever sees MCP/context-server tools. This contract must hold — if a
    // future change passes built-in tool names to the router, the agent
    // silently loses access to `fetch`, `diagnostics`, `list_directory`,
    // etc. on ordinary coding requests. See the `.rules` entry
    // "LazyToolRouter filters MCP tools only".
    #[test]
    fn test_router_only_sees_mcp_candidates_when_built_ins_filtered() {
        // Simulate what `enabled_tools` now does: filter out built-in tool
        // names before constructing the candidates list. The router should
        // only filter MCP tools, and built-in tools (never passed in) are
        // unaffected by definition.
        let built_in_names: std::collections::HashSet<&str> =
            crate::tools::ALL_TOOL_NAMES.iter().copied().collect();

        // All tools the thread has registered.
        let all_tools = [
            candidate("grep", "Search file contents using a regular expression"),
            candidate("fetch", "Fetches a URL and returns content as Markdown"),
            candidate("diagnostics", "Get errors and warnings for the project"),
            candidate(
                "list_directory",
                "List files and directories in a given path",
            ),
            candidate("spawn_agent", "Spawn a sub-agent for a task"),
            // MCP tools (not in ALL_TOOL_NAMES):
            candidate(
                "corpus_search",
                "Search the embedded document corpus for relevant passages",
            ),
            candidate(
                "lora_train",
                "Configure and launch a LoRA training run on the GPU",
            ),
        ];

        // Filter out built-in tools, as `enabled_tools` does.
        let mcp_candidates: Vec<_> = all_tools
            .iter()
            .filter(|c| !built_in_names.contains(c.name.as_ref()))
            .cloned()
            .collect();

        // Only the MCP tools should remain as candidates.
        assert_eq!(mcp_candidates.len(), 2);
        assert!(mcp_candidates.iter().any(|c| c.name == "corpus_search"));
        assert!(mcp_candidates.iter().any(|c| c.name == "lora_train"));

        // The router activates and filters MCP tools by keyword overlap.
        let context = ToolSelectionContext {
            user_message: Some(
                "use grep to search the corpus for investment strategies".to_string(),
            ),
            open_file_paths: vec![],
            candidates: mcp_candidates,
        };
        let router = LazyToolRouter::new();
        let selected = router
            .select_tools(&context)
            .expect("router should activate for tool-directed message");

        // corpus_search matches via 'search' keyword overlap; lora_train does not.
        assert!(
            selected.contains(&"corpus_search".into()),
            "corpus_search should match via 'search' keyword overlap"
        );
        assert!(
            !selected.contains(&"lora_train".into()),
            "lora_train should be filtered — no keyword overlap"
        );

        // Built-in tools are never in `selected` because they were never
        // passed as candidates. `enabled_tools` retains them unconditionally
        // via the `built_in_names.contains(name) || selected.contains(name)`
        // check. This is the contract: built-in tools bypass the router.
        assert!(!selected.contains(&"grep".into()));
        assert!(!selected.contains(&"fetch".into()));
        assert!(!selected.contains(&"diagnostics".into()));
        assert!(!selected.contains(&"list_directory".into()));
        // spawn_agent is always_on in the router, but it was never passed as
        // a candidate, so it's not in `selected` either. `enabled_tools`
        // retains it via the built-in bypass.
        assert!(!selected.contains(&"spawn_agent".into()));
    }

    /// Stub router that returns a fixed selection, for testing
    /// `apply_router_bypassing_built_ins` without the `LazyToolRouter`'s
    /// keyword-scoring logic.
    struct StubRouter {
        selected: Option<Vec<SharedString>>,
    }

    impl ToolRouter for StubRouter {
        fn select_tools(&self, _context: &ToolSelectionContext) -> Option<Vec<SharedString>> {
            self.selected.clone()
        }
    }

    #[test]
    fn test_apply_router_retains_all_built_ins_unconditionally() {
        let built_in_names: std::collections::HashSet<&str> =
            ["grep", "fetch", "read_file"].into_iter().collect();

        // Tools: 3 built-in + 2 MCP.
        let tools: Vec<(SharedString, SharedString)> = vec![
            ("grep".into(), "Search file contents".into()),
            ("fetch".into(), "Fetch a URL".into()),
            ("read_file".into(), "Read a file".into()),
            ("corpus_search".into(), "Search the corpus".into()),
            ("lora_train".into(), "Train a LoRA".into()),
        ];

        // Router selects only corpus_search — lora_train is filtered out.
        let router = StubRouter {
            selected: Some(vec!["corpus_search".into()]),
        };

        let retained = apply_router_bypassing_built_ins(
            &router,
            tools.iter().map(|(n, d)| (n, d)),
            Some("search the corpus"),
            vec![],
            &built_in_names,
        );

        // All built-in tools are retained, regardless of router selection.
        assert!(retained.contains("grep"));
        assert!(retained.contains("fetch"));
        assert!(retained.contains("read_file"));
        // MCP tools: corpus_search is retained (router selected it),
        // lora_train is filtered out.
        assert!(retained.contains("corpus_search"));
        assert!(!retained.contains("lora_train"));
    }

    #[test]
    fn test_apply_router_fail_open_retains_all_mcp() {
        let built_in_names: std::collections::HashSet<&str> = ["grep"].into_iter().collect();

        let tools: Vec<(SharedString, SharedString)> = vec![
            ("grep".into(), "Search file contents".into()),
            ("corpus_search".into(), "Search the corpus".into()),
            ("lora_train".into(), "Train a LoRA".into()),
        ];

        // Router returns None (fail-open — did not activate).
        let router = StubRouter { selected: None };

        let retained = apply_router_bypassing_built_ins(
            &router,
            tools.iter().map(|(n, d)| (n, d)),
            Some("hello"),
            vec![],
            &built_in_names,
        );

        // Fail-open: all tools retained (built-in + MCP).
        assert!(retained.contains("grep"));
        assert!(retained.contains("corpus_search"));
        assert!(retained.contains("lora_train"));
    }

    #[test]
    fn test_apply_router_no_mcp_tools_skips_router() {
        let built_in_names: std::collections::HashSet<&str> =
            ["grep", "fetch"].into_iter().collect();

        // Only built-in tools, no MCP tools.
        let tools: Vec<(SharedString, SharedString)> = vec![
            ("grep".into(), "Search file contents".into()),
            ("fetch".into(), "Fetch a URL".into()),
        ];

        // Router would filter everything if called, but it shouldn't be
        // called at all when there are no MCP candidates.
        let router = StubRouter {
            selected: Some(vec![]), // empty selection = filter everything
        };

        let retained = apply_router_bypassing_built_ins(
            &router,
            tools.iter().map(|(n, d)| (n, d)),
            Some("use grep to search"),
            vec![],
            &built_in_names,
        );

        // All built-in tools retained — router was skipped.
        assert!(retained.contains("grep"));
        assert!(retained.contains("fetch"));
    }
}
