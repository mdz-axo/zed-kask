//! Corpus YAML generation and augmentation for the discovery pipeline.

use crate::embed::{CorpusConfig, EntityConfig, Work};
use hkask_memory::salience::DeclaredMethod;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use std::path::{Path, PathBuf};

use super::types::DiscoveredWork;

/// Generate a corpus.yaml from a list of discovered works.
///
/// Public so the CLI can regenerate the config after curation —
/// selected web/YouTube candidates are added to the works list
/// and a fresh corpus.yaml is written.
///
/// When `entities` and `methods` are provided (from LLM extraction phases),
/// they are included in the generated config. Sets `corpus_type: "academic"`
/// since this is the academic discovery pipeline.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  author_slug must be non-empty; works must be non-empty; output_dir must exist
/// post: corpus.yaml is written to output_dir; returns PathBuf to the written file; Err on serialization or I/O failure
#[must_use = "result must be used"]
pub fn generate_corpus_yaml(
    author_slug: &str,
    works: &[DiscoveredWork],
    output_dir: &Path,
    entities: Option<EntityConfig>,
    methods: &[DeclaredMethod],
) -> Result<PathBuf, ServiceError> {
    // P9: Regulation span
    tracing::info!(target: "hkask.discover", operation = "generate_corpus_yaml", author = %author_slug, work_count = works.len(), method_count = methods.len(), "REG");

    let corpus_works: Vec<Work> = works
        .iter()
        .map(|w| {
            let format = match w.work_type.as_str() {
                "journal_article" | "preprint" => "pdf",
                "video_transcript" => "text",
                _ => "web",
            };
            let document_type = match w.work_type.as_str() {
                "journal_article" | "preprint" => Some("research-paper".to_string()),
                _ => None,
            };
            Work {
                title: w.title.clone(),
                slug: w.slug.clone(),
                url: w.url.clone(),
                local_path: None,
                format: format.to_string(),
                document_type,
                dimensions: vec![],
                section_types: vec![],
                mds_categories: vec![],
            }
        })
        .collect();

    let mut config = default_corpus_config(author_slug);
    config.works = corpus_works;
    config.entities = entities.unwrap_or_default();
    config.methods = methods.to_vec();
    config.corpus_type = "academic".to_string();

    let config_yaml = serde_yaml_neo::to_string(&config).map_err(|e| {
        let msg = format!("Failed to serialize corpus config: {e}");
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    let config_path = output_dir.join("corpus.yaml");
    std::fs::write(&config_path, &config_yaml).map_err(|e| {
        let msg = format!(
            "Failed to write corpus.yaml to '{}': {e}",
            config_path.display()
        );
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    Ok(config_path)
}

/// Default corpus configuration for a given author slug.
///
/// Shared between `generate_corpus_yaml` and the CLI curation section
/// to prevent default drift. All corpus config defaults live here.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  author_slug must be non-empty
/// post: returns CorpusConfig with default embedding, chunking, validation, and budget settings
#[must_use]
pub fn default_corpus_config(author_slug: &str) -> CorpusConfig {
    CorpusConfig {
        author: author_slug.to_string(),
        embedding: crate::embed::EmbeddingConfig {
            model: "DI/Qwen/Qwen3-Embedding-0.6B".to_string(),
            dim: 1024,
            batch_size: 64,
        },
        works: vec![],
        foundational_rules: vec![],
        chunking: crate::embed::ChunkingConfig {
            min_words: 50,
            max_words: 200,
            sentence_boundary: ".!? ".to_string(),
        },
        centroid_entity_ref: format!("style:{author_slug}:centroid"),
        validation: crate::embed::ValidationConfig {
            centroid_distance_max: 0.25,
            exemplar_count_min: 3,
            exemplar_count_max: 7,
        },
        budget: hkask_memory::salience::BudgetConfig::PerPage {
            per_100_pages: 3750,
        },
        entities: Default::default(),
        methods: vec![],
        corpus_type: "literary".to_string(),
        dimension_centroids: vec![],
        tag_sets: vec![],
        tag_weights: Default::default(),
        classifier: String::new(),
        triple_classifier: String::new(),
        fusion: None,
    }
}

/// Augment an existing corpus.yaml with newly discovered works,
/// extracted concepts, and inferred methods.
///
/// Loads the existing config, merges new works (deduplicated by URL),
/// merges new concepts (deduplicated by name), merges new methods
/// (deduplicated by name), and preserves all other existing metadata.
pub(crate) fn augment_corpus_yaml(
    author_slug: &str,
    new_works: &[DiscoveredWork],
    output_dir: &Path,
    entities: Option<EntityConfig>,
    methods: &[DeclaredMethod],
) -> Result<PathBuf, ServiceError> {
    let config_path = output_dir.join("corpus.yaml");

    // Load existing config
    let existing_yaml = std::fs::read_to_string(&config_path).map_err(|e| {
        let msg = format!("Failed to read existing corpus.yaml for augmentation: {e}");
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;
    let mut config: CorpusConfig = serde_yaml_neo::from_str(&existing_yaml).map_err(|e| {
        let msg = format!("Failed to parse existing corpus.yaml for augmentation: {e}");
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    // Collect existing URLs for dedup
    let existing_urls: std::collections::HashSet<&str> =
        config.works.iter().map(|w| w.url.as_str()).collect();

    // Merge new works (skip duplicates by URL)
    let added: Vec<Work> = new_works
        .iter()
        .filter(|w| !existing_urls.contains(w.url.as_str()))
        .map(|w| {
            let format = match w.work_type.as_str() {
                "journal_article" | "preprint" => "pdf",
                "video_transcript" => "text",
                _ => "web",
            };
            let document_type = match w.work_type.as_str() {
                "journal_article" | "preprint" => Some("research-paper".to_string()),
                _ => None,
            };
            Work {
                title: w.title.clone(),
                slug: w.slug.clone(),
                url: w.url.clone(),
                local_path: None,
                format: format.to_string(),
                document_type,
                dimensions: vec![],
                section_types: vec![],
                mds_categories: vec![],
            }
        })
        .collect();

    let added_count = added.len();
    config.works.extend(added);

    // Merge new concepts (dedup by name)
    if let Some(ref new_entities) = entities {
        let existing_concept_names: std::collections::HashSet<&str> = config
            .entities
            .concepts
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        let new_concepts: Vec<crate::embed::Entity> = new_entities
            .concepts
            .iter()
            .filter(|e| !existing_concept_names.contains(e.name.as_str()))
            .cloned()
            .collect();
        config.entities.concepts.extend(new_concepts);

        let existing_place_names: std::collections::HashSet<&str> = config
            .entities
            .places
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        let new_places: Vec<crate::embed::Entity> = new_entities
            .places
            .iter()
            .filter(|e| !existing_place_names.contains(e.name.as_str()))
            .cloned()
            .collect();
        config.entities.places.extend(new_places);

        let existing_event_names: std::collections::HashSet<&str> = config
            .entities
            .events
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        let new_events: Vec<crate::embed::Entity> = new_entities
            .events
            .iter()
            .filter(|e| !existing_event_names.contains(e.name.as_str()))
            .cloned()
            .collect();
        config.entities.events.extend(new_events);
    }

    // Merge new methods (dedup by name)
    if !methods.is_empty() {
        let existing_method_names: std::collections::HashSet<&str> =
            config.methods.iter().map(|m| m.name.as_str()).collect();
        let new_methods: Vec<DeclaredMethod> = methods
            .iter()
            .filter(|m| !existing_method_names.contains(m.name.as_str()))
            .cloned()
            .collect();
        config.methods.extend(new_methods);
    }

    // Write back
    let config_yaml = serde_yaml_neo::to_string(&config).map_err(|e| {
        let msg = format!("Failed to serialize augmented config: {e}");
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;
    std::fs::write(&config_path, &config_yaml).map_err(|e| {
        let msg = format!(
            "Failed to write augmented corpus.yaml to '{}': {e}",
            config_path.display()
        );
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    tracing::info!(target: "hkask.discover", slug = %author_slug, existing_works = config.works.len() - added_count, added = added_count, total = config.works.len(), "Corpus augmented");

    Ok(config_path)
}
