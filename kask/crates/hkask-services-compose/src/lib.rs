#![forbid(unsafe_code)]
//! Style composition — exemplar retrieval, prose generation, centroid validation.

use std::path::PathBuf;
use std::sync::Arc;

use hkask_memory::SemanticMemory;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{Database, EmbeddingStore, HMemStore};
use hkask_types::InferencePort;
use hkask_types::template::LLMParameters;
use serde::Deserialize;

use tracing::debug;

use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_services_inference::InferenceContext;

// ── Cognition configuration ──────────────────────────────────────────────

/// Cognition configuration for the style composition pipeline.
///
/// Deserialized from a YAML file that specifies the embedding model,
/// retrieval parameters, and centroid validation threshold.
///
/// Example YAML:
/// ```yaml
/// embedding:
///   model: "Qwen/Qwen3-Embedding-0.6B"
///   dim: 1024
///   centroid_entity_ref: "style:hemingway:centroid"
///   retrieval:
///     k_min: 3
///     k_max: 7
///     distance_threshold: 0.50
/// validation:
///   centroid_distance_max: 0.35
/// ```
#[derive(Debug, Deserialize)]
pub struct CognitionConfig {
    /// Author identifier — used in the system prompt and centroid entity ref.
    /// When a `jinja2_template` is declared, the author is available as
    /// `{{ author }}` in the template. When no template is present, the
    /// author is used in a generic fallback prompt.
    #[serde(default = "default_author")]
    pub author: String,
    pub embedding: EmbeddingSection,
    pub validation: ValidationSection,
    /// Jinja2 template for the system prompt. When present, this is rendered
    /// with context variables (prompt, exemplars, author, rules) and used
    /// as the system prompt. When absent, falls back to the hardcoded
    /// Rust function for the author.
    #[serde(default)]
    pub jinja2_template: Option<String>,
}

