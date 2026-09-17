//! Complete reviewed passage-quality decisions for deterministic QA reruns.

use std::collections::{HashMap, HashSet};

use hkask_mcp_server::server::McpToolError;
use serde::Deserialize;

use crate::helpers::read_jsonl;
use crate::services::qa_pipeline::PreparedQaPrompt;

pub(crate) const PASSAGE_ADJUDICATION_PROTOCOL: &str = "prepared-qa-passage-adjudication-v1";

#[derive(Clone)]
pub(crate) enum ReviewedPassageDecision {
    Admit,
    Skip(String),
}

pub(crate) struct ReviewedPassageAdjudications {
    decisions: HashMap<String, ReviewedPassageDecision>,
    admits: usize,
    skips: usize,
}

impl ReviewedPassageAdjudications {
    pub fn decision(&self, prompt_id: &str) -> Option<&ReviewedPassageDecision> {
        self.decisions.get(prompt_id)
    }

    pub fn admits(&self) -> usize {
        self.admits
    }

    pub fn skips(&self) -> usize {
        self.skips
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewedPassageAdjudicationRow {
    protocol: String,
    prompt_id: String,
    chunk_ref: String,
    source: String,
    decision: String,
    reason: Option<String>,
}

/// expect: A reviewed manifest deterministically covers every prepared prompt exactly once.
/// [P8] Motivating: Reviewed passage decisions cannot drift from their source identity.
/// pre: path names a contained readable JSONL file and prompts have passed prepared validation.
/// post: every prompt has one closed identity-matched decision, or the whole manifest is rejected.
/// [P1] Constraining: Preserve prompt and source identity without inferred aliases.
/// [P4] Constraining: Read only through the corpus path boundary.
pub(crate) fn read_complete_adjudications(
    path: &str,
    prompts: &[PreparedQaPrompt],
) -> Result<ReviewedPassageAdjudications, McpToolError> {
    let rows: Vec<ReviewedPassageAdjudicationRow> =
        read_jsonl(path, "quality_adjudications_jsonl")?;
    let prompt_by_id = prompts
        .iter()
        .map(|prompt| (prompt.prompt_id.as_str(), prompt))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::with_capacity(rows.len());
    let mut decisions = HashMap::with_capacity(rows.len());
    let mut admits = 0;
    let mut skips = 0;

    for row in rows {
        if row.protocol != PASSAGE_ADJUDICATION_PROTOCOL {
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
        let decision = match (row.decision.as_str(), row.reason.as_deref()) {
            ("admit", None) => {
                admits += 1;
                ReviewedPassageDecision::Admit
            }
            ("skip", Some(reason))
                if matches!(
                    reason,
                    "contaminated_or_garbled" | "non_substantive_passage"
                ) =>
            {
                skips += 1;
                ReviewedPassageDecision::Skip(reason.to_string())
            }
            _ => {
                return Err(McpToolError::invalid_argument(format!(
                    "Adjudication '{}' must be admit with no reason or skip with one canonical prompt-wide reason",
                    row.prompt_id
                )));
            }
        };
        decisions.insert(row.prompt_id, decision);
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

    Ok(ReviewedPassageAdjudications {
        decisions,
        admits,
        skips,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::qa_pipeline::{PREPARED_QA_PROTOCOL, PreparedQaPassage};
    use crate::tools::corpus::QaType;
    use serde_json::json;

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

    #[test]
    fn complete_identity_bound_manifest_is_required() -> Result<(), Box<dyn std::error::Error>> {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa-adjudication-test");
        std::fs::create_dir_all(&root)?;
        let directory = tempfile::Builder::new().prefix("case-").tempdir_in(root)?;
        let path = directory.path().join("adjudications.jsonl");
        let rows = [
            json!({"protocol":PASSAGE_ADJUDICATION_PROTOCOL,"prompt_id":"qa-1","chunk_ref":"chunk-qa-1","source":"source.txt","decision":"admit","reason":null}),
            json!({"protocol":PASSAGE_ADJUDICATION_PROTOCOL,"prompt_id":"qa-2","chunk_ref":"chunk-qa-2","source":"source.txt","decision":"skip","reason":"contaminated_or_garbled"}),
        ];
        let body = rows
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        std::fs::write(&path, format!("{body}\n"))?;
        let prompts = [prompt("qa-1"), prompt("qa-2")];
        let adjudications = read_complete_adjudications(&path.to_string_lossy(), &prompts)?;
        assert_eq!(adjudications.admits(), 1);
        assert_eq!(adjudications.skips(), 1);

        std::fs::write(&path, format!("{}\n", serde_json::to_string(&rows[0])?))?;
        assert!(read_complete_adjudications(&path.to_string_lossy(), &prompts).is_err());
        Ok(())
    }
}
