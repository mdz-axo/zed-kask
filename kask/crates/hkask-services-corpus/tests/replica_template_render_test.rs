//! Integration tests: replica corpus.yaml templates render to valid CorpusConfig.
//!
//! Each replica class template (academic, literary, exemplar) is rendered with
//! sample inputs via minijinja, then the output is parsed via
//! `EmbedService::parse_config`. This guards the template ↔ CorpusConfig
//! contract: if a template emits a field the struct doesn't recognize,
//! `#[serde(deny_unknown_fields)]` rejects it here.
//! # REQ: P8 — every test verifies a stated behavioral property of a public seam.

use hkask_services_corpus::EmbedService;
use minijinja::{Environment, UndefinedBehavior, value::Value};
use serde_json::json;

/// Render a replica template with the given context, resolving `{% extends %}`
/// and `{% include %}` from the registry templates directory.
fn render_replica_template(template_name: &str, context: serde_json::Value) -> String {
    let templates_dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/templates");

    let template_path = templates_dir.join(template_name);
    let template_source = std::fs::read_to_string(&template_path)
        .unwrap_or_else(|e| panic!("failed to read {template_name}: {e}"));

    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Lenient);

    let base = templates_dir.clone();
    env.set_loader(
        move |name: &str| -> Result<Option<String>, minijinja::Error> {
            let path = base.join(name);
            if let Ok(content) = std::fs::read_to_string(&path) {
                return Ok(Some(content));
            }
            let j2_name = format!("{name}.j2");
            if let Ok(content) = std::fs::read_to_string(base.join(&j2_name)) {
                return Ok(Some(content));
            }
            Ok(None)
        },
    );

    env.add_template("main", &template_source)
        .unwrap_or_else(|e| panic!("template {template_name} failed to parse: {e}"));

    let tmpl = env.get_template("main").unwrap();
    let ctx = Value::from_serialize(&context);
    tmpl.render(ctx)
        .unwrap_or_else(|e| panic!("template {template_name} failed to render: {e}"))
}

/// Write rendered YAML to a temp file and parse it via EmbedService::parse_config.
fn parse_rendered(rendered: &str) -> hkask_services_corpus::CorpusConfig {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let config_path = temp_dir.path().join("corpus.yaml");
    std::fs::write(&config_path, rendered).expect("write temp yaml");
    EmbedService::parse_config(&config_path)
        .unwrap_or_else(|e| panic!("rendered YAML failed to parse as CorpusConfig: {e}"))
}

#[test]
fn academic_corpus_template_renders_and_parses() {
    let context = json!({
        "author_slug": "david-dunning",
        "author_name": "David Dunning",
        "generation_date": "2026-07-26",
        "works": [
            {
                "title": "Unskilled and Unaware of It",
                "slug": "unskilled-unaware",
                "url": "https://example.com/paper1.pdf",
                "format": "pdf",
                "document_type": "research-paper"
            }
        ],
        "chunking": { "min_words": 50, "max_words": 200, "sentence_boundary": ".!? " },
        "validation": { "centroid_distance_max": 0.25, "exemplar_count_min": 3, "exemplar_count_max": 7 }
    });

    let rendered = render_replica_template("replica/academic-corpus.j2", context);
    let config = parse_rendered(&rendered);

    assert_eq!(config.author, "david-dunning");
    assert_eq!(config.corpus_type, "academic");
    assert_eq!(config.works.len(), 1);
    assert_eq!(config.works[0].slug, "unskilled-unaware");
    assert_eq!(config.works[0].format, "pdf");
    assert_eq!(
        config.works[0].document_type.as_deref(),
        Some("research-paper")
    );
    assert!(config.foundational_rules.is_empty());
    match &config.budget {
        hkask_memory::salience::BudgetConfig::PerPage { per_100_pages } => {
            assert_eq!(*per_100_pages, 3750);
        }
        other => panic!("academic budget must be PerPage, got {other:?}"),
    }
}