fn default_author() -> String {
    "hemingway".to_string()
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingSection {
    pub model: String,
    pub dim: usize,
    pub centroid_entity_ref: String,
    #[serde(default)]
    pub retrieval: RetrievalSection,
}

#[derive(Debug, Deserialize)]
pub struct RetrievalSection {
    #[serde(default = "default_k_min")]
    pub k_min: usize,
    #[serde(default = "default_k_max")]
    pub k_max: usize,
    #[serde(default = "default_distance_threshold")]
    pub distance_threshold: f64,
    /// Salience floor: only consider passages with salience >= this value.
    #[serde(default)]
    pub salience_min: f64,
    /// Top-K by salience: only consider the K most salient matching passages.
    #[serde(default)]
    pub salience_top_k: Option<usize>,
}

impl Default for RetrievalSection {
    fn default() -> Self {
        Self {
            k_min: default_k_min(),
            k_max: default_k_max(),
            distance_threshold: default_distance_threshold(),
            salience_min: 0.0,
            salience_top_k: None,
        }
    }
}

fn default_k_min() -> usize {
    3
}
fn default_k_max() -> usize {
    7
}
fn default_distance_threshold() -> f64 {
    0.50
}

#[derive(Debug, Deserialize)]
pub struct ValidationSection {
    pub centroid_distance_max: f64,
}

// ── Request / Response types ────────────────────────────────────────────

/// Input for `ComposeService::compose()`.
pub struct ComposeRequest {
    /// The user's prompt for prose generation.
    pub prompt: String,
    /// Path to the per-agent semantic database.
    pub db_path: PathBuf,
    /// Passphrase for opening the database.
    pub db_passphrase: String,
    /// Parsed cognition configuration.
    pub cognition: CognitionConfig,
    /// Inference context for model resolution.
    pub inference_ctx: InferenceContext,
    /// Skip centroid distance validation.
    pub no_validate: bool,
}

/// Result of a style composition operation.
pub struct ComposeResult {
    /// The generated prose text.
    pub generated_prose: String,
    /// Number of exemplar passages used.
    pub exemplar_count: usize,
    /// Centroid validation result (None if validation was skipped).
    pub validation: Option<CentroidValidation>,
}

/// Centroid distance validation result.
pub struct CentroidValidation {
    /// Cosine distance between generated prose and style centroid.
    pub distance: f64,
    /// Maximum allowed distance threshold.
    pub threshold: f64,
    /// Whether the prose passes validation (distance <= threshold).
    pub passed: bool,
}

// ── Service ──────────────────────────────────────────────────────────────

/// Style composition service — exemplar retrieval, prose generation, centroid validation.
pub struct ComposeService;

impl ComposeService {
    /// Execute the full style composition pipeline.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  request.db_path must point to a valid database; request.prompt must be non-empty; request.cognition must have valid embedding config
    /// post: returns ComposeResult with generated_prose, exemplar_count, and optional CentroidValidation; Err on DB open failure, embedding failure, or inference failure
    /// # REQ: P3-svc-compose-001 — compose returns generated prose with exemplar retrieval
    /// # expect: "The service layer enables generative access to domain capabilities"
    /// # REQ: P3-svc-compose-002 — compose validates centroid distance when no_validate is false
    /// # expect: "The service layer enables generative access to domain capabilities"
    /// # REQ: P3-svc-compose-003 — compose returns validation=None when no_validate is true
    /// # expect: "The service layer enables generative access to domain capabilities"
    /// # REQ: P3-svc-compose-004 — compose uses Jinja2 template from cognition config
    /// # expect: "The service layer enables generative access to domain capabilities"
    #[must_use = "result must be used"]
    pub async fn compose(request: ComposeRequest) -> Result<ComposeResult, ServiceError> {
        // Input length validation
        if request.prompt.len() > 50_000 {
            return Err(ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::User,
                source: None,
                message: format!(
                    "Prompt exceeds 50KB limit (received {} bytes)",
                    request.prompt.len()
                ),
            });
        }

        // 1. Open DB + construct memory infrastructure
        let db = Database::open(&request.db_path.to_string_lossy(), &request.db_passphrase)
            .map_err(|e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: None,
                message: e.to_string(),
            })?;
        let pool = db.sqlite_pool().map_err(|e| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Storage,
            source: None,
            message: format!("SQLite pool creation failed: {e}"),
        })?;
        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
            Arc::new(SqliteDriver::new(pool.clone()));
        let h_mem_store = HMemStore::from_driver(Arc::clone(&driver));
        let embedding_store =
            EmbeddingStore::from_driver(Arc::clone(&driver), request.cognition.embedding.dim);
        let semantic = SemanticMemory::new(h_mem_store, embedding_store);
        let driver2 = SqliteDriver::new(pool);
        let embedding_store_direct = EmbeddingStore::from_driver(
            Arc::new(driver2) as Arc<dyn hkask_storage::database::driver::DatabaseDriver>,
            request.cognition.embedding.dim,
        );

        // 2. Create EmbeddingRouter and embed prompt
        let embedder =
            hkask_inference::EmbeddingRouter::new(request.inference_ctx.inference_config.clone());
        let prompt_vector = embedder
            .embed_sentence(&request.cognition.embedding.model, &request.prompt)
            .await?;

        // 3. KNN search for exemplar passages
        let results = semantic
            .search_similar(&prompt_vector, request.cognition.embedding.retrieval.k_max)
            .map_err(|e| ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Memory,
                source: None,
                message: e.to_string(),
            })?;

        // Debug: log top-5 distances regardless of threshold to diagnose retrieval gaps
        if !results.is_empty() {
            let top_n = results.iter().take(5.min(results.len()));
            debug!(
                "KNN search returned {} results. Top-{} distances: [{}]",
                results.len(),
                top_n.len(),
                top_n
                    .map(|r| format!("{:.4}", r.distance))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        } else {
            debug!("KNN search returned 0 results — corpus may be empty or unembedded");
        }

        // 4. Filter by prefix, centroid exclusion, rule exclusion, distance threshold
        let prefix = format!("style:{}", &request.cognition.author);
        let retrieval = &request.cognition.embedding.retrieval;
        let mut matched: Vec<(f64, String, f64)> = Vec::new(); // (distance, entity_ref, salience)

        for r in &results {
            if !r.embedding.entity_ref.starts_with(&prefix)
                || r.embedding.entity_ref == request.cognition.embedding.centroid_entity_ref
                || r.embedding.entity_ref.contains(":rule:")
                || r.distance > retrieval.distance_threshold
            {
                continue;
            }

            // Look up salience from h_mems
            let salience = match semantic.query_deduped(&r.embedding.entity_ref) {
                Ok(h_mems) => h_mems
                    .iter()
                    .find(|t| t.attribute == "salience")
                    .and_then(|t| t.value.as_f64())
                    .unwrap_or(0.0),
                _ => 0.0,
            };

            if salience < retrieval.salience_min {
                continue;
            }

            matched.push((r.distance, r.embedding.entity_ref.clone(), salience));
        }

        debug!(
            "Filtered: {} of {} results passed prefix/distance/salience gates (threshold={})",
            matched.len(),
            results.len(),
            retrieval.distance_threshold
        );

        // Sort by salience descending if salience_top_k is set, then take top K
        if let Some(top_k) = retrieval.salience_top_k {
            matched.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
            matched.truncate(top_k);
        }

        // Extract passage text from h_mems.
        //
        // NOTE: Passages in the embedding-only set (not budget-selected for h_mems)
        // will lack a `text` h_mem and fall back to placeholder strings like
        // "[work_title: entity_ref]". These are useless as style exemplars.
        //
        // Two resolution paths (architectural decision pending):
        //   (a) Lower the budget threshold so more passages earn h_mem storage.
        //   (b) Store `text` h_mems for ALL passages regardless of budget, and
        //       only gate the richer metadata (entity tags, method signals, salience)
        //       on the budget. The `text` h_mem is ~150 words — tiny compared to
        //       the 11 method signals and entity tags.
        let exemplar_passages: Vec<String> = matched
            .into_iter()
            .take(retrieval.k_max)
            .filter_map(|(_distance, entity_ref, _salience)| {
                match semantic.query_deduped(&entity_ref) {
                    Ok(h_mems) => {
                        let text = h_mems
                            .iter()
                            .find(|t| t.attribute == "text")
                            .and_then(|t| t.value.as_str().map(|s| s.to_string()));
                        text.or_else(|| {
                            let work = h_mems
                                .iter()
                                .find(|t| t.attribute == "work_title")
                                .and_then(|t| t.value.as_str());
                            work.map(|w| {
                                format!("[{}: {} — passage text not in h_mems]", w, entity_ref)
                            })
                        })
                    }
                    _ => Some(format!("[passage: {}]", entity_ref)),
                }
            })
            .collect();

        let exemplar_count = exemplar_passages.len();

        // 5. Compose system prompt — Jinja2 template if declared, else generic fallback
        let system_prompt = if let Some(ref template) = request.cognition.jinja2_template {
            render_jinja2_prompt(
                template,
                &request.cognition.author,
                &request.prompt,
                &exemplar_passages,
                request.no_validate,
                request.cognition.validation.centroid_distance_max,
            )?
        } else {
            generic_system_prompt(
                &request.cognition.author,
                &request.prompt,
                &exemplar_passages,
                request.no_validate,
                request.cognition.validation.centroid_distance_max,
            )
        };

        // 6. Generate prose — model comes from InferenceContext (operational concern),
        // not from CognitionConfig (pipeline/corpus concern). The embedding model
        // is tied to stored vector dimensions; the generation model is deployment-specific.
        let gen_model = request.inference_ctx.default_model.clone();
        let inference = hkask_services_inference::InferenceService::resolve_port(
            &request.inference_ctx,
            &gen_model,
        )?;
        let params = LLMParameters {
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            min_p: 0.0,
            typical_p: 0.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            max_tokens: 512,
            seed: None,
            disable_thinking: false,
            adapter: None,
            bypass_fusion: false,
            fusion_config: None,
            system_prompt: None,
        };
        let result = inference.generate(&system_prompt, &params, None).await?;
        let generated_prose = result.text.trim().to_string();

        // 7. Validate centroid distance (optional)
        let validation = if request.no_validate {
            None
        } else {
            let prose_vector = embedder
                .embed_sentence(&request.cognition.embedding.model, &generated_prose)
                .await?;
            match embedding_store_direct.get(&request.cognition.embedding.centroid_entity_ref) {
                Ok(centroid_embedding) => {
                    let distance = cosine_distance(&prose_vector, &centroid_embedding.vector);
                    let threshold = request.cognition.validation.centroid_distance_max;
                    Some(CentroidValidation {
                        distance,
                        threshold,
                        passed: distance <= threshold,
                    })
                }
                Err(_) => None,
            }
        };

        Ok(ComposeResult {
            generated_prose,
            exemplar_count,
            validation,
        })
    }
}

