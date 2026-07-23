//! Salience scoring and method signal extraction for style corpora.
//!
//! Computes three things from raw passage text (zero LLM cost):
//! 1. **Method signals** — cheap stylometric metrics (parataxis ratio,
//!    adjective density, dialogue ratio, etc.) that constitute the "how"
//!    dimension of the 5W1H framework.
//! 2. **Salience score** — weighted graph degree centrality combining entity
//!    tag counts, method coverage, category diversity, and positional
//!    significance into a single 0.0-1.0+ score.
//! 3. **Keyword overlap score** — lightweight keyword-matching scorer for
//!    ranking chat episodes by relevance to a query.
//!
//! Used by `EmbedService` at embed time (budget gating), by the style
//! synthesizer at query time (salience-parameterized retrieval), and by chat
//! recall (episode ranking via keyword overlap).

// ── Method Signals ────────────────────────────────────────────────────────

/// Cheaply-computed stylometric signals for a passage.
///
/// All fields are derived from simple text analysis — no model inference.
/// These constitute the "how" (methods/techniques) dimension of the 5W1H
/// metadata layer.

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct MethodSignals {
    /// Ratio of coordinating conjunctions (and, but, or) to total
    /// conjunctions. High = paratactic (Hemingway). Low = hypotactic (Wilde).
    pub parataxis_ratio: f32,

    /// Approximate adjective count per 100 words. Uses suffix heuristics
    /// (-y, -ous, -ful, -less, -ive, -able, -al, -ent, -ic, -ish).
    pub adjective_density: f32,

    /// Words ending in -ly per 100 words (filtered for common false
    /// positives like "only", "early", "family").
    pub adverb_density: f32,

    /// Ratio of "was/were `<verb>ed`" patterns to total verbs.
    pub passive_voice_ratio: f32,

    /// Words inside double-quote characters divided by total words.
    pub dialogue_ratio: f32,

    /// Standard deviation of sentence lengths within the passage.
    pub sentence_length_variance: f32,

    /// Hedge words ("perhaps", "maybe", "seemed", "almost", "rather",
    /// "quite") per 100 words. Indicates qualification/uncertainty.
    pub hedge_density: f32,

    /// Intensifiers ("very", "really", "absolutely", "extremely",
    /// "utterly", "completely") per 100 words.
    pub intensifier_density: f32,

    /// Tangible/concrete nouns ÷ abstract nouns (rough suffix heuristic).
    /// High = sensory, concrete. Low = abstract, conceptual.
    pub concrete_noun_ratio: f32,

    /// Sensory words (sight, sound, touch, taste, smell) per 100 words.
    pub sensory_word_ratio: f32,

    /// Total word count of the passage.
    pub word_count: usize,

    /// Number of sentences in the passage.
    pub sentence_count: usize,

    /// Average sentence length (words/sentences).
    pub avg_sentence_length: f32,

    // ── Academic-specific signals ───────────────────────────────────────
    /// Citation count per 1000 words. Detects patterns like "(Author, Year)"
    /// and ``[1]``, ``[2,3]`` reference markers.
    pub citation_density: f32,
    /// Ratio of formal notation (math, code, LaTeX) characters to total
    /// characters. High in quantitative/CS papers, low in humanities.
    pub formalism_ratio: f32,
    /// Domain-specific terminology per 100 words. Detects multi-syllable
    /// words with Greek/Latin roots, acronyms, and technical compounds.
    pub technical_term_density: f32,
}