#[test]
fn literary_corpus_template_renders_and_parses() {
    let context = json!({
        "author_slug": "woolf",
        "author_name": "Virginia Woolf",
        "generation_date": "2026-07-26",
        "works": [
            {
                "title": "Mrs Dalloway",
                "slug": "mrs-dalloway",
                "url": "https://www.gutenberg.org/cache/epub/71865/pg71865.txt",
                "format": "text"
            }
        ],
        "foundational_rules": [
            {
                "slug": "modern-fiction",
                "text": "Examine an ordinary mind on an ordinary day.",
                "dimensions": ["Gentle"],
                "section_type": "Statement"
            }
        ],
        "chunking": { "min_words": 50, "max_words": 200, "sentence_boundary": ".!? " },
        "validation": { "centroid_distance_max": 0.25, "exemplar_count_min": 3, "exemplar_count_max": 7 },
        "budget": { "total_passages": 0, "triple_budget_per_100": 3750 }
    });

    let rendered = render_replica_template("replica/literary-corpus.j2", context);
    let config = parse_rendered(&rendered);

    assert_eq!(config.author, "woolf");
    assert_eq!(config.corpus_type, "literary");
    assert_eq!(config.works.len(), 1);
    assert_eq!(config.works[0].slug, "mrs-dalloway");
    assert_eq!(config.foundational_rules.len(), 1);
    assert_eq!(config.foundational_rules[0].slug, "modern-fiction");
    assert_eq!(config.foundational_rules[0].dimensions, vec!["Gentle"]);
    assert_eq!(config.triple_classifier, "triple-extractor-literary");
    match &config.budget {
        hkask_memory::salience::BudgetConfig::Flat {
            total_passages,
            triple_budget_per_100,
        } => {
            assert_eq!(*total_passages, 0);
            assert_eq!(*triple_budget_per_100, 3750);
        }
        other => panic!("literary budget must be Flat, got {other:?}"),
    }
}

#[test]
fn exemplar_corpus_template_renders_and_parses() {
    let context = json!({
        "author_slug": "john-brooks",
        "author_name": "John Brooks",
        "generation_date": "2026-07-26",
        "works": [
            {
                "title": "Capabilities Researcher Corpus",
                "slug": "capabilities-researcher",
                "url": "",
                "local_path": "corpus/extracted/researcher",
                "format": "text"
            }
        ],
        "foundational_rules": [
            {
                "slug": "capability-performance-gap",
                "text": "What is the economic significance of the gap between capability and achievement?"
            }
        ],
        "dimension_centroids": [
            { "name": "gentle", "ref_name": "style:john-brooks:gentle", "weight": 0.25, "description": "Accuracy and grounding" },
            { "name": "lovelace", "ref_name": "style:john-brooks:lovelace", "weight": 0.25, "description": "Precision" }
        ],
        "chunking": { "min_words": 40, "max_words": 150, "sentence_boundary": ".!? " },
        "validation": { "centroid_distance_max": 0.40, "exemplar_count_min": 100, "exemplar_count_max": 10000 },
        "budget": { "max_triples": 0 },
        "entities": {
            "concepts": [
                { "name": "capability gap" },
                { "name": "economic significance" }
            ]
        },
        "classifier": "",
        "triple_classifier": "h_mem-extractor"
    });

    let rendered = render_replica_template("replica/exemplar-corpus.j2", context);
    let config = parse_rendered(&rendered);

    assert_eq!(config.author, "john-brooks");
    assert_eq!(config.corpus_type, "literary");
    assert_eq!(config.works.len(), 1);
    assert_eq!(config.works[0].slug, "capabilities-researcher");
    assert!(config.works[0].local_path.as_deref() == Some("corpus/extracted/researcher"));
    assert_eq!(config.foundational_rules.len(), 1);
    assert_eq!(
        config.foundational_rules[0].slug,
        "capability-performance-gap"
    );
    assert_eq!(config.dimension_centroids.len(), 2);
    assert!((config.dimension_centroids[0].weight - 0.25).abs() < 1e-9);
    assert_eq!(config.entities.concepts.len(), 2);
    assert_eq!(config.entities.concepts[0].name, "capability gap");
    match &config.budget {
        hkask_memory::salience::BudgetConfig::Absolute { max_triples } => {
            assert_eq!(*max_triples, 0);
        }
        other => panic!("exemplar budget must be Absolute, got {other:?}"),
    }
}

