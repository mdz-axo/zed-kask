//! Deterministic chunk-retrieval calibration representations.

use crate::{CorpusServer, McpToolError, Parameters, execute_tool, tool, tool_router};
use hkask_memory::text_chunking::{
    ChunkConfig, TextChunk, chunk_text_with_config, filter_boilerplate_pages_with_report,
    sanitize_text,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const REFERENCE_MIN_WORDS: usize = 50;
const REFERENCE_MAX_WORDS: usize = 100;
const REFERENCE_SENTENCE_BOUNDARY: &str = ".!?";

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct AcceptedSource {
    pub source: String,
    pub raw_path: String,
    pub raw_sha256: String,
    pub canonical_path: String,
    pub canonical_sha256: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ChunkPolicyParams {
    pub min_words: usize,
    pub max_words: usize,
    pub overlap_words: usize,
    pub sentence_boundary: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BuildChunkRepresentationsRequest {
    pub accepted_sources: Vec<AcceptedSource>,
    pub output_dir: String,
    pub entity_ref_prefix: String,
    pub current_policy: ChunkPolicyParams,
    pub fine_policy: ChunkPolicyParams,
    pub parent_policy: ChunkPolicyParams,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmbeddingInventoryRequest {
    /// Exact shard JSONL whose entity references must be reconciled.
    pub chunks_jsonl: String,
    /// Existing SQLCipher embedding database. Inventory never creates a database.
    pub db_path: String,
    /// Provider-confirmed model identity required for every durable row.
    pub expected_model: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Provenance {
    raw_path: String,
    raw_sha256: String,
    canonical_path: String,
    canonical_sha256: String,
    canonical_normalization: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RepresentationRow {
    entity_ref: String,
    source: String,
    text: String,
    word_count: usize,
    provenance: Provenance,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ChildParentRow {
    child_ref: String,
    source: String,
    parent_refs: Vec<String>,
    provenance: Provenance,
}

#[derive(Clone, Debug, Serialize)]
struct Artifact {
    path: String,
    sha256: String,
    rows: usize,
}

#[derive(Debug, Serialize)]
struct PolicyManifest {
    engine: &'static str,
    word_unit: &'static str,
    boundary_preference: &'static str,
    final_remainder: &'static str,
    min_words: usize,
    max_words: usize,
    overlap_words: usize,
    sentence_boundary: String,
}

#[derive(Debug, Serialize)]
struct SourceFilterExclusion {
    reason: String,
    boundary_unit: String,
    start: usize,
    end: usize,
    removed_words: usize,
}

#[derive(Debug, Serialize)]
struct SourceFilterReport {
    input_words: usize,
    retained_words: usize,
    exclusions: Vec<SourceFilterExclusion>,
}

#[derive(Debug, Serialize)]
struct RepresentationManifest {
    schema_version: u32,
    entity_ref_prefix: String,
    accepted_sources: Vec<AcceptedSource>,
    boilerplate_exclusion_reports: BTreeMap<String, SourceFilterReport>,
    policies: BTreeMap<&'static str, PolicyManifest>,
    artifacts: BTreeMap<&'static str, Artifact>,
    validation: ValidationReport,
}

#[derive(Debug, Serialize)]
struct ValidationReport {
    accepted_source_count: usize,
    boilerplate_filter_applied: bool,
    unique_entity_refs: bool,
    normalized_source_reconstruction: bool,
    every_child_mapped: bool,
    every_parent_exists: bool,
    child_map_parent_sources_agree: bool,
}

#[derive(Clone, Copy, Debug)]
struct WordSpan {
    start: usize,
    end: usize,
}

struct SourceRepresentations {
    reference: Vec<RepresentationRow>,
    current: Vec<RepresentationRow>,
    children: Vec<RepresentationRow>,
    parents: Vec<RepresentationRow>,
    child_parent_map: Vec<ChildParentRow>,
}

/// expect: "I can compare chunk policies over the same furniture-filtered accepted source view without losing or relabeling retained text."
/// [P3] Motivating: one deterministic construction publishes comparable retrieval policies.
/// [P1] Constraining: accepted source identity and provenance remain explicit.
/// [P4] Constraining: incomplete coverage, reconstruction, or hierarchy fails before publication.
/// pre: source hashes and complete word-window policies are caller supplied.
/// post: one atomic manifest names validated source exclusions plus reference, current, child, parent, and map artifacts.
#[tool_router(router = calibration_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Build one validated calibration manifest over the same canonical boilerplate-filtered source view used by production chunking. It contains source-level exclusion reports; the fixed greedy sentence-bounded 100-word/no-overlap reference with a final sub-50-word remainder merged backward; the current hkask-memory shared chunk contract with complete policy parameters; and deterministic fine children, source-faithful parents, and a complete child-to-parent map. Every row carries entity_ref, source, text, word_count, and raw/canonical provenance. Fails before publication on empty retained sources, source omission, duplicate IDs, reconstruction failure, incomplete maps, missing parents, or source disagreement."
    )]
    pub async fn corpus_build_chunk_representations(
        &self,
        Parameters(request): Parameters<BuildChunkRepresentationsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_build_chunk_representations", async move {
            build_representations(request)
        })
        .await
    }

    /// expect: "An interrupted calibration can retry only refs not durably stored under its confirmed model."
    /// [P4] Motivating: recovery is derived from durable state rather than response-file existence.
    /// [P1] Constraining: inventory is bounded to caller-supplied shard identities.
    /// pre: chunks_jsonl and an existing database identify one calibration shard.
    /// post: returns exact sorted missing, model-mismatched, and retry entity-reference sets without writing.
    #[tool(
        description = "Reconcile one calibration shard against an existing durable embedding database. Returns exact sorted missing refs, refs stored under a different model, their union as retry_entity_refs, and a complete flag. The expected_model must be the provider-confirmed actual identity; the tool never embeds or creates a database."
    )]
    pub async fn corpus_embedding_inventory(
        &self,
        Parameters(request): Parameters<EmbeddingInventoryRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_embedding_inventory", async move {
            embedding_inventory(request)
        })
        .await
    }
}

