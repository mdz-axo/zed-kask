//! EmbedService — Style corpus embedding pipeline with metadata layer.

use super::hmems::store_passage_h_mems;
use super::passage::TaggedPassage;
use super::strategies::ChunkingStrategy;
use super::types::{
    CURATOR_PERSONA, CorpusConfig, DimensionCentroidResult, EmbedPhase, EmbedProgress, EmbedResult,
    ProgressFn,
};
use crate::corpus::embed::Entity;
use crate::runtime::TripleExtraction;
use hkask_memory::SemanticMemory;
use hkask_memory::salience::{self, EntityTags};
use hkask_services_core::{DomainKind, ErrorKind, HkaskSettings, ServiceError};
use hkask_types::InferencePort;
use hkask_types::id::WebID;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Service for the style corpus embedding pipeline with metadata layer.
pub struct EmbedService;

/// Resolve the text content for a work from local path, cache, or download.
async fn resolve_work_text(
    work: &super::types::Work,
    cache_path: &Path,
) -> Result<String, ServiceError> {
    if let Some(ref local) = work.local_path {
        let local_path = std::path::Path::new(local);
        if local_path.is_dir() {
            tracing::info!(work = %work.title, path = %local, "Reading directory of .txt files");
            let mut sources: Vec<_> = std::fs::read_dir(local_path)
                .map_err(|e| {
                    let msg = format!("Failed to read directory {}: {e}", local_path.display());
                    ServiceError::Domain {
                        domain: DomainKind::Wallet,
                        kind: ErrorKind::ServiceUnavailable,
                        source: Some(Box::new(e)),
                        message: msg,
                    }
                })?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "txt"))
                .collect();
            sources.sort();
            let mut combined = String::new();
            for source in &sources {
                match std::fs::read_to_string(source) {
                    Ok(text) => {
                        combined.push_str(&text);
                        combined.push_str("\n\n");
                    }
                    Err(e) => {
                        tracing::warn!(path = %source.display(), error = %e, "Skipping unreadable .txt file");
                    }
                }
            }
            Ok(combined)
        } else if local_path.exists() {
            tracing::info!(work = %work.title, path = %local, "Reading local file");
            std::fs::read_to_string(local_path).map_err(|e| {
                let msg = format!("Failed to read local file {}: {e}", local_path.display());
                ServiceError::Domain {
                    domain: DomainKind::Wallet,
                    kind: ErrorKind::ServiceUnavailable,
                    source: Some(Box::new(e)),
                    message: msg,
                }
            })
        } else {
            tracing::warn!(work = %work.title, path = %local, "Local file not found, falling back to cache/download");
            resolve_from_cache_or_download(work, cache_path).await
        }
    } else {
        resolve_from_cache_or_download(work, cache_path).await
    }
}

/// Read from cache or download if cache miss.
async fn resolve_from_cache_or_download(
    work: &super::types::Work,
    cache_path: &Path,
) -> Result<String, ServiceError> {
    if cache_path.exists() {
        tracing::info!(work = %work.title, "Using cached");
        std::fs::read_to_string(cache_path).map_err(|e| {
            let msg = format!("Failed to read cache {}: {e}", cache_path.display());
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })
    } else {
        tracing::info!(work = %work.title, "Downloading");
        let text = crate::corpus::fetch::fetch_text(&work.url).await?;
        if let Err(e) = std::fs::write(cache_path, &text) {
            tracing::warn!(path = %cache_path.display(), error = %e, "Could not cache download");
        }
        Ok(text)
    }
}

