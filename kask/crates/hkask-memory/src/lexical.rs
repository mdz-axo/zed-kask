//! The lexical leg of hybrid federated retrieval — a rare-term inverted
//! index built over a sealed source's chunks at admission.
//!
//! Pure dense KNN cannot retrieve a document by its rare proper-noun name:
//! the query embedding carries the phrase's semantic meaning, not its
//! referential identity (observed live 2026-10-09 — the query "magnifica
//! humanitas" ranked the charter whose own title chunk carries that name
//! below "LUX" and Hume's virtues, while a descriptive query recalled the
//! same chunks at ranks 1–3). This module is the deterministic complement:
//! an inverted index over each chunk's decoded source filename (the
//! document-identity channel — every chunk of `magnifica-humanitas.md`
//! carries its document's name) and its rare text terms. Common text terms
//! are pruned by document frequency, so the index stays small and the leg
//! stays silent on ordinary semantic queries, firing only where dense
//! retrieval is weakest. Ranks from the two legs are fused by
//! reciprocal-rank fusion in
//! `federated_recall::ReadOnlyPassageSource::search`.
//!
//! The index is derived data, built in memory from the sealed database's
//! rows at admission; the sealed file is never modified. Chunks from the
//! corpus's own build metadata (`MANIFEST.json`) carry no postings and are
//! flagged so the dense rank can be penalized — metadata chunks crowded 5
//! of the top-10 slots for real mechanics queries before the penalty.

use std::collections::{HashMap, HashSet};

/// Maximum document frequency for an indexed *text* term. Terms appearing
/// in more rows are common vocabulary — dense retrieval already handles
/// them, and skipping their postings keeps the index small. Filename
/// (identity) terms are exempt: a large document has more chunks than this
/// ceiling, and its name must stay findable.
const RARE_TERM_MAX_DF: usize = 64;

/// Reciprocal-rank-fusion smoothing constant (Cormack et al. 2009).
pub(crate) const RRF_K: usize = 60;

/// Rank penalty applied to a metadata row's dense contribution. Metadata
/// chunks still surface when nothing else matches, but never on rank
/// parity with content chunks.
pub(crate) const METADATA_RANK_PENALTY: usize = 100;

/// The corpus's own build-metadata files — chunked into the corpus by the
/// build, but never evidence for a retrieval query.
const METADATA_SOURCE_BASENAMES: &[&str] = &["MANIFEST.json"];

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;
const MIN_TOKEN_LEN: usize = 2;

/// English function words, never the referential signal this leg exists
/// for. In a large corpus the document-frequency ceiling prunes them
/// anyway; the stoplist makes small sealed sources behave the same way —
/// without it, a six-row fixture indexes "the"/"of"/"and" as rare terms
/// and any ordinary sentence lexically matches every row that contains
/// function words.
const STOPWORDS: &[&str] = &[
    "the", "and", "of", "to", "a", "an", "in", "is", "are", "was", "were", "be", "been", "it",
    "its", "for", "on", "with", "as", "at", "by", "from", "or", "but", "not", "no", "if", "then",
    "than", "so", "too", "very", "into", "this", "that", "these", "those", "how", "does", "do",
    "did", "can", "could", "should", "would", "will", "shall", "may", "might", "must", "have",
    "has", "had", "i", "you", "he", "she", "we", "they", "them", "their", "what", "which", "who",
    "when", "where", "why",
];

/// One (row, term-frequency) posting.
struct Posting {
    row: u32,
    tf: u32,
}

/// A lexical-leg hit: the row index and its BM25 score.
#[derive(Debug)]
pub struct LexicalHit {
    pub row: u32,
    pub score: f64,
}

/// The rare-term inverted index over one sealed source.
pub struct LexicalIndex {
    row_ids: Vec<String>,
    id_to_row: HashMap<String, u32>,
    /// Filename (document-identity) postings — never pruned, so a
    /// document's name stays findable however many chunks it has.
    identity_postings: HashMap<String, Vec<Posting>>,
    /// Passage-text postings for rare terms only.
    text_postings: HashMap<String, Vec<Posting>>,
    doc_lengths: Vec<u32>,
    metadata_rows: HashSet<u32>,
    avg_doc_len: f64,
}