fn embedding_inventory(
    request: EmbeddingInventoryRequest,
) -> Result<serde_json::Value, McpToolError> {
    if request.expected_model.trim().is_empty() {
        return Err(McpToolError::invalid_argument(
            "expected_model must be non-empty",
        ));
    }
    if !Path::new(&request.db_path).is_file() {
        return Err(McpToolError::invalid_argument(format!(
            "embedding database does not exist: {}",
            request.db_path
        )));
    }

    // The DB passphrase resolves server-side only (fail-closed; never an
    // empty-key open).
    let passphrase = crate::helpers::resolve_corpus_passphrase()?;

    let rows = crate::read_jsonl::<serde_json::Value>(&request.chunks_jsonl, "chunks_jsonl")?;
    let mut requested = BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        let entity_ref = row
            .get("entity_ref")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                McpToolError::invalid_argument(format!(
                    "chunks_jsonl line {} has no non-empty entity_ref",
                    index + 1
                ))
            })?;
        if !requested.insert(entity_ref.to_string()) {
            return Err(McpToolError::invalid_argument(format!(
                "chunks_jsonl contains duplicate entity_ref: {entity_ref}"
            )));
        }
    }
    if requested.is_empty() {
        return Err(McpToolError::invalid_argument(
            "chunks_jsonl contains no entity refs",
        ));
    }

    let requested_refs = requested.iter().cloned().collect::<Vec<_>>();
    let store =
        hkask_memory::MemoryStore::open(&request.db_path, &passphrase, crate::embedding_dim())
            .map_err(|error| {
                McpToolError::internal(format!(
                    "cannot open embedding database {}: {error}",
                    request.db_path
                ))
            })?;
    let stored = store
        .embedding_models_for_refs(&requested_refs)
        .map_err(|error| McpToolError::internal(format!("cannot inventory embeddings: {error}")))?;

    let mut stored_by_ref = BTreeMap::<String, Vec<String>>::new();
    for (entity_ref, model) in stored {
        stored_by_ref.entry(entity_ref).or_default().push(model);
    }
    if let Some((entity_ref, models)) = stored_by_ref.iter().find(|(_, models)| models.len() != 1) {
        return Err(McpToolError::failed_precondition(format!(
            "embedding database contains {} durable rows for entity_ref {entity_ref}",
            models.len()
        )));
    }

    let mut missing_entity_refs = Vec::new();
    let mut mismatched_model_entity_refs = Vec::new();
    let mut retry_entity_refs = Vec::new();
    let mut stored_matching_model = 0usize;
    for entity_ref in &requested_refs {
        match stored_by_ref.get(entity_ref) {
            None => {
                missing_entity_refs.push(entity_ref.clone());
                retry_entity_refs.push(entity_ref.clone());
            }
            Some(models) if models[0] == request.expected_model => {
                stored_matching_model += 1;
            }
            Some(models) => {
                mismatched_model_entity_refs.push(serde_json::json!({
                    "entity_ref": entity_ref,
                    "stored_model": models[0],
                }));
                retry_entity_refs.push(entity_ref.clone());
            }
        }
    }

    let complete = retry_entity_refs.is_empty();
    Ok(serde_json::json!({
        "requested": requested_refs.len(),
        "stored_matching_model": stored_matching_model,
        "missing_entity_refs": missing_entity_refs,
        "mismatched_model_entity_refs": mismatched_model_entity_refs,
        "retry_entity_refs": retry_entity_refs,
        "complete": complete,
        "expected_model": request.expected_model,
    }))
}