/// Classify section types and extract semantic h_mems for all passages.
async fn classify_and_extract(
    all_passages: &mut [TaggedPassage],
    config: &CorpusConfig,
    registry_dir: &Path,
    inference_port: &Arc<dyn InferencePort>,
) -> Result<(), ServiceError> {
    let passage_count = all_passages.len();
    let texts: Vec<String> = all_passages.iter().map(|p| p.text.clone()).collect();

    // ── Classify section types ──
    let mut classifier_config = if config.classifier.is_empty() {
        tracing::info!("No classifier configured — all passages default to Statement");
        crate::runtime::ClassifierConfig::from_def(&Default::default())
    } else {
        let def = crate::runtime::load_classifier_config(&config.classifier, registry_dir)?;
        crate::runtime::ClassifierConfig::from_def(&def)
    };

    let settings_model = hkask_services_core::HkaskSettings::load().classifier_model();
    if !settings_model.is_empty() {
        classifier_config.model = settings_model;
    }

    tracing::info!(
        total_passages = passage_count,
        model = %classifier_config.model,
        concurrency = classifier_config.concurrency,
        "Starting section type classification"
    );

    let classify_results =
        crate::runtime::classify_batch(&texts, classifier_config, Arc::clone(inference_port))
            .await?;

    for (passage, result) in all_passages.iter_mut().zip(classify_results.iter()) {
        passage.section_type = result.category.clone();
    }

    let classified_counts: HashMap<String, usize> =
        classify_results.iter().fold(HashMap::new(), |mut acc, r| {
            *acc.entry(r.category.clone()).or_insert(0) += 1;
            acc
        });
    tracing::info!(?classified_counts, "Section type classification complete");

    // ── Extract semantic h_mems ──
    if !config.triple_classifier.is_empty() {
        let def = crate::runtime::load_classifier_config(&config.triple_classifier, registry_dir)?;
        let classifier_config = crate::runtime::ClassifierConfig::from_def(&def);

        let settings = HkaskSettings::load();
        let settings_model = settings.classifier_model();
        let mut model_config = classifier_config.clone();
        if !settings_model.is_empty() {
            model_config.model = settings_model;
        }

        tracing::info!(
            total_passages = passage_count,
            model = %model_config.model,
            "Single-model h_mem extraction"
        );

        let a_extractions = crate::runtime::extract_triples_batch(
            &texts,
            &model_config,
            Arc::clone(inference_port),
        )
        .await?;

        for (passage, ext) in all_passages.iter_mut().zip(a_extractions.iter()) {
            passage.semantic_triples = ext.clone();
        }
    } else {
        tracing::info!("HMem classifier disabled — skipping semantic extraction");
    }

    Ok(())
}

/// Compute and store centroid(s) — single or multi-dimension path.
fn compute_centroids(
    all_passages: &[TaggedPassage],
    config: &CorpusConfig,
    semantic: &SemanticMemory,
    author_prefix: &str,
    centroid_ref: &str,
) -> Result<
    (
        DimensionCentroidResult,
        Vec<DimensionCentroidResult>,
        usize,
        bool,
    ),
    ServiceError,
