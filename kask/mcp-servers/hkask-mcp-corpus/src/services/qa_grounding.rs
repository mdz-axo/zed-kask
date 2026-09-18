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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::corpus::qa_parsing::parse_qa_record;
    use serde_json::{Value, json};

    const CANDIDATE: &str = r#"{"prompt_id":"qa-1","chunk_ref":"chunk-1","source":"source.txt","qa_type":"factual","response":{"instruction":"What is the delay?","output":"72 hours","type":"factual","concepts":["delay"],"evidence_quotes":[{"chunk_ref":"chunk-1","source":"source.txt","quote":"The delay is 72 hours."}]},"provenance":{"generator_model":"offline-model","grounding_status":"pending_external_verification","prompt_protocol":"prepared-qa-grounding-candidate-v1"}}"#;
    const SECOND_CANDIDATE: &str = r#"{"prompt_id":"qa-2","chunk_ref":"chunk-1","source":"source.txt","qa_type":"conceptual","response":{"instruction":"Why does the delay persist?","output":"Because the wait was verified twice.","type":"conceptual","concepts":["delay"],"evidence_quotes":[{"chunk_ref":"chunk-1","source":"source.txt","quote":"The wait was verified twice."}]},"provenance":{"generator_model":"offline-model","grounding_status":"pending_external_verification","prompt_protocol":"prepared-qa-grounding-candidate-v1"}}"#;
    const STALE_CANDIDATE: &str = r#"{"prompt_id":"qa-1","chunk_ref":"chunk-1","source":"source.txt","qa_type":"factual","response":{"instruction":"What is the delay now?","output":"72 hours","type":"factual","concepts":["delay"],"evidence_quotes":[{"chunk_ref":"chunk-1","source":"source.txt","quote":"The delay is 72 hours."}]},"provenance":{"generator_model":"offline-model","grounding_status":"pending_external_verification","prompt_protocol":"prepared-qa-grounding-candidate-v1"}}"#;
    const UNANCHORED_CANDIDATE: &str = r#"{"prompt_id":"qa-3","chunk_ref":"chunk-2","source":"source.txt","qa_type":"factual","response":{"instruction":"What is the delay?","output":"72 hours","type":"factual","concepts":["delay"],"evidence_quotes":[{"chunk_ref":"chunk-2","source":"source.txt","quote":"The delay is 72 hours, sharp."}]},"provenance":{"generator_model":"offline-model","grounding_status":"pending_external_verification","prompt_protocol":"prepared-qa-grounding-candidate-v1"}}"#;

    fn chunk_value() -> Value {
        json!({"entity_ref":"chunk-1","source":"source.txt","text":"The delay is 72 hours. The wait was verified twice."})
    }

    fn judgment(field: &str, text: &str, provenance: &str, strength: u8, quote: &str) -> Value {
        json!({
            "field": field,
            "text": text,
            "provenance": provenance,
            "strength": strength,
            "entailment": true,
            "why": "This rationale clears the mandatory forty-character floor for the recorded judgment.",
            "ontology_anchor": {"term":"delay","tier":"domain","namespace":"corpus","concept":"temporal-deferral"},
            "source_reference": {"chunk_ref":"chunk-1","source":"source.txt","quote":quote}
        })
    }

    fn base_report(candidate_sha256: &str) -> Value {
        json!({
            "protocol": QA_GROUNDING_PROTOCOL,
            "candidate_sha256": candidate_sha256,
            "prompt_id": "qa-1",
            "chunk_ref": "chunk-1",
            "source": "source.txt",
            "qa_type": "factual",
            "judgments": [
                judgment("instruction", "What is the delay?", "tool_verified", 2, "The delay is 72 hours."),
                judgment("output", "72 hours", "model_inference", 1, "The delay is 72 hours.")
            ],
            "fact_score_breakdown": {"sar":1.0,"cvr":1.0,"hfr":1.0,"nlr":0.5,"claims_checked":2},
            "fact_score": 0.875,
            "confidence_band": "medium",
            "decoupling": "spawn_agent",
            "verdict": "accept",
            "findings": []
        })
    }

    fn candidate_hash(line: &str) -> String {
        parse_qa_record(line)
            .expect("parseable fixture candidate")
            .row_sha256
    }

    fn grounding(
        report_rows: &[Value],
        chunk_rows: &[Value],
        candidates: &[&str],
    ) -> Result<(), McpToolError> {
        // read_jsonl enforces crate-rooted path containment; keep fixtures inside
        // the allowed root like the qa_batch test fixtures do.
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa-grounding-test");
        std::fs::create_dir_all(&root).expect("test fixture root");
        let directory = tempfile::Builder::new()
            .prefix("case-")
            .tempdir_in(root)
            .expect("test directory");
        let report_path = directory.path().join("grounding.jsonl");
        let chunks_path = directory.path().join("chunks.jsonl");
        let write = |path: &std::path::Path, rows: &[Value]| {
            let body = rows
                .iter()
                .map(|row| serde_json::to_string(row).expect("serialize row"))
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(path, format!("{body}\n")).expect("write jsonl");
        };
        write(&report_path, report_rows);
        write(&chunks_path, chunk_rows);
        let qas: Vec<ParsedQa> = candidates
            .iter()
            .map(|line| parse_qa_record(line).expect("parseable candidate"))
            .collect();
        verify_complete_grounding(
            &report_path.to_string_lossy(),
            &chunks_path.to_string_lossy(),
            &qas,
        )
    }

    fn assert_rejected(
        name: &str,
        report_rows: &[Value],
        chunk_rows: &[Value],
        candidates: &[&str],
        fragment: &str,
    ) {
        let error = grounding(report_rows, chunk_rows, candidates)
            .err()
            .unwrap_or_else(|| panic!("{name} must fail closed"));
        assert!(
            error.message.contains(fragment),
            "{name}: expected '{fragment}' in '{}'",
            error.message
        );
    }

    /// expect: A complete, exact-identity, decoupled manifest accepts every candidate.
    #[test]
    fn complete_valid_manifest_accepts_every_candidate() {
        let first = base_report(&candidate_hash(CANDIDATE));
        let second = json!({
            "protocol": QA_GROUNDING_PROTOCOL,
            "candidate_sha256": candidate_hash(SECOND_CANDIDATE),
            "prompt_id": "qa-2",
            "chunk_ref": "chunk-1",
            "source": "source.txt",
            "qa_type": "conceptual",
            "judgments": [
                judgment("instruction", "Why does the delay persist?", "tool_verified", 2, "The wait was verified twice."),
                judgment("output", "Because the wait was verified twice.", "model_inference", 1, "The wait was verified twice.")
            ],
            "fact_score_breakdown": {"sar":1.0,"cvr":1.0,"hfr":1.0,"nlr":1.0,"claims_checked":2},
            "fact_score": 1.0,
            "confidence_band": "high",
            "decoupling": "spawn_agent",
            "verdict": "accept",
            "findings": []
        });
        assert!(
            grounding(
                &[first, second],
                &[chunk_value()],
                &[CANDIDATE, SECOND_CANDIDATE]
            )
            .is_ok()
        );
    }

    /// expect: Every single-field report defect fails closed with its named reason.
    #[test]
    fn report_row_defects_fail_closed() {
        let hash = candidate_hash(CANDIDATE);
        let cases: Vec<(&str, Box<dyn Fn(&mut Value)>, &str)> = vec![
            (
                "unknown protocol",
                Box::new(|r: &mut Value| {
                    r["protocol"] = json!("prepared-qa-grounding-verification-v2")
                }),
                "unsupported protocol",
            ),
            (
                "single judgment",
                Box::new(|r: &mut Value| {
                    r["judgments"] = json!([judgment(
                        "instruction",
                        "What is the delay?",
                        "tool_verified",
                        2,
                        "The delay is 72 hours."
                    )]);
                    r["fact_score_breakdown"]["claims_checked"] = json!(1);
                }),
                "exactly once",
            ),
            (
                "repeated judgment field",
                Box::new(|r: &mut Value| r["judgments"][1]["field"] = json!("instruction")),
                "repeats judgment field",
            ),
            (
                "unsupported judgment field",
                Box::new(|r: &mut Value| r["judgments"][1]["field"] = json!("question")),
                "unsupported judgment field",
            ),
            (
                "judgment text drift",
                Box::new(|r: &mut Value| {
                    r["judgments"][0]["text"] = json!("What is the delay now?")
                }),
                "judgment text does not match candidate field",
            ),
            (
                "short rationale",
                Box::new(|r: &mut Value| r["judgments"][0]["why"] = json!("too short")),
                "40-character rationale",
            ),
            (
                "entailment false",
                Box::new(|r: &mut Value| r["judgments"][0]["entailment"] = json!(false)),
                "requires entailment",
            ),
            (
                "unlisted provenance",
                Box::new(|r: &mut Value| r["judgments"][0]["provenance"] = json!("unavailable")),
                "invalid provenance strength",
            ),
            (
                "wrong strength",
                Box::new(|r: &mut Value| r["judgments"][0]["strength"] = json!(1)),
                "invalid provenance strength",
            ),
            (
                "invalid anchor tier",
                Box::new(|r: &mut Value| {
                    r["judgments"][0]["ontology_anchor"]["tier"] = json!("speculative")
                }),
                "no valid ontology anchor",
            ),
            (
                "blank anchor term",
                Box::new(|r: &mut Value| r["judgments"][0]["ontology_anchor"]["term"] = json!(" ")),
                "no valid ontology anchor",
            ),
            (
                "quote outside candidate evidence",
                Box::new(|r: &mut Value| {
                    r["judgments"][0]["source_reference"]["quote"] =
                        json!("The wait was verified twice.")
                }),
                "candidate's exact evidence quotes",
            ),
            (
                "claims_checked mismatch",
                Box::new(|r: &mut Value| r["fact_score_breakdown"]["claims_checked"] = json!(3)),
                "claims_checked does not match",
            ),
            (
                "out-of-range ratio",
                Box::new(|r: &mut Value| r["fact_score_breakdown"]["sar"] = json!(1.5)),
                "finite non-null ratios",
            ),
            (
                "weight mismatch",
                Box::new(|r: &mut Value| r["fact_score"] = json!(0.9)),
                "does not match the canonical weights",
            ),
            (
                "fact_score below threshold",
                Box::new(|r: &mut Value| {
                    r["fact_score_breakdown"] =
                        json!({"sar":0.5,"cvr":0.5,"hfr":0.5,"nlr":0.5,"claims_checked":2});
                    r["fact_score"] = json!(0.5);
                }),
                "is below 0.8",
            ),
            (
                "non-accept verdict",
                Box::new(|r: &mut Value| r["verdict"] = json!("reject")),
                "not a clean decoupled acceptance",
            ),
            (
                "nonempty findings",
                Box::new(|r: &mut Value| r["findings"] = json!(["subject drift"])),
                "not a clean decoupled acceptance",
            ),
            (
                "in-thread decoupling",
                Box::new(|r: &mut Value| r["decoupling"] = json!("in_thread")),
                "not a clean decoupled acceptance",
            ),
            (
                "flagged confidence band",
                Box::new(|r: &mut Value| r["confidence_band"] = json!("flagged")),
                "not a clean decoupled acceptance",
            ),
        ];
        for (name, mutate, fragment) in cases {
            let mut report = base_report(&hash);
            mutate(&mut report);
            assert_rejected(name, &[report], &[chunk_value()], &[CANDIDATE], fragment);
        }
    }

    /// expect: Manifest structure — counts, duplicates, identity, and canonical
    /// source bytes — fails closed before any acceptance.
    #[test]
    fn manifest_structure_defects_fail_closed() {
        let hash = candidate_hash(CANDIDATE);
        let report = base_report(&hash);
        assert_rejected(
            "missing report row",
            &[],
            &[chunk_value()],
            &[CANDIDATE],
            "covers 0 of 1",
        );
        let mut extra = base_report(&candidate_hash(SECOND_CANDIDATE));
        extra["prompt_id"] = json!("qa-1");
        extra["qa_type"] = json!("factual");
        assert_rejected(
            "extra report row",
            &[report.clone(), extra],
            &[chunk_value()],
            &[CANDIDATE],
            "covers 2 of 1",
        );
        assert_rejected(
            "duplicate candidate hash",
            &[report.clone(), report],
            &[chunk_value()],
            &[CANDIDATE, SECOND_CANDIDATE],
            "Duplicate grounding candidate_sha256",
        );
        let mut identity = base_report(&hash);
        identity["prompt_id"] = json!("qa-2");
        assert_rejected(
            "identity mismatch",
            &[identity],
            &[chunk_value()],
            &[CANDIDATE],
            "does not match candidate identity",
        );
        assert_rejected(
            "stale candidate line",
            &[base_report(&hash)],
            &[chunk_value()],
            &[STALE_CANDIDATE],
            "No grounding report matches candidate SHA-256",
        );
        // Identity matches the candidate; only the judgments' chunk_ref is
        // unknown to the canonical chunks file.
        let mut unanchored = base_report(&candidate_hash(UNANCHORED_CANDIDATE));
        unanchored["prompt_id"] = json!("qa-3");
        unanchored["chunk_ref"] = json!("chunk-2");
        for index in 0..2 {
            unanchored["judgments"][index]["source_reference"]["chunk_ref"] = json!("chunk-2");
            unanchored["judgments"][index]["source_reference"]["quote"] =
                json!("The delay is 72 hours, sharp.");
        }
        assert_rejected(
            "unknown chunk_ref",
            &[unanchored],
            &[chunk_value()],
            &[UNANCHORED_CANDIDATE],
            "references unknown chunk_ref",
        );
        assert_rejected(
            "duplicate canonical chunk",
            &[base_report(&hash)],
            &[chunk_value(), chunk_value()],
            &[CANDIDATE],
            "Duplicate canonical chunk_ref",
        );
        let mut wrong_source = chunk_value();
        wrong_source["source"] = json!("other.txt");
        assert_rejected(
            "wrong chunk source",
            &[base_report(&hash)],
            &[wrong_source],
            &[CANDIDATE],
            "does not match canonical source bytes",
        );
        let mut truncated = chunk_value();
        truncated["text"] = json!("The delay is not stated here.");
        assert_rejected(
            "quote missing from canonical text",
            &[base_report(&hash)],
            &[truncated],
            &[CANDIDATE],
            "does not match canonical source bytes",
        );
    }

    /// expect: Report rows are closed shapes — extra fields are malformed input,
    /// never silently ignored.
    #[test]
    fn unknown_report_field_is_rejected() {
        let mut report = base_report(&candidate_hash(CANDIDATE));
        report["verifier_model"] = json!("OpenRouter/offline-verifier");
        assert_rejected(
            "unknown report field",
            &[report],
            &[chunk_value()],
            &[CANDIDATE],
            "is not valid JSON",
        );
    }
}