fn build_representations(
    request: BuildChunkRepresentationsRequest,
) -> Result<serde_json::Value, McpToolError> {
    validate_request(&request)?;
    let output_dir = PathBuf::from(&request.output_dir);
    if output_dir.exists() {
        return Err(McpToolError::invalid_argument(format!(
            "output_dir already exists: {}",
            output_dir.display()
        )));
    }
    let parent = output_dir
        .parent()
        .ok_or_else(|| McpToolError::invalid_argument("output_dir must have a parent directory"))?;
    fs::create_dir_all(parent).map_err(|error| {
        McpToolError::internal(format!(
            "cannot create output parent {}: {error}",
            parent.display()
        ))
    })?;
    let temporary = tempfile::Builder::new()
        .prefix("chunk-representations-")
        .tempdir_in(parent)
        .map_err(|error| {
            McpToolError::internal(format!("cannot create temporary output: {error}"))
        })?;

    let mut reference = Vec::new();
    let mut current = Vec::new();
    let mut children = Vec::new();
    let mut parents = Vec::new();
    let mut child_parent_map = Vec::new();
    let mut boilerplate_exclusion_reports = BTreeMap::new();
    let mut reconstructed = true;

    for accepted in &request.accepted_sources {
        let source = load_source(accepted)?;
        let (source, filter_report) = filter_source(accepted, &source)?;
        boilerplate_exclusion_reports.insert(accepted.source.clone(), filter_report);
        let built = build_source(&request, accepted, &source)?;
        reconstructed &= reconstructs(&built.reference, 0, &source)
            && reconstructs(
                &built.current,
                request.current_policy.overlap_words,
                &source,
            )
            && reconstructs(&built.children, request.fine_policy.overlap_words, &source)
            && reconstructs(&built.parents, request.parent_policy.overlap_words, &source);
        reference.extend(built.reference);
        current.extend(built.current);
        children.extend(built.children);
        parents.extend(built.parents);
        child_parent_map.extend(built.child_parent_map);
    }

    let accepted_names: BTreeSet<_> = request
        .accepted_sources
        .iter()
        .map(|source| source.source.clone())
        .collect();
    for (label, rows) in [
        ("reference", &reference),
        ("current", &current),
        ("fine children", &children),
        ("parents", &parents),
    ] {
        let represented: BTreeSet<_> = rows.iter().map(|row| row.source.clone()).collect();
        if represented != accepted_names {
            return Err(McpToolError::failed_precondition(format!(
                "{label} does not represent every accepted source"
            )));
        }
    }
    if !reconstructed {
        return Err(McpToolError::failed_precondition(
            "normalized source reconstruction failed",
        ));
    }

    let all_refs: Vec<_> = reference
        .iter()
        .chain(&current)
        .chain(&children)
        .chain(&parents)
        .map(|row| row.entity_ref.as_str())
        .collect();
    let unique_refs: BTreeSet<_> = all_refs.iter().copied().collect();
    if unique_refs.len() != all_refs.len() {
        return Err(McpToolError::failed_precondition(
            "representation entity_ref values are not globally unique",
        ));
    }

    let children_by_ref: BTreeMap<_, _> = children
        .iter()
        .map(|row| (row.entity_ref.as_str(), row.source.as_str()))
        .collect();
    let parents_by_ref: BTreeMap<_, _> = parents
        .iter()
        .map(|row| (row.entity_ref.as_str(), row.source.as_str()))
        .collect();
    if child_parent_map.len() != children.len()
        || child_parent_map
            .iter()
            .any(|mapping| !children_by_ref.contains_key(mapping.child_ref.as_str()))
    {
        return Err(McpToolError::failed_precondition(
            "every fine child must have exactly one child-parent map row",
        ));
    }
    for mapping in &child_parent_map {
        let child_source = children_by_ref
            .get(mapping.child_ref.as_str())
            .ok_or_else(|| {
                McpToolError::failed_precondition("child-parent map references an unknown child")
            })?;
        if mapping.parent_refs.is_empty() || *child_source != mapping.source {
            return Err(McpToolError::failed_precondition(
                "child and map sources disagree or a child has no parent",
            ));
        }
        for parent_ref in &mapping.parent_refs {
            let parent_source = parents_by_ref.get(parent_ref.as_str()).ok_or_else(|| {
                McpToolError::failed_precondition(format!(
                    "child-parent map references missing parent {parent_ref}"
                ))
            })?;
            if *parent_source != mapping.source {
                return Err(McpToolError::failed_precondition(
                    "child, map, and parent sources disagree",
                ));
            }
        }
    }

    let reference_artifact = write_jsonl(temporary.path(), "reference.jsonl", &reference)?;
    let current_artifact = write_jsonl(temporary.path(), "current.jsonl", &current)?;
    let children_artifact = write_jsonl(temporary.path(), "fine-children.jsonl", &children)?;
    let parents_artifact = write_jsonl(temporary.path(), "parents.jsonl", &parents)?;
    let map_artifact = write_jsonl(
        temporary.path(),
        "child-parent-map.jsonl",
        &child_parent_map,
    )?;

    let mut policies = BTreeMap::new();
    policies.insert(
        "reference",
        policy_manifest(
            REFERENCE_MIN_WORDS,
            REFERENCE_MAX_WORDS,
            0,
            REFERENCE_SENTENCE_BOUNDARY,
            "merge_backward_below_50_words",
        ),
    );
    policies.insert("current", shared_policy_manifest(&request.current_policy));
    policies.insert("fine", shared_policy_manifest(&request.fine_policy));
    policies.insert("parent", shared_policy_manifest(&request.parent_policy));
    let mut artifacts = BTreeMap::new();
    artifacts.insert("reference", reference_artifact);
    artifacts.insert("current", current_artifact);
    artifacts.insert("fine_children", children_artifact);
    artifacts.insert("parents", parents_artifact);
    artifacts.insert("child_parent_map", map_artifact);
    let manifest = RepresentationManifest {
        schema_version: 2,
        entity_ref_prefix: request.entity_ref_prefix,
        accepted_sources: request.accepted_sources,
        boilerplate_exclusion_reports,
        policies,
        artifacts,
        validation: ValidationReport {
            accepted_source_count: accepted_names.len(),
            boilerplate_filter_applied: true,
            unique_entity_refs: true,
            normalized_source_reconstruction: true,
            every_child_mapped: true,
            every_parent_exists: true,
            child_map_parent_sources_agree: true,
        },
    };
    let manifest_path = temporary.path().join("manifest.json");
    let mut file = fs::File::create(&manifest_path).map_err(|error| {
        McpToolError::internal(format!(
            "cannot create {}: {error}",
            manifest_path.display()
        ))
    })?;
    serde_json::to_writer_pretty(&mut file, &manifest)
        .map_err(|error| McpToolError::internal(format!("cannot serialize manifest: {error}")))?;
    file.write_all(b"\n")
        .map_err(|error| McpToolError::internal(format!("cannot finish manifest: {error}")))?;

    let temporary_path = temporary.path().to_path_buf();
    fs::rename(&temporary_path, &output_dir).map_err(|error| {
        McpToolError::internal(format!(
            "cannot publish {} as {}: {error}",
            temporary_path.display(),
            output_dir.display()
        ))
    })?;
    Ok(serde_json::json!({
        "manifest": output_dir.join("manifest.json"),
        "output_dir": output_dir,
        "accepted_sources": accepted_names.len(),
        "reference_rows": reference.len(),
        "current_rows": current.len(),
        "fine_child_rows": children.len(),
        "parent_rows": parents.len(),
        "map_rows": child_parent_map.len()
    }))
}

