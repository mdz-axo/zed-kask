//! Normalize the two legitimate QA I/O shapes before validating metadata.
//! Citation strings and numeric passage indices are not accepted formats.

use hkask_types::corpus::QaEvidence;
use serde::Deserialize;

pub(crate) struct ParsedQa {
    pub instruction: String,
    pub output: String,
    pub qa_type: String,
    pub response_type: Option<String>,
    pub difficulty: usize,
    pub concepts: Vec<String>,
    pub source: String,
    pub chunk_ref: Option<String>,
    pub evidence_quotes: Vec<QaEvidence>,
    pub prompt_id: Option<String>,
    pub provenance: Option<serde_json::Value>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum QaRecordError {
    GeneratorError,
    Malformed,
}

#[derive(Deserialize)]
struct Body {
    #[serde(default)]
    instruction: String,
    #[serde(default)]
    output: String,
    #[serde(rename = "type")]
    response_type: Option<String>,
    difficulty: Option<usize>,
    #[serde(default)]
    concepts: Vec<String>,
    evidence_quotes: Vec<QaEvidence>,
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(default)]
    qa_type: String,
    #[serde(default)]
    source: String,
    chunk_ref: Option<String>,
    prompt_id: Option<String>,
    provenance: Option<serde_json::Value>,
}

/// Envelope metadata is outside `response`; flat training metadata is beside
/// `instruction`/`output`. Both use exactly the same citation and metadata
/// validation. Ingest checks structure only; audit verifies against sources.
pub(crate) fn parse_qa_record(line: &str) -> Result<ParsedQa, QaRecordError> {
    let value: serde_json::Value =
        serde_json::from_str(line).map_err(|_| QaRecordError::Malformed)?;
    if !value.is_object() {
        return Err(QaRecordError::Malformed);
    }
    let body = value.get("response").unwrap_or(&value);
    if [value.get("error"), body.get("error")]
        .into_iter()
        .flatten()
        .any(|error| !error.is_null())
    {
        return Err(QaRecordError::GeneratorError);
    }
    let body: Body = serde_json::from_value(body.clone()).map_err(|_| QaRecordError::Malformed)?;
    let metadata: Metadata = serde_json::from_value(value).map_err(|_| QaRecordError::Malformed)?;
    if body
        .evidence_quotes
        .iter()
        .any(|citation| !citation.is_complete())
        || body
            .concepts
            .iter()
            .any(|concept| concept.trim().is_empty())
        || metadata
            .prompt_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
        || metadata
            .provenance
            .as_ref()
            .is_some_and(|value| !value.is_object())
    {
        return Err(QaRecordError::Malformed);
    }
    Ok(ParsedQa {
        instruction: body.instruction,
        output: body.output,
        qa_type: metadata.qa_type,
        response_type: body.response_type,
        difficulty: body.difficulty.unwrap_or(3),
        concepts: body.concepts,
        source: metadata.source,
        chunk_ref: metadata.chunk_ref,
        evidence_quotes: body.evidence_quotes,
        prompt_id: metadata.prompt_id,
        provenance: metadata.provenance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::qa_pipeline::{
        PREPARED_QA_PROTOCOL, PreparedQaPassage, PreparedQaPrompt, QaCompletion, QaCompletionError,
        QaOutput,
    };
    use crate::tools::corpus::QaType;
    use serde_json::json;

    /// expect: Generation retains distinct source identities and quotes in both
    /// legitimate ingest shapes; identified inference errors are never QA.
    #[test]
    fn shared_qa_output_is_ingest_compatible() -> Result<(), Box<dyn std::error::Error>> {
        let prompt = PreparedQaPrompt {
            prompt_id: "qa-1".into(),
            protocol: PREPARED_QA_PROTOCOL.into(),
            passages: vec![
                PreparedQaPassage {
                    local_id: "p0".into(),
                    chunk_ref: "chunk-1".into(),
                    source: "source.txt".into(),
                    text: "Answer.".into(),
                },
                PreparedQaPassage {
                    local_id: "p1".into(),
                    chunk_ref: "chunk-2".into(),
                    source: "other.txt".into(),
                    text: "Other evidence.".into(),
                },
            ],
            candidate_terms: vec!["concept".into()],
            qa_types: vec![QaType::Factual],
        };
        let quotes = json!([
            {"chunk_ref":"chunk-1", "source":"source.txt", "quote":"Answer."},
            {"chunk_ref":"chunk-2", "source":"other.txt", "quote":"Other evidence."}
        ]);
        let mut bytes = Vec::new();
        let mut output = QaOutput::new(&mut bytes, 2);
        output.complete(
            &prompt,
            Ok(QaCompletion {
                text: json!([[
                    "factual",
                    "Question?",
                    "Answer.",
                    [["p0", "Answer."], ["p1", "Other evidence."]]
                ]])
                .to_string(),
                tokens_used: 10,
            }),
            "offline-model",
        )?;
        let failed = PreparedQaPrompt {
            prompt_id: "qa-2".into(),
            ..prompt.clone()
        };
        output.complete(
            &failed,
            Err(QaCompletionError::BatchProvider("failed inference".into())),
            "offline-model",
        )?;
        assert_eq!(output.finish("unused", false)?["qa_rows_written"], 1);
        let text = String::from_utf8(bytes)?;
        let mut lines = text.lines();
        let first = lines.next().ok_or("missing QA row")?;
        let parsed = parse_qa_record(first).map_err(|_| "ingest rejected QA")?;
        assert_eq!(parsed.instruction, "Question?");
        assert_eq!(parsed.output, "Answer.");
        assert_eq!(parsed.qa_type, "factual");
        assert_eq!(parsed.source, prompt.primary().source);
        assert_eq!(
            parsed.chunk_ref.as_deref(),
            Some(prompt.primary().chunk_ref.as_str())
        );
        assert_eq!(parsed.concepts, prompt.candidate_terms);
        assert_eq!(serde_json::to_value(parsed.evidence_quotes)?, quotes);
        assert_eq!(parsed.prompt_id.as_deref(), Some("qa-1"));
        assert_eq!(
            parsed.provenance.as_ref().ok_or("missing provenance")?["generator_model"],
            "offline-model"
        );
        let value: serde_json::Value = serde_json::from_str(first)?;
        let mut flat = value["response"].clone();
        for key in ["chunk_ref", "source", "qa_type", "prompt_id", "provenance"] {
            flat[key] = value[key].clone();
        }
        let flat = parse_qa_record(&flat.to_string()).map_err(|_| "flat rejected")?;
        assert_eq!(serde_json::to_value(flat.evidence_quotes)?, quotes);
        assert_eq!(flat.provenance, parsed.provenance);
        assert!(matches!(
            parse_qa_record(lines.next().ok_or("missing failure row")?),
            Err(QaRecordError::GeneratorError)
        ));
        assert!(lines.next().is_none());
        Ok(())
    }

    /// expect: No missing/old/malformed citation shape silently becomes evidence.
    #[test]
    fn rejects_obsolete_or_incomplete_citations() {
        for quotes in [
            json!(["quote"]),
            json!([1]),
            json!([{"quote":"quote"}]),
            json!([{"chunk_ref":"c", "source":" ", "quote":"q"}]),
            json!(null),
        ] {
            let row = json!({"instruction":"Q?", "output":"A", "qa_type":"factual",
                "chunk_ref":"c", "source":"s", "evidence_quotes":quotes});
            assert!(matches!(
                parse_qa_record(&row.to_string()),
                Err(QaRecordError::Malformed)
            ));
        }
    }
}
