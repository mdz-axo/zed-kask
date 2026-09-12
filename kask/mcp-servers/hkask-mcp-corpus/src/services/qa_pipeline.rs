//! Canonical prepared QA records and completion/output accounting for both
//! batch transports. Single-chunk generation retains its own prompt formatter.

use std::collections::{HashMap, HashSet};
use std::io::Write;

use hkask_types::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::batch::BatchOutcome;
use crate::helpers::{map_corpus_io_error, read_jsonl};
use crate::tools::corpus::{QaType, qa_type_instruction};
use crate::tools::semantic::qa::QaPair;
use crate::{
    CONTENT_GUARD_INSTRUCTION, McpToolError, extract_json_from_response, render_docproc_template,
};

/// Canonical model response shape. Empty evidence means no source citation;
/// quotes never confer semantic verification on ordinary answer prose.
pub(crate) const QA_RESPONSE_CONTRACT: &str = r#"Respond only in JSON: {"qa_pairs":[{"question":"...","answer":"...","bloom_level":"factual|conceptual|analyze|evaluate|create","evidence_quotes":[{"chunk_ref":"exact supplied chunk ID","source":"exact supplied source ID","quote":"exact nonempty substring"}]}]}. Use only the requested Bloom levels. Question and answer must be nonblank; concise answers are valid. Cite only supplied source/chunk identities, never fabricate them. If source identity is unavailable, return evidence_quotes: []. No numeric passage citations or bare quoted-string arrays. A matching citation does not verify answer synthesis."#;

pub(crate) const PREPARED_QA_PROTOCOL: &str = "prepared-qa-local-evidence-v1";

/// One server-owned passage identity. Only `local_id` and guarded `text` enter
/// the model prompt; canonical identity is restored after quote verification.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreparedQaPassage {
    pub local_id: String,
    pub chunk_ref: String,
    pub source: String,
    pub text: String,
}

/// Compact prepared request. Deterministic rendering replaces repeated stored
/// system/user messages; one request may produce several Bloom-level pairs.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreparedQaPrompt {
    pub prompt_id: String,
    pub protocol: String,
    pub passages: Vec<PreparedQaPassage>,
    pub candidate_terms: Vec<String>,
    pub qa_types: Vec<QaType>,
}

impl PreparedQaPrompt {
    pub fn validate(&self) -> Result<(), McpToolError> {
        if self.protocol != PREPARED_QA_PROTOCOL {
            return Err(McpToolError::invalid_argument(format!(
                "Prepared QA prompt '{}' has unsupported protocol '{}'",
                self.prompt_id, self.protocol
            )));
        }
        if self.prompt_id.is_empty()
            || self.prompt_id.len() > 64
            || !self
                .prompt_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(McpToolError::invalid_argument(
                "prompt_id must be 1–64 ASCII letters, digits, hyphens or underscores",
            ));
        }
        if self.passages.is_empty() || self.qa_types.is_empty() {
            return Err(McpToolError::invalid_argument(format!(
                "Prepared QA prompt '{}' needs passages and qa_types",
                self.prompt_id
            )));
        }
        let mut local_ids = HashSet::new();
        for (index, passage) in self.passages.iter().enumerate() {
            if passage.local_id != format!("p{index}")
                || passage.chunk_ref.trim().is_empty()
                || passage.source.trim().is_empty()
                || passage.text.trim().is_empty()
                || !local_ids.insert(&passage.local_id)
            {
                return Err(McpToolError::invalid_argument(format!(
                    "Prepared QA prompt '{}' has invalid passage {index}",
                    self.prompt_id
                )));
            }
        }
        if self
            .candidate_terms
            .iter()
            .any(|term| term.trim().is_empty())
        {
            return Err(McpToolError::invalid_argument(format!(
                "Prepared QA prompt '{}' has an empty candidate term",
                self.prompt_id
            )));
        }
        Ok(())
    }

    pub fn primary(&self) -> &PreparedQaPassage {
        &self.passages[0]
    }
}