/// Compute method signals from raw passage text.
///
/// All signals are cheap substring/character operations. No allocations
/// beyond what's needed for word splitting.
///
/// expect: "The system scores passage salience to gate h_mem storage budget"
/// \[P3\] Motivating: Generative Space — extracts cheap stylometric signals for method-aware retrieval
/// \[P8\] Constraining: Semantic Grounding — signals are deterministic heuristics over raw text
/// pre:  text is a valid &str
/// post: returns MethodSignals with computed linguistic features
/// post: returns MethodSignals::default() for empty text
pub fn compute_method_signals(text: &str) -> MethodSignals {
    let words: Vec<&str> = text.split_whitespace().collect();
    let word_count = words.len();
    if word_count == 0 {
        return MethodSignals::default();
    }

    // Sentence boundaries
    let sentence_count = text
        .chars()
        .filter(|c| matches!(c, '.' | '!' | '?'))
        .count()
        .max(1);
    let sentence_lengths = sentence_lengths(text);
    let avg_sentence_length = word_count as f32 / sentence_count as f32;
    let sentence_length_variance = if sentence_lengths.len() > 1 {
        let mean = sentence_lengths.iter().sum::<usize>() as f32 / sentence_lengths.len() as f32;
        let variance = sentence_lengths
            .iter()
            .map(|&l| (l as f32 - mean).powi(2))
            .sum::<f32>()
            / sentence_lengths.len() as f32;
        variance.sqrt()
    } else {
        0.0
    };

    // ── Signal extraction ─────────────────────────────────────────

    let per_100 = |count: usize| -> f32 { (count as f32 / word_count as f32) * 100.0 };

    // Parataxis ratio: coordinating ÷ (coordinating + subordinating)
    let coord_count = words
        .iter()
        .filter(|w| matches!(w.to_lowercase().as_str(), "and" | "but" | "or" | "nor"))
        .count();
    let subord_count = words
        .iter()
        .filter(|w| {
            matches!(
                w.to_lowercase().as_str(),
                "because"
                    | "although"
                    | "though"
                    | "since"
                    | "while"
                    | "unless"
                    | "until"
                    | "whereas"
                    | "after"
                    | "before"
                    | "if"
                    | "when"
            )
        })
        .count();
    let total_conj = coord_count + subord_count;
    let parataxis_ratio = if total_conj > 0 {
        coord_count as f32 / total_conj as f32
    } else {
        0.5 // neutral
    };

    // Adjective density: suffix-heuristic
    let adj_suffixes = [
        "y", "ous", "ful", "less", "ive", "able", "ible", "al", "ent", "ic", "ish",
    ];
    let adj_count = words
        .iter()
        .filter(|w| {
            let lower = w.to_lowercase();
            // Exclude very short words and common non-adjective -y words
            if lower.len() < 3 {
                return false;
            }
            adj_suffixes
                .iter()
                .any(|suf| lower.ends_with(suf) && lower.len() > suf.len() + 1)
        })
        .count();
    let adjective_density = per_100(adj_count);

    // Adverb density: -ly suffix, minus false positives
    let ly_false_positives = [
        "only",
        "early",
        "family",
        "july",
        "jelly",
        "belly",
        "holy",
        "reply",
        "apply",
        "supply",
        "multiply",
        "butterfly",
        "barfly",
    ];
    let adv_count = words
        .iter()
        .filter(|w| {
            let lower = w.to_lowercase();
            lower.ends_with("ly")
                && lower.len() > 3
                && !ly_false_positives.contains(&lower.as_str())
        })
        .count();
    let adverb_density = per_100(adv_count);

    // Passive voice: count "was *ed", "were *ed" patterns
    let passive_count = words
        .windows(2)
        .filter(|pair| {
            let first = pair[0].to_lowercase();
            let second = pair[1].to_lowercase();
            (first == "was" || first == "were") && (second.ends_with("ed") && second.len() > 4)
        })
        .count();
    let passive_voice_ratio = if word_count > 0 {
        passive_count as f32 / (word_count / 10) as f32
    } else {
        0.0
    };

    // Dialogue ratio: words inside double quotes
    let in_quotes = {
        let mut inside = false;
        let mut quote_words = 0usize;
        for word in &words {
            let has_open = word.contains('"') || word.contains('\u{201c}');
            let has_close = word.contains('"') || word.contains('\u{201d}');
            if inside {
                quote_words += 1;
            }
            if has_open && !has_close {
                inside = true;
                quote_words += 1;
            } else if has_close && !has_open {
                inside = false;
            } else if has_open && has_close {
                // Single-word quote
                quote_words += 1;
            }
        }
        quote_words
    };
    let dialogue_ratio = if word_count > 0 {
        in_quotes as f32 / word_count as f32
    } else {
        0.0
    };

    // Hedge density
    let hedge_words = [
        "perhaps",
        "maybe",
        "seemed",
        "almost",
        "rather",
        "quite",
        "fairly",
        "somewhat",
        "apparently",
        "presumably",
    ];
    let hedge_count = words
        .iter()
        .filter(|w| hedge_words.contains(&w.to_lowercase().as_str()))
        .count();
    let hedge_density = per_100(hedge_count);

    // Intensifier density
    let intensifiers = [
        "very",
        "really",
        "absolutely",
        "extremely",
        "utterly",
        "completely",
        "totally",
        "entirely",
        "thoroughly",
    ];
    let intensifier_count = words
        .iter()
        .filter(|w| intensifiers.contains(&w.to_lowercase().as_str()))
        .count();
    let intensifier_density = per_100(intensifier_count);

    // Concrete noun ratio: rough heuristic based on common concrete suffixes
    // vs abstract suffixes (-tion, -ness, -ity, -ism, -ment, -ance, -ence)
    let abstract_suffixes = [
        "tion", "ness", "ity", "ism", "ment", "ance", "ence", "hood", "ship",
    ];
    let abstract_count = words
        .iter()
        .filter(|w| {
            let lower = w.to_lowercase();
            lower.len() > 4 && abstract_suffixes.iter().any(|s| lower.ends_with(s))
        })
        .count();
    let concrete_count = words.len().saturating_sub(abstract_count);
    let concrete_noun_ratio = if !words.is_empty() {
        concrete_count as f32 / words.len() as f32
    } else {
        0.7 // default: mostly concrete
    };

    // Sensory word density
    let sensory_words = [
        // Sight
        "bright",
        "dark",
        "red",
        "blue",
        "green",
        "white",
        "black",
        "pale",
        "shadow",
        "light",
        "color",
        "gleam",
        "glitter",
        "shine",
        "dim",
        "glow",
        // Sound
        "loud",
        "quiet",
        "silent",
        "noise",
        "sound",
        "echo",
        "whisper",
        "roar",
        "crash",
        // Touch
        "cold",
        "hot",
        "warm",
        "cool",
        "rough",
        "smooth",
        "hard",
        "soft",
        "wet",
        "dry",
        "sharp",
        "heavy",
        "light",
        // Taste
        "sweet",
        "bitter",
        "sour",
        "salty",
        // Smell
        "scent",
        "smell",
        "odor",
        "fragrance",
        "stink",
    ];
    let sensory_count = words
        .iter()
        .filter(|w| sensory_words.contains(&w.to_lowercase().as_str()))
        .count();
    let sensory_word_ratio = per_100(sensory_count);

    // ── Academic signals ────────────────────────────────────────────

    // Citation density: detect "(Author, Year)" and "[N]" patterns
    let per_1000 = |count: usize| -> f32 { (count as f32 / word_count as f32) * 1000.0 };
    let citation_pattern_count = {
        // Count "(...)" parenthetical patterns that look like citations
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        let mut count = 0usize;
        while i + 2 < chars.len() {
            if chars[i] == '(' {
                // Look for "Author, Year" or "Author Year" pattern
                let slice: String = chars[i + 1..].iter().take_while(|&&c| c != ')').collect();
                let has_year = slice
                    .split_whitespace()
                    .any(|s| s.len() == 4 && s.chars().all(|c| c.is_ascii_digit()));
                if has_year && slice.len() > 3 {
                    count += 1;
                }
                i += slice.len() + 1;
            } else if chars[i] == '[' {
                // Look for "[N]" or "[N,N]" numeric citation pattern
                let slice: String = chars[i + 1..].iter().take_while(|&&c| c != ']').collect();
                let is_numeric = slice
                    .split(',')
                    .all(|s| s.trim().chars().all(|c| c.is_ascii_digit() || c == '-'));
                if is_numeric && !slice.is_empty() {
                    count += 1;
                }
                i += slice.len() + 1;
            } else {
                i += 1;
            }
        }
        count
    };
    let citation_density = per_1000(citation_pattern_count);

    // Formalism ratio: math/LaTeX/notation characters to total
    let formal_chars = text
        .chars()
        .filter(|c| {
            matches!(
                c,
                '$' | '\\'
                    | '{'
                    | '}'
                    | '^'
                    | '_'
                    | '∑'
                    | '∫'
                    | '∂'
                    | 'π'
                    | '∞'
                    | '≤'
                    | '≥'
                    | '±'
                    | '→'
                    | '⇒'
                    | 'α'
                    | 'β'
                    | 'γ'
                    | 'δ'
                    | 'ε'
                    | 'θ'
                    | 'λ'
                    | 'μ'
                    | 'σ'
                    | 'φ'
                    | 'ω'
            )
        })
        .count();
    let total_chars = text.chars().count().max(1);
    let formalism_ratio = formal_chars as f32 / total_chars as f32;

    // Technical term density: multi-syllable words with Latin/Greek roots
    // Heuristic: words ≥8 chars with common academic suffixes
    let academic_suffixes = [
        "tion", "sion", "ology", "ism", "icity", "ization", "ability", "ential", "istical",
        "ogenous", "opathy", "oscopy", "ometric", "ographic",
    ];
    let tech_count = words
        .iter()
        .filter(|w| {
            let lower = w.to_lowercase();
            lower.len() >= 8 && academic_suffixes.iter().any(|suf| lower.ends_with(suf))
        })
        .count();
    let technical_term_density = per_100(tech_count);

    MethodSignals {
        parataxis_ratio: parataxis_ratio.clamp(0.0, 1.0),
        adjective_density,
        adverb_density,
        passive_voice_ratio: passive_voice_ratio.clamp(0.0, 1.0),
        dialogue_ratio,
        sentence_length_variance,
        hedge_density,
        intensifier_density,
        concrete_noun_ratio: concrete_noun_ratio.clamp(0.0, 1.0),
        sensory_word_ratio,
        word_count,
        sentence_count,
        avg_sentence_length,
        citation_density,
        formalism_ratio: formalism_ratio.clamp(0.0, 1.0),
        technical_term_density,
    }
}

