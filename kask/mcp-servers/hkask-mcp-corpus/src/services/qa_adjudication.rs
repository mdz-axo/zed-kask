//! Complete identity-bound passage and level decisions for deterministic QA reruns.

use std::collections::{HashMap, HashSet};

use hkask_mcp_server::server::McpToolError;
use serde::Deserialize;

use crate::helpers::read_jsonl;
use crate::services::qa_pipeline::{
    PreparedQaPrompt, conceptual_relation_is_supported, support_absent_reason,
};
use crate::tools::corpus::QaType;

pub(crate) const QA_ADJUDICATION_PROTOCOL: &str = "prepared-qa-adjudication-v2";

#[derive(Clone)]
pub(crate) enum ReviewedPassageDecision {
    Admit,
    Skip(String),
}

#[derive(Clone)]
pub(crate) enum ReviewedLevelDecision {
    Generate { relation: Option<String> },
    Skip { reason: String },
}

#[derive(Clone)]
pub(crate) struct ReviewedQaAdjudication {
    passage: ReviewedPassageDecision,
    levels: Vec<ReviewedLevelDecision>,
}

impl ReviewedQaAdjudication {
    pub fn passage(&self) -> &ReviewedPassageDecision {
        &self.passage
    }

    pub fn levels(&self) -> &[ReviewedLevelDecision] {
        &self.levels
    }
}

pub(crate) struct ReviewedQaAdjudications {
    decisions: HashMap<String, ReviewedQaAdjudication>,
    passage_admits: usize,
    passage_skips: usize,
    level_generates: usize,
    level_skips: usize,
}

impl ReviewedQaAdjudications {
    pub fn decision(&self, prompt_id: &str) -> Option<&ReviewedQaAdjudication> {
        self.decisions.get(prompt_id)
    }

    pub fn passage_admits(&self) -> usize {
        self.passage_admits
    }

    pub fn passage_skips(&self) -> usize {
        self.passage_skips
    }

    pub fn level_generates(&self) -> usize {
        self.level_generates
    }