/// Render the one canonical model request used by both QA transports.
pub(crate) fn render_prepared_messages(
    prompt: &PreparedQaPrompt,
) -> Result<[ChatMessage; 2], McpToolError> {
    prompt.validate()?;
    let instructions = prompt
        .qa_types
        .iter()
        .map(|qa_type| format!("{}: {}", qa_type.as_str(), qa_type_instruction(*qa_type)))
        .collect::<Vec<_>>()
        .join("\n");
    let passages = prompt
        .passages
        .iter()
        .map(|passage| {
            json!({
                "id": passage.local_id,
                "text": crate::guard_content(&passage.text),
            })
        })
        .collect::<Vec<_>>();
    let user = serde_json::to_string(&json!({
        "requested_levels": prompt.qa_types,
        "candidate_terms": prompt.candidate_terms,
        "passages": passages,
    }))
    .map_err(|error| McpToolError::internal(format!("Cannot render prepared QA: {error}")))?;
    let system = format!(
        "Generate exactly {} source-grounded QA pairs, one per requested level in the supplied order.\n{}\nUse p0 as the primary passage; other local passages are context only. Every pair needs at least one exact nonempty quote. Return only JSON tuples: [[\"level\",\"question\",\"answer\",[[\"p0\",\"exact quote\"]]]]. Local passage IDs are mandatory; never emit canonical source or chunk identities.",
        prompt.qa_types.len(),
        instructions
    );
    Ok([
        ChatMessage {
            role: "system".to_string(),
            content: system,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user,
        },
    ])
}

#[derive(Deserialize)]
struct PreparedQaPair(String, String, String, Vec<(String, String)>);

fn parse_prepared_qa_response(
    response: &str,
    prompt: &PreparedQaPrompt,
) -> Result<Vec<QaPair>, String> {
    let raw: Vec<PreparedQaPair> = serde_json::from_str(response)
        .map_err(|error| format!("invalid compact QA JSON: {error}"))?;
    if raw.len() != prompt.qa_types.len() {
        return Err(format!(
            "expected {} QA pairs, received {}",
            prompt.qa_types.len(),
            raw.len()
        ));
    }
    let passages = prompt
        .passages
        .iter()
        .map(|passage| (passage.local_id.as_str(), passage))
        .collect::<HashMap<_, _>>();
    raw.into_iter()
        .zip(&prompt.qa_types)
        .enumerate()
        .map(
            |(index, (PreparedQaPair(level, question, answer, citations), expected))| {
                if level != expected.as_str() {
                    return Err(format!(
                        "pair {index} expected Bloom level '{}', received '{level}'",
                        expected.as_str()
                    ));
                }
                if question.trim().is_empty() || answer.trim().is_empty() || citations.is_empty() {
                    return Err(format!(
                        "pair {index} needs nonblank question, answer, and evidence"
                    ));
                }
                let mut evidence_quotes = Vec::with_capacity(citations.len());
                for (local_id, quote) in citations {
                    let passage = passages.get(local_id.as_str()).ok_or_else(|| {
                        format!("pair {index} cites unknown passage '{local_id}'")
                    })?;
                    if quote.trim().is_empty() || !passage.text.contains(&quote) {
                        return Err(format!(
                            "pair {index} quote is not an exact substring of '{local_id}'"
                        ));
                    }
                    evidence_quotes.push(hkask_types::corpus::QaEvidence {
                        chunk_ref: passage.chunk_ref.clone(),
                        source: passage.source.clone(),
                        quote,
                    });
                }
                Ok(QaPair {
                    question,
                    answer,
                    bloom_level: level,
                    evidence_quotes,
                })
            },
        )
        .collect()
}

/// expect: Every prepared instruction is validated before any paid inference.
/// [P8] Motivating: Reject ambiguous identities instead of losing prompt provenance.
/// pre: path is a contained JSONL input.
/// post: all records are canonical, nonempty and uniquely identified; repeated chunks are valid.
/// [P1] Constraining: Preserve the user's prepared instructions and source metadata.
/// [P4] Constraining: Read only through the corpus path boundary.
pub(crate) fn read_prompts(path: &str) -> Result<Vec<PreparedQaPrompt>, McpToolError> {
    let prompts: Vec<PreparedQaPrompt> = read_jsonl(path, "prompts_jsonl")?;
    if prompts.is_empty() {
        return Err(McpToolError::invalid_argument(
            "prompts_jsonl contains no prompts",
        ));
    }
    let mut identities = HashSet::with_capacity(prompts.len());
    for prompt in &prompts {
        prompt.validate()?;
        if !identities.insert(&prompt.prompt_id) {
            return Err(McpToolError::invalid_argument(format!(
                "Duplicate prompt_id '{}'",
                prompt.prompt_id
            )));
        }
    }
    Ok(prompts)
}