/// Compute sentence lengths by splitting on `. ! ?` boundaries.
fn sentence_lengths(text: &str) -> Vec<usize> {
    let mut lengths = Vec::new();
    let mut current = 0usize;
    for word in text.split_whitespace() {
        current += 1;
        let last_char = word.chars().last();
        if matches!(last_char, Some('.') | Some('!') | Some('?')) && current > 0 {
            lengths.push(current);
            current = 0;
        }
    }
    if current > 0 {
        lengths.push(current);
    }
    lengths
}

// ── Declared Method Matching ──────────────────────────────────────────────

/// A declared method with signal thresholds for matching.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DeclaredMethod {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub signal: MethodThresholds,
    /// Simplified single-threshold mode.
    /// When set, applies as a minimum across all method signals
    /// instead of using per-signal thresholds.
    #[serde(default)]
    pub threshold: Option<f64>,
}

/// Thresholds for matching a declared method to a passage's signals.
///
/// Each field is optional — only the fields present constrain the match.
/// A passage matches if ALL present constraints are satisfied.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct MethodThresholds {
    #[serde(default)]
    pub parataxis_ratio_min: Option<f32>,
    #[serde(default)]
    pub parataxis_ratio_max: Option<f32>,
    #[serde(default)]
    pub adjective_density_max: Option<f32>,
    #[serde(default)]
    pub adverb_density_max: Option<f32>,
    #[serde(default)]
    pub passive_voice_ratio_max: Option<f32>,
    #[serde(default)]
    pub dialogue_ratio_min: Option<f32>,
    #[serde(default)]
    pub sentence_length_variance_max: Option<f32>,
    #[serde(default)]
    pub hedge_density_max: Option<f32>,
    #[serde(default)]
    pub intensifier_density_max: Option<f32>,
    #[serde(default)]
    pub concrete_noun_ratio_min: Option<f32>,
    #[serde(default)]
    pub sensory_word_ratio_min: Option<f32>,
    #[serde(default)]
    pub avg_sentence_length_min: Option<f32>,
    #[serde(default)]
    pub avg_sentence_length_max: Option<f32>,

    // ── Academic-specific signals ────────────────────────────────────────
    /// Citations per 1000 words (academic corpora).
    #[serde(default)]
    pub citation_density_min: Option<f32>,
    #[serde(default)]
    pub citation_density_max: Option<f32>,
    /// Ratio of math/code/notation to prose (academic corpora).
    #[serde(default)]
    pub formalism_ratio_min: Option<f32>,
    #[serde(default)]
    pub formalism_ratio_max: Option<f32>,
    /// Domain-specific terminology per 100 words (academic corpora).
    #[serde(default)]
    pub technical_term_density_min: Option<f32>,
    #[serde(default)]
    pub technical_term_density_max: Option<f32>,
}