fn validate_request(request: &BuildChunkRepresentationsRequest) -> Result<(), McpToolError> {
    if request.accepted_sources.is_empty() {
        return Err(McpToolError::invalid_argument(
            "accepted_sources must not be empty",
        ));
    }
    if request.entity_ref_prefix.trim().is_empty() {
        return Err(McpToolError::invalid_argument(
            "entity_ref_prefix must not be blank",
        ));
    }
    let mut names = BTreeSet::new();
    for source in &request.accepted_sources {
        if source.source.trim().is_empty() || !names.insert(source.source.as_str()) {
            return Err(McpToolError::invalid_argument(
                "accepted source names must be nonblank and unique",
            ));
        }
        for (label, digest) in [
            ("raw_sha256", source.raw_sha256.as_str()),
            ("canonical_sha256", source.canonical_sha256.as_str()),
        ] {
            if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(McpToolError::invalid_argument(format!(
                    "{} has invalid {label}",
                    source.source
                )));
            }
        }
    }
    for (label, policy) in [
        ("current_policy", &request.current_policy),
        ("fine_policy", &request.fine_policy),
        ("parent_policy", &request.parent_policy),
    ] {
        ChunkConfig::new(
            policy.min_words,
            policy.max_words,
            policy.overlap_words,
            &policy.sentence_boundary,
        )
        .map_err(|error| McpToolError::invalid_argument(format!("invalid {label}: {error}")))?;
        if policy.sentence_boundary.trim().is_empty() {
            return Err(McpToolError::invalid_argument(format!(
                "{label}.sentence_boundary must not be blank"
            )));
        }
    }
    if request.fine_policy.max_words >= request.parent_policy.max_words {
        return Err(McpToolError::invalid_argument(
            "fine_policy.max_words must be smaller than parent_policy.max_words",
        ));
    }
    Ok(())
}

fn load_source(accepted: &AcceptedSource) -> Result<String, McpToolError> {
    let raw = fs::read(&accepted.raw_path).map_err(|error| {
        McpToolError::invalid_argument(format!(
            "cannot read raw source {}: {error}",
            accepted.raw_path
        ))
    })?;
    if sha256_bytes(&raw) != accepted.raw_sha256.to_ascii_lowercase() {
        return Err(McpToolError::failed_precondition(format!(
            "raw source hash changed for {}",
            accepted.source
        )));
    }
    let canonical = fs::read(&accepted.canonical_path).map_err(|error| {
        McpToolError::invalid_argument(format!(
            "cannot read canonical source {}: {error}",
            accepted.canonical_path
        ))
    })?;
    if sha256_bytes(&canonical) != accepted.canonical_sha256.to_ascii_lowercase() {
        return Err(McpToolError::failed_precondition(format!(
            "canonical source hash changed for {}",
            accepted.source
        )));
    }
    let text = String::from_utf8(canonical).map_err(|error| {
        McpToolError::invalid_argument(format!(
            "canonical source {} is not UTF-8: {error}",
            accepted.canonical_path
        ))
    })?;
    if normalize_words(&text).is_empty() {
        return Err(McpToolError::failed_precondition(format!(
            "canonical source {} has no words",
            accepted.source
        )));
    }
    Ok(text)
}