    pub fn level_skips(&self) -> usize {
        self.level_skips
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
struct RequiredNullableString(Option<String>);

impl RequiredNullableString {
    fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }

    fn into_option(self) -> Option<String> {
        self.0
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewedPassageRow {
    decision: String,
    reason: RequiredNullableString,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewedLevelRow {
    level: String,
    decision: String,
    relation: RequiredNullableString,
    reason: RequiredNullableString,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewedQaAdjudicationRow {
    protocol: String,
    prompt_id: String,
    chunk_ref: String,
    source: String,
    passage: ReviewedPassageRow,
    levels: Vec<ReviewedLevelRow>,
}

fn prompt_wide_reason(reason: &str) -> bool {
    matches!(
        reason,
        "contaminated_or_garbled" | "non_substantive_passage"
    )
}

fn validate_admitted_level(
    prompt_id: &str,
    index: usize,
    expected: QaType,
    row: ReviewedLevelRow,
) -> Result<ReviewedLevelDecision, McpToolError> {
    match row.decision.as_str() {
        "generate" => {
            if row.reason.as_deref().is_some() {
                return Err(McpToolError::invalid_argument(format!(
                    "Adjudication '{prompt_id}' generated level {index} must use null reason"
                )));
            }
            let relation = row.relation.into_option();
            if expected == QaType::Conceptual {
                let relation_value = relation.as_deref().ok_or_else(|| {
                    McpToolError::invalid_argument(format!(
                        "Adjudication '{prompt_id}' conceptual level {index} needs a closed relation"
                    ))
                })?;
                if !conceptual_relation_is_supported(relation_value) {
                    return Err(McpToolError::invalid_argument(format!(
                        "Adjudication '{prompt_id}' conceptual level {index} has unsupported relation '{relation_value}'"
                    )));
                }
            } else if relation.is_some() {
                return Err(McpToolError::invalid_argument(format!(
                    "Adjudication '{prompt_id}' non-conceptual level {index} must use null relation"
                )));
            }
            Ok(ReviewedLevelDecision::Generate { relation })
        }
        "skip" if expected != QaType::Factual => {
            if row.relation.as_deref().is_some()
                || row.reason.as_deref() != Some(support_absent_reason(expected))
            {
                return Err(McpToolError::invalid_argument(format!(
                    "Adjudication '{prompt_id}' skipped level {index} must use null relation and reason '{}'",
                    support_absent_reason(expected)
                )));
            }
            Ok(ReviewedLevelDecision::Skip {
                reason: support_absent_reason(expected).to_string(),
            })
        }
        "skip" => Err(McpToolError::invalid_argument(format!(
            "Adjudication '{prompt_id}' admitted factual level {index} must generate"
        ))),
        decision => Err(McpToolError::invalid_argument(format!(
            "Adjudication '{prompt_id}' level {index} has unsupported decision '{decision}'"
        ))),
    }
}

/// expect: A reviewed manifest deterministically covers every prepared prompt exactly once.
/// [P8] Motivating: Reviewed passage and level decisions cannot drift from source identity.
/// pre: path names a contained readable JSONL file and prompts have passed prepared validation.
/// post: every prompt has one complete identity-matched passage decision and ordered level mandate,
/// or the whole manifest is rejected before output creation.
/// [P1] Constraining: Preserve prompt, source, and requested-level identity without inferred aliases.
/// [P4] Constraining: Read only through the corpus path boundary.
pub(crate) fn read_complete_adjudications(
    path: &str,
    prompts: &[PreparedQaPrompt],
) -> Result<ReviewedQaAdjudications, McpToolError> {
    let rows: Vec<ReviewedQaAdjudicationRow> = read_jsonl(path, "quality_adjudications_jsonl")?;
    let prompt_by_id = prompts
        .iter()
        .map(|prompt| (prompt.prompt_id.as_str(), prompt))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::with_capacity(rows.len());
    let mut decisions = HashMap::with_capacity(rows.len());
    let mut passage_admits = 0;
    let mut passage_skips = 0;
    let mut level_generates = 0;
    let mut level_skips = 0;

    for row in rows {
        if row.protocol != QA_ADJUDICATION_PROTOCOL {
            return Err(McpToolError::invalid_argument(format!(
                "Adjudication '{}' has unsupported protocol '{}'",
                row.prompt_id, row.protocol
            )));
        }
        if !seen.insert(row.prompt_id.clone()) {
            return Err(McpToolError::invalid_argument(format!(
                "Duplicate adjudication prompt_id '{}'",
                row.prompt_id
            )));
        }
        let prompt = prompt_by_id.get(row.prompt_id.as_str()).ok_or_else(|| {
            McpToolError::invalid_argument(format!(
                "Adjudication references unknown prompt_id '{}'",
                row.prompt_id
            ))
        })?;
        if row.chunk_ref != prompt.primary().chunk_ref || row.source != prompt.primary().source {
            return Err(McpToolError::invalid_argument(format!(
                "Adjudication '{}' does not match its prepared chunk_ref and source",
                row.prompt_id
            )));
        }
        if row.levels.len() != prompt.qa_types.len() {
            return Err(McpToolError::invalid_argument(format!(
                "Adjudication '{}' covers {} of {} requested levels",
                row.prompt_id,
                row.levels.len(),
                prompt.qa_types.len()
            )));
        }
        for (index, (level, expected)) in row.levels.iter().zip(&prompt.qa_types).enumerate() {
            if level.level != expected.as_str() {
                return Err(McpToolError::invalid_argument(format!(
                    "Adjudication '{}' level {index} expected '{}', received '{}'",
                    row.prompt_id,
                    expected.as_str(),
                    level.level
                )));
            }
        }

        let (passage, levels) = match (row.passage.decision.as_str(), row.passage.reason.as_deref())
        {
            ("admit", None) => {
                passage_admits += 1;
                let levels = row
                    .levels
                    .into_iter()
                    .zip(&prompt.qa_types)
                    .enumerate()
                    .map(|(index, (level, expected))| {
                        validate_admitted_level(&row.prompt_id, index, *expected, level)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                (ReviewedPassageDecision::Admit, levels)
            }
            ("skip", Some(reason)) if prompt_wide_reason(reason) => {
                passage_skips += 1;
                let levels = row
                    .levels
                    .into_iter()
                    .enumerate()
                    .map(|(index, level)| {
                        if level.decision != "skip"
                            || level.relation.as_deref().is_some()
                            || level.reason.as_deref() != Some(reason)
                        {
                            return Err(McpToolError::invalid_argument(format!(
                                "Adjudication '{}' passage skip requires level {index} to skip with null relation and the same prompt-wide reason '{reason}'",
                                row.prompt_id
                            )));
                        }
                        Ok(ReviewedLevelDecision::Skip {
                            reason: reason.to_string(),
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                (ReviewedPassageDecision::Skip(reason.to_string()), levels)
            }
            _ => {
                return Err(McpToolError::invalid_argument(format!(
                    "Adjudication '{}' passage must be admit with null reason or skip with one canonical prompt-wide reason",
                    row.prompt_id
                )));
            }
        };
        level_generates += levels
            .iter()
            .filter(|level| matches!(level, ReviewedLevelDecision::Generate { .. }))
            .count();
        level_skips += levels.len()
            - levels
                .iter()
                .filter(|level| matches!(level, ReviewedLevelDecision::Generate { .. }))
                .count();
        decisions.insert(row.prompt_id, ReviewedQaAdjudication { passage, levels });
    }

    if decisions.len() != prompts.len() {
        let missing = prompts
            .iter()
            .filter(|prompt| !decisions.contains_key(&prompt.prompt_id))
            .map(|prompt| prompt.prompt_id.as_str())
            .collect::<Vec<_>>();
        return Err(McpToolError::invalid_argument(format!(
            "Reviewed adjudications cover {} of {} prompts; missing: {}",
            decisions.len(),
            prompts.len(),
            missing.join(", ")
        )));
    }

    Ok(ReviewedQaAdjudications {
        decisions,
        passage_admits,
        passage_skips,
        level_generates,
        level_skips,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::qa_pipeline::{PREPARED_QA_PROTOCOL, PreparedQaPassage};
    use serde_json::{Value, json};

    fn prompt(id: &str) -> PreparedQaPrompt {
        PreparedQaPrompt {
            prompt_id: id.to_string(),
            protocol: PREPARED_QA_PROTOCOL.to_string(),
            passages: vec![PreparedQaPassage {
                local_id: "p0".to_string(),
                chunk_ref: format!("chunk-{id}"),
                source: "source.txt".to_string(),
                text: "Source text.".to_string(),
            }],
            candidate_terms: Vec::new(),
            qa_types: vec![QaType::Factual, QaType::Conceptual],
        }
    }

    fn admit_row(id: &str) -> Value {
        json!({
            "protocol": QA_ADJUDICATION_PROTOCOL,
            "prompt_id": id,
            "chunk_ref": format!("chunk-{id}"),
            "source": "source.txt",
            "passage": {"decision":"admit","reason":null},
            "levels": [
                {"level":"factual","decision":"generate","relation":null,"reason":null},
                {"level":"conceptual","decision":"generate","relation":"mechanism","reason":null}
            ]
        })
    }

    fn skip_row(id: &str) -> Value {
        json!({
            "protocol": QA_ADJUDICATION_PROTOCOL,
            "prompt_id": id,
            "chunk_ref": format!("chunk-{id}"),
            "source": "source.txt",
            "passage": {"decision":"skip","reason":"contaminated_or_garbled"},
            "levels": [
                {"level":"factual","decision":"skip","relation":null,"reason":"contaminated_or_garbled"},
                {"level":"conceptual","decision":"skip","relation":null,"reason":"contaminated_or_garbled"}
            ]
        })
    }

    fn write_rows(
        directory: &tempfile::TempDir,
        rows: &[Value],
    ) -> Result<String, Box<dyn std::error::Error>> {
        let path = directory.path().join("adjudications.jsonl");
        let body = rows
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        std::fs::write(&path, format!("{body}\n"))?;
        Ok(path.to_string_lossy().into_owned())
    }

    fn fixture() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa-adjudication-test");
        std::fs::create_dir_all(&root)?;
        Ok(tempfile::Builder::new().prefix("case-").tempdir_in(root)?)
    }

    #[test]
    fn complete_valid_v2_manifest_is_identity_bound() -> Result<(), Box<dyn std::error::Error>> {
        let directory = fixture()?;
        let prompts = [prompt("qa-1"), prompt("qa-2")];
        let path = write_rows(&directory, &[admit_row("qa-1"), skip_row("qa-2")])?;
        let adjudications = read_complete_adjudications(&path, &prompts)?;
        assert_eq!(adjudications.passage_admits(), 1);
        assert_eq!(adjudications.passage_skips(), 1);
        assert_eq!(adjudications.level_generates(), 2);
        assert_eq!(adjudications.level_skips(), 2);
        Ok(())
    }

    #[test]
    fn v1_and_incomplete_prompt_coverage_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let directory = fixture()?;
        let prompts = [prompt("qa-1"), prompt("qa-2")];
        let mut row = admit_row("qa-1");
        row["protocol"] = json!("prepared-qa-passage-adjudication-v1");
        let path = write_rows(&directory, &[row])?;
        assert!(read_complete_adjudications(&path, &prompts).is_err());
        let path = write_rows(&directory, &[admit_row("qa-1")])?;
        assert!(read_complete_adjudications(&path, &prompts).is_err());
        Ok(())
    }

    #[test]
    fn wrong_level_order_and_coverage_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let directory = fixture()?;
        let prompts = [prompt("qa-1")];
        let mut row = admit_row("qa-1");
        row["levels"].as_array_mut().expect("levels").swap(0, 1);
        let path = write_rows(&directory, &[row])?;
        assert!(read_complete_adjudications(&path, &prompts).is_err());
        let mut row = admit_row("qa-1");
        row["levels"].as_array_mut().expect("levels").pop();
        let path = write_rows(&directory, &[row])?;
        assert!(read_complete_adjudications(&path, &prompts).is_err());
        Ok(())
    }

    #[test]
    fn wrong_relation_and_reason_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let directory = fixture()?;
        let prompts = [prompt("qa-1")];
        let mut row = admit_row("qa-1");
        row["levels"][1]["relation"] = json!("open_ended_relation");
        let path = write_rows(&directory, &[row])?;
        assert!(read_complete_adjudications(&path, &prompts).is_err());
        let mut row = admit_row("qa-1");
        row["levels"][1] = json!({
            "level":"conceptual",
            "decision":"skip",
            "relation":null,
            "reason":"conceptual_missing"
        });
        let path = write_rows(&directory, &[row])?;
        assert!(read_complete_adjudications(&path, &prompts).is_err());
        Ok(())
    }

    #[test]
    fn passage_skip_requires_uniform_level_reasons() -> Result<(), Box<dyn std::error::Error>> {
        let directory = fixture()?;
        let prompts = [prompt("qa-1")];
        let mut row = skip_row("qa-1");
        row["levels"][1]["reason"] = json!("conceptual_support_absent");
        let path = write_rows(&directory, &[row])?;
        assert!(read_complete_adjudications(&path, &prompts).is_err());
        Ok(())
    }
}