> {
    if config.dimension_centroids.is_empty() {
        // ── Single-centroid path ──
        tracing::info!("Computing style centroid (single)");
        let rule_prefix = format!("style:{}:rule:", &config.author);
        let centroid_result = semantic
            .compute_centroid(
                author_prefix,
                &rule_prefix,
                centroid_ref,
                config.embedding.dim,
                Some(centroid_ref),
                Some(&config.embedding.model),
            )
            .map_err(|e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Memory,
                source: None,
                message: e.to_string(),
            })?;

        return Ok((
            DimensionCentroidResult {
                name: String::new(),
                ref_name: String::new(),
                passage_count: centroid_result.passage_count,
            },
            Vec::new(),
            centroid_result.passage_count,
            centroid_result.stored,
        ));
    }

    // ── Multi-dimension centroid path ──
    tracing::info!(
        dimensions = config.dimension_centroids.len(),
        "Computing per-dimension centroids"
    );

    let centroid_store = semantic.embedding_store();

    let mut dim_refs: HashMap<String, Vec<String>> = HashMap::new();
    for passage in all_passages {
        if passage.is_rule || passage.dimension.is_empty() {
            continue;
        }
        dim_refs
            .entry(passage.dimension.clone())
            .or_default()
            .push(passage.entity_ref.clone());
    }

    let mut dim_centroids: Vec<(String, Vec<f32>, usize)> = Vec::new();

    for dc in &config.dimension_centroids {
        let refs = dim_refs.get(&dc.name);
        let count = refs.map(|r| r.len()).unwrap_or(0);

        if count == 0 {
            tracing::warn!(dimension = %dc.name, "No passages for dimension — skipping centroid");
            continue;
        }

        let Some(refs) = refs else { continue };

        let mut centroid = vec![0.0f32; config.embedding.dim];
        let mut fetched = 0usize;

        for entity_ref in refs {
            if let Ok(emb) = centroid_store.get(entity_ref) {
                for (i, v) in emb.vector.iter().enumerate() {
                    if i < config.embedding.dim {
                        centroid[i] += v;
                    }
                }
                fetched += 1;
            }
        }

        if fetched == 0 {
            tracing::warn!(dimension = %dc.name, "No embeddings fetched for dimension — skipping centroid");
            continue;
        }

        let n = fetched as f32;
        for v in centroid.iter_mut() {
            *v /= n;
        }

        centroid_store
            .store(&dc.ref_name, &centroid, &config.embedding.model)
            .map_err(|e| {
                let msg = format!("Failed to store dimension centroid: {e}");
                ServiceError::Domain {
                    domain: DomainKind::Wallet,
                    kind: ErrorKind::ServiceUnavailable,
                    source: Some(Box::new(e)),
                    message: msg,
                }
            })?;

        tracing::info!(dimension = %dc.name, ref_name = %dc.ref_name, passages = fetched, "Dimension centroid stored");

        dim_centroids.push((dc.name.clone(), centroid, fetched));
    }

    // ── Compute composite centroid (weighted mean) ──
    let mut composite_stored = false;
    if !dim_centroids.is_empty() {
        let mut composite = vec![0.0f32; config.embedding.dim];
        let mut total_weight = 0.0f64;

        for dc in &config.dimension_centroids {
            if let Some((_name, vec, _count)) =
                dim_centroids.iter().find(|(name, _, _)| name == &dc.name)
            {
                for (i, v) in vec.iter().enumerate() {
                    composite[i] += *v * dc.weight as f32;
                }
                total_weight += dc.weight;
            }
        }

        if total_weight > 0.0 {
            for v in composite.iter_mut() {
                *v /= total_weight as f32;
            }

            centroid_store
                .store(centroid_ref, &composite, &config.embedding.model)
                .map_err(|e| {
                    let msg = format!("Failed to store composite centroid: {e}");
                    ServiceError::Domain {
                        domain: DomainKind::Wallet,
                        kind: ErrorKind::ServiceUnavailable,
                        source: Some(Box::new(e)),
                        message: msg,
                    }
                })?;

            tracing::info!(
                composite_ref = %centroid_ref,
                composite_weight = total_weight,
                dimensions = dim_centroids.len(),
                "Composite centroid stored"
            );
            composite_stored = true;
        }
    }

    let multi_passage_count: usize = dim_centroids.iter().map(|(_, _, c)| c).sum();

    let dim_results: Vec<DimensionCentroidResult> = dim_centroids
        .iter()
        .map(|(name, _vec, count)| {
            let ref_name = config
                .dimension_centroids
                .iter()
                .find(|dc| &dc.name == name)
                .map(|dc| dc.ref_name.clone())
                .unwrap_or_default();
            DimensionCentroidResult {
                name: name.clone(),
                ref_name,
                passage_count: *count,
            }
        })
        .collect();

    Ok((
        DimensionCentroidResult {
            name: String::new(),
            ref_name: String::new(),
            passage_count: multi_passage_count,
        },
        dim_results,
        multi_passage_count,
        composite_stored,
    ))
}

