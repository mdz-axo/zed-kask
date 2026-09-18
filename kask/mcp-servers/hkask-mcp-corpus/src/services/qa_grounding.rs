//! Server-owned grounding records for generated QA candidates.
//!
//! `corpus_ground_generated_qa` writes a hash-bound bundle (`manifest.json` +
//! `grounding-rows.jsonl`, protocol `corpus-qa-grounding-v1`) that records
//! mechanically derived grounding facts only: identity, byte spans into
//! canonical source bytes, and server-recomputed ontology resolutions. The
//! bundle deliberately contains no verified/authorized/confidence semantics —
//! no row, claim, or score asserts acceptance.
//!
//! `corpus_ingest_qa` re-executes every check from the candidates and canonical
//! chunks before dedup, output, or DB access: it re-hashes the bundle, checks
//! row bijection, requires sources classified under the current
//! published-ontology protocol, recomputes ontology resolutions, re-derives
//! every claim, requires each artifact row to equal its re-execution, and
//! admits only rows whose applicable factual claims are all strength 2
//! (`tool_verified`/`platform_derived`). A `model_inference` answer fails
//! closed: paraphrase and conceptual answers stay blocked until an independent
//! semantic oracle is specified. Authority is derived by re-execution, never
//! read from the artifact.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use hkask_bridge_ontology::term_resolution::{
    TERM_RESOLUTION_PROTOCOL, TermResolution, resolve_term,
};
use hkask_mcp_server::server::McpToolError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::helpers::read_text_capped;
use crate::tools::corpus::qa_parsing::ParsedQa;
use crate::tools::corpus::read_tagged_chunks;

pub(crate) const QA_GROUNDING_PROTOCOL: &str = "corpus-qa-grounding-v1";
const MANIFEST_SCHEMA_VERSION: u32 = 1;
const MANIFEST_FILE: &str = "manifest.json";
const GROUNDING_ROWS_FILE: &str = "grounding-rows.jsonl";

const ANSWER_EXACT_METHOD: &str = "exact_substring_of_own_source_grounded_evidence";
const CITATION_EXACT_METHOD: &str = "exact_nonempty_substring_and_source_identity";
const SEMANTIC_SUPPORT_METHOD: &str = "semantic_support";
const ROLE_CITED: &str = "cited_substring";
const ROLE_ANSWER: &str = "answer";
const ROLE_INSTRUCTION: &str = "instruction_premise";
const MODE_IS: &str = "is";
const MODE_UNCLASSIFIED: &str = "unclassified";
const MODE_INTERROGATIVE: &str = "interrogative";
const TIER_TOOL_VERIFIED: &str = "tool_verified";
const TIER_PLATFORM_DERIVED: &str = "platform_derived";
const TIER_MODEL_INFERENCE: &str = "model_inference";
const TIER_REJECTED: &str = "rejected";
const TIER_UNAVAILABLE: &str = "unavailable";
const TIER_NOT_APPLICABLE: &str = "not_applicable";

