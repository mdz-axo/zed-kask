//! Integration test: the persona YAML generator produces configs that parse
//! back into `CorpusConfig` and match the hand-authored literary personas
//! semantically. Guards the generator as the single producer of persona
//! YAMLs so they cannot drift from `CorpusConfig`.
//! # REQ: P8 — every test verifies a stated behavioral property of a public seam.

use hkask_memory::salience::BudgetConfig;
use hkask_services_corpus::{
    ChunkingConfig, DimensionCentroid, Entity, EntityConfig, FoundationalRule, PersonaSpec,
    ValidationConfig, Work, generate_persona_yaml,
};
use std::collections::HashMap;

/// The generator's output must round-trip: serialize a `PersonaSpec` to YAML,
/// parse it back via `EmbedService::parse_config`, and the parsed config must
/// match the inputs. This proves the generator is a valid producer of
/// `CorpusConfig`-shaped YAML.
#[test]
fn generate_persona_yaml_round_trips() {
    let works = vec![Work {
        title: "Capabilities Researcher Corpus".to_string(),
        slug: "capabilities-researcher".to_string(),
        url: String::new(),
        local_path: Some("corpus/extracted/researcher".to_string()),
        format: "text".to_string(),
        document_type: None,
        dimensions: vec![],
        section_types: vec![],
        mds_categories: vec![],
    }];

    let spec = PersonaSpec {
        foundational_rules: vec![FoundationalRule {
            slug: "capability-performance-gap".to_string(),
            text: "The central analytical question: what is the economic significance of the gap between what a system is capable of and what it actually achieves?".to_string(),
            dimensions: vec![],
            section_type: None,
        }],
        dimension_centroids: vec![
            DimensionCentroid {
                name: "gentle".to_string(),
                ref_name: "style:john-brooks:gentle".to_string(),
                weight: 0.25,
                description: "Accuracy and grounding".to_string(),
            },
            DimensionCentroid {
                name: "lovelace".to_string(),
                ref_name: "style:john-brooks:lovelace".to_string(),
                weight: 0.25,
                description: "Precision".to_string(),
            },
        ],
        chunking: Some(ChunkingConfig {
            min_words: 40,
            max_words: 150,
            sentence_boundary: ".!? ".to_string(),
        }),
        validation: Some(ValidationConfig {
            centroid_distance_max: 0.40,
            exemplar_count_min: 100,
            exemplar_count_max: 10000,
        }),
        budget: Some(BudgetConfig::Absolute { max_triples: 0 }),
        batch_size: Some(25),
        entities: Some(EntityConfig {
            concepts: vec![
                Entity { name: "capability gap".to_string(), appears_in: vec![] },
                Entity { name: "economic significance".to_string(), appears_in: vec![] },
            ],
            ..Default::default()
        }),
        ..Default::default()
    };

    let temp_dir = tempfile::tempdir().expect("temp dir");
    let path = generate_persona_yaml("john-brooks", &works, &spec, temp_dir.path())
        .expect("generate_persona_yaml must write corpus.yaml");

    // Print the generated YAML for inspection (not asserted; informational).
    let generated = std::fs::read_to_string(&path).expect("read generated yaml");
    eprintln!("--- generated john-brooks.yaml ---\n{generated}--- end ---");

    // Round-trip: parse the generated YAML back.
    let parsed = hkask_services_corpus::EmbedService::parse_config(&written_path)
        .expect("generated persona YAML must parse back into CorpusConfig");

    assert_eq!(parsed.author, "john-brooks");
    assert_eq!(parsed.centroid_entity_ref, "style:john-brooks:centroid");
    assert_eq!(
        parsed.embedding.batch_size, 25,
        "batch_size override applied"
    );
    assert_eq!(parsed.works.len(), 1);
    assert_eq!(parsed.works[0].slug, "capabilities-researcher");
    assert_eq!(parsed.foundational_rules.len(), 1);
    assert_eq!(
        parsed.foundational_rules[0].slug,
        "capability-performance-gap"
    );
    assert_eq!(parsed.chunking.min_words, 40, "chunking override applied");
    assert_eq!(
        parsed.validation.exemplar_count_min, 100,
        "validation override applied"
    );
    assert_eq!(parsed.dimension_centroids.len(), 2);
    match &parsed.budget {
        BudgetConfig::Absolute { max_triples } => {
            assert_eq!(*max_triples, 0, "budget override applied");
        }
        other => panic!("budget must be Absolute, got {other:?}"),
    }
    assert_eq!(parsed.entities.concepts.len(), 2);
    assert_eq!(parsed.entities.concepts[0].name, "capability gap");
}

/// The generator must apply `deny_unknown_fields`-clean output: the generated
/// YAML must not contain any field that `CorpusConfig` does not recognize.
/// This is implicitly verified by the round-trip parse above, but this test
/// makes the intent explicit and fast (no file I/O for the assertion).
#[test]
fn generated_persona_yaml_has_no_unknown_fields() {
    let spec = PersonaSpec {
        corpus_type: Some("literary".to_string()),
        ..Default::default()
    };
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let works = vec![Work {
        title: "Test".to_string(),
        slug: "test".to_string(),
        url: "https://example.com".to_string(),
        local_path: None,
        format: "text".to_string(),
        document_type: None,
        dimensions: vec![],
        section_types: vec![],
        mds_categories: vec![],
    }];
    let path = generate_persona_yaml("test-author", &works, &spec, temp_dir.path())
        .expect("generate must succeed");
    let parsed = hkask_services_corpus::EmbedService::parse_config(&path)
        .expect("generated YAML must parse with zero unknown fields");
    assert_eq!(parsed.corpus_type, "literary");
    assert_eq!(parsed.author, "test-author");
}

/// `tag_weights` (a nested HashMap) must survive the round-trip. This guards
/// the gentle-lovelace persona's tag-weight overrides, which are the most
/// structurally complex field in `CorpusConfig`.
#[test]
fn generated_persona_yaml_preserves_tag_weights() {
    let mut tag_weights: HashMap<String, HashMap<String, f64>> = HashMap::new();
    let mut inner = HashMap::new();
    inner.insert("Gentle".to_string(), 0.50);
    inner.insert("Lovelace".to_string(), 0.30);
    tag_weights.insert("specification".to_string(), inner);

    let spec = PersonaSpec {
        tag_weights,
        ..Default::default()
    };
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let works = vec![Work {
        title: "T".to_string(),
        slug: "t".to_string(),
        url: "u".to_string(),
        local_path: None,
        format: "text".to_string(),
        document_type: None,
        dimensions: vec![],
        section_types: vec![],
        mds_categories: vec![],
    }];
    let path = generate_persona_yaml("tw-author", &works, &spec, temp_dir.path())
        .expect("generate must succeed");
    let parsed = hkask_services_corpus::EmbedService::parse_config(&path)
        .expect("tag_weights must round-trip");
    let spec_weights = parsed
        .tag_weights
        .get("specification")
        .expect("specification key preserved");
    assert!((spec_weights["Gentle"] - 0.50).abs() < 1e-9);
    assert!((spec_weights["Lovelace"] - 0.30).abs() < 1e-9);
}