pub(crate) struct QaCompletion {
    pub text: String,
    pub tokens_used: u64,
}

/// Per-prompt inference failure to produce a QA completion. Recorded via
/// Display in the output record for later inspection — downstream code
/// never matches variants.
#[derive(Debug, thiserror::Error)]
pub(crate) enum QaCompletionError {
    #[error("Batch API returned no result for prompt")]
    BatchNoResult,
    #[error("Batch provider error: {0}")]
    BatchProvider(String),
    #[error("Malformed batch result: expected exactly one of text or error")]
    BatchMalformed,
    #[error("Batch API returned {0} duplicate results for prompt")]
    BatchDuplicates(usize),
    #[error("LLM failed after {0} retries: {1}")]
    LlmFailed(usize, String),
    #[error("QA task join failed: {0}")]
    JoinFailed(String),
    #[error("QA response rejected: {0}")]
    Rejected(String),
}

/// One owner writes completions as they arrive; neither transport owns counts
/// or swallows output failures. Generic Write permits real I/O failure tests.
pub(crate) struct QaOutput<W: Write> {
    writer: W,
    prompts_total: usize,
    prompts_succeeded: usize,
    prompts_failed: usize,
    qa_rows_written: usize,
    tokens_used: u64,
}

impl<W: Write> QaOutput<W> {
    pub fn new(writer: W, prompts_total: usize) -> Self {
        Self {
            writer,
            prompts_total,
            prompts_succeeded: 0,
            prompts_failed: 0,
            qa_rows_written: 0,
            tokens_used: 0,
        }
    }

    fn write_record(&mut self, record: &serde_json::Value) -> Result<(), McpToolError> {
        let bytes = serde_json::to_vec(record).map_err(|error| {
            McpToolError::internal(format!("Cannot serialize QA output: {error}"))
        })?;
        self.writer
            .write_all(&bytes)
            .map_err(|error| map_corpus_io_error(error, "Cannot write QA output"))?;
        self.writer
            .write_all(b"\n")
            .map_err(|error| map_corpus_io_error(error, "Cannot write QA output newline"))?;
        Ok(())
    }

