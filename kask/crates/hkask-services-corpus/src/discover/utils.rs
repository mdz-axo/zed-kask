//! Utility functions for the discovery pipeline.

/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  s may be any string (including empty)
/// post: returns lowercase, alphanumeric-only slug with hyphens; empty string becomes empty slug
#[must_use]
pub fn slugify(s: &str) -> String {
    let slug = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    // Fallback to UUID-based slug for all-non-ASCII titles
    if slug.is_empty() {
        let uuid_slug = uuid::Uuid::new_v4().to_string();
        tracing::warn!(target: "hkask.discover", input = %s, fallback = %uuid_slug, "slugify produced empty string — using UUID fallback");
        uuid_slug
    } else {
        slug
    }
}

pub(crate) fn extract_search_terms(author: &str, titles: &[String]) -> String {
    if titles.is_empty() {
        return author.to_string();
    }

    let stopwords: &[&str] = &[
        "study",
        "studies",
        "analysis",
        "effect",
        "effects",
        "evidence",
        "research",
        "review",
        "approach",
        "model",
        "theory",
        "data",
        "using",
        "based",
        "new",
        "role",
        "among",
        "across",
        "within",
        "toward",
        "towards",
        "understanding",
        "implications",
        "introduction",
        "overview",
        "perspective",
        "commentary",
        "response",
        "reply",
        "the",
        "and",
        "for",
        "from",
        "with",
    ];

    let mut word_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for title in titles {
        for word in title.split_whitespace() {
            let cleaned: String = word
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_lowercase();
            if cleaned.len() < 4 || stopwords.contains(&cleaned.as_str()) {
                continue;
            }
            *word_counts.entry(cleaned).or_insert(0) += 1;
        }
    }

    let mut sorted: Vec<(&String, &usize)> = word_counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    let top_words: Vec<&str> = sorted
        .iter()
        .take(5)
        .filter(|(_, count)| **count >= 2)
        .map(|(word, _)| word.as_str())
        .collect();

    if top_words.is_empty() {
        return author.to_string();
    }

    format!("{} {}", author, top_words.join(" "))
}