/// A byte range in an identified byte source. The gate re-derives every span
/// and requires the artifact to agree, so spans are re-executable facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum QaByteSpan {
    CanonicalChunk {
        chunk_ref: String,
        offset: usize,
        length: usize,
    },
    EvidenceQuote {
        evidence_index: usize,
        offset: usize,
        length: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QaClaimCrossCheck {
    method: String,
    performed: bool,
    matched: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QaFinding {
    code: String,
    detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QaClaim {
    claim_id: String,
    role: String,
    text: String,
    epistemic_mode: String,
    provenance: String,
    strength: u8,
    byte_span: Option<QaByteSpan>,
    cross_check: QaClaimCrossCheck,
    finding: Option<QaFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QaGroundingRow {
    row_key: String,
    line: usize,
    candidate_sha256: String,
    prompt_id: String,
    qa_type: String,
    chunk_ref: String,
    source: String,
    candidate_terms: Vec<String>,
    term_resolutions: Vec<TermResolution>,
    claims: Vec<QaClaim>,
    findings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QaBundleArtifact {
    path: String,
    sha256: String,
    rows: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct QaBundleInput {
    path: String,
    sha256: String,
    rows: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QaBundleInputs {
    candidates: QaBundleInput,
    source_chunks: QaBundleInput,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QaGroundingCounts {
    candidates: usize,
    tool_verified_answers: usize,
    model_inference_answers: usize,
    rejected_citations: usize,
    unavailable_citations: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QaGroundingScope {
    verified_semantics: String,
    admission: String,
    instruction_premises: String,
    paraphrase_policy: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QaGroundingManifest {
    schema_version: u32,
    protocol: String,
    ontology_protocol: String,
    inputs: QaBundleInputs,
    artifacts: QaBundleArtifacts,
    counts: QaGroundingCounts,
    scope: QaGroundingScope,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QaBundleArtifacts {
    grounding_rows: QaBundleArtifact,
}

/// Request for the deterministic grounding bundle tool. Zero inference.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GroundQaRequest {
    /// Generated candidate rows JSONL from corpus_generate_qa_batch. Every
    /// nonblank line must parse as a structurally complete QA candidate with
    /// prompt_id; skip and error rows fail closed.
    pub generated_jsonl: String,
    /// Canonical tagged-chunk JSONL. Every chunk must be classified under the
    /// current published-ontology protocol (`published-term-resolution-v1`)
    /// with reconciling candidate terms.
    pub source_chunks_jsonl: String,
    /// Destination directory for the bundle. Must not already exist; the
    /// bundle is written to a temporary directory and atomically renamed.
    pub output_dir: String,
}

/// Identity of the accepted grounding bundle, persisted with every stored row.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GroundingGateReport {
    pub manifest_sha256: String,
    pub rows: usize,
}

fn invalid(message: impl Into<String>) -> McpToolError {
    McpToolError::invalid_argument(message.into())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// `row_key` binds one candidate to one grounding row: prompt identity, level,
/// and the physical line the candidate occupies in the generated file.
pub(crate) fn candidate_row_key(prompt_id: &str, qa_type: &str, line: usize) -> String {
    format!("{prompt_id}|{qa_type}|line-{line}")
}

/// Read the candidate file under the grounding standard: every nonblank line
/// must parse as a structurally complete QA candidate carrying prompt_id.
/// Returns `(physical_line, candidate)` pairs; physical lines are 1-based
/// over every line of the file so the row key is stable for its bytes.
pub(crate) fn read_grounding_candidates(
    path: &str,
) -> Result<Vec<(usize, ParsedQa)>, McpToolError> {
    let content = read_text_capped(path, "generated_jsonl")?;
    let mut candidates = Vec::new();
    let mut malformed = 0usize;
    let mut non_qa = 0usize;
    let mut incomplete = 0usize;
    for (index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match crate::tools::corpus::qa_parsing::parse_qa_record(line) {
            Ok(qa) => {
                let complete = !qa.instruction.trim().is_empty()
                    && !qa.output.trim().is_empty()
                    && !qa.qa_type.trim().is_empty()
                    && !qa.source.trim().is_empty()
                    && qa
                        .chunk_ref
                        .as_ref()
                        .is_some_and(|value| !value.trim().is_empty())
                    && qa
                        .prompt_id
                        .as_ref()
                        .is_some_and(|value| !value.trim().is_empty());
                if complete {
                    candidates.push((index + 1, qa));
                } else {
                    incomplete += 1;
                }
            }
            Err(crate::tools::corpus::qa_parsing::QaRecordError::GeneratorError) => non_qa += 1,
            Err(crate::tools::corpus::qa_parsing::QaRecordError::Malformed) => malformed += 1,
        }
    }
    if malformed > 0 || non_qa > 0 || incomplete > 0 {
        return Err(invalid(format!(
            "Grounding requires a complete candidate file: {malformed} malformed, \
             {non_qa} skip-or-error rows, {incomplete} structurally incomplete rows"
        )));
    }
    Ok(candidates)
}

/// Read the canonical sources under the grounding standard: every chunk must
/// carry current published-ontology classification whose derived fields still
/// reconcile, and entity references must be unique.
pub(crate) fn read_grounding_chunks(
    path: &str,
) -> Result<Vec<hkask_types::corpus::TaggedChunk>, McpToolError> {
    let chunks = read_tagged_chunks(path)?;
    let mut seen = std::collections::HashSet::new();
    for chunk in &chunks {
        if !chunk.has_current_canonical_terms() {
            return Err(invalid(format!(
                "Grounding requires sources classified under {TERM_RESOLUTION_PROTOCOL} \
                 with reconciling candidate terms; chunk '{}' is stale or unreconciled",
                chunk.entity_ref
            )));
        }
        if !seen.insert(chunk.entity_ref.as_str()) {
            return Err(invalid(format!(
                "Duplicate canonical chunk_ref '{}'",
                chunk.entity_ref
            )));
        }
    }
    Ok(chunks)
}

fn chunk_index<'a>(
    chunks: &'a [hkask_types::corpus::TaggedChunk],
) -> HashMap<&'a str, &'a hkask_types::corpus::TaggedChunk> {
    chunks
        .iter()
        .map(|chunk| (chunk.entity_ref.as_str(), chunk))
        .collect()
}

/// Derive one grounding row. Pure: the same candidates, chunks, and line
/// number always produce the same row, so the gate can re-execute and compare.
pub(crate) fn ground_row(
    line: usize,
    qa: &ParsedQa,
    chunks: &HashMap<&str, &hkask_types::corpus::TaggedChunk>,
) -> Result<QaGroundingRow, McpToolError> {
    let prompt_id = qa
        .prompt_id
        .as_deref()
        .ok_or_else(|| invalid("Grounding requires candidate prompt_id"))?;
    let chunk_ref = qa
        .chunk_ref
        .as_deref()
        .ok_or_else(|| invalid("Grounding requires candidate chunk_ref"))?;
    let row_key = candidate_row_key(prompt_id, &qa.qa_type, line);
    let mut claims = Vec::with_capacity(qa.evidence_quotes.len() + 2);

    // Evidence-quote claims: exact nonempty substrings of uniquely identified
    // canonical source bytes.
    for (index, evidence) in qa.evidence_quotes.iter().enumerate() {
        let claim_id = format!("{row_key}:citation-{}", index + 1);
        let (provenance, strength, span, cross_check, finding) = match chunks
            .get(evidence.chunk_ref.as_str())
        {
            None => (
                TIER_UNAVAILABLE,
                0,
                None,
                QaClaimCrossCheck {
                    method: CITATION_EXACT_METHOD.to_string(),
                    performed: false,
                    matched: None,
                },
                Some(QaFinding {
                    code: "unknown_chunk_ref".into(),
                    detail: format!(
                        "No canonical chunk with entity_ref '{}' exists in the supplied sources",
                        evidence.chunk_ref
                    ),
                }),
            ),
            Some(chunk) if chunk.source != evidence.source => (
                TIER_REJECTED,
                0,
                None,
                QaClaimCrossCheck {
                    method: CITATION_EXACT_METHOD.to_string(),
                    performed: true,
                    matched: Some(false),
                },
                Some(QaFinding {
                    code: "wrong_source".into(),
                    detail: format!(
                        "Chunk '{}' belongs to source '{}', not the cited '{}'",
                        evidence.chunk_ref, chunk.source, evidence.source
                    ),
                }),
            ),
            Some(chunk) => match chunk.text.find(evidence.quote.as_str()) {
                Some(offset) if !evidence.quote.is_empty() => (
                    TIER_TOOL_VERIFIED,
                    2,
                    Some(QaByteSpan::CanonicalChunk {
                        chunk_ref: evidence.chunk_ref.clone(),
                        offset,
                        length: evidence.quote.len(),
                    }),
                    QaClaimCrossCheck {
                        method: CITATION_EXACT_METHOD.to_string(),
                        performed: true,
                        matched: Some(true),
                    },
                    None,
                ),
                _ => (
                    TIER_REJECTED,
                    0,
                    None,
                    QaClaimCrossCheck {
                        method: CITATION_EXACT_METHOD.to_string(),
                        performed: true,
                        matched: Some(false),
                    },
                    Some(QaFinding {
                        code: "evidence_quote_absent_from_chunk".into(),
                        detail: format!(
                            "The cited quote does not occur byte-exactly in canonical chunk '{}'",
                            evidence.chunk_ref
                        ),
                    }),
                ),
            },
        };
        claims.push(QaClaim {
            claim_id,
            role: ROLE_CITED.to_string(),
            text: evidence.quote.clone(),
            epistemic_mode: MODE_IS.to_string(),
            provenance: provenance.to_string(),
            strength,
            byte_span: span,
            cross_check,
            finding,
        });
    }

    // Answer claim: strength 2 only when the answer is byte-exact inside one of
    // the row's own tool-verified evidence quotes (the closed library the row
    // itself asserts). A paraphrase is model-mediated — an observation, never
    // authority — and stays model_inference until an independent semantic
    // oracle is specified.
    let exact = if qa.output.trim().is_empty() {
        None
    } else {
        qa.evidence_quotes
            .iter()
            .enumerate()
            .filter_map(|(index, evidence)| {
                let verified = claims
                    .get(index)
                    .is_some_and(|claim| claim.provenance == TIER_TOOL_VERIFIED);
                verified
                    .then(|| {
                        evidence
                            .quote
                            .find(qa.output.as_str())
                            .map(|offset| (index, offset))
                    })
                    .flatten()
            })
            .next()
    };
    let answer_claim = match exact {
        Some((evidence_index, offset)) => QaClaim {
            claim_id: format!("{row_key}:answer"),
            role: ROLE_ANSWER.to_string(),
            text: qa.output.clone(),
            epistemic_mode: MODE_IS.to_string(),
            provenance: TIER_TOOL_VERIFIED.to_string(),
            strength: 2,
            byte_span: Some(QaByteSpan::EvidenceQuote {
                evidence_index,
                offset,
                length: qa.output.len(),
            }),
            cross_check: QaClaimCrossCheck {
                method: ANSWER_EXACT_METHOD.to_string(),
                performed: true,
                matched: Some(true),
            },
            finding: None,
        },
        None => QaClaim {
            claim_id: format!("{row_key}:answer"),
            role: ROLE_ANSWER.to_string(),
            text: qa.output.clone(),
            epistemic_mode: MODE_UNCLASSIFIED.to_string(),
            provenance: TIER_MODEL_INFERENCE.to_string(),
            strength: 1,
            byte_span: None,
            cross_check: QaClaimCrossCheck {
                method: ANSWER_EXACT_METHOD.to_string(),
                performed: true,
                matched: Some(false),
            },
            finding: Some(QaFinding {
                code: "answer_not_source_exact".into(),
                detail: "The answer is not byte-exact within the row's source-grounded \
                         evidence; paraphrase admission requires the independent semantic \
                         oracle, which this protocol version does not specify"
                    .into(),
            }),
        },
    };
    claims.push(answer_claim);

    // Instruction claim: question premises are semantic and no mechanical
    // check exists; the record states the unperformed check instead of
    // silently trusting or silently rejecting the question.
    claims.push(QaClaim {
        claim_id: format!("{row_key}:instruction"),
        role: ROLE_INSTRUCTION.to_string(),
        text: qa.instruction.clone(),
        epistemic_mode: MODE_INTERROGATIVE.to_string(),
        provenance: TIER_NOT_APPLICABLE.to_string(),
        strength: 0,
        byte_span: None,
        cross_check: QaClaimCrossCheck {
            method: SEMANTIC_SUPPORT_METHOD.to_string(),
            performed: false,
            matched: None,
        },
        finding: Some(QaFinding {
            code: "instruction_premise_verification_unperformed".into(),
            detail: "Question-premise verification is semantic; the instruction is \
                     recorded structurally and stays outside the mechanical admission \
                     inventory"
                .into(),
        }),
    });

    let term_resolutions: Vec<TermResolution> =
        qa.concepts.iter().map(|term| resolve_term(term)).collect();
    let findings = claims
        .iter()
        .filter_map(|claim| claim.finding.as_ref().map(|finding| finding.code.clone()))
        .collect();
    Ok(QaGroundingRow {
        row_key,
        line,
        candidate_sha256: qa.row_sha256.clone(),
        prompt_id: prompt_id.to_string(),
        qa_type: qa.qa_type.clone(),
        chunk_ref: chunk_ref.to_string(),
        source: qa.source.clone(),
        candidate_terms: qa.concepts.clone(),
        term_resolutions,
        claims,
        findings,
    })
}

fn serialize_rows(rows: &[QaGroundingRow]) -> Result<String, McpToolError> {
    let mut body = String::new();
    for row in rows {
        body.push_str(&serde_json::to_string(row).map_err(|error| {
            McpToolError::internal(format!("Cannot serialize grounding row: {error}"))
        })?);
        body.push('\n');
    }
    Ok(body)
}

/// Build the hash-bound bundle. Deterministic and zero-inference: this is a
/// mechanical record, never a verdict. A row with a model_inference answer is
/// recorded honestly; only the ingestion gate decides admission.
pub(crate) fn build_grounding_bundle(request: &GroundQaRequest) -> Result<Value, McpToolError> {
    let candidates = read_grounding_candidates(&request.generated_jsonl)?;
    let chunks = read_grounding_chunks(&request.source_chunks_jsonl)?;
    let index = chunk_index(&chunks);
    let rows = candidates
        .iter()
        .map(|(line, qa)| ground_row(*line, qa, &index))
        .collect::<Result<Vec<_>, _>>()?;
    let mut row_keys = std::collections::HashSet::new();
    for row in &rows {
        if !row_keys.insert(row.row_key.as_str()) {
            return Err(invalid(format!(
                "Duplicate row key '{}' — candidate identities collide",
                row.row_key
            )));
        }
    }

    let rows_body = serialize_rows(&rows)?;
    let candidates_bytes = std::fs::read(&request.generated_jsonl).map_err(|error| {
        crate::helpers::map_corpus_io_error(
            error,
            &format!("Cannot read '{}'", request.generated_jsonl),
        )
    })?;
    let chunks_bytes = std::fs::read(&request.source_chunks_jsonl).map_err(|error| {
        crate::helpers::map_corpus_io_error(
            error,
            &format!("Cannot read '{}'", request.source_chunks_jsonl),
        )
    })?;
    let counts = QaGroundingCounts {
        candidates: rows.len(),
        tool_verified_answers: rows
            .iter()
            .filter(|row| {
                row.claims
                    .iter()
                    .any(|claim| claim.role == "answer" && claim.provenance == "tool_verified")
            })
            .count(),
        model_inference_answers: rows
            .iter()
            .filter(|row| {
                row.claims
                    .iter()
                    .any(|claim| claim.role == "answer" && claim.provenance == "model_inference")
            })
            .count(),
        rejected_citations: rows
            .iter()
            .flat_map(|row| row.claims.iter())
            .filter(|claim| claim.role == "cited_substring" && claim.provenance == "rejected")
            .count(),
        unavailable_citations: rows
            .iter()
            .flat_map(|row| row.claims.iter())
            .filter(|claim| claim.role == "cited_substring" && claim.provenance == "unavailable")
            .count(),
    };

    let output_path = Path::new(&request.output_dir);
    let parent = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let temporary = match parent {
        Some(parent) => tempfile::Builder::new()
            .prefix("qa-grounding-")
            .tempdir_in(parent),
        None => tempfile::Builder::new().prefix("qa-grounding-").tempdir(),
    }
    .map_err(|error| {
        crate::helpers::map_corpus_io_error(
            error,
            &format!(
                "Cannot create temporary bundle near '{}'",
                request.output_dir
            ),
        )
    })?;
    let rows_path = temporary.path().join(GROUNDING_ROWS_FILE);
    std::fs::write(&rows_path, rows_body.as_bytes()).map_err(|error| {
        crate::helpers::map_corpus_io_error(
            error,
            &format!("Cannot write '{}'", rows_path.display()),
        )
    })?;
    let manifest = QaGroundingManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        protocol: QA_GROUNDING_PROTOCOL.to_string(),
        ontology_protocol: TERM_RESOLUTION_PROTOCOL.to_string(),
        inputs: QaBundleInputs {
            candidates: QaBundleInput {
                path: request.generated_jsonl.clone(),
                sha256: sha256_hex(&candidates_bytes),
                rows: candidates.len(),
            },
            source_chunks: QaBundleInput {
                path: request.source_chunks_jsonl.clone(),
                sha256: sha256_hex(&chunks_bytes),
                rows: chunks.len(),
            },
        },
        artifacts: QaBundleArtifacts {
            grounding_rows: QaBundleArtifact {
                path: GROUNDING_ROWS_FILE.to_string(),
                sha256: sha256_hex(rows_body.as_bytes()),
                rows: rows.len(),
            },
        },
        counts,
        scope: QaGroundingScope {
            verified_semantics: "none — this bundle records mechanical facts only; no row \
                                 is verified, authorized, or confidence-scored"
                .to_string(),
            admission: "corpus_ingest_qa re-executes every check and admits only rows \
                        whose applicable factual claims are strength 2"
                .to_string(),
            instruction_premises: "question premises are not semantically verified; the \
                                   instruction claim records the unperformed check"
                .to_string(),
            paraphrase_policy: "answers that are not byte-exact within the row's own \
                                source-grounded evidence stay model_inference and fail \
                                ingestion until an independent semantic oracle is specified"
                .to_string(),
        },
    };
    let manifest_path = temporary.path().join(MANIFEST_FILE);
    let manifest_body = serde_json::to_string_pretty(&manifest).map_err(|error| {
        McpToolError::internal(format!("Cannot serialize grounding manifest: {error}"))
    })?;
    std::fs::write(&manifest_path, manifest_body.as_bytes()).map_err(|error| {
        crate::helpers::map_corpus_io_error(
            error,
            &format!("Cannot write '{}'", manifest_path.display()),
        )
    })?;
    let temporary_path = temporary.path().to_path_buf();
    std::fs::rename(&temporary_path, output_path).map_err(|error| {
        let detail = format!(
            "Cannot publish bundle '{}' (the destination may already exist): {error}",
            output_path.display()
        );
        crate::helpers::map_corpus_io_error(error, &detail)
    })?;

    Ok(json!({
        "protocol": QA_GROUNDING_PROTOCOL,
        "manifest": output_path.join(MANIFEST_FILE).to_string_lossy(),
        "manifest_sha256": sha256_hex(manifest_body.as_bytes()),
        "grounding_rows": output_path.join(GROUNDING_ROWS_FILE).to_string_lossy(),
        "candidates": manifest.counts.candidates,
        "tool_verified_answers": manifest.counts.tool_verified_answers,
        "model_inference_answers": manifest.counts.model_inference_answers,
        "rejected_citations": manifest.counts.rejected_citations,
        "unavailable_citations": manifest.counts.unavailable_citations,
        "authorizes": "nothing — admission is derived by re-execution at corpus_ingest_qa",
    }))
}

fn resolve_rows_path(manifest_path: &str, rows_name: &str) -> Result<PathBuf, McpToolError> {
    if rows_name.contains('/')
        || rows_name.contains('\\')
        || rows_name.contains("..")
        || rows_name.is_empty()
    {
        return Err(invalid(format!(
            "Grounding manifest names an invalid rows artifact '{rows_name}'"
        )));
    }
    Ok(Path::new(manifest_path)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(rows_name))
}

/// The ingestion gate: re-execute every mechanical check and admit only rows
/// whose applicable factual claims are all strength 2. Runs before dedup,
/// output, and DB access. The artifact is a record — authority is derived by
/// re-execution, never read.
pub(crate) fn verify_grounding_gate(
    manifest_path: &str,
    candidates: &[(usize, &ParsedQa)],
    source_chunks_path: &str,
) -> Result<GroundingGateReport, McpToolError> {
    let manifest_bytes = read_text_capped(manifest_path, "grounding_manifest")?;
    let manifest_sha256 = sha256_hex(manifest_bytes.as_bytes());
    let manifest: QaGroundingManifest = serde_json::from_str(&manifest_bytes).map_err(|error| {
        invalid(format!(
            "Grounding manifest '{manifest_path}' is not a valid {QA_GROUNDING_PROTOCOL} \
             bundle: {error}"
        ))
    })?;
    if manifest.protocol != QA_GROUNDING_PROTOCOL {
        return Err(invalid(format!(
            "Grounding manifest protocol '{}' is not '{QA_GROUNDING_PROTOCOL}'; \
             self-reported verification reports cannot open the ingestion gate",
            manifest.protocol
        )));
    }
    let rows_path = resolve_rows_path(manifest_path, &manifest.artifacts.grounding_rows.path)?;
    let rows_bytes = read_text_capped(&rows_path.to_string_lossy(), "grounding_rows_jsonl")?;
    if sha256_hex(rows_bytes.as_bytes()) != manifest.artifacts.grounding_rows.sha256 {
        return Err(invalid(
            "Grounding rows do not match the manifest SHA-256; the bundle was modified \
             after it was written",
        ));
    }
    let mut artifact_rows = Vec::new();
    for line in rows_bytes.lines().filter(|line| !line.trim().is_empty()) {
        let row: QaGroundingRow = serde_json::from_str(line).map_err(|error| {
            invalid(format!(
                "Grounding row is not a valid {QA_GROUNDING_PROTOCOL} record: {error}"
            ))
        })?;
        artifact_rows.push(row);
    }
    if artifact_rows.len() != manifest.artifacts.grounding_rows.rows {
        return Err(invalid(format!(
            "Grounding manifest declares {} rows but the rows file carries {}",
            manifest.artifacts.grounding_rows.rows,
            artifact_rows.len()
        )));
    }

    let chunks = read_grounding_chunks(source_chunks_path)?;
    let index = chunk_index(&chunks);
    let mut by_row_key = HashMap::with_capacity(artifact_rows.len());
    for row in artifact_rows {
        let key = row.row_key.clone();
        if by_row_key.insert(key.clone(), row).is_some() {
            return Err(invalid(format!(
                "Grounding artifact repeats row key '{key}'"
            )));
        }
    }
    if by_row_key.len() != candidates.len() {
        return Err(invalid(format!(
            "Grounding bundle covers {} of {} ingestible candidates",
            by_row_key.len(),
            candidates.len()
        )));
    }

    for (line, qa) in candidates {
        let recomputed = ground_row(*line, qa, &index)?;
        let artifact = by_row_key.remove(&recomputed.row_key).ok_or_else(|| {
            invalid(format!(
                "No grounding row matches candidate '{}' at line {line}",
                recomputed.row_key
            ))
        })?;
        if artifact != recomputed {
            return Err(invalid(format!(
                "Grounding artifact row '{}' does not match re-execution; \
                 self-reported strengths, spans, or resolutions cannot open the gate",
                recomputed.row_key
            )));
        }
        for claim in &recomputed.claims {
            let applicable = claim.role == ROLE_ANSWER || claim.role == ROLE_CITED;
            let strength_two = matches!(
                claim.provenance.as_str(),
                TIER_TOOL_VERIFIED | TIER_PLATFORM_DERIVED
            ) && claim.strength == 2;
            if applicable && !strength_two {
                return Err(invalid(format!(
                    "Candidate '{}' claim '{}' is not strength-2 grounded ({}); \
                     only tool-verified or platform-derived claims can be ingested",
                    recomputed.row_key, claim.claim_id, claim.provenance
                )));
            }
        }
    }
    if !by_row_key.is_empty() {
        return Err(invalid(format!(
            "Grounding bundle carries {} rows that do not correspond to any candidate",
            by_row_key.len()
        )));
    }
    Ok(GroundingGateReport {
        manifest_sha256,
        rows: candidates.len(),
    })
}