impl DeclaredMethod {
    /// Check whether a passage's signals match this method's thresholds.
    ///
    /// expect: "The system scores passage salience to gate h_mem storage budget"
    /// \[P3\] Motivating: Generative Space — matches passage signals against declared method thresholds
    /// \[P8\] Constraining: Semantic Grounding — unconfigured thresholds are always satisfied
    /// pre:  signals is a valid MethodSignals
    /// post: returns true iff all configured min/max thresholds are satisfied
    /// post: unconfigured thresholds (None) are always satisfied
    pub fn matches(&self, signals: &MethodSignals) -> bool {
        let t = &self.signal;
        check_min(t.parataxis_ratio_min, signals.parataxis_ratio)
            && check_max(t.parataxis_ratio_max, signals.parataxis_ratio)
            && check_max(t.adjective_density_max, signals.adjective_density)
            && check_max(t.adverb_density_max, signals.adverb_density)
            && check_max(t.passive_voice_ratio_max, signals.passive_voice_ratio)
            && check_min(t.dialogue_ratio_min, signals.dialogue_ratio)
            && check_max(
                t.sentence_length_variance_max,
                signals.sentence_length_variance,
            )
            && check_max(t.hedge_density_max, signals.hedge_density)
            && check_max(t.intensifier_density_max, signals.intensifier_density)
            && check_min(t.concrete_noun_ratio_min, signals.concrete_noun_ratio)
            && check_min(t.sensory_word_ratio_min, signals.sensory_word_ratio)
            && check_min(t.avg_sentence_length_min, signals.avg_sentence_length)
            && check_max(t.avg_sentence_length_max, signals.avg_sentence_length)
            && check_min(t.citation_density_min, signals.citation_density)
            && check_max(t.citation_density_max, signals.citation_density)
            && check_min(t.formalism_ratio_min, signals.formalism_ratio)
            && check_max(t.formalism_ratio_max, signals.formalism_ratio)
            && check_min(t.technical_term_density_min, signals.technical_term_density)
            && check_max(t.technical_term_density_max, signals.technical_term_density)
    }
}