// ── Prompt composition ───────────────────────────────────────────────────

/// Render a Jinja2 system prompt template with context variables.
///
/// Template variables:
/// - `{{ prompt }}` — user's input text
/// - `{{ author }}` — author identifier
/// - `{{ exemplars }}` — list of retrieved passage strings
/// - `{{ exemplar_count }}` — number of exemplars retrieved
/// - `{{ no_validate }}` — whether validation is skipped
/// - `{{ centroid_distance_max }}` — validation threshold
fn render_jinja2_prompt(
    template: &str,
    author: &str,
    prompt: &str,
    exemplars: &[String],
    no_validate: bool,
    centroid_distance_max: f64,
) -> Result<String, ServiceError> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.add_template("system_prompt", template).map_err(|e| {
        let msg = format!("Jinja2 template parse error: {e}");
        ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Wallet,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;
    let tmpl = env.get_template("system_prompt").map_err(|e| {
        let msg = format!("Jinja2 template lookup error: {e}");
        ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Wallet,
            source: Some(Box::new(e)),
            message: msg,
        }
    })?;

    let ctx = minijinja::context! {
        prompt,
        author,
        exemplars,
        exemplar_count => exemplars.len(),
        no_validate,
        centroid_distance_max,
    };

    tmpl.render(&ctx).map_err(|e| {
        let msg = format!("Jinja2 render error: {e}");
        ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Wallet,
            source: Some(Box::new(e)),
            message: msg,
        }
    })
}