impl LexicalIndex {
    /// Build from the sealed source's rows — `(embedding id, entity_ref,
    /// passage_text)` per row. Infallible: the index is pure derivation
    /// over already-fetched rows.
    pub fn build(rows: impl IntoIterator<Item = (String, String, Option<String>)>) -> Self {
        let mut row_ids = Vec::new();
        let mut id_to_row = HashMap::new();
        let mut identity_postings: HashMap<String, Vec<Posting>> = HashMap::new();
        let mut text_postings: HashMap<String, Vec<Posting>> = HashMap::new();
        let mut doc_lengths = Vec::new();
        let mut metadata_rows = HashSet::new();

        for (row_index, (id, entity_ref, passage_text)) in rows.into_iter().enumerate() {
            let row = row_index as u32;
            row_ids.push(id.clone());
            id_to_row.insert(id, row);
            let filename = filename_from_entity_ref(&entity_ref);
            let is_metadata = filename
                .as_deref()
                .is_some_and(|name| METADATA_SOURCE_BASENAMES.contains(&name));
            if is_metadata {
                metadata_rows.insert(row);
            }
            let filename_tokens = filename.as_deref().map(tokenize).unwrap_or_default();
            let text_tokens = passage_text
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                .map(tokenize)
                .unwrap_or_default();
            doc_lengths.push((filename_tokens.len() + text_tokens.len()) as u32);
            if is_metadata {
                continue;
            }
            for (term, count) in term_counts(&filename_tokens) {
                identity_postings
                    .entry(term.to_string())
                    .or_default()
                    .push(Posting { row, tf: count });
            }
            for (term, count) in term_counts(&text_tokens) {
                text_postings
                    .entry(term.to_string())
                    .or_default()
                    .push(Posting { row, tf: count });
            }
        }
        let avg_doc_len = if doc_lengths.is_empty() {
            0.0
        } else {
            doc_lengths.iter().map(|&len| len as f64).sum::<f64>() / doc_lengths.len() as f64
        };
        text_postings.retain(|_, postings| postings.len() <= RARE_TERM_MAX_DF);
        Self {
            row_ids,
            id_to_row,
            identity_postings,
            text_postings,
            doc_lengths,
            metadata_rows,
            avg_doc_len,
        }
    }

    /// Rank rows by BM25 over the query's rare terms, combining a term's
    /// filename and text frequency into one per-row tf. Queries whose
    /// terms are all common (or absent) return no hits — the leg is
    /// silent without rare-term signal, by construction.
    pub fn search(&self, query: &str, limit: usize) -> Vec<LexicalHit> {
        if limit == 0 || self.row_ids.is_empty() {
            return Vec::new();
        }
        let total = self.row_ids.len() as f64;
        let mut scores: HashMap<u32, f64> = HashMap::new();
        let terms: HashSet<String> = tokenize(query).into_iter().collect();
        for term in terms {
            let identity = self.identity_postings.get(&term);
            let text = self.text_postings.get(&term);
            if identity.is_none() && text.is_none() {
                continue;
            }
            let mut rows: HashMap<u32, u32> = HashMap::new();
            if let Some(postings) = identity {
                for posting in postings {
                    *rows.entry(posting.row).or_insert(0) += posting.tf;
                }
            }
            if let Some(postings) = text {
                for posting in postings {
                    *rows.entry(posting.row).or_insert(0) += posting.tf;
                }
            }
            let df = rows.len() as f64;
            let idf = (1.0 + (total - df + 0.5) / (df + 0.5)).ln();
            for (row, tf) in rows {
                let tf = tf as f64;
                let length = self.doc_lengths[row as usize] as f64;
                let denominator =
                    tf + BM25_K1 * (1.0 - BM25_B + BM25_B * length / self.avg_doc_len.max(1.0));
                *scores.entry(row).or_insert(0.0) += idf * (tf * (BM25_K1 + 1.0)) / denominator;
            }
        }
        let mut hits: Vec<LexicalHit> = scores
            .into_iter()
            .map(|(row, score)| LexicalHit { row, score })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        hits
    }

    pub fn row_by_id(&self, id: &str) -> Option<u32> {
        self.id_to_row.get(id).copied()
    }

    pub fn row_id(&self, row: u32) -> Option<&str> {
        self.row_ids.get(row as usize).map(String::as_str)
    }

    pub fn is_metadata(&self, row: u32) -> bool {
        self.metadata_rows.contains(&row)
    }

    pub fn row_count(&self) -> usize {
        self.row_ids.len()
    }

    pub fn term_count(&self) -> usize {
        self.identity_postings.len() + self.text_postings.len()
    }
}

/// Reciprocal-rank-fusion contribution for a 1-based rank. The two legs'
/// scores are never compared — each contributes `1/(k + rank)`, so a rank-1
/// lexical hit and a rank-1 dense hit contribute equally.
pub(crate) fn rrf_contribution(rank: usize) -> f64 {
    1.0 / (RRF_K + rank) as f64
}

fn term_counts(tokens: &[String]) -> HashMap<&str, u32> {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for token in tokens {
        *counts.entry(token.as_str()).or_insert(0) += 1;
    }
    counts
}

/// Decode the source filename from a producer entity ref
/// (`{prefix}:{index}:utf8-<hex>:<chunk>`). Non-conforming refs carry no
/// filename tokens — their rows stay findable by text terms only.
fn filename_from_entity_ref(entity_ref: &str) -> Option<String> {
    let segment = entity_ref
        .split(':')
        .find(|segment| segment.len() > "utf8-".len() && segment.starts_with("utf8-"))?;
    decode_hex_utf8(&segment["utf8-".len()..])
}