fn filter_source(
    accepted: &AcceptedSource,
    source: &str,
) -> Result<(String, SourceFilterReport), McpToolError> {
    let filtered = filter_boilerplate_pages_with_report(source);
    if normalize_words(&filtered.text).is_empty() {
        return Err(McpToolError::failed_precondition(format!(
            "canonical source {} has no retained words after boilerplate filtering",
            accepted.source
        )));
    }
    let report = SourceFilterReport {
        input_words: filtered.input_words,
        retained_words: filtered.retained_words,
        exclusions: filtered
            .exclusions
            .into_iter()
            .map(|exclusion| SourceFilterExclusion {
                reason: exclusion.reason.to_string(),
                boundary_unit: exclusion.boundary_unit.to_string(),
                start: exclusion.start,
                end: exclusion.end,
                removed_words: exclusion.removed_words,
            })
            .collect(),
    };
    Ok((filtered.text, report))
}

fn build_source(
    request: &BuildChunkRepresentationsRequest,
    accepted: &AcceptedSource,
    source_text: &str,
) -> Result<SourceRepresentations, McpToolError> {
    let encoded_source = encode_source(&accepted.source);
    let provenance = provenance(accepted);
    let mut reference_chunks = shared_chunks(
        source_text,
        &format!("{}:reference:{encoded_source}", request.entity_ref_prefix),
        REFERENCE_MIN_WORDS,
        REFERENCE_MAX_WORDS,
        0,
        REFERENCE_SENTENCE_BOUNDARY,
    )?;
    merge_reference_tail(&mut reference_chunks);
    let reference = rows(accepted, &provenance, reference_chunks);
    let current = rows(
        accepted,
        &provenance,
        policy_chunks(
            source_text,
            &format!("{}:current:{encoded_source}", request.entity_ref_prefix),
            &request.current_policy,
        )?,
    );
    let child_chunks = policy_chunks(
        source_text,
        &format!("{}:fine:{encoded_source}", request.entity_ref_prefix),
        &request.fine_policy,
    )?;
    let parent_chunks = policy_chunks(
        source_text,
        &format!("{}:parent:{encoded_source}", request.entity_ref_prefix),
        &request.parent_policy,
    )?;
    let child_spans = spans(&child_chunks, request.fine_policy.overlap_words)?;
    let parent_spans = spans(&parent_chunks, request.parent_policy.overlap_words)?;
    let children = rows(accepted, &provenance, child_chunks);
    let parents = rows(accepted, &provenance, parent_chunks);
    let mut child_parent_map = Vec::with_capacity(children.len());
    for (child, child_span) in children.iter().zip(child_spans) {
        let parent_refs: Vec<_> = parents
            .iter()
            .zip(&parent_spans)
            .filter(|(_, parent_span)| {
                child_span.start < parent_span.end && parent_span.start < child_span.end
            })
            .map(|(parent, _)| parent.entity_ref.clone())
            .collect();
        if parent_refs.is_empty() {
            return Err(McpToolError::failed_precondition(format!(
                "fine child {} has no source-overlapping parent",
                child.entity_ref
            )));
        }
        child_parent_map.push(ChildParentRow {
            child_ref: child.entity_ref.clone(),
            source: accepted.source.clone(),
            parent_refs,
            provenance: provenance.clone(),
        });
    }
    Ok(SourceRepresentations {
        reference,
        current,
        children,
        parents,
        child_parent_map,
    })
}

fn policy_chunks(
    text: &str,
    prefix: &str,
    policy: &ChunkPolicyParams,
) -> Result<Vec<TextChunk>, McpToolError> {
    shared_chunks(
        text,
        prefix,
        policy.min_words,
        policy.max_words,
        policy.overlap_words,
        &policy.sentence_boundary,
    )
}

fn shared_chunks(
    text: &str,
    prefix: &str,
    min_words: usize,
    max_words: usize,
    overlap_words: usize,
    sentence_boundary: &str,
) -> Result<Vec<TextChunk>, McpToolError> {
    let config = ChunkConfig::new(min_words, max_words, overlap_words, sentence_boundary)
        .map_err(|error| McpToolError::invalid_argument(error.to_string()))?;
    Ok(chunk_text_with_config(text, prefix, config).chunks)
}