/// Generic fallback system prompt — used when no Jinja2 template is declared.
/// Concatenates exemplar passages with a simple instruction to match the author's style.
fn generic_system_prompt(
    author: &str,
    prompt: &str,
    exemplar_passages: &[String],
    no_validate: bool,
    centroid_distance_max: f64,
) -> String {
    let mut parts = vec![format!(
        "You are an expert prose stylist writing in the authentic style of {author}."
    )];

    if !exemplar_passages.is_empty() {
        parts.push(
            "\n## Exemplar Passages\nThe following passages exemplify the target style. \
             Use them as reference for rhythm, syntax, and cadence — not as content to imitate.\n"
                .to_string(),
        );
        for passage in exemplar_passages {
            parts.push(format!("---\n{passage}\n---\n"));
        }
    }

    if !no_validate {
        parts.push(format!(
            "\n## Centroid Validation\nYour output will be embedded and compared against the style centroid.\n\
             Centroid distance threshold: {centroid_distance_max}\n\
             If the distance exceeds {centroid_distance_max}, the output will be rejected.\n"
        ));
    }

    parts.push(format!("\n## Task\n{prompt}"));
    parts.join("")
}

// ── Utility ─────────────────────────────────────────────────────────────

/// Compute cosine distance between two vectors.
/// Returns 0.0 for identical vectors, 2.0 for opposite vectors.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  a and b must be non-empty f32 slices of equal length; mismatched or empty returns 2.0
/// post: returns f64 in range [0.0, 2.0]; 0.0 = identical, 1.0 = orthogonal, 2.0 = opposite or degenerate
#[must_use]
pub fn cosine_distance(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 2.0;
    }
    let dot: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as f64) * (*y as f64))
        .sum();
    let norm_a: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 2.0;
    }
    let similarity = dot / (norm_a * norm_b);
    1.0 - similarity
}