fn decode_hex_utf8(hex: &str) -> Option<String> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut chars = hex.bytes();
    while let (Some(high), Some(low)) = (chars.next(), chars.next()) {
        let high = (high as char).to_digit(16)?;
        let low = (low as char).to_digit(16)?;
        bytes.push((high * 16 + low) as u8);
    }
    String::from_utf8(bytes).ok()
}

/// Lowercase alphanumeric runs of length ≥ 2, minus function words —
/// one tokenizer for index and query alike.
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.len() >= MIN_TOKEN_LEN && !STOPWORDS.contains(token))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(filename: &str) -> String {
        filename.bytes().map(|byte| format!("{byte:02x}")).collect()
    }

    fn row(id: &str, filename: &str, text: &str) -> (String, String, Option<String>) {
        (
            id.to_string(),
            format!("corpus:fixture::fine:utf8-{}:0", hex(filename)),
            Some(text.to_string()),
        )
    }

    #[test]
    fn decodes_the_source_filename_from_an_entity_ref() {
        let entity = format!(
            "corpus:zk-ref-open::fine:utf8-{}:49",
            hex("magnifica-humanitas.md")
        );
        assert_eq!(
            filename_from_entity_ref(&entity).as_deref(),
            Some("magnifica-humanitas.md")
        );
        assert_eq!(filename_from_entity_ref("corpus:plain:ref:0"), None);
    }

    #[test]
    fn tokenizes_lowercase_alphanumeric_runs() {
        let tokens = tokenize("Magnifica-Humanitas.md, §50–53!");
        assert_eq!(tokens, ["magnifica", "humanitas", "md", "50", "53"]);
    }

    #[test]
    fn entity_name_query_hits_the_named_documents_chunks() {
        let rows = [
            row(
                "a0",
                "alpha-charter.md",
                "dignity of work expresses and enhances lives",
            ),
            row(
                "a1",
                "alpha-charter.md",
                "subsidiarity places decisions closest to the person",
            ),
            row(
                "a2",
                "alpha-charter.md",
                "the limit is positive and systems flourish",
            ),
            row(
                "d0",
                "decoy-instructions.md",
                "system prompt instructions template",
            ),
            row(
                "d1",
                "decoy-dialogue.md",
                "dialogue history and working memory",
            ),
        ];
        let index = LexicalIndex::build(rows);
        let hits = index.search("alpha charter", 5);
        assert!(
            !hits.is_empty(),
            "the bare entity name must hit the named document"
        );
        assert!(
            hits.iter().all(|hit| hit.row < 3),
            "every hit must be an alpha-charter row, got {hits:?}"
        );
    }

    #[test]
    fn metadata_rows_carry_no_postings() {
        let rows = [
            row(
                "m0",
                "MANIFEST.json",
                r#"{"slug": "alpha-charter", "retrieval": "fixture"}"#,
            ),
            row("a0", "alpha-charter.md", "dignity of work"),
        ];
        let index = LexicalIndex::build(rows);
        let hits = index.search("alpha charter retrieval", 5);
        assert!(
            hits.iter().all(|hit| hit.row == 1),
            "the metadata row must not surface lexically, got {hits:?}"
        );
        assert!(index.is_metadata(0));
        assert!(!index.is_metadata(1));
    }

    #[test]
    fn common_text_terms_are_pruned_but_identity_terms_survive() {
        let mut rows = Vec::new();
        for index in 0..=(RARE_TERM_MAX_DF + 1) {
            rows.push((
                format!("id-{index}"),
                format!("corpus:fixture::fine:utf8-{}:{index}", hex("big-book.md")),
                Some("shared commonterm vocabulary throughout".to_string()),
            ));
        }
        let index = LexicalIndex::build(rows);
        assert!(
            index.search("commonterm", 5).is_empty(),
            "text terms above the df ceiling are pruned"
        );
        let hits = index.search("big book", RARE_TERM_MAX_DF + 2);
        assert_eq!(
            hits.len(),
            RARE_TERM_MAX_DF + 2,
            "filename terms survive the df ceiling for every chunk of the document"
        );
    }

    #[test]
    fn queries_without_rare_terms_return_no_hits() {
        let rows = [row("a0", "alpha-charter.md", "dignity of work")];
        let index = LexicalIndex::build(rows);
        assert!(index.search("qqq www unindexed", 5).is_empty());
    }

    #[test]
    fn rrf_contribution_decreases_with_rank() {
        assert!(rrf_contribution(1) > rrf_contribution(2));
        assert!(rrf_contribution(2) > rrf_contribution(METADATA_RANK_PENALTY + 1));
    }
}