#[test]
fn exemplar_corpus_template_supports_flat_budget() {
    // gentle-lovelace is an exemplar replica that uses Flat budget, not Absolute.
    // The exemplar template must support budget_shape: "flat" to regenerate it.
    let context = json!({
        "author_slug": "gentle-lovelace",
        "author_name": "Gentle Lovelace",
        "generation_date": "2026-07-26",
        "works": [
            {
                "title": "Notes on the Analytical Engine",
                "slug": "lovelace-notes",
                "url": "https://www.fourmilab.ch/babbage/sketch.html",
                "local_path": "registry/styles/gentle-lovelace/corpus-sources/lovelace-notes.txt",
                "format": "text",
                "document_type": "specification",
                "dimensions": ["Lovelace"]
            }
        ],
        "foundational_rules": [],
        "dimension_centroids": [
            { "name": "Gentle", "ref_name": "style:gentle-lovelace:gentle-centroid", "weight": 0.50, "description": "Agent-correctness" },
            { "name": "Schriver", "ref_name": "style:gentle-lovelace:schriver-centroid", "weight": 0.30, "description": "Findability" }
        ],
        "chunking": { "min_words": 50, "max_words": 300, "sentence_boundary": "." },
        "validation": { "centroid_distance_max": 0.85, "exemplar_count_min": 4, "exemplar_count_max": 14 },
        "budget_shape": "flat",
        "budget": { "total_passages": 5000, "triple_budget_per_100": 8100 },
        "classifier": "section-classifier",
        "triple_classifier": "h_mem-extractor"
    });

    let rendered = render_replica_template("replica/exemplar-corpus.j2", context);
    let config = parse_rendered(&rendered);

    assert_eq!(config.author, "gentle-lovelace");
    assert_eq!(config.dimension_centroids.len(), 2);
    assert!((config.dimension_centroids[0].weight - 0.50).abs() < 1e-9);
    match &config.budget {
        hkask_memory::salience::BudgetConfig::Flat {
            total_passages,
            triple_budget_per_100,
        } => {
            assert_eq!(*total_passages, 5000);
            assert_eq!(*triple_budget_per_100, 8100);
        }
        other => panic!("exemplar flat budget must be Flat, got {other:?}"),
    }
}

#[test]
fn compound_corpus_template_renders_and_parses() {
    // A compound/mashup replica combining two authors (Agatha Christie ×
    // George Eliot) using the mixture_of_experts strategy.
    let context = json!({
        "author_slug": "agatha-eliot",
        "author_name": "Agatha Christie × George Eliot",
        "generation_date": "2026-07-26",
        "mashup_strategy": "mixture_of_experts",
        "compound_authors": [
            {
                "name": "Agatha Christie",
                "slug": "christie",
                "role": "structural architecture",
                "works": [
                    {
                        "title": "The Mysterious Affair at Styles",
                        "slug": "christie-mysterious-affair",
                        "url": "https://www.gutenberg.org/cache/epub/863/pg863.txt",
                        "format": "text"
                    }
                ]
            },
            {
                "name": "George Eliot",
                "slug": "eliot",
                "role": "psychological depth",
                "works": [
                    {
                        "title": "Middlemarch",
                        "slug": "eliot-middlemarch",
                        "url": "https://www.gutenberg.org/cache/epub/161/pg161.txt",
                        "format": "text"
                    }
                ]
            }
        ],
        "foundational_rules": [
            {
                "slug": "christie-closed-circle",
                "text": "Every suspect must have a motive."
            },
            {
                "slug": "eliot-sympathetic-consciousness",
                "text": "The web of relations is the subject."
            }
        ],
        "chunking": { "min_words": 50, "max_words": 200, "sentence_boundary": ".!? " },
        "validation": { "centroid_distance_max": 0.25, "exemplar_count_min": 3, "exemplar_count_max": 7 },
        "budget_shape": "flat",
        "budget": { "total_passages": 0, "triple_budget_per_100": 3750 },
        "triple_classifier": "triple-extractor-literary"
    });

    let rendered = render_replica_template("replica/compound-corpus.j2", context);
    let config = parse_rendered(&rendered);

    assert_eq!(config.author, "agatha-eliot");
    assert_eq!(config.corpus_type, "literary");
    // Works from both contributors are merged into one list.
    assert_eq!(config.works.len(), 2);
    assert_eq!(config.works[0].slug, "christie-mysterious-affair");
    assert_eq!(config.works[1].slug, "eliot-middlemarch");
    // Foundational rules from both contributors are merged.
    assert_eq!(config.foundational_rules.len(), 2);
    assert_eq!(config.foundational_rules[0].slug, "christie-closed-circle");
    assert_eq!(
        config.foundational_rules[1].slug,
        "eliot-sympathetic-consciousness"
    );
    match &config.budget {
        hkask_memory::salience::BudgetConfig::Flat {
            total_passages,
            triple_budget_per_100,
        } => {
            assert_eq!(*total_passages, 0);
            assert_eq!(*triple_budget_per_100, 3750);
        }
        other => panic!("compound budget must be Flat, got {other:?}"),
    }
}
