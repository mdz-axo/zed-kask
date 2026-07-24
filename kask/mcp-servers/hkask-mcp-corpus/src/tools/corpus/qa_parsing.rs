//! QA record parsing — flat and envelope format support.
//!
//! Used by `docproc_ingest_qa` in `mod.rs` to parse generated QA JSONL.

/// A parsed QA record from a JSONL line. Handles both flat and envelope formats.
pub(crate) struct ParsedQa {
    pub instruction: String,
    pub output: String,
    pub qa_type: String,
    pub difficulty: usize,
    pub concepts: Vec<String>,
    pub source: String,
    pub chunk_ref: Option<String>,
    pub evidence_quotes: Vec<String>,
}

/// Parse a QA record from a JSONL line. Handles both flat and envelope formats.
///
/// Flat format: `{"instruction": ..., "output": ..., "qa_type": ...}`
/// Envelope format: `{"chunk_ref": ..., "source": ..., "qa_type": ..., "response": {...}}`
pub(crate) fn parse_qa_record(line: &str) -> Option<ParsedQa> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
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
    if instruction.is_empty() || output.is_empty() {
        return None;
    }
    Some(ParsedQa {
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