impl EmbedService {
    /// Run the full style corpus embedding pipeline with metadata tagging,
    /// salience scoring, and budget-gated h_mem storage.
    ///
    /// See module-level docs for the full phase breakdown.
    #[must_use = "result must be used"]
    pub async fn embed_corpus(
        config_path: &Path,
        db_path: &str,
        db_passphrase: &str,
        cache_dir: Option<&Path>,
        progress: Option<ProgressFn>,
        inference_port: std::sync::Arc<dyn hkask_types::InferencePort>,
    ) -> Result<EmbedResult, ServiceError> {
        // P9: Regulation span
        tracing::info!(target: "hkask.embed", operation = "embed_corpus", config = %config_path.display(), "REG");

        let started = Instant::now();

        // ── Phase 1: Parse config ──────────────────────────────────────
        let config_str = std::fs::read_to_string(config_path).map_err(|e| {
            let msg = format!(
                "Failed to read corpus config {}: {e}",
                config_path.display()
            );
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;
        let config: CorpusConfig = serde_yaml_neo::from_str(&config_str).map_err(|e| {
            let msg = format!("Failed to parse corpus config YAML: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

        let author = config.author.clone();
        let author_prefix = format!("style:{}:", &author);
        let centroid_ref = config.centroid_entity_ref.clone();
        let validation = config.validation.clone();
        let curator_webid = WebID::from_persona(CURATOR_PERSONA);

        // ── Shared progress state + heartbeat ──
        let shared = Arc::new(Mutex::new(EmbedProgress {
            phase: EmbedPhase::Parsing,
            author: author.clone(),
            current_work: String::new(),
            total_passages: 0,
            completed_passages: 0,
            elapsed: Duration::ZERO,
        }));
        let _heartbeat = if let Some(ref cb) = progress {
            let shared_hb = Arc::clone(&shared);
            let cb_hb = Arc::clone(cb);
            Some(tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    let p = {
                        let mut p = shared_hb.lock().unwrap_or_else(|e| e.into_inner());
                        p.elapsed = started.elapsed();
                        p.clone()
                    };
                    if p.phase == EmbedPhase::Done {
                        cb_hb(&p);
                        break;
                    }
                    cb_hb(&p);
                }
            }))
        } else {
            None
        };

        // ── Open DB ────────────────────────────────────────────────────
        let semantic =
            SemanticMemory::open(db_path, db_passphrase, config.embedding.dim).map_err(|e| {
                ServiceError::Domain {
                    kind: ErrorKind::BadRequest,
                    domain: DomainKind::Storage,
                    source: None,
                    message: e.to_string(),
                }
            })?;

        // Purge existing embeddings for idempotent re-ingest
        let purged = semantic.purge_by_prefix(&author_prefix).map_err(|e| {
            let msg = format!("Failed to purge embeddings: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

        // ── Resolve cache directory ────────────────────────────────────
        let default_cache_dir;
        let cache = match cache_dir {
            Some(p) => p,
            None => {
                default_cache_dir = config_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(".cache");
                &default_cache_dir
            }
        };

        // ── Phase 2: Download, cache, chunk, and tag ───────────────────
        {
            let mut p = shared.lock().unwrap_or_else(|e| e.into_inner());
            p.phase = EmbedPhase::Tagging;
        }

        let mut all_passages: Vec<TaggedPassage> = Vec::new();

        for (work_idx, work) in config.works.iter().enumerate() {
            if work_idx > 0 {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }

            {
                let mut p = shared.lock().unwrap_or_else(|e| e.into_inner());
                p.current_work = work.title.clone();
                p.completed_passages = work_idx + 1;
                p.total_passages = config.works.len();
            }

            let cache_path = cache.join(format!("{}.txt", work.slug));
            let text = resolve_work_text(work, &cache_path).await?;

            let cleaned = crate::text::strip_gutenberg_headers(&text);
            let entity_ref_prefix = format!("style:{}:{}", &config.author, work.slug);
            let chunker = crate::corpus::embed::WordCountChunker {
                min_words: config.chunking.min_words,
                max_words: config.chunking.max_words,
                sentence_boundary: config.chunking.sentence_boundary.clone(),
            };
            let chunks = chunker.chunk(&cleaned, &entity_ref_prefix);

            // Tag each chunk
            let total_chunks = chunks.len();
            let work_characters = Entity::name_strings(&config.entities.characters, &work.slug);
            let work_places = Entity::name_strings(&config.entities.places, &work.slug);
            let work_events = Entity::name_strings(&config.entities.events, &work.slug);
            let work_concepts = Entity::name_strings(&config.entities.concepts, &work.slug);

            for (chunk_idx, (entity_ref, text)) in chunks.into_iter().enumerate() {
                let signals = salience::compute_method_signals(&text);
                let mut tags = salience::tag_entities(
                    &text,
                    &work_characters,
                    &work_places,
                    &work_events,
                    &work_concepts,
                );

                // Match declared methods
                for method in &config.methods {
                    if method.matches(&signals) {
                        tags.methods.push(method.name.clone());
                    }
                }

                let position = if total_chunks > 1 {
                    chunk_idx as f32 / (total_chunks - 1) as f32
                } else {
                    0.5
                };

                all_passages.push(TaggedPassage {
                    entity_ref,
                    text,
                    work_slug: work.slug.clone(),
                    work_title: work.title.clone(),
                    position,
                    is_rule: false,
                    tags,
                    signals,
                    salience: 0.0, // computed in batch below
                    dimension: work.dimensions.first().cloned().unwrap_or_default(),
                    document_type: work.document_type.clone().unwrap_or_default(),
                    mds_categories: work.mds_categories.clone(),
                    section_type: String::new(), // filled by classifier below
                    semantic_triples: TripleExtraction::default(), // filled by h_mem classifier
                });
            }

            tracing::info!(
                work = %work.title,
                passages = total_chunks,
                "Chunked and tagged"
            );
        }

        // Append foundational rules as passages (no tagging, position=0.5, low salience)
        for rule in &config.foundational_rules {
            let entity_ref = format!("style:{}:rule:{}", &config.author, rule.slug);
            let signals = salience::compute_method_signals(&rule.text);
            all_passages.push(TaggedPassage {
                entity_ref,
                text: rule.text.clone(),
                work_slug: String::new(),
                work_title: String::new(),
                position: 0.5,
                is_rule: true,
                tags: EntityTags::default(),
                signals,
                salience: 0.0,
                dimension: rule.dimensions.first().cloned().unwrap_or_default(),
                document_type: String::new(),
                mds_categories: Vec::new(),
                section_type: rule.section_type.clone().unwrap_or_default(),
                semantic_triples: TripleExtraction::default(), // rules get empty extraction
            });
        }

        // ── Classify section types + extract semantic h_mems ──
        {
            let mut p = shared.lock().unwrap_or_else(|e| e.into_inner());
            p.phase = EmbedPhase::Tagging;
            p.current_work = "classifying section types".into();
        }

        let registry_dir = config_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .unwrap_or_else(|| Path::new("registry"));

        classify_and_extract(&mut all_passages, &config, registry_dir, &inference_port).await?;

        // ── Compute batch salience (graph centrality) ────────────────
        {
            let mut p = shared.lock().unwrap_or_else(|e| e.into_inner());
            p.phase = EmbedPhase::Tagging; // still in metadata phase
            p.current_work = "computing salience".into();
        }
        let all_tags: Vec<EntityTags> = all_passages.iter().map(|p| p.tags.clone()).collect();
        let salience_scores = salience::compute_salience_batch(&all_tags);
        for (passage, score) in all_passages.iter_mut().zip(salience_scores.iter()) {
            passage.salience = *score;
        }

        tracing::info!(
            total_passages = all_passages.len(),
            max_salience = salience_scores.iter().cloned().fold(0.0f32, f32::max),
            mean_salience =
                salience_scores.iter().sum::<f32>() / salience_scores.len().max(1) as f32,
            "Salience computed"
        );

        // ── Phase 3: Budget gate ───────────────────────────────────────
        {
            let mut p = shared.lock().unwrap_or_else(|e| e.into_inner());
            p.current_work = "applying budget gate".into();
        }
        let total_passages = all_passages.len();
        let budget = config.budget.resolve(total_passages);

        // Sort by salience descending, then determine which passages are
        // h_mem-eligible. Foundational rules always get h_mems (they
        // carry the style guide / exemplar text).
        let mut indexed: Vec<(usize, f32, usize)> = all_passages
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.salience, p.metadata_triple_count()))
            .collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut triple_eligible: HashSet<usize> = HashSet::new();
        let mut triples_allocated = 0usize;

        for (idx, _salience, triple_cost) in &indexed {
            if all_passages[*idx].is_rule {
                triple_eligible.insert(*idx);
                triples_allocated += *triple_cost;
                continue;
            }
            if triples_allocated + triple_cost <= budget {
                triple_eligible.insert(*idx);
                triples_allocated += triple_cost;
            }
        }

        let tagged_count = triple_eligible.len();
        let embedding_only = total_passages.saturating_sub(tagged_count);

        tracing::info!(
            total_passages = total_passages,
            budget = budget,
            tagged = tagged_count,
            embedding_only = embedding_only,
            triples_allocated = triples_allocated,
            "Budget gate applied"
        );

        // ── Phase 4: Embed all passages ────────────────────────────────
        tracing::info!(
            total_passages = total_passages,
            batch_size = config.embedding.batch_size,
            model = %config.embedding.model,
            "Starting embedding phase"
        );
        {
            let mut p = shared.lock().unwrap_or_else(|e| e.into_inner());
            p.phase = EmbedPhase::Embedding;
            p.current_work.clear();
            p.total_passages = total_passages;
            p.completed_passages = 0;
        }

        let batch_size = config.embedding.batch_size;
        let mut embedded_count = 0;
        let all_refs_and_texts: Vec<(&str, &str)> = all_passages
            .iter()
            .map(|p| (p.entity_ref.as_str(), p.text.as_str()))
            .collect();

        for chunk in all_refs_and_texts.chunks(batch_size) {
            let texts: Vec<String> = chunk.iter().map(|(_, text)| (*text).to_string()).collect();
            let vectors = inference_port
                .embed(&config.embedding.model, &texts)
                .await
                .map_err(|e| {
                    let msg = format!("Failed to embed batch: {e}");
                    ServiceError::Domain {
                        domain: DomainKind::Wallet,
                        kind: ErrorKind::ServiceUnavailable,
                        source: Some(Box::new(e)),
                        message: msg,
                    }
                })?;

            for ((entity_ref, _text), vector) in chunk.iter().zip(vectors.iter()) {
                semantic
                    .store_embedding(entity_ref, vector, &config.embedding.model)
                    .map_err(|e| ServiceError::Domain {
                        kind: ErrorKind::BadRequest,
                        domain: DomainKind::Memory,
                        source: None,
                        message: e.to_string(),
                    })?;
            }
            embedded_count += chunk.len();
            {
                let mut p = shared.lock().unwrap_or_else(|e| e.into_inner());
                p.completed_passages = embedded_count;
            }
            tracing::info!(
                embedded = embedded_count,
                total = total_passages,
                "Embedding progress"
            );
        }

        // ── Phase 5: Store h_mems for budget-selected passages ────────
        {
            let mut p = shared.lock().unwrap_or_else(|e| e.into_inner());
            p.phase = EmbedPhase::Triples;
            p.completed_passages = 0;
            p.total_passages = tagged_count;
        }

        let mut triples_stored = 0usize;
        let mut triple_progress = 0usize;

        for (i, passage) in all_passages.iter().enumerate() {
            if !triple_eligible.contains(&i) {
                continue;
            }

            store_passage_h_mems(&semantic, passage, &author, curator_webid)?;
            triples_stored += passage.triple_count();
            triple_progress += 1;

            {
                let mut p = shared.lock().unwrap_or_else(|e| e.into_inner());
                p.completed_passages = triple_progress;
            }
        }

        tracing::info!(
            triples_stored = triples_stored,
            tagged_passages = tagged_count,
            "Triples stored"
        );

        // ── Phase 6: Compute centroid(s) ────────────────────────────
        // ── Phase 6: Compute centroid(s) ──────────────────────────────────────────
        {
            let mut p = shared.lock().unwrap_or_else(|e| e.into_inner());
            p.phase = EmbedPhase::Centroid;
        }

        let (_single_result, dim_results, passage_count_centroid, centroid_stored) =
            compute_centroids(
                &all_passages,
                &config,
                &semantic,
                &author_prefix,
                &centroid_ref,
            )?;

        {
            let mut p = shared.lock().unwrap_or_else(|e| e.into_inner());
            p.phase = EmbedPhase::Done;
            p.completed_passages = total_passages;
        }

        Ok(EmbedResult {
            author,
            purged,
            total_passages,
            centroid_ref,
            passage_count: passage_count_centroid,
            centroid_stored,
            validation,
            budget,
            tagged_passages: tagged_count,
            triples_stored,
            embedding_only,
            dimension_centroids: dim_results,
        })
    }

    /// Parse a corpus config YAML file.
    #[must_use = "result must be used"]
    pub fn parse_config(path: &Path) -> Result<CorpusConfig, ServiceError> {
        // P9: Regulation span
        tracing::info!(target: "hkask.embed", operation = "parse_config", config = %path.display(), "REG");

        let config_str = std::fs::read_to_string(path).map_err(|e| {
            let msg = format!("Failed to read corpus config {}: {e}", path.display());
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;
        serde_yaml_neo::from_str(&config_str).map_err(|e| {
            let msg = format!("Failed to parse corpus config YAML: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })
    }
}
