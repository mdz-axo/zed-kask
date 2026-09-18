//! Identity-bound external grounding required before generated QA ingestion.

use std::collections::{HashMap, HashSet};

use hkask_mcp_server::server::McpToolError;
use serde::Deserialize;

use crate::helpers::read_jsonl;
use crate::tools::corpus::qa_parsing::ParsedQa;

pub(crate) const QA_GROUNDING_PROTOCOL: &str = "prepared-qa-grounding-verification-v1";
const MIN_FACT_SCORE: f64 = 0.80;
const SCORE_TOLERANCE: f64 = 1e-12;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalChunk {
    entity_ref: String,
    source: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OntologyAnchor {
    term: String,
    tier: String,
    namespace: String,
    concept: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceReference {
    chunk_ref: String,
    source: String,
    quote: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroundedJudgment {
    field: String,
    text: String,
    provenance: String,
    strength: u8,
    entailment: bool,
    why: String,
    ontology_anchor: OntologyAnchor,
    source_reference: SourceReference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FactScoreBreakdown {
    sar: f64,
    cvr: f64,
    hfr: f64,
    nlr: f64,
    claims_checked: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroundingReportRow {
    protocol: String,
    candidate_sha256: String,
    prompt_id: String,
    chunk_ref: String,
    source: String,
    qa_type: String,
    judgments: Vec<GroundedJudgment>,
    fact_score_breakdown: FactScoreBreakdown,
    fact_score: f64,
    confidence_band: String,
    decoupling: String,
    verdict: String,
    findings: Vec<String>,
}

fn invalid(message: impl Into<String>) -> McpToolError {
    McpToolError::invalid_argument(message.into())
}

fn nonblank(value: &str) -> bool {
    !value.trim().is_empty()
}

fn valid_ratio(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn valid_anchor(anchor: &OntologyAnchor) -> bool {
    nonblank(&anchor.term)
        && nonblank(&anchor.namespace)
        && nonblank(&anchor.concept)
        && matches!(
            anchor.tier.as_str(),
            "domain" | "derived" | "upper" | "core"
        )
}

fn expected_strength(provenance: &str) -> Option<u8> {
    match provenance {
        "tool_verified" | "platform_derived" => Some(2),
        "model_inference" => Some(1),
        _ => None,
    }
}

fn validate_score(row: &GroundingReportRow) -> Result<(), McpToolError> {
    let metrics = &row.fact_score_breakdown;
    if metrics.claims_checked != row.judgments.len() || metrics.claims_checked == 0 {
        return Err(invalid(format!(
            "Grounding report '{}' claims_checked does not match its nonempty judgments",
            row.prompt_id
        )));
    }
    if ![
        metrics.sar,
        metrics.cvr,
        metrics.hfr,
        metrics.nlr,
        row.fact_score,
    ]
    .into_iter()
    .all(valid_ratio)
    {
        return Err(invalid(format!(
            "Grounding report '{}' requires finite non-null ratios",
            row.prompt_id
        )));
    }
    let expected =
        0.30 * metrics.sar + 0.25 * metrics.cvr + 0.20 * metrics.hfr + 0.25 * metrics.nlr;
    if (row.fact_score - expected).abs() >= SCORE_TOLERANCE {
        return Err(invalid(format!(
            "Grounding report '{}' fact_score does not match the canonical weights",
            row.prompt_id
        )));
    }
    if row.fact_score < MIN_FACT_SCORE {
        return Err(invalid(format!(
            "Grounding report '{}' fact_score {} is below {MIN_FACT_SCORE}",
            row.prompt_id, row.fact_score
        )));
    }
    Ok(())
}

fn validate_judgments(
    row: &GroundingReportRow,
    qa: &ParsedQa,
    chunks: &HashMap<&str, &CanonicalChunk>,
) -> Result<(), McpToolError> {
    if row.judgments.len() != 2 {
        return Err(invalid(format!(
            "Grounding report '{}' must judge instruction and output exactly once",
            row.prompt_id
        )));
    }
    let mut fields = HashSet::with_capacity(2);
    for judgment in &row.judgments {
        if !fields.insert(judgment.field.as_str()) {
            return Err(invalid(format!(
                "Grounding report '{}' repeats judgment field '{}'",
                row.prompt_id, judgment.field
            )));
        }
        let expected_text = match judgment.field.as_str() {
            "instruction" => &qa.instruction,
            "output" => &qa.output,
            field => {
                return Err(invalid(format!(
                    "Grounding report '{}' has unsupported judgment field '{field}'",
                    row.prompt_id
                )));
            }
        };
        if judgment.text != *expected_text {
            return Err(invalid(format!(
                "Grounding report '{}' judgment text does not match candidate field '{}'",
                row.prompt_id, judgment.field
            )));
        }
        if !judgment.entailment || judgment.why.chars().count() < 40 {
            return Err(invalid(format!(
                "Grounding report '{}' judgment '{}' requires entailment and a 40-character rationale",
                row.prompt_id, judgment.field
            )));
        }
        if expected_strength(&judgment.provenance) != Some(judgment.strength) {
            return Err(invalid(format!(
                "Grounding report '{}' judgment '{}' has invalid provenance strength",
                row.prompt_id, judgment.field
            )));
        }
        if !valid_anchor(&judgment.ontology_anchor) {
            return Err(invalid(format!(
                "Grounding report '{}' judgment '{}' has no valid ontology anchor",
                row.prompt_id, judgment.field
            )));
        }
        let reference = &judgment.source_reference;
        if !nonblank(&reference.quote)
            || !qa.evidence_quotes.iter().any(|evidence| {
                evidence.chunk_ref == reference.chunk_ref
                    && evidence.source == reference.source
                    && evidence.quote == reference.quote
            })
        {
            return Err(invalid(format!(
                "Grounding report '{}' judgment '{}' does not cite one of the candidate's exact evidence quotes",
                row.prompt_id, judgment.field
            )));
        }
        let chunk = chunks.get(reference.chunk_ref.as_str()).ok_or_else(|| {
            invalid(format!(
                "Grounding report '{}' references unknown chunk_ref '{}'",
                row.prompt_id, reference.chunk_ref
            ))
        })?;
        if chunk.source != reference.source || !chunk.text.contains(&reference.quote) {
            return Err(invalid(format!(
                "Grounding report '{}' judgment '{}' evidence does not match canonical source bytes",
                row.prompt_id, judgment.field
            )));
        }
    }
    if fields != HashSet::from(["instruction", "output"]) {
        return Err(invalid(format!(
            "Grounding report '{}' must cover instruction and output",
            row.prompt_id
        )));
    }
    Ok(())
}

/// expect: Every ingestible QA candidate has one exact, decoupled grounding acceptance.
/// pre: generated candidates, grounding report, and canonical chunks are readable JSONL.
/// post: all candidates are identity-bound to accepted source-grounded judgments, or ingestion stops.
pub(crate) fn verify_complete_grounding(
    report_path: &str,
    chunks_path: &str,
    qas: &[ParsedQa],
) -> Result<(), McpToolError> {
    let reports: Vec<GroundingReportRow> = read_jsonl(report_path, "grounding_verification_jsonl")?;
    let canonical_chunks: Vec<CanonicalChunk> = read_jsonl(chunks_path, "source_chunks_jsonl")?;
    let mut chunks = HashMap::with_capacity(canonical_chunks.len());
    for chunk in &canonical_chunks {
        if chunks.insert(chunk.entity_ref.as_str(), chunk).is_some() {
            return Err(invalid(format!(
                "Duplicate canonical chunk_ref '{}'",
                chunk.entity_ref
            )));
        }
    }
    if reports.len() != qas.len() {
        return Err(invalid(format!(
            "Grounding report covers {} of {} ingestible QA candidates",
            reports.len(),
            qas.len()
        )));
    }
    let mut by_hash = HashMap::with_capacity(reports.len());
    for report in &reports {
        if report.protocol != QA_GROUNDING_PROTOCOL {
            return Err(invalid(format!(
                "Grounding report '{}' has unsupported protocol '{}'",
                report.prompt_id, report.protocol
            )));
        }
        if by_hash
            .insert(report.candidate_sha256.as_str(), report)
            .is_some()
        {
            return Err(invalid(format!(
                "Duplicate grounding candidate_sha256 '{}'",
                report.candidate_sha256
            )));
        }
    }
    for qa in qas {
        let report = by_hash.get(qa.row_sha256.as_str()).ok_or_else(|| {
            invalid(format!(
                "No grounding report matches candidate SHA-256 '{}'",
                qa.row_sha256
            ))
        })?;
        let prompt_id = qa
            .prompt_id
            .as_deref()
            .ok_or_else(|| invalid("Grounding-gated ingestion requires candidate prompt_id"))?;
        let chunk_ref = qa
            .chunk_ref
            .as_deref()
            .ok_or_else(|| invalid("Grounding-gated ingestion requires candidate chunk_ref"))?;
        if report.prompt_id != prompt_id
            || report.chunk_ref != chunk_ref
            || report.source != qa.source
            || report.qa_type != qa.qa_type
        {
            return Err(invalid(format!(
                "Grounding report '{}' does not match candidate identity",
                report.prompt_id
            )));
        }
        if report.verdict != "accept"
            || !report.findings.is_empty()
            || report.decoupling != "spawn_agent"
            || !matches!(report.confidence_band.as_str(), "medium" | "high")
        {
            return Err(invalid(format!(
                "Grounding report '{}' is not a clean decoupled acceptance",
                report.prompt_id
            )));
        }
        validate_score(report)?;
        validate_judgments(report, qa, &chunks)?;
    }
    Ok(())
}