fn merge_reference_tail(chunks: &mut Vec<TextChunk>) {
    let should_merge = chunks.len() > 1
        && chunks
            .last()
            .is_some_and(|chunk| word_count(&chunk.text) < REFERENCE_MIN_WORDS);
    if !should_merge {
        return;
    }
    if let Some(last) = chunks.pop() {
        if let Some(previous) = chunks.last_mut() {
            previous.text.push(' ');
            previous.text.push_str(&last.text);
        }
    }
}

fn rows(
    accepted: &AcceptedSource,
    provenance: &Provenance,
    chunks: Vec<TextChunk>,
) -> Vec<RepresentationRow> {
    chunks
        .into_iter()
        .map(|chunk| RepresentationRow {
            entity_ref: chunk.entity_ref,
            source: accepted.source.clone(),
            word_count: word_count(&chunk.text),
            text: chunk.text,
            provenance: provenance.clone(),
        })
        .collect()
}

fn spans(chunks: &[TextChunk], overlap_words: usize) -> Result<Vec<WordSpan>, McpToolError> {
    let mut result = Vec::with_capacity(chunks.len());
    let mut previous_end = 0usize;
    for (index, chunk) in chunks.iter().enumerate() {
        let start = if index == 0 {
            0
        } else {
            previous_end.checked_sub(overlap_words).ok_or_else(|| {
                McpToolError::failed_precondition("chunk overlap exceeds previous passage")
            })?
        };
        let end = start.saturating_add(word_count(&chunk.text));
        result.push(WordSpan { start, end });
        previous_end = end;
    }
    Ok(result)
}

fn reconstructs(rows: &[RepresentationRow], overlap_words: usize, source: &str) -> bool {
    let mut reconstructed: Vec<&str> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let words: Vec<_> = row.text.split_whitespace().collect();
        if index == 0 {
            reconstructed.extend(words);
            continue;
        }
        if words.len() < overlap_words || reconstructed.len() < overlap_words {
            return false;
        }
        if reconstructed[reconstructed.len() - overlap_words..] != words[..overlap_words] {
            return false;
        }
        reconstructed.extend(&words[overlap_words..]);
    }
    reconstructed.join(" ") == normalize_words(source)
}

fn policy_manifest(
    min_words: usize,
    max_words: usize,
    overlap_words: usize,
    sentence_boundary: &str,
    final_remainder: &'static str,
) -> PolicyManifest {
    PolicyManifest {
        engine: "hkask_memory::text_chunking::chunk_text_with_config",
        word_unit: "unicode_whitespace_delimited",
        boundary_preference: "structural_then_sentence_then_hard_max",
        final_remainder,
        min_words,
        max_words,
        overlap_words,
        sentence_boundary: sentence_boundary.to_string(),
    }
}

fn shared_policy_manifest(policy: &ChunkPolicyParams) -> PolicyManifest {
    policy_manifest(
        policy.min_words,
        policy.max_words,
        policy.overlap_words,
        &policy.sentence_boundary,
        "retain",
    )
}

fn write_jsonl<T: Serialize>(
    directory: &Path,
    filename: &str,
    rows: &[T],
) -> Result<Artifact, McpToolError> {
    let path = directory.join(filename);
    let mut file = fs::File::create(&path).map_err(|error| {
        McpToolError::internal(format!("cannot create {}: {error}", path.display()))
    })?;
    for row in rows {
        serde_json::to_writer(&mut file, row).map_err(|error| {
            McpToolError::internal(format!("cannot serialize {filename}: {error}"))
        })?;
        file.write_all(b"\n")
            .map_err(|error| McpToolError::internal(format!("cannot write {filename}: {error}")))?;
    }
    file.flush()
        .map_err(|error| McpToolError::internal(format!("cannot flush {filename}: {error}")))?;
    let bytes = fs::read(&path).map_err(|error| {
        McpToolError::internal(format!("cannot hash {}: {error}", path.display()))
    })?;
    Ok(Artifact {
        path: filename.to_string(),
        sha256: sha256_bytes(&bytes),
        rows: rows.len(),
    })
}

