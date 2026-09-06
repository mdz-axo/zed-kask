//! Canonical prepared QA records and completion/output accounting for both
//! batch transports. Single-chunk generation retains its own prompt formatter.

use std::collections::HashSet;
use std::io::Write;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::batch::BatchOutcome;
use crate::helpers::{map_corpus_io_error, read_jsonl};
use crate::tools::semantic::qa::{QaPair, parse_qa_response};
use crate::{
    CONTENT_GUARD_INSTRUCTION, McpToolError, extract_json_from_response, render_docproc_template,
};

/// The only prepared-prompt JSONL contract. Identity belongs to the prompt,
/// not its source chunk: several prompts may refer to the same chunk.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreparedQaPrompt {
    pub prompt_id: String,
    pub chunk_ref: String,
    pub source: String,
    pub concepts: Vec<String>,
    pub salience: f64,
    pub qa_type: String,
    pub system: String,
    pub user: String,
}

impl PreparedQaPrompt {
    pub fn validate(&self) -> Result<(), McpToolError> {
        for (field, value) in [
            ("prompt_id", &self.prompt_id),
            ("chunk_ref", &self.chunk_ref),
            ("source", &self.source),
            ("qa_type", &self.qa_type),
            ("system", &self.system),
            ("user", &self.user),
        ] {
            if value.trim().is_empty() {
                return Err(McpToolError::invalid_argument(format!(
                    "Prepared QA prompt '{}': {field} must not be empty",
                    self.prompt_id
                )));
            }
        }
        // Keep identities portable across the provider batch APIs.
        if self.prompt_id.len() > 64
            || !self
                .prompt_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(McpToolError::invalid_argument(
                "prompt_id must be 1–64 ASCII letters, digits, hyphens or underscores",
            ));
        }
        if !self.salience.is_finite()
            || self
                .concepts
                .iter()
                .any(|concept| concept.trim().is_empty())
        {
            return Err(McpToolError::invalid_argument(format!(
                "Prepared QA prompt '{}': salience must be finite and concepts must not contain empty strings",
                self.prompt_id
            )));
        }
        Ok(())
    }
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

/// One owner writes completions as they arrive; neither transport owns counts
/// or swallows output failures. Generic Write permits real I/O failure tests.
pub(crate) struct QaOutput<W: Write> {
    writer: W,
    prompts_total: usize,
    prompts_succeeded: usize,
    prompts_failed: usize,
    qa_rows_written: usize,
}

