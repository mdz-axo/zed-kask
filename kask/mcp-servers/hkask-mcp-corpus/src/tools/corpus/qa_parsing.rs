//! QA record parsing — flat and envelope format support.
//!
//! Used by `corpus_ingest_qa` in `tools/corpus.rs` to parse generated QA JSONL.

/// A parsed QA record from a JSONL line. Handles both flat and envelope formats.
pub(crate) struct ParsedQa {
    pub instruction: String,
    pub output: String,
    pub qa_type: String,
    pub response_type: Option<String>,
    pub difficulty: usize,
    pub concepts: Vec<String>,
    pub source: String,
    pub chunk_ref: Option<String>,
    pub evidence_quotes: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum QaRecordError {
    GeneratorError,
    Malformed,
}

/// Parse object-shaped QA records; missing or blank fields are counted by the
/// caller's structural filter, not confused with malformed JSON or generator failures.
///
/// Flat format: `{"instruction": ..., "output": ..., "qa_type": ...}`
/// Envelope format: `{"chunk_ref": ..., "source": ..., "qa_type": ..., "response": {...}}`
pub(crate) fn parse_qa_record(line: &str) -> Result<ParsedQa, QaRecordError> {
    let v: serde_json::Value = serde_json::from_str(line).map_err(|_| QaRecordError::Malformed)?;
    if !v.is_object() {
        return Err(QaRecordError::Malformed);
    }
    if v.get("error").is_some_and(|error| !error.is_null()) {
        return Err(QaRecordError::GeneratorError);
    }
    if v.get("response")
        .is_some_and(|response| !response.is_object())
    {
        return Err(QaRecordError::Malformed);
    }
    let (instruction, output, qa_type, difficulty, concepts, source, chunk_ref, evidence_quotes) =
        if let Some(resp) = v.get("response").and_then(|r| r.as_object()) {
            // Envelope format
            (
                resp.get("instruction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                resp.get("output")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("qa_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                resp.get("difficulty").and_then(|v| v.as_u64()).unwrap_or(3) as usize,
                resp.get("concepts")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                v.get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("chunk_ref")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                resp.get("evidence_quotes")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            )
        } else {
            // Flat format
            (
                v.get("instruction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("output")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("qa_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("difficulty").and_then(|v| v.as_u64()).unwrap_or(3) as usize,
                v.get("concepts")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                v.get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("chunk_ref")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                v.get("evidence_quotes")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            )
        };
    let response_type = v
        .get("response")
        .unwrap_or(&v)
        .get("type")
        .and_then(|value| value.as_str())
        .map(String::from);
    Ok(ParsedQa {
        response_type,
        instruction,
        output,
        qa_type,
        difficulty,
        concepts,
        source,
        chunk_ref,
        evidence_quotes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::qa_pipeline::{
        PreparedQaPrompt, QaCompletion, QaCompletionError, QaOutput,
    };

    /// expect: [P8] Generated QA rows remain ingestible with their source metadata; failure rows are never training data.
    #[test]
    fn shared_qa_output_is_ingest_compatible() -> Result<(), Box<dyn std::error::Error>> {
        let prompt = PreparedQaPrompt {
            prompt_id: "qa-1".into(),
            chunk_ref: "chunk-1".into(),
            source: "source.txt".into(),
            concepts: vec!["concept".into()],
            salience: 0.5,
            qa_type: "factual".into(),
            system: "prepared system".into(),
            user: "prepared user".into(),
        };
        let mut bytes = Vec::new();
        let mut output = QaOutput::new(&mut bytes, 2);
        output.complete(&prompt, Ok(QaCompletion {
            text: r#"{"qa_pairs":[{"question":"Question?", "answer":"Answer.", "bloom_level":"factual"}]}"#.into(), tokens_used: 10,
        }), "offline-model")?;
        let failed = PreparedQaPrompt {
            prompt_id: "qa-2".into(),
            ..prompt.clone()
        };
        output.complete(
            &failed,
            Err(QaCompletionError::BatchProvider("failed inference".into())),
            "offline-model",
        )?;
        let summary = output.finish("unused", false)?;
        assert_eq!(summary["qa_rows_written"], 1);
        let text = String::from_utf8(bytes)?;
        let mut lines = text.lines();
        let first = lines.next().expect("QA row");
        let parsed = parse_qa_record(first).expect("ingest accepted QA");
        assert_eq!(parsed.instruction, "Question?");
        assert_eq!(parsed.output, "Answer.");
        assert_eq!(parsed.qa_type, prompt.qa_type);
        assert_eq!(parsed.source, prompt.source);
        assert_eq!(parsed.chunk_ref.as_deref(), Some(prompt.chunk_ref.as_str()));
        assert_eq!(parsed.concepts, prompt.concepts);
        let value: serde_json::Value = serde_json::from_str(first)?;
        assert_eq!(value["provenance"]["prompt_id"], prompt.prompt_id);
        assert!(matches!(
            parse_qa_record(lines.next().expect("failure row")),
            Err(QaRecordError::GeneratorError)
        ));
        assert!(lines.next().is_none());
        Ok(())
    }
}