fn provenance(source: &AcceptedSource) -> Provenance {
    Provenance {
        raw_path: source.raw_path.clone(),
        raw_sha256: source.raw_sha256.to_ascii_lowercase(),
        canonical_path: source.canonical_path.clone(),
        canonical_sha256: source.canonical_sha256.to_ascii_lowercase(),
        canonical_normalization:
            "filter_boilerplate_then_sanitize_c0_split_whitespace_join_single_space".into(),
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn normalize_words(text: &str) -> String {
    sanitize_text(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

fn encode_source(source: &str) -> String {
    let mut encoded = String::from("utf8-");
    for byte in source.as_bytes() {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::template::LLMParameters;
    use hkask_types::{ChatToolDefinition, InferenceError, InferencePort, InferenceResult};
    use rmcp::handler::server::wrapper::Parameters;
    use std::{future::Future, pin::Pin, sync::Arc};

    struct NoInference;
    impl InferencePort for NoInference {
        fn generate(
            &self,
            _: &str,
            _: &LLMParameters,
            _: Option<&[ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            panic!("representation construction must not invoke inference")
        }
    }

    fn server() -> CorpusServer {
        crate::helpers::seed_test_passphrase();
        let port: Arc<dyn InferencePort> = Arc::new(NoInference);
        let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(Arc::clone(&port)));
        CorpusServer::new(
            hkask_types::WebID::new(),
            None,
            port,
            Default::default(),
            ocr,
        )
    }

    fn fixture() -> anyhow::Result<(tempfile::TempDir, BuildChunkRepresentationsRequest)> {
        let root = std::env::current_dir()?.join("target/chunk-calibration-test");
        fs::create_dir_all(&root)?;
        let directory = tempfile::tempdir_in(root)?;
        let canonical = directory.path().join("source-a.txt");
        let text = (1..=230)
            .map(|index| {
                if index % 20 == 0 {
                    format!("word{index}.")
                } else {
                    format!("word{index}")
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        fs::write(&canonical, text)?;
        let digest = sha256_bytes(&fs::read(&canonical)?);
        let request = BuildChunkRepresentationsRequest {
            accepted_sources: vec![AcceptedSource {
                source: "source-a.txt".into(),
                raw_path: canonical.to_string_lossy().into_owned(),
                raw_sha256: digest.clone(),
                canonical_path: canonical.to_string_lossy().into_owned(),
                canonical_sha256: digest,
            }],
            output_dir: directory
                .path()
                .join("representations")
                .to_string_lossy()
                .into_owned(),
            entity_ref_prefix: "calibration:test".into(),
            current_policy: ChunkPolicyParams {
                min_words: 40,
                max_words: 80,
                overlap_words: 10,
                sentence_boundary: ".!?".into(),
            },
            fine_policy: ChunkPolicyParams {
                min_words: 15,
                max_words: 30,
                overlap_words: 0,
                sentence_boundary: ".!?".into(),
            },
            parent_policy: ChunkPolicyParams {
                min_words: 50,
                max_words: 100,
                overlap_words: 0,
                sentence_boundary: ".!?".into(),
            },
        };
        Ok((directory, request))
    }

    /// expect: the public tool publishes all three source-faithful policies and a complete map.
    #[tokio::test]
    async fn builds_and_validates_all_representations() -> anyhow::Result<()> {
        let (_directory, request) = fixture()?;
        let output_dir = PathBuf::from(&request.output_dir);
        let response = server()
            .corpus_build_chunk_representations(Parameters(request))
            .await?;
        let response: serde_json::Value = serde_json::from_str(&response)?;
        let content = hkask_types::tool_response::unwrap_tool_envelope(response);
        assert_eq!(content["accepted_sources"], 1);
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output_dir.join("manifest.json"))?)?;
        assert_eq!(manifest["policies"]["reference"]["max_words"], 100);
        assert_eq!(manifest["policies"]["reference"]["overlap_words"], 0);
        assert_eq!(
            manifest["validation"]["normalized_source_reconstruction"],
            true
        );
        let child_count = manifest["artifacts"]["fine_children"]["rows"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("missing child count"))?;
        assert_eq!(
            manifest["artifacts"]["child_parent_map"]["rows"],
            child_count
        );
        let reference: Vec<RepresentationRow> =
            fs::read_to_string(output_dir.join("reference.jsonl"))?
                .lines()
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()?;
        assert_eq!(
            reference
                .iter()
                .map(|row| row.word_count)
                .collect::<Vec<_>>(),
            vec![100, 130],
            "the final 30-word reference remainder must merge backward"
        );
        let children: Vec<RepresentationRow> =
            fs::read_to_string(output_dir.join("fine-children.jsonl"))?
                .lines()
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()?;
        let parents: Vec<RepresentationRow> = fs::read_to_string(output_dir.join("parents.jsonl"))?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        let mappings: Vec<ChildParentRow> =
            fs::read_to_string(output_dir.join("child-parent-map.jsonl"))?
                .lines()
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()?;
        let parent_sources: BTreeMap<_, _> = parents
            .iter()
            .map(|row| (row.entity_ref.as_str(), row.source.as_str()))
            .collect();
        assert_eq!(children.len(), mappings.len());
        for child in &children {
            let mapping = mappings
                .iter()
                .find(|mapping| mapping.child_ref == child.entity_ref)
                .ok_or_else(|| anyhow::anyhow!("child is not mapped"))?;
            assert_eq!(mapping.source, child.source);
            assert!(!mapping.parent_refs.is_empty());
            for parent_ref in &mapping.parent_refs {
                assert_eq!(
                    parent_sources.get(parent_ref.as_str()),
                    Some(&child.source.as_str())
                );
            }
        }
        assert!(children.iter().all(|row| {
            row.provenance.raw_sha256 == row.provenance.canonical_sha256
                && row.provenance.canonical_normalization
                    == "filter_boilerplate_then_sanitize_c0_split_whitespace_join_single_space"
        }));
        Ok(())
    }

    /// expect: calibration compares retrieval policies over the same furniture-filtered source view used by production chunking.
    /// [P3] Motivating: retrieval calibration selects a policy for substantive source knowledge rather than book furniture.
    /// [P1] Constraining: every removed source range remains reviewable in the sealed manifest.
    /// [P4] Constraining: every representation must reconstruct the filtered view before publication.
    /// pre: accepted canonical text contains bounded page front matter and an inline promotional call to action
    /// post: all policy artifacts exclude those spans and the manifest reconciles their source-level exclusions
    #[tokio::test]
    async fn calibration_filters_source_furniture_before_every_representation() -> anyhow::Result<()>
    {
        let (_directory, mut request) = fixture()?;
        let accepted = request
            .accepted_sources
            .first_mut()
            .ok_or_else(|| anyhow::anyhow!("fixture source missing"))?;
        let canonical = PathBuf::from(&accepted.canonical_path);
        let body = "Chapter 1\nSubstantive analysis explains how retrieval evidence supports a decision while preserving its source identity. ".repeat(80);
        let formatting_artifact = format!("\\uparrow {}\\q�", "\\qquad ".repeat(20));
        let source = format!(
            "A USEFUL BOOK\nJANE AUTHOR\u{000c}OceanofPDF.com Page 160 {body}Thanks for reading Example Research! Subscribe for free to receive new posts and support my work. Subscribe now\n{formatting_artifact}\n{body}"
        );
        fs::write(&canonical, source)?;
        let digest = sha256_bytes(&fs::read(&canonical)?);
        accepted.raw_sha256 = digest.clone();
        accepted.canonical_sha256 = digest;

        let output_dir = PathBuf::from(&request.output_dir);
        server()
            .corpus_build_chunk_representations(Parameters(request))
            .await?;

        for artifact in [
            "reference.jsonl",
            "current.jsonl",
            "fine-children.jsonl",
            "parents.jsonl",
        ] {
            let text = fs::read_to_string(output_dir.join(artifact))?;
            assert!(!text.contains("A USEFUL BOOK"), "{artifact}");
            assert!(!text.contains("Thanks for reading"), "{artifact}");
            assert!(!text.contains("OceanofPDF.com"), "{artifact}");
            assert!(!text.contains("Subscribe now"), "{artifact}");
            assert!(!text.contains("\\qquad"), "{artifact}");
            assert!(text.contains("Substantive analysis"), "{artifact}");
        }
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output_dir.join("manifest.json"))?)?;
        let report = &manifest["boilerplate_exclusion_reports"]["source-a.txt"];
        assert!(report["input_words"].as_u64().is_some());
        assert!(report["retained_words"].as_u64().is_some());
        assert_eq!(report["exclusions"].as_array().map(Vec::len), Some(5));
        assert_eq!(manifest["validation"]["boilerplate_filter_applied"], true);
        Ok(())
    }

    /// expect: reconstruction applies the same C0 sanitization as every emitted chunk policy.
    #[tokio::test]
    async fn reconstruction_matches_shared_c0_sanitization() -> anyhow::Result<()> {
        let (_directory, mut request) = fixture()?;
        let accepted = request
            .accepted_sources
            .first_mut()
            .ok_or_else(|| anyhow::anyhow!("fixture source missing"))?;
        let canonical = PathBuf::from(&accepted.canonical_path);
        let text = fs::read_to_string(&canonical)?.replace("word100", "word100\u{2}µ");
        fs::write(&canonical, text)?;
        let digest = sha256_bytes(&fs::read(&canonical)?);
        accepted.raw_sha256 = digest.clone();
        accepted.canonical_sha256 = digest;

        let output_dir = PathBuf::from(&request.output_dir);
        server()
            .corpus_build_chunk_representations(Parameters(request))
            .await?;
        let reference = fs::read_to_string(output_dir.join("reference.jsonl"))?;
        assert!(!reference.contains('\u{2}'));
        let rows: Vec<RepresentationRow> = reference
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        assert!(
            rows.iter()
                .map(|row| row.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
                .contains("word100 µ")
        );
        let row = rows
            .first()
            .ok_or_else(|| anyhow::anyhow!("reference row missing"))?;
        assert_eq!(
            row.provenance.canonical_normalization,
            "filter_boilerplate_then_sanitize_c0_split_whitespace_join_single_space"
        );
        Ok(())
    }

    /// expect: changed accepted-source bytes fail before any representation directory is published.
    #[tokio::test]
    async fn rejects_changed_source_identity_without_residue() -> anyhow::Result<()> {
        let (_directory, mut request) = fixture()?;
        request
            .accepted_sources
            .first_mut()
            .ok_or_else(|| anyhow::anyhow!("fixture source missing"))?
            .canonical_sha256 = "0".repeat(64);
        let output_dir = PathBuf::from(&request.output_dir);
        let error = server()
            .corpus_build_chunk_representations(Parameters(request))
            .await
            .expect_err("changed source hash must fail");
        assert!(error.to_string().contains("canonical source hash changed"));
        assert!(!output_dir.exists());
        Ok(())
    }
}