impl<W: Write> QaOutput<W> {
    pub fn new(writer: W, prompts_total: usize) -> Self {
        Self {
            writer,
            prompts_total,
            prompts_succeeded: 0,
            prompts_failed: 0,
            qa_rows_written: 0,
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
        completion: Result<QaCompletion, String>,
        model: &str,
    ) -> Result<(), McpToolError> {
        let parsed = completion.and_then(|completion| {
            parse_qa_response(
                &extract_json_from_response(&completion.text),
                std::slice::from_ref(&prompt.qa_type),
                None,
            )
            .map(|response| (response, completion.tokens_used))
            .map_err(|error| format!("QA response rejected: {error}"))
        });
        match parsed {
            Ok((response, tokens_used)) => {
                for pair in response.qa_pairs {
                    self.write_record(&qa_result_envelope(prompt, pair, model, tokens_used))?;
                    self.qa_rows_written += 1;
                }
                self.prompts_succeeded += 1;
            }
            Err(error) => {
                self.write_record(&json!({
                    "prompt_id": prompt.prompt_id,
                    "chunk_ref": prompt.chunk_ref,
                    "source": prompt.source,
                    "error": error,
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
    let tpl = render_docproc_template("generate-qa", &vars);
    if tpl.is_empty() {
        FormattedQaPrompt {
            text: format!(
                "{CONTENT_GUARD_INSTRUCTION}Based on the following text, generate question-answer pairs at these Bloom's taxonomy levels: {levels_str}.\n\nText (chunk {chunk_id}):\n{text}\n\nFor each level, provide question, answer, and bloom_level.\nRespond in JSON: {{\"qa_pairs\": [{{\"question\": \"...\", \"answer\": \"...\", \"bloom_level\": \"...\"}}]}}",
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
            "[Passage {}]\n{}\n\n",
            i + 1,
            crate::guard_content(p)
        ));
    }
    FormattedQaPrompt {
        text: format!(
            "{CONTENT_GUARD_INSTRUCTION}You are synthesizing knowledge across {n} passages.\n\nGenerate question-answer pairs at these Bloom's taxonomy levels: {levels_str}.\n\nThe questions should require synthesizing information from MULTIPLE passages — compare, contrast, diagnose patterns, or trace causal connections across sources.\n\nFor each QA, cite which passages support the answer (e.g., 'Per Passage 1, ... while Passage 2 notes ...').\n\nPassages (chunk group {chunk_id}):\n{text}\n\nRespond in JSON: {{\"qa_pairs\": [{{\"question\": \"...\", \"answer\": \"...\", \"bloom_level\": \"...\", \"sources\": [1, 3]}}]}}",
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
    tokens_used: impl Into<u64>,
) -> serde_json::Value {
    json!({
        "prompt_id": prompt.prompt_id,
        "chunk_ref": prompt.chunk_ref,
        "salience": prompt.salience,
        "source": prompt.source,
        "qa_type": pair.bloom_level,
        "response": {
            "instruction": pair.question,
            "output": pair.answer,
            "type": pair.bloom_level,
            "concepts": prompt.concepts,
        },
        "provenance": {
            "generator_model": model,
            "prompt_template": "prepared-qa",
            "prompt_id": prompt.prompt_id,
            "source_chunk_ref": prompt.chunk_ref,
        },
        "tokens_used": tokens_used.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared() -> PreparedQaPrompt {
        PreparedQaPrompt {
            prompt_id: "qa-1".into(),
            chunk_ref: "chunk-1".into(),
            source: "source.txt".into(),
            concepts: Vec::new(),
            salience: 0.5,
            qa_type: "factual".into(),
            system: "system".into(),
            user: "user".into(),
        }
    }

    fn accepted() -> Result<QaCompletion, String> {
        Ok(QaCompletion { text: json!({"qa_pairs": [{"question":"Question?", "answer":"Answer.", "bloom_level":"factual"}]}).to_string(), tokens_used: 10 })
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
                .complete(&prepared(), Err("provider failure".into()), "offline-model")
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
            "{\"qa_pairs\":[]}",
            r#"{"qa_pairs":[{"question":"", "answer":"answer", "bloom_level":"factual"}]}"#,
            r#"{"qa_pairs":[{"question":"question", "answer":" ", "bloom_level":"factual"}]}"#,
            r#"{"qa_pairs":[{"question":"question", "answer":"answer", "bloom_level":"create"}]}"#,
            r#"{"qa_pairs":[{"question":"question", "answer":"answer", "bloom_level":"factual"}, {"question":"bad"}]}"#,
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
    fn format_single_chunk_prompt_falls_back_inline() {
        // The template registry is not available in tests (HKASK_TEMPLATE_ROOT
        // unset), so the inline fallback fires.
        let result = format_single_chunk_prompt("factual", "chunk-1", "some text");
        assert!(result.text.contains("some text"));
        assert!(result.text.contains("chunk-1"));
        assert!(result.text.contains("factual"));
    }

    #[test]
    fn format_cross_reference_prompt_includes_all_passages() {
        let passages = vec!["first".to_string(), "second".to_string()];
        let result = format_cross_reference_prompt("factual", "group-1", &passages);
        assert!(result.text.contains("[Passage 1]"));
        assert!(result.text.contains("[Passage 2]"));
        assert!(result.text.contains("first"));
        assert!(result.text.contains("second"));
        assert_eq!(result.template_source, "inline-cross-reference");
    }
}
