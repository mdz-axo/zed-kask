use super::config::default_corpus_config;
use super::utils::{extract_search_terms, slugify};
use super::*;

// ── slugify ─────────────────────────────────────────────────────────

#[test]
fn slugify_ascii_name() {
    let s = slugify("David Dunning");
    assert_eq!(s, "david-dunning");
}

#[test]
fn slugify_with_special_chars() {
    let s = slugify("J. R. R. Tolkien");
    assert!(s.contains("tolkien"));
}

#[test]
fn slugify_non_ascii_fallback() {
    // All non-ASCII characters produce empty slug → UUID fallback
    let s = slugify("中文作者");
    assert!(!s.is_empty());
    // UUID format: 8-4-4-4-12 hex chars
    assert_eq!(s.len(), 36);
    assert_eq!(s.chars().filter(|c| *c == '-').count(), 4);
}

#[test]
fn slugify_empty_string() {
    let s = slugify("");
    assert!(!s.is_empty()); // UUID fallback
    assert_eq!(s.len(), 36);
}

// ── parse_template_model ────────────────────────────────────────────

#[test]
fn parse_model_directive_present() {
    let src = "{# model: OM/qwen3:14b #}\nrest of template";
    assert_eq!(
        super::llm::parse_template_model(src),
        Some("OM/qwen3:14b".to_string())
    );
}

#[test]
fn parse_model_directive_absent() {
    let src = "You are analyzing the academic work of {{ author_name }}.";
    assert_eq!(super::llm::parse_template_model(src), None);
}

#[test]
fn parse_model_directive_empty_template() {
    assert_eq!(super::llm::parse_template_model(""), None);
}

#[test]
fn parse_model_directive_whitespace_handling() {
    let src = "  {# model: DI/meta-llama/Llama-3.3-70B-Instruct #}  \nrest";
    assert_eq!(
        super::llm::parse_template_model(src),
        Some("DI/meta-llama/Llama-3.3-70B-Instruct".to_string())
    );
}

// ── default_corpus_config ───────────────────────────────────────────

#[test]
fn default_corpus_config_has_correct_defaults() {
    let config = default_corpus_config("test-author");
    assert_eq!(config.author, "test-author");
    assert_eq!(config.corpus_type, "literary");
    assert_eq!(config.embedding.dim, 1024);
    assert_eq!(config.chunking.min_words, 50);
    assert_eq!(config.chunking.max_words, 200);
    assert_eq!(config.centroid_entity_ref, "style:test-author:centroid");
    assert!(config.works.is_empty());
    assert!(config.methods.is_empty());
    assert!(config.foundational_rules.is_empty());
}

#[test]
fn default_corpus_config_academic_entities_empty_by_default() {
    let config = default_corpus_config("author");
    assert!(config.entities.co_authors.is_empty());
    assert!(config.entities.venues.is_empty());
    assert!(config.entities.topics.is_empty());
    assert!(config.entities.paradigms.is_empty());
}

// ── DiscoveredWork with abstract ────────────────────────────────────

#[test]
fn discovered_work_serializes_abstract() {
    let work = DiscoveredWork {
        title: "Test Paper".to_string(),
        slug: "test-paper".to_string(),
        url: "https://example.com".to_string(),
        year: Some(2024),
        source: "semantic_scholar".to_string(),
        work_type: "journal_article".to_string(),
        abstract_text: Some("This paper explores...".to_string()),
    };
    let json = serde_json::to_string(&work).unwrap();
    assert!(json.contains("abstract_text"));
    assert!(json.contains("This paper explores"));
}

#[test]
fn discovered_work_omits_none_abstract() {
    let work = DiscoveredWork {
        title: "Test".to_string(),
        slug: "test".to_string(),
        url: "https://example.com".to_string(),
        year: None,
        source: "web".to_string(),
        work_type: "web_page".to_string(),
        abstract_text: None,
    };
    let json = serde_json::to_string(&work).unwrap();
    // serde(default) serializes None as null, not omitted
    assert!(json.contains("\"abstract_text\":null"));
}

// ── extract_search_terms ────────────────────────────────────────────

#[test]
fn extract_search_terms_from_titles() {
    let titles = vec![
        "Unskilled and Unaware of It".to_string(),
        "Flawed Self-Assessment".to_string(),
        "Why People Fail to Recognize Their Own Incompetence".to_string(),
    ];
    let terms = extract_search_terms("David Dunning", &titles);
    assert!(terms.starts_with("David Dunning"));
    assert!(!terms.is_empty());
}

#[test]
fn extract_search_terms_empty_titles() {
    let terms = extract_search_terms("Author", &[]);
    assert_eq!(terms, "Author");
}

// ── DiscoverRequest defaults ────────────────────────────────────────

#[test]
fn discover_request_defaults() {
    let req = DiscoverRequest {
        author_name: "Test".to_string(),
        max_works: 10,
        cache_dir: "./cache".to_string(),
        output_dir: None,
        serpapi_key: None,
        include_transcripts: true,
        include_web: true,
        curated: true,
        web_search_terms: None,
        augment: false,
        include_methods: true,
        biographical_details: None,
    };
    assert!(req.include_methods);
    assert!(req.curated);
    assert!(!req.augment);
    assert!(req.biographical_details.is_none());
}

#[test]
fn discover_request_with_bio() {
    let req = DiscoverRequest {
        author_name: "J. Smith".to_string(),
        max_works: 10,
        cache_dir: "./cache".to_string(),
        output_dir: None,
        serpapi_key: None,
        include_transcripts: true,
        include_web: true,
        curated: true,
        web_search_terms: None,
        augment: false,
        include_methods: true,
        biographical_details: Some("professor of psychology at Cornell".to_string()),
    };
    assert_eq!(
        req.biographical_details.as_deref(),
        Some("professor of psychology at Cornell")
    );
}