    /// expect: A success count means accepted QA rows were written, not merely attempted.
    /// [P9] Motivating: Every prompt gets one truthful terminal outcome.
    /// pre: prompt was validated and is completed exactly once by its transport.
    /// post: malformed or failed inference emits an identified error row; output failures propagate.
    pub fn complete(
        &mut self,
        prompt: &PreparedQaPrompt,
        completion: Result<QaCompletion, QaCompletionError>,
        model: &str,
    ) -> Result<(), McpToolError> {
        let parsed = match completion {
            Ok(completion) => {
                self.tokens_used += completion.tokens_used;
                parse_prepared_qa_response(&extract_json_from_response(&completion.text), prompt)
                    .map_err(QaCompletionError::Rejected)
            }
            Err(error) => Err(error),
        };
        match parsed {
            Ok(pairs) => {
                for pair in pairs {
                    self.write_record(&qa_result_envelope(prompt, pair, model))?;
                    self.qa_rows_written += 1;
                }
                self.prompts_succeeded += 1;
            }
            Err(error) => {
                self.write_record(&json!({
                    "prompt_id": prompt.prompt_id,
                    "chunk_ref": prompt.primary().chunk_ref,
                    "source": prompt.primary().source,
                    "error": error.to_string(),
                }))?;
                self.prompts_failed += 1;
            }
        }
        if (self.prompts_succeeded + self.prompts_failed).is_multiple_of(10) {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), McpToolError> {
        self.writer
            .flush()
            .map_err(|error| map_corpus_io_error(error, "Cannot flush QA output"))
    }

    pub fn finish(
        mut self,
        output: &str,
        batch_api: bool,
    ) -> Result<serde_json::Value, McpToolError> {
        self.flush()?;
        if self.prompts_succeeded + self.prompts_failed != self.prompts_total {
            return Err(McpToolError::internal(
                "QA completion accounting does not match prompts_total",
            ));
        }
        let outcome = BatchOutcome::from_counts(self.prompts_failed, self.prompts_total);
        outcome.log_if_degraded("hkask.mcp.docproc.qa_batch", "QA batch");
        Ok(json!({
            "prompts_total": self.prompts_total,
            "prompts_succeeded": self.prompts_succeeded,
            "prompts_failed": self.prompts_failed,
            "qa_rows_written": self.qa_rows_written,
            "tokens_used": self.tokens_used,
            "output": output,
            "batch_api": batch_api,
            "degraded": BatchOutcome::is_degraded(self.prompts_failed, self.prompts_total),
        }))
    }
}

/// The LLM parameters used by all QA generation paths.
///
/// Single source of truth — previously duplicated in `corpus_generate_qa`
/// and the synchronous batch path with identical values.
pub(crate) fn qa_llm_parameters() -> hkask_types::template::LLMParameters {
    hkask_types::template::LLMParameters {
        temperature: 0.3,
        top_p: 0.95,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
        top_k: 0,
        min_p: 0.0,
        typical_p: 0.0,
        thinking_allowed: false,
        ..Default::default()
    }
}

/// Default Bloom's taxonomy levels when none are specified.
pub(crate) fn default_bloom_levels() -> Vec<String> {
    vec!["factual".to_string(), "conceptual".to_string()]
}

/// A formatted QA generation prompt.
pub(crate) struct FormattedQaPrompt {
    /// The full prompt text (template-rendered or inline fallback).
    pub text: String,
    /// The provenance identifier for the template used.
    pub template_source: &'static str,
}

/// Format a single-chunk QA generation prompt.
///
/// Renders the `generate-qa` docproc template with the chunk's levels, id,
/// and text. Falls back to an inline prompt when the template is unavailable.
/// The text is NOT content-guarded here — callers guard their own input
/// (the single-chunk path guards, the batch paths receive pre-composed text).
pub(crate) fn format_single_chunk_prompt(
    levels_str: &str,
    chunk_id: &str,
    text: &str,
) -> FormattedQaPrompt {
    let mut vars: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    vars.insert("levels", levels_str.to_string());
    vars.insert("chunk_id", chunk_id.to_string());
    vars.insert("text", text.to_string());
    vars.insert("response_contract", QA_RESPONSE_CONTRACT.to_string());
    let tpl = render_docproc_template("generate-qa", &vars);
    if tpl.is_empty() {
        FormattedQaPrompt {
            text: format!(
                "{CONTENT_GUARD_INSTRUCTION}Based on the following text, generate question-answer pairs at these Bloom's taxonomy levels: {levels_str}.\n\nText (chunk {chunk_id}; source identity unavailable):\n{text}\n\nReturn evidence_quotes: [] because no source identity was supplied.\n{QA_RESPONSE_CONTRACT}",
            ),
            template_source: "inline-fallback",
        }
    } else {
        FormattedQaPrompt {
            text: tpl,
            template_source: "registry/templates/docproc/generate-qa.j2",
        }
    }
}

/// Format a cross-reference QA generation prompt (multi-passage synthesis).
///
/// Used only by `corpus_generate_qa` when `texts` is provided. Always inline
/// — there is no template for the cross-reference format.
pub(crate) fn format_cross_reference_prompt(
    levels_str: &str,
    chunk_id: &str,
    passages: &[String],
) -> FormattedQaPrompt {
    let mut text = String::new();
    for (i, p) in passages.iter().enumerate() {
        text.push_str(&format!(
            "[Chunk group {chunk_id}, passage {}; source identity unavailable]\n{}\n\n",
            i + 1,
            crate::guard_content(p)
        ));
    }
    FormattedQaPrompt {
        text: format!(
            "{CONTENT_GUARD_INSTRUCTION}You are synthesizing knowledge across {n} passages.\n\nGenerate question-answer pairs at these Bloom's taxonomy levels: {levels_str}.\n\nThe questions should require synthesizing information from MULTIPLE passages — compare, contrast, diagnose patterns, or trace causal connections.\n\nPassages (chunk group {chunk_id}):\n{text}\n\nSource identities were not supplied. Return evidence_quotes: []; do not turn passage numbers or the group name into invented sources.\n{QA_RESPONSE_CONTRACT}",
            n = passages.len(),
        ),
        template_source: "inline-cross-reference",
    }
}

/// Build the QA result envelope for one QA pair.
///
/// The envelope format matches what `corpus_ingest_qa`'s `parse_qa_record`
/// expects: `chunk_ref`, `source`, `qa_type`, `response`, `provenance`,
/// `tokens_used`.
pub(crate) fn qa_result_envelope(
    prompt: &PreparedQaPrompt,
    pair: QaPair,
    model: &str,
) -> serde_json::Value {
    json!({
        "prompt_id": prompt.prompt_id,
        "chunk_ref": prompt.primary().chunk_ref,
        "source": prompt.primary().source,
        "qa_type": pair.bloom_level,
        "response": {
            "instruction": pair.question,
            "output": pair.answer,
            "type": pair.bloom_level,
            "concepts": prompt.candidate_terms,
            "evidence_quotes": pair.evidence_quotes,
        },
        "provenance": {
            "generator_model": model,
            "prompt_protocol": PREPARED_QA_PROTOCOL,
            "prompt_id": prompt.prompt_id,
            "source_chunk_ref": prompt.primary().chunk_ref,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared() -> PreparedQaPrompt {
        PreparedQaPrompt {
            prompt_id: "qa-1".into(),
            protocol: PREPARED_QA_PROTOCOL.into(),
            passages: vec![PreparedQaPassage {
                local_id: "p0".into(),
                chunk_ref: "chunk-1".into(),
                source: "source.txt".into(),
                text: "The answer is grounded here.".into(),
            }],
            candidate_terms: vec!["grounded answer".into()],
            qa_types: vec![QaType::Factual],
        }
    }

    fn accepted() -> Result<QaCompletion, QaCompletionError> {
        Ok(QaCompletion {
            text: json!([["factual", "Question?", "Answer.", [["p0", "grounded"]]]]).to_string(),
            tokens_used: 10,
        })
    }

    enum Failure {
        Body,
        Newline,
        Flush,
    }

    struct RejectingWriter {
        failure: Failure,
    }

    impl Write for RejectingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if matches!(self.failure, Failure::Body)
                || (matches!(self.failure, Failure::Newline) && bytes == b"\n")
            {
                return Err(std::io::Error::other("injected write failure"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            if matches!(self.failure, Failure::Flush) {
                return Err(std::io::Error::other("injected flush failure"));
            }
            Ok(())
        }
    }

    /// expect: [P9] Body, newline and final flush failures propagate, never returning a success summary.
    #[test]
    fn output_failures_propagate() -> Result<(), McpToolError> {
        for failure in [Failure::Body, Failure::Newline] {
            let mut output = QaOutput::new(RejectingWriter { failure }, 1);
            let error = output
                .complete(&prepared(), accepted(), "offline-model")
                .expect_err("write must fail");
            assert!(error.to_string().contains("injected write failure"));
            assert!(output.finish("unused", false).is_err());
        }
        let mut output = QaOutput::new(
            RejectingWriter {
                failure: Failure::Flush,
            },
            1,
        );
        output.complete(&prepared(), accepted(), "offline-model")?;
        let error = output.finish("unused", false).expect_err("flush must fail");
        assert!(error.to_string().contains("injected flush failure"));
        Ok(())
    }

    /// expect: [P9] Incremental output flush failures and failure-record write failures are equally fatal.
    #[test]
    fn incremental_and_error_output_failures_propagate() -> Result<(), McpToolError> {
        let mut output = QaOutput::new(
            RejectingWriter {
                failure: Failure::Flush,
            },
            10,
        );
        for identity in 1..10 {
            let mut prompt = prepared();
            prompt.prompt_id = format!("qa-{identity}");
            output.complete(&prompt, accepted(), "offline-model")?;
        }
        let error = output
            .complete(&prepared(), accepted(), "offline-model")
            .expect_err("incremental flush must fail");
        assert!(error.to_string().contains("injected flush failure"));
        let mut output = QaOutput::new(
            RejectingWriter {
                failure: Failure::Newline,
            },
            1,
        );
        assert!(
            output
                .complete(
                    &prepared(),
                    Err(QaCompletionError::BatchProvider("provider failure".into())),
                    "offline-model",
                )
                .is_err()
        );
        Ok(())
    }

    /// expect: [P9] Empty or malformed QA is a failed prompt and writes no accepted QA rows.
    #[test]
    fn rejected_qa_outputs_are_accounted_without_partial_acceptance()
    -> Result<(), Box<dyn std::error::Error>> {
        for response in [
            "not JSON",
            "[]",
            r#"[["factual","","answer",[["p0","grounded"]]]]"#,
            r#"[["factual","question"," ",[["p0","grounded"]]]]"#,
            r#"[["create","question","answer",[["p0","grounded"]]]]"#,
            r#"[["factual","question","answer",[["p0","grounded"]]],["factual","extra","answer",[["p0","grounded"]]]]"#,
            r#"[["factual","question","answer",[["p9","grounded"]]]]"#,
            r#"[["factual","question","answer",[["p0","not in passage"]]]]"#,
        ] {
            let mut bytes = Vec::new();
            let mut output = QaOutput::new(&mut bytes, 1);
            output.complete(
                &prepared(),
                Ok(QaCompletion {
                    text: response.into(),
                    tokens_used: 0,
                }),
                "offline-model",
            )?;
            let summary = output.finish("unused", true)?;
            assert_eq!(summary["prompts_total"], 1);
            assert_eq!(summary["prompts_succeeded"], 0);
            assert_eq!(summary["prompts_failed"], 1);
            assert_eq!(summary["qa_rows_written"], 0);
            let row: serde_json::Value = serde_json::from_slice(&bytes)?;
            assert_eq!(row["prompt_id"], "qa-1");
            assert!(row["error"].as_str().expect("error").contains("rejected"));
            assert!(row.get("response").is_none());
        }
        Ok(())
    }

    /// expect: [P9] A transport that loses a completion cannot report a successful run.
    #[test]
    fn unfinished_accounting_is_not_success() {
        let output = QaOutput::new(Vec::new(), 1);
        assert!(output.finish("unused", false).is_err());
    }

    #[test]
    fn qa_llm_parameters_match_historical_values() {
        let params = qa_llm_parameters();
        assert_eq!(params.temperature, 0.3);
        assert_eq!(params.top_p, 0.95);
        assert!(!params.thinking_allowed);
    }

    #[test]
    fn default_bloom_levels_are_factual_and_conceptual() {
        let levels = default_bloom_levels();
        assert_eq!(
            levels,
            vec!["factual".to_string(), "conceptual".to_string()]
        );
    }

    #[test]
    fn prepared_contract_fuses_levels_and_restores_verified_canonical_evidence() {
        let mut prompt = prepared();
        prompt.qa_types = vec![QaType::Factual, QaType::Conceptual];
        let messages = render_prepared_messages(&prompt).expect("render");
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("p0"));
        assert!(!rendered.contains("chunk-1"));
        assert!(!rendered.contains("source.txt"));

        let response = json!([
            [
                "factual",
                "What is grounded?",
                "The answer.",
                [["p0", "answer"]]
            ],
            [
                "conceptual",
                "How is it grounded?",
                "By evidence.",
                [["p0", "grounded"]]
            ]
        ])
        .to_string();
        let pairs = parse_prepared_qa_response(&response, &prompt).expect("parse");
        assert_eq!(pairs.len(), 2);
        for pair in pairs {
            assert_eq!(pair.evidence_quotes[0].chunk_ref, "chunk-1");
            assert_eq!(pair.evidence_quotes[0].source, "source.txt");
        }
    }

    #[test]
    fn single_text_without_source_requests_no_citations() {
        let result = format_single_chunk_prompt("factual", "chunk-1", "some text");
        assert!(result.text.contains("some text"));
        assert!(result.text.contains("chunk-1"));
        assert!(result.text.contains("factual"));
        assert!(result.text.contains("evidence_quotes: []"));
    }

    #[test]
    fn format_cross_reference_prompt_includes_all_passages() {
        let passages = vec!["first".to_string(), "second".to_string()];
        let result = format_cross_reference_prompt("factual", "group-1", &passages);
        assert!(
            result
                .text
                .contains("[Chunk group group-1, passage 1; source identity unavailable]")
        );
        assert!(
            result
                .text
                .contains("[Chunk group group-1, passage 2; source identity unavailable]")
        );
        assert!(result.text.contains("evidence_quotes: []"));
        assert!(result.text.contains("first"));
        assert!(result.text.contains("second"));
        assert_eq!(result.template_source, "inline-cross-reference");
    }
}