fn check_min(min: Option<f32>, value: f32) -> bool {
    min.is_none_or(|m| value >= m)
}

fn check_max(max: Option<f32>, value: f32) -> bool {
    max.is_none_or(|m| value <= m)
}

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

/// Tag a passage by matching declared entity names against the text.
///
/// Uses simple case-insensitive substring matching. Returns distinct
/// tags only (no duplicates within a category).
///
/// expect: "The system scores passage salience to gate h_mem storage budget"
/// \[P3\] Motivating: Generative Space — tags passages with declared entities for the salience graph
/// \[P8\] Constraining: Semantic Grounding — case-insensitive substring matching
/// pre:  text is non-empty, entity lists are valid
/// post: returns EntityTags with matched entities per category
/// post: methods field is empty (filled separately)
pub fn tag_entities(
    text: &str,
    characters: &[String],
    places: &[String],
    events: &[String],
    concepts: &[String],
) -> EntityTags {
    let lower = text.to_lowercase();
    EntityTags {
        characters: filter_matches(&lower, characters),
        places: filter_matches(&lower, places),
        events: filter_matches(&lower, events),
        concepts: filter_matches(&lower, concepts),
        methods: Vec::new(), // filled separately by method matching
    }
}

fn filter_matches(lower_text: &str, candidates: &[String]) -> Vec<String> {
    candidates
        .iter()
        .filter(|c| lower_text.contains(&c.to_lowercase()))
        .cloned()
        .collect()
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

    /// Number of distinct tags across all categories.
    ///
    /// expect: "The system scores passage salience to gate h_mem storage budget"
    /// \[P3\] Motivating: Generative Space — counts distinct tags across all categories
    /// \[P5\] Constraining: Essentialism — simple sum of category lengths
    /// post: returns sum of lengths of all tag category vectors
    pub fn tag_count(&self) -> usize {
        self.characters.len()
            + self.places.len()
            + self.events.len()
            + self.concepts.len()
            + self.methods.len()
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
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum BudgetConfig {
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
    /// Absolute hard cap.
    Absolute { max_triples: usize },
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

// ── Keyword Overlap (chat recall ranking) ─────────────────────────────────

/// Extract keywords from a query string for overlap-based episode ranking.
///
/// Lowercases the input, splits on whitespace, and filters to words longer
/// than 2 characters. Returns owned strings so the caller does not need to
/// hold the lowercased source.
///
/// Used by chat recall (`MemoryService::recall_episodic`) and the memory MCP
/// server (`memory_recall`, `episodic_recall_context`) to rank episodic
/// memories by keyword overlap with the user's input.
///
/// expect: "The system ranks recalled memories by relevance to the query"
/// pre:  text is a valid &str
/// post: returns lowercased keywords with length > 2, in input order
pub fn extract_keywords(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split_whitespace()
        .filter(|w| w.len() > 2)
        .map(|w| w.to_string())
        .collect()
}

/// Score how many keywords appear in a text (case-insensitive substring match).
///
/// Returns the count of keywords from `keywords` that appear as substrings
/// in the lowercased `text`. Higher = more relevant. Used to rank chat
/// episodes by keyword overlap with the query.
///
/// expect: "The system ranks recalled memories by relevance to the query"
/// pre:  keywords are lowercased (as produced by `extract_keywords`)
/// post: returns count of keywords found as substrings in text (0 if none)
pub fn keyword_overlap_score(keywords: &[String], text: &str) -> usize {
    let text_lower = text.to_lowercase();
    keywords
        .iter()
        .filter(|kw| text_lower.contains(kw.as_str()))
        .count()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_signals_hemingway_like() {
        let text = "He drank the wine. It was good. He walked out into the rain.";
        let signals = compute_method_signals(text);
        assert!(signals.parataxis_ratio > 0.0);
        assert!(signals.adverb_density < 5.0); // no -ly words
        assert!(signals.sentence_count >= 3);
        assert!(signals.avg_sentence_length < 10.0);
    }

    #[test]
    fn method_signals_wilde_like() {
        let text = "The utterly magnificent and beautifully ornate chandelier \
                    glittered brilliantly in the perfectly silent ballroom, \
                    casting remarkably delicate shadows upon the exquisitely \
                    dressed guests who seemed almost entirely entranced.";
        let signals = compute_method_signals(text);
        assert!(signals.adjective_density > 5.0);
        assert!(signals.adverb_density > 5.0);
        assert!(signals.hedge_density > 0.0);
        assert!(signals.intensifier_density > 0.0);
    }

    #[test]
    fn declared_method_matches() {
        let method = DeclaredMethod {
            name: "low_adjective".into(),
            description: String::new(),
            signal: MethodThresholds {
                adjective_density_max: Some(5.0),
                ..Default::default()
            },
            threshold: None,
        };
        let hemingway_signals =
            compute_method_signals("He drank the wine. It was good. He walked out.");
        assert!(method.matches(&hemingway_signals));

        let wilde_signals = compute_method_signals(
            "The magnificent and beautifully ornate chandelier glittered brilliantly.",
        );
        assert!(!method.matches(&wilde_signals));
    }

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

    #[test]
    fn entity_tagging_case_insensitive() {
        let text = "Jake Barnes walked through Paris in the rain.";
        let tags = tag_entities(text, &["Jake Barnes".into()], &["Paris".into()], &[], &[]);
        assert_eq!(tags.characters, vec!["Jake Barnes"]);
        assert_eq!(tags.places, vec!["Paris"]);
    }

    #[test]
    fn dialogue_ratio_detection() {
        let text = "\"I'm not drunk,\" he said. \"You are,\" she replied. The rain fell.";
        let signals = compute_method_signals(text);
        assert!(signals.dialogue_ratio > 0.3);
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

    // ── Keyword overlap tests ──────────────────────────────────────────

    #[test]
    fn extract_keywords_lowercases_and_filters_short_words() {
        let kw = extract_keywords("The Quick Brown Fox");
        assert_eq!(kw, vec!["the", "quick", "brown", "fox"]);
    }

    #[test]
    fn extract_keywords_drops_words_of_length_2_or_less() {
        let kw = extract_keywords("a be cat dog");
        assert_eq!(kw, vec!["cat", "dog"]);
    }

    #[test]
    fn keyword_overlap_score_counts_substring_matches() {
        let kw = extract_keywords("rust memory");
        assert_eq!(keyword_overlap_score(&kw, "I love rust programming"), 1);
        assert_eq!(keyword_overlap_score(&kw, "rust memory and rust"), 2);
    }

    #[test]
    fn keyword_overlap_score_is_case_insensitive() {
        let kw = vec!["hello".to_string()];
        assert_eq!(keyword_overlap_score(&kw, "HELLO WORLD"), 1);
    }

    #[test]
    fn keyword_overlap_score_returns_zero_for_no_matches() {
        let kw = extract_keywords("nonexistent");
        assert_eq!(keyword_overlap_score(&kw, "completely different text"), 0);
    }
}
