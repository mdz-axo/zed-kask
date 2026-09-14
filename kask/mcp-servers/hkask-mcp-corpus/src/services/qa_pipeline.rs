//! Canonical prepared QA records, validation, and completion accounting for
//! the synchronous generation path.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;

use hkask_types::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::batch::BatchOutcome;
use crate::helpers::{map_corpus_io_error, read_jsonl};
use crate::tools::corpus::{QaType, qa_type_instruction};

use crate::{McpToolError, extract_json_from_response};

pub(crate) const PREPARED_QA_PROTOCOL: &str = "prepared-qa-local-evidence-v1";

struct QaPair {
    question: String,
    answer: String,
    bloom_level: String,
    evidence_quotes: Vec<hkask_types::corpus::QaEvidence>,
}

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
    pub completion_tokens: Option<u64>,
    pub finish_reason: Option<String>,
    pub cost_usd: Option<f64>,
}

/// Per-prompt inference failure to produce a QA completion. Recorded via
/// Display in the output record for later inspection — downstream code
/// never matches variants.
#[derive(Debug, thiserror::Error)]
pub(crate) enum QaCompletionError {
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
    completion_tokens_used: u64,
    completion_token_reports: usize,
    finish_reason_counts: BTreeMap<String, usize>,
    finish_reason_reports: usize,
    provider_responses: usize,
    cost_reports: usize,
    reported_cost_usd: f64,
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
            completion_tokens_used: 0,
            completion_token_reports: 0,
            finish_reason_counts: BTreeMap::new(),
            finish_reason_reports: 0,
            provider_responses: 0,
            cost_reports: 0,
            reported_cost_usd: 0.0,
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
        let mut completion_metadata = None;
        let parsed = match completion {
            Ok(completion) => {
                self.tokens_used += completion.tokens_used;
                self.provider_responses += 1;
                if let Some(completion_tokens) = completion.completion_tokens {
                    self.completion_tokens_used += completion_tokens;
                    self.completion_token_reports += 1;
                }
                if let Some(finish_reason) = completion.finish_reason.as_ref() {
                    *self
                        .finish_reason_counts
                        .entry(finish_reason.clone())
                        .or_default() += 1;
                    self.finish_reason_reports += 1;
                }
                if let Some(cost_usd) = completion.cost_usd {
                    self.cost_reports += 1;
                    self.reported_cost_usd += cost_usd;
                }
                completion_metadata =
                    Some((completion.completion_tokens, completion.finish_reason));
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
                let (completion_tokens, finish_reason) = completion_metadata.unwrap_or_default();
                self.write_record(&json!({
                    "prompt_id": prompt.prompt_id,
                    "chunk_ref": prompt.primary().chunk_ref,
                    "source": prompt.primary().source,
                    "error": error.to_string(),
                    "completion_tokens": completion_tokens,
                    "finish_reason": finish_reason,
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

    pub fn finish(mut self, output: &str) -> Result<serde_json::Value, McpToolError> {
        self.flush()?;
        if self.prompts_succeeded + self.prompts_failed != self.prompts_total {
            return Err(McpToolError::internal(
                "QA completion accounting does not match prompts_total",
            ));
        }
        let outcome = BatchOutcome::from_counts(self.prompts_failed, self.prompts_total);
        outcome.log_if_degraded("hkask.mcp.docproc.qa_batch", "QA batch");
        let cost_reporting_complete = self.provider_responses == self.prompts_total
            && self.cost_reports == self.provider_responses;
        let reported_cost_usd = cost_reporting_complete.then_some(self.reported_cost_usd);
        let completion_token_reporting_complete =
            self.completion_token_reports == self.provider_responses;
        let finish_reason_reporting_complete =
            self.finish_reason_reports == self.provider_responses;
        Ok(json!({
            "prompts_total": self.prompts_total,
            "prompts_succeeded": self.prompts_succeeded,
            "prompts_failed": self.prompts_failed,
            "qa_rows_written": self.qa_rows_written,
            "tokens_used": self.tokens_used,
            "completion_tokens_used": self.completion_tokens_used,
            "completion_token_reporting_complete": completion_token_reporting_complete,
            "finish_reason_counts": self.finish_reason_counts,
            "finish_reason_reporting_complete": finish_reason_reporting_complete,
            "provider_responses": self.provider_responses,
            "reported_cost_usd": reported_cost_usd,
            "cost_reporting_complete": cost_reporting_complete,
            "output": output,
            "degraded": BatchOutcome::is_degraded(self.prompts_failed, self.prompts_total),
        }))
    }
}

/// The LLM parameters used by all QA generation paths.
///
/// Single source of truth for prepared QA generation.
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

/// Build the QA result envelope for one QA pair.
///
/// The envelope format matches what `corpus_ingest_qa`'s `parse_qa_record`
/// expects: primary identity, QA type, response, canonical evidence and
/// provenance. Prompt-level token usage stays in the batch summary.
fn qa_result_envelope(prompt: &PreparedQaPrompt, pair: QaPair, model: &str) -> serde_json::Value {
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
            completion_tokens: Some(5),
            finish_reason: Some("stop".into()),
            cost_usd: Some(0.01),
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
            assert!(output.finish("unused").is_err());
        }
        let mut output = QaOutput::new(
            RejectingWriter {
                failure: Failure::Flush,
            },
            1,
        );
        output.complete(&prepared(), accepted(), "offline-model")?;
        let error = output.finish("unused").expect_err("flush must fail");
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
                    Err(QaCompletionError::LlmFailed(1, "provider failure".into())),
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
                    tokens_used: 7,
                    completion_tokens: Some(5),
                    finish_reason: Some("length".into()),
                    cost_usd: Some(0.02),
                }),
                "offline-model",
            )?;
            let summary = output.finish("unused")?;
            assert_eq!(summary["prompts_total"], 1);
            assert_eq!(summary["prompts_succeeded"], 0);
            assert_eq!(summary["prompts_failed"], 1);
            assert_eq!(summary["qa_rows_written"], 0);
            assert_eq!(summary["tokens_used"], 7);
            assert_eq!(summary["completion_tokens_used"], 5);
            assert_eq!(summary["completion_token_reporting_complete"], true);
            assert_eq!(summary["finish_reason_counts"]["length"], 1);
            assert_eq!(summary["finish_reason_reporting_complete"], true);
            assert_eq!(summary["reported_cost_usd"], 0.02);
            assert_eq!(summary["cost_reporting_complete"], true);
            let row: serde_json::Value = serde_json::from_slice(&bytes)?;
            assert_eq!(row["prompt_id"], "qa-1");
            assert!(row["error"].as_str().expect("error").contains("rejected"));
            assert_eq!(row["completion_tokens"], 5);
            assert_eq!(row["finish_reason"], "length");
            assert!(row.get("response").is_none());
        }
        Ok(())
    }

    /// expect: [P9] A transport that loses a completion cannot report a successful run.
    #[test]
    fn unfinished_accounting_is_not_success() {
        let output = QaOutput::new(Vec::new(), 1);
        assert!(output.finish("unused").is_err());
    }

    #[test]
    fn qa_llm_parameters_match_historical_values() {
        let params = qa_llm_parameters();
        assert_eq!(params.temperature, 0.3);
        assert_eq!(params.top_p, 0.95);
        assert!(!params.thinking_allowed);
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
}
