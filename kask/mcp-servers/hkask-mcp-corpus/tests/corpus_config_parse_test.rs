//! Integration tests guarding every operator-authored corpus config YAML
//! against the `CorpusConfig` consumer struct.
//!
//! Each persona corpus.yaml under `kask/registry/styles/` and
//! `kask/corpus/replica/` must parse via `EmbedService::parse_config` and
//! populate the required `CorpusConfig` fields. These tests pin the contract
//! so schema drift fails loudly in CI, not silently at runtime.
//! # REQ: P8 — every test verifies a stated behavioral property of a public seam.

use hkask_mcp_corpus::corpus::EmbedService;
use hkask_memory::salience::BudgetConfig;

/// Resolve a workspace-relative corpus config path from the test crate's
/// manifest dir. Keeps each test free of path-joining boilerplate.
fn corpus_config(path: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// Assert the required `CorpusConfig` invariants that every persona config
/// must satisfy regardless of persona-specific values. Centralized so each
/// per-persona test stays focused on what is distinctive about that persona.
fn assert_required_contract(
    config: &hkask_mcp_corpus::corpus::CorpusConfig,
    author: &str,
    centroid_prefix: &str,
) {
    assert_eq!(config.author, author, "author must match persona name");
    assert!(
        config.centroid_entity_ref.starts_with(centroid_prefix),
        "centroid_entity_ref must be namespaced under style:{author}:, got {}",
        config.centroid_entity_ref
    );
    assert_eq!(config.embedding.dim, 1024, "embedding dim must be 1024");
    assert!(
        config.embedding.batch_size > 0,
        "batch_size must be positive"
    );
    assert!(
        config.validation.exemplar_count_min <= config.validation.exemplar_count_max,
        "exemplar_count_min must be <= exemplar_count_max"
    );
}

// ── Registry persona corpus configs ──────────────────────────────────────

#[test]
fn parse_gentle_lovelace_corpus_yaml() {
    let config = EmbedService::parse_config(&corpus_config(
        "../../registry/styles/gentle-lovelace/corpus.yaml",
    ))
    .expect("gentle-lovelace corpus.yaml must parse");
    assert_required_contract(&config, "gentle-lovelace", "style:gentle-lovelace:");
    assert_eq!(config.works.len(), 11, "gentle-lovelace has 11 works");
    assert_eq!(
        config.dimension_centroids.len(),
        4,
        "four quality dimensions"
    );
    // Budget is Flat { total_passages, triple_budget_per_100 }.
    match &config.budget {
        BudgetConfig::Flat {
            total_passages,
            triple_budget_per_100,
        } => {
            assert_eq!(*total_passages, 5000);
            assert_eq!(*triple_budget_per_100, 8100);
        }
        other => panic!("gentle-lovelace budget must be Flat, got {other:?}"),
    }
}

#[test]
fn parse_woolf_corpus_yaml() {
    let config =
        EmbedService::parse_config(&corpus_config("../../registry/styles/woolf/corpus.yaml"))
            .expect("woolf corpus.yaml must parse");
    assert_required_contract(&config, "woolf", "style:woolf:");
    assert!(
        !config.works.is_empty(),
        "woolf must declare at least one work"
    );
    match &config.budget {
        BudgetConfig::Flat {
            total_passages,
            triple_budget_per_100,
        } => {
            assert_eq!(*total_passages, 0);
            assert_eq!(*triple_budget_per_100, 3750);
        }
        other => panic!("woolf budget must be Flat, got {other:?}"),
    }
}

#[test]
fn parse_ulysses_s_twain_corpus_yaml() {
    let config = EmbedService::parse_config(&corpus_config(
        "../../registry/styles/ulysses-s-twain/corpus.yaml",
    ))
    .expect("ulysses-s-twain corpus.yaml must parse");
    assert_required_contract(&config, "ulysses-s-twain", "style:ulysses-s-twain:");
    assert!(
        !config.works.is_empty(),
        "ulysses-s-twain must declare at least one work"
    );
    match &config.budget {
        BudgetConfig::Flat {
            total_passages,
            triple_budget_per_100,
        } => {
            assert_eq!(*total_passages, 0);
            assert_eq!(*triple_budget_per_100, 3750);
        }
        other => panic!("ulysses-s-twain budget must be Flat, got {other:?}"),
    }
}

#[test]
fn parse_jane_wilde_corpus_yaml() {
    let config = EmbedService::parse_config(&corpus_config(
        "../../registry/styles/jane-wilde/corpus.yaml",
    ))
    .expect("jane-wilde corpus.yaml must parse");
    assert_required_contract(&config, "jane-wilde", "style:jane-wilde:");
    assert!(
        !config.works.is_empty(),
        "jane-wilde must declare at least one work"
    );
    match &config.budget {
        BudgetConfig::Flat {
            total_passages,
            triple_budget_per_100,
        } => {
            assert_eq!(*total_passages, 0);
            assert_eq!(*triple_budget_per_100, 3750);
        }
        other => panic!("jane-wilde budget must be Flat, got {other:?}"),
    }
}

#[test]
fn parse_david_dunning_corpus_yaml() {
    let config = EmbedService::parse_config(&corpus_config(
        "../../registry/styles/david-dunning/corpus.yaml",
    ))
    .expect("david-dunning corpus.yaml must parse");
    assert_required_contract(&config, "david-dunning", "style:david-dunning:");
    assert_eq!(config.corpus_type, "academic", "david-dunning is academic");
    assert!(
        config.works.is_empty(),
        "david-dunning uses discovery (no static works)"
    );
    match &config.budget {
        BudgetConfig::PerPage { per_100_pages } => {
            assert_eq!(*per_100_pages, 3750);
        }
        other => panic!("david-dunning budget must be PerPage, got {other:?}"),
    }
}

#[test]
fn parse_hemingway_corpus_yaml() {
    let config = EmbedService::parse_config(&corpus_config(
        "../../registry/styles/hemingway/corpus.yaml",
    ))
    .expect("hemingway corpus.yaml must parse");
    assert_required_contract(&config, "hemingway", "style:hemingway:");
    assert!(
        !config.works.is_empty(),
        "hemingway must declare at least one work"
    );
    match &config.budget {
        BudgetConfig::Flat {
            total_passages,
            triple_budget_per_100,
        } => {
            assert_eq!(*total_passages, 0);
            assert_eq!(*triple_budget_per_100, 3750);
        }
        other => panic!("hemingway budget must be Flat, got {other:?}"),
    }
}

#[test]
fn parse_agatha_eliot_corpus_yaml() {
    let config = EmbedService::parse_config(&corpus_config(
        "../../registry/styles/agatha-eliot/corpus.yaml",
    ))
    .expect("agatha-eliot corpus.yaml must parse");
    assert_required_contract(&config, "agatha-eliot", "style:agatha-eliot:");
    assert!(
        !config.works.is_empty(),
        "agatha-eliot must declare at least one work"
    );
    match &config.budget {
        BudgetConfig::Flat {
            total_passages,
            triple_budget_per_100,
        } => {
            assert_eq!(*total_passages, 0);
            assert_eq!(*triple_budget_per_100, 3750);
        }
        other => panic!("agatha-eliot budget must be Flat, got {other:?}"),
    }
}

// ── Replica persona corpus config ────────────────────────────────────────

#[test]
fn parse_john_brooks_replica_yaml() {
    let config =
        EmbedService::parse_config(&corpus_config("../../corpus/replica/john-brooks.yaml"))
            .expect("john-brooks.yaml must parse");
    assert_required_contract(&config, "john-brooks", "style:john-brooks:");
    assert_eq!(
        config.works.len(),
        1,
        "john-brooks has one work (local corpus)"
    );
    assert_eq!(config.works[0].slug, "capabilities-researcher");
    assert_eq!(config.foundational_rules.len(), 1);
    assert_eq!(
        config.dimension_centroids.len(),
        4,
        "four quality dimensions"
    );
    let total_weight: f64 = config.dimension_centroids.iter().map(|dc| dc.weight).sum();
    assert!(
        (total_weight - 1.0).abs() < 1e-6,
        "dimension centroid weights must sum to 1.0, got {total_weight}"
    );
    // john-brooks.yaml declares `budget: max_triples: 0` to disable triples.
    // See tasks/phase5-refactor-decision.md ADR for the BudgetConfig variant
    // reorder that makes Absolute reachable from YAML.
    match &config.budget {
        BudgetConfig::Absolute { max_triples } => {
            assert_eq!(
                *max_triples, 0,
                "john-brooks budget must be Absolute{max_triples:0}"
            );
        }
        other => panic!("john-brooks budget must be Absolute, got {other:?}"),
    }
}
