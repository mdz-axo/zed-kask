//! Canonical prepared QA records, validation, and completion accounting for
//! the synchronous generation path.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;

use hkask_types::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::batch::BatchOutcome;
use crate::helpers::{map_corpus_io_error, read_jsonl};
use crate::services::qa_adjudication::{ReviewedLevelDecision, ReviewedQaAdjudication};
use crate::tools::corpus::{QaType, qa_type_instruction};

use crate::{CONTENT_GUARD_INSTRUCTION, McpToolError, extract_json_from_response};

pub(crate) const PREPARED_QA_PROTOCOL: &str = "prepared-qa-local-evidence-v1";
const PASSAGE_QUALITY_PROTOCOL: &str = "prepared-qa-passage-quality-v1";
const QA_DISPOSITION_PROTOCOL: &str = "prepared-qa-disposition-plan-v1";
const QA_GENERATION_PROTOCOL: &str = "prepared-qa-staged-quality-v8";
const QA_VERIFICATION_PROTOCOL: &str = "prepared-qa-verification-verdict-v1";
const PASSAGE_QUALITY_POLICY: &str = "Judge the complete primary passage before any QA planning. The passage is contaminated_or_garbled when OCR or layout materially corrupts words, interleaves page or line furniture with prose, splices footnotes into a sentence, embeds unrelated bare page-number fragments, interface controls, media titles, or navigation residue between otherwise usable prose, appends bibliographic navigation or an isolated table or figure caption, joins unrelated sections, or truncates a thought required for an answer. The passage is non_substantive_passage when it is only navigation, marketing, legal or publication furniture, an unfilled template, an isolated caption, or an isolated anecdote or cross-document fragment whose purpose is not inferable from the passage. Do not reject a coherent continuation fragment or short legible factual passage merely because it begins mid-sentence, contains notation, lacks conceptual support, or has a single broken word or line-break hyphen, footnote marker, or page number that does not obstruct meaning.";
const EVIDENCE_CANDIDATE_WORDS: usize = 24;
const EVIDENCE_CANDIDATE_OVERLAP_WORDS: usize = 6;

struct QaPair {
    question: String,
    answer: String,
    bloom_level: String,
    evidence_quotes: Vec<hkask_types::corpus::QaEvidence>,
}

enum QaLevelDisposition {
    Generated(QaPair),
    Skipped { bloom_level: String, reason: String },
}

#[derive(Clone)]
enum PlannedQaLevel {
    Generate {
        bloom_level: String,
        relation: Option<String>,
        evidence_ids: Vec<String>,
    },
    Skipped {
        bloom_level: String,
        reason: String,
    },
}

#[derive(Clone)]
pub(crate) struct QaDispositionPlan {
    levels: Vec<PlannedQaLevel>,
}

pub(crate) enum PassageQuality {
    Clean,
    Skip(String),
}

impl QaDispositionPlan {
    pub fn needs_writer(&self) -> bool {
        self.levels
            .iter()
            .any(|level| matches!(level, PlannedQaLevel::Generate { .. }))
    }
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

#[derive(Debug, Clone)]
struct EvidenceCandidate {
    local_id: String,
    quote: String,
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

fn evidence_candidates(prompt: &PreparedQaPrompt) -> Vec<EvidenceCandidate> {
    let text = &prompt.primary().text;
    let mut words = Vec::new();
    let mut word_start = None;
    for (offset, character) in text.char_indices() {
        if character.is_whitespace() {
            if let Some(start) = word_start.take() {
                words.push((start, offset));
            }
        } else if word_start.is_none() {
            word_start = Some(offset);
        }
    }
    if let Some(start) = word_start {
        words.push((start, text.len()));
    }

    let stride = EVIDENCE_CANDIDATE_WORDS - EVIDENCE_CANDIDATE_OVERLAP_WORDS;
    (0..words.len())
        .step_by(stride)
        .enumerate()
        .map(|(index, start)| {
            let end = (start + EVIDENCE_CANDIDATE_WORDS).min(words.len());
            EvidenceCandidate {
                local_id: format!("e{index}"),
                quote: text[words[start].0..words[end - 1].1].to_string(),
            }
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedQaPlanLevel {
    level: String,
    disposition: String,
    relation: Option<String>,
    reason: Option<String>,
    evidence_ids: Vec<String>,
}

pub(crate) fn conceptual_relation_is_supported(relation: &str) -> bool {
    matches!(
        relation,
        "mechanism"
            | "relationship"
            | "causal_relationship"
            | "distinction"
            | "purpose"
            | "framework"
            | "transferable_principle"
    )
}

/// expect: Passage quality and requested-level support are decided before any QA is written.
/// [P9] Motivating: Bad or unsupported source material cannot become accepted training prose.
/// pre: prompt carries validated primary text and ordered requested levels.
/// post: the model receives one whole-passage decision task with server-owned evidence candidates.
pub(crate) fn render_passage_quality_messages(
    prompt: &PreparedQaPrompt,
) -> Result<[ChatMessage; 2], McpToolError> {
    prompt.validate()?;
    let user = serde_json::to_string(&json!({
        "passage_quality_protocol": PASSAGE_QUALITY_PROTOCOL,
        "primary_passage": crate::guard_content(&prompt.primary().text),
    }))
    .map_err(|error| McpToolError::internal(format!("Cannot render passage quality: {error}")))?;
    let system = format!(
        "{CONTENT_GUARD_INSTRUCTION}{PASSAGE_QUALITY_POLICY} Return exactly [\"clean\"] for an eligible passage, [\"skip\",\"contaminated_or_garbled\"], or [\"skip\",\"non_substantive_passage\"]. This is a focused passage-quality gate: do not judge Bloom-level support, draft questions, or select evidence. Emit one compact JSON array and nothing else."
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

/// expect: An initially clean passage receives an independent quality decision before QA planning.
/// [P9] Motivating: A single model miss cannot admit contaminated or non-substantive training prose.
/// pre: prompt is valid and proposed_quality is the first pass's compact response.
/// post: the verification model receives the complete passage, proposal, and identical closed decision schema.
/// [P1] Constraining: Preserve the prepared passage without rewriting or inferred identity.
pub(crate) fn render_passage_quality_review_messages(
    prompt: &PreparedQaPrompt,
    proposed_quality: &str,
) -> Result<[ChatMessage; 2], McpToolError> {
    let mut messages = render_passage_quality_messages(prompt)?;
    messages[0].content.push_str(
        " Independently review the proposed passage-quality decision against the complete passage. The proposal is not authoritative. Return one complete corrected decision in the identical compact schema and nothing else.",
    );
    let mut user: Value = serde_json::from_str(&messages[1].content).map_err(|error| {
        McpToolError::internal(format!(
            "Cannot parse rendered passage quality request: {error}"
        ))
    })?;
    let object = user.as_object_mut().ok_or_else(|| {
        McpToolError::internal("Rendered passage quality request is not a JSON object")
    })?;
    object.insert(
        "proposed_passage_quality".to_string(),
        serde_json::from_str(proposed_quality)
            .unwrap_or_else(|_| Value::String(proposed_quality.into())),
    );
    messages[1].content = serde_json::to_string(&user).map_err(|error| {
        McpToolError::internal(format!("Cannot render passage quality review: {error}"))
    })?;
    Ok(messages)
}

pub(crate) fn parse_passage_quality_response(response: &str) -> Result<PassageQuality, String> {
    let fields: Vec<String> = serde_json::from_str(response)
        .map_err(|error| format!("invalid compact passage-quality JSON: {error}"))?;
    match fields.as_slice() {
        [decision] if decision == "clean" => Ok(PassageQuality::Clean),
        [decision, reason]
            if decision == "skip"
                && matches!(
                    reason.as_str(),
                    "contaminated_or_garbled" | "non_substantive_passage"
                ) =>
        {
            Ok(PassageQuality::Skip(reason.clone()))
        }
        _ => Err(
            "passage quality must be exactly one canonical clean or prompt-wide skip disposition"
                .to_string(),
        ),
    }
}

pub(crate) fn prompt_wide_skip_plan(prompt: &PreparedQaPrompt, reason: &str) -> QaDispositionPlan {
    QaDispositionPlan {
        levels: prompt
            .qa_types
            .iter()
            .map(|qa_type| PlannedQaLevel::Skipped {
                bloom_level: qa_type.as_str().to_string(),
                reason: reason.to_string(),
            })
            .collect(),
    }
}

pub(crate) fn render_disposition_plan_messages(
    prompt: &PreparedQaPrompt,
    reviewed: Option<&ReviewedQaAdjudication>,
) -> Result<[ChatMessage; 2], McpToolError> {
    prompt.validate()?;
    let level_requirements = prompt
        .qa_types
        .iter()
        .map(|qa_type| {
            json!({
                "level": qa_type,
                "requirement": qa_type_instruction(*qa_type),
            })
        })
        .collect::<Vec<_>>();
    let evidence_candidates = evidence_candidates(prompt)
        .into_iter()
        .map(|candidate| {
            json!({
                "id": candidate.local_id,
                "text": crate::guard_content(&candidate.quote),
            })
        })
        .collect::<Vec<_>>();
    let mut user = json!({
        "disposition_protocol": QA_DISPOSITION_PROTOCOL,
        "primary_passage": crate::guard_content(&prompt.primary().text),
        "requested_levels": prompt.qa_types,
        "level_requirements": level_requirements,
        "evidence_candidates": evidence_candidates,
    });
    if let Some(reviewed) = reviewed {
        let mandates = reviewed
            .levels()
            .iter()
            .zip(&prompt.qa_types)
            .map(|(decision, qa_type)| match decision {
                ReviewedLevelDecision::Generate { relation } => json!({
                    "level": qa_type,
                    "decision": "generate",
                    "relation": relation,
                    "reason": null,
                }),
                ReviewedLevelDecision::Skip { reason } => json!({
                    "level": qa_type,
                    "decision": "skip",
                    "relation": null,
                    "reason": reason,
                }),
            })
            .collect::<Vec<_>>();
        user.as_object_mut()
            .ok_or_else(|| McpToolError::internal("Rendered disposition request is not an object"))?
            .insert("reviewed_level_mandates".to_string(), json!(mandates));
    }
    let user = serde_json::to_string(&user).map_err(|error| {
        McpToolError::internal(format!("Cannot render QA disposition plan: {error}"))
    })?;
    let mut system = format!(
        "{CONTENT_GUARD_INSTRUCTION}{PASSAGE_QUALITY_POLICY} Return one typed disposition plan before any QA is written. Prompt-wide quality reasons override all level dispositions. For a clean passage return [\"clean\",[{{\"level\":\"factual\",\"disposition\":\"generate\",\"relation\":null,\"reason\":null,\"evidence_ids\":[\"e0\"]}},{{\"level\":\"conceptual\",\"disposition\":\"skip\",\"relation\":null,\"reason\":\"conceptual_support_absent\",\"evidence_ids\":[]}}]], with exactly one ordered object per requested level. Generated levels require one to three unique evidence IDs that together contain every premise and answer component the writer will need. Never emit a generate disposition with an empty evidence list: copy the supporting eN IDs, or use the level's support-absent skip when no candidate supports it. Conceptual generation additionally requires exactly one relation from mechanism, relationship, causal_relationship, distinction, purpose, framework, transferable_principle. Conceptual support exists when evidence explicitly connects a formula to its inputs or discrete values, a method to both construction and ongoing use, examples to a stated general claim, a modeling assumption to its practical justification, an action to an outcome with purpose or result language, or components to distinct roles or interactions. A denominator or entry count that constrains a formula's possible values is a supported mathematical relationship even in a short passage. Explicit result language supports a relationship even when the outcome is qualified by hope; preserve that qualification rather than skipping the relation. A stated threshold or sufficiently large parameter connected to infeasibility is a supported relationship without requiring an unstated mechanism. A characterization followed by how a subject treats its stated faults is a supported characterization relationship. A structured set of components supports framework when the QA can explain how they organize dependencies, estimates, or decisions. Copying listed criteria and adding that they form a framework or lead to the already stated outcome remains factual recall. A purpose relation requires explicit intent or goal language; a statement that someone urges an action to produce an outcome is explicit purpose and should be retained. Adjacent future actions, hopes, or preferences alone do not establish why an action is taken. An explicit condition, decision, action, and resulting configuration supports mechanism and must not be skipped merely because each step is directly stated. A passage that merely asserts a relation with phrases such as useful, enables, or shows, without explaining how the relation works, does not support mechanism or causal_relationship. When a passage states an overall effect and separately defines a formula without saying which factor causes the effect, conceptual support is limited to the formula or framework—not an invented component-level causal mechanism. If answering would only retrieve a name, label, list, title, number, explanation label, or sentence paraphrase without explaining one of those relations, skip conceptual support. Other generated levels use null relation. A level skip uses only its canonical support-absent reason and no evidence. Emit compact JSON only."
    );
    if reviewed.is_some() {
        system.push_str(
            " The supplied reviewed_level_mandates are authoritative. Copy every mandated decision and conceptual relation exactly; select evidence IDs only for mandated generate levels. Do not re-evaluate passage quality or level support.",
        );
    }
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

pub(crate) fn render_disposition_review_messages(
    prompt: &PreparedQaPrompt,
    proposed_plan: &str,
    validation_error: Option<&str>,
) -> Result<[ChatMessage; 2], McpToolError> {
    let mut messages = render_disposition_plan_messages(prompt, None)?;
    messages[0].content.push_str(
        " Independently review the proposed disposition against the complete passage. Correct missed contamination, false prompt-wide skips, false level skips, recall mislabeled as conceptual, missing evidence IDs, and unsupported relation labels. The proposed plan is not authoritative. Return one complete corrected plan in the same typed schema and nothing else.",
    );
    let mut user: Value = serde_json::from_str(&messages[1].content).map_err(|error| {
        McpToolError::internal(format!(
            "Cannot parse rendered disposition request: {error}"
        ))
    })?;
    let object = user.as_object_mut().ok_or_else(|| {
        McpToolError::internal("Rendered disposition request is not a JSON object")
    })?;
    object.insert(
        "proposed_plan".to_string(),
        serde_json::from_str(proposed_plan).unwrap_or_else(|_| Value::String(proposed_plan.into())),
    );
    object.insert(
        "validation_error".to_string(),
        validation_error.map_or(Value::Null, |error| Value::String(error.into())),
    );
    messages[1].content = serde_json::to_string(&user).map_err(|error| {
        McpToolError::internal(format!("Cannot render disposition review: {error}"))
    })?;
    Ok(messages)
}

pub(crate) fn parse_disposition_plan_response(
    response: &str,
    prompt: &PreparedQaPrompt,
) -> Result<QaDispositionPlan, String> {
    let value: Value = serde_json::from_str(response)
        .map_err(|error| format!("invalid compact QA disposition JSON: {error}"))?;
    let fields = value
        .as_array()
        .ok_or_else(|| "QA disposition plan must be an array".to_string())?;
    match fields.as_slice() {
        [Value::String(decision), Value::String(reason)]
            if decision == "skip"
                && matches!(
                    reason.as_str(),
                    "contaminated_or_garbled" | "non_substantive_passage"
                ) =>
        {
            Ok(QaDispositionPlan {
                levels: prompt
                    .qa_types
                    .iter()
                    .map(|qa_type| PlannedQaLevel::Skipped {
                        bloom_level: qa_type.as_str().to_string(),
                        reason: reason.clone(),
                    })
                    .collect(),
            })
        }
        [Value::String(decision), Value::Array(raw_levels)] if decision == "clean" => {
            if raw_levels.len() != prompt.qa_types.len() {
                return Err(format!(
                    "expected {} planned QA levels, received {}",
                    prompt.qa_types.len(),
                    raw_levels.len()
                ));
            }
            let candidates = evidence_candidates(prompt);
            let candidate_ids = candidates
                .iter()
                .map(|candidate| candidate.local_id.as_str())
                .collect::<HashSet<_>>();
            let mut levels = Vec::with_capacity(raw_levels.len());
            for (index, (raw_level, expected)) in
                raw_levels.iter().zip(&prompt.qa_types).enumerate()
            {
                let PreparedQaPlanLevel {
                    level,
                    disposition,
                    relation,
                    reason,
                    evidence_ids,
                } = serde_json::from_value(raw_level.clone()).map_err(|error| {
                    format!("invalid QA disposition level {index}: {error}")
                })?;
                if level != expected.as_str() {
                    return Err(format!(
                        "planned level {index} expected '{}', received '{level}'",
                        expected.as_str()
                    ));
                }
                match disposition.as_str() {
                    "generate" => {
                        if evidence_ids.is_empty() || evidence_ids.len() > 3 {
                            return Err(format!(
                                "planned level {index} needs one to three evidence IDs"
                            ));
                        }
                        let mut seen = HashSet::with_capacity(evidence_ids.len());
                        for evidence_id in &evidence_ids {
                            if !seen.insert(evidence_id.as_str()) {
                                return Err(format!(
                                    "planned level {index} repeats evidence candidate '{evidence_id}'"
                                ));
                            }
                            if !candidate_ids.contains(evidence_id.as_str()) {
                                return Err(format!(
                                    "planned level {index} cites unknown evidence candidate '{evidence_id}'"
                                ));
                            }
                        }
                        if reason.is_some() {
                            return Err(format!(
                                "planned generated level {index} must use null reason"
                            ));
                        }
                        if *expected == QaType::Conceptual {
                            let relation = relation.as_deref().ok_or_else(|| {
                                format!("planned conceptual level {index} needs a relation")
                            })?;
                            if !conceptual_relation_is_supported(relation) {
                                return Err(format!(
                                    "planned conceptual level {index} has unsupported relation '{relation}'"
                                ));
                            }
                        } else if relation.is_some() {
                            return Err(format!(
                                "planned non-conceptual level {index} must use null relation"
                            ));
                        }
                        levels.push(PlannedQaLevel::Generate {
                            bloom_level: level,
                            relation,
                            evidence_ids,
                        });
                    }
                    "skip" => {
                        if relation.is_some() {
                            return Err(format!(
                                "planned skip level {index} must use null relation"
                            ));
                        }
                        let reason = reason.ok_or_else(|| {
                            format!("planned skip level {index} needs a reason")
                        })?;
                        if reason != support_absent_reason(*expected) || !evidence_ids.is_empty() {
                            return Err(format!(
                                "planned skip level {index} must use '{}' with no evidence",
                                support_absent_reason(*expected)
                            ));
                        }
                        levels.push(PlannedQaLevel::Skipped {
                            bloom_level: level,
                            reason,
                        });
                    }
                    _ => {
                        return Err(format!(
                            "planned level {index} has unsupported disposition '{disposition}'"
                        ));
                    }
                }
            }
            Ok(QaDispositionPlan { levels })
        }
        _ => Err("QA disposition plan must be one canonical prompt-wide skip or a clean ordered level plan".to_string()),
    }
}

/// Render the evidence-constrained writer request for planned generated levels.
pub(crate) fn render_planned_qa_messages(
    prompt: &PreparedQaPrompt,
    plan: &QaDispositionPlan,
) -> Result<Option<[ChatMessage; 2]>, McpToolError> {
    prompt.validate()?;
    let candidates = evidence_candidates(prompt);
    let candidates_by_id = candidates
        .iter()
        .map(|candidate| (candidate.local_id.as_str(), candidate))
        .collect::<HashMap<_, _>>();
    let mut planned_levels = Vec::new();
    for level in &plan.levels {
        let PlannedQaLevel::Generate {
            bloom_level,
            relation,
            evidence_ids,
        } = level
        else {
            continue;
        };
        let evidence = evidence_ids
            .iter()
            .map(|evidence_id| {
                let candidate = candidates_by_id.get(evidence_id.as_str()).ok_or_else(|| {
                    McpToolError::internal(format!(
                        "Planned evidence candidate '{evidence_id}' disappeared before QA writing"
                    ))
                })?;
                Ok(json!({
                    "id": evidence_id,
                    "text": crate::guard_content(&candidate.quote),
                }))
            })
            .collect::<Result<Vec<_>, McpToolError>>()?;
        planned_levels.push(json!({
            "level": bloom_level,
            "conceptual_relation": relation,
            "evidence": evidence,
        }));
    }
    if planned_levels.is_empty() {
        return Ok(None);
    }
    let user = serde_json::to_string(&json!({
        "generation_protocol": QA_GENERATION_PROTOCOL,
        "planned_levels": planned_levels,
    }))
    .map_err(|error| McpToolError::internal(format!("Cannot render planned QA: {error}")))?;
    let system = format!(
        "{CONTENT_GUARD_INSTRUCTION}Write exactly {} ordered QA objects for the supplied planned levels. Evidence and dispositions are already fixed: do not add, remove, reorder, relabel, or skip a level, and do not select new evidence. Return one outer JSON array containing every object; never emit separate arrays or any prose before, between, or after them. Each object has exactly level, question, and answer, for example {{\"level\":\"factual\",\"question\":\"What is stated?\",\"answer\":\"The stated fact.\"}}. The question premise and every answer claim must be entailed by the supplied evidence alone. A factual level asks only what, which, who, when, or how many is directly stated; never turn sequence or timing into causation, reverse a condition, or imply that an action already happened. A statement that it is time to act when X does not mean X occurs when or because the action is taken. Do not ask for a complete list unless the supplied evidence contains the complete list. Preserve negation and modality exactly: hope, may, likely, and possibility are not facts or purposes. Use a why-question only when the evidence explicitly states the cause or purpose. Do not remove not, cannot, or another qualification. Do not replace a source term with a broader consequence such as viability. Preserve the source category name and intentionality; never substitute an accidental, opposite, or merely related label. Do not invent advice, a normative should, a causal mechanism, or which component changed unless the evidence states it. For conceptual QA, the question and answer must explain the named relation rather than retrieve or paraphrase a list, label, title, number, or stated phrase. For an exemplification relationship, synthesize how the supplied examples support the passage's stated general claim; do not ask how one example's label relates to its own stated implication. Return compact JSON only.",
        planned_levels.len(),
    );
    Ok(Some([
        ChatMessage {
            role: "system".to_string(),
            content: system,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user,
        },
    ]))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedQaDraft {
    level: String,
    question: String,
    answer: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PreparedQaVerdictKind {
    Accept,
    Correct,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PreparedQaVerdict {
    level: String,
    verdict: PreparedQaVerdictKind,
    subject: bool,
    condition: bool,
    premise: bool,
    entailment: bool,
    completeness: bool,
    actual_difficulty: bool,
    findings: Vec<String>,
}

impl PreparedQaVerdict {
    fn all_checks_pass(&self) -> bool {
        self.subject
            && self.condition
            && self.premise
            && self.entailment
            && self.completeness
            && self.actual_difficulty
    }
}

pub(crate) struct QaVerificationVerdicts {
    levels: Vec<PreparedQaVerdict>,
}

impl QaVerificationVerdicts {
    pub fn requires_correction(&self) -> bool {
        self.levels
            .iter()
            .any(|level| level.verdict == PreparedQaVerdictKind::Correct)
    }

    pub fn correction_findings(&self) -> String {
        self.levels
            .iter()
            .filter(|level| level.verdict == PreparedQaVerdictKind::Correct)
            .map(|level| format!("{}: {}", level.level, level.findings.join("; ")))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// Render independent checks only. Self-Refine (arXiv:2303.17651) supplies
/// feedback/refine separation; Chain-of-Verification (arXiv:2309.11495) supplies
/// independent checks. A distinct model mitigates MT-Bench self-enhancement bias
/// (arXiv:2306.05685), while the bounded recovery block preserves N-version
/// design diversity by preventing the verifier from becoming a second writer.
pub(crate) fn render_planned_qa_review_messages(
    prompt: &PreparedQaPrompt,
    plan: &QaDispositionPlan,
    proposed_qa: &str,
) -> Result<[ChatMessage; 2], McpToolError> {
    let mut messages = render_planned_qa_messages(prompt, plan)?.ok_or_else(|| {
        McpToolError::internal("Cannot review QA when the disposition plan has no generated levels")
    })?;
    messages[0].content = format!(
        "{CONTENT_GUARD_INSTRUCTION}Independently verify each proposed QA object against only its fixed evidence and plan. Return exactly one ordered verdict object per generated level and nothing else. Each object has exactly level, verdict, subject, condition, premise, entailment, completeness, actual_difficulty, and findings. verdict is accept or correct. subject checks that the grammatical subject and source category are unchanged. condition checks conditions, negation, modality, timing, and intentionality. premise checks that the question assumes nothing unstated. entailment checks every question and answer claim against the evidence. completeness checks that the answer fully answers the bounded question without claiming a complete list absent complete evidence. actual_difficulty checks that the QA performs the planned Bloom level and conceptual relation rather than easier recall. accept requires all six checks true and findings []. correct requires at least one false check and one or more specific nonblank findings. Level support and relation are already fixed by the plan; judge only the generated QA and never replace rejection with a support skip. Do not output question, answer, replacement_qa, revised_qa, or any other QA content; the generator alone writes QA. Protocol: {QA_VERIFICATION_PROTOCOL}."
    );
    let mut user: Value = serde_json::from_str(&messages[1].content).map_err(|error| {
        McpToolError::internal(format!("Cannot parse rendered QA writer request: {error}"))
    })?;
    let object = user
        .as_object_mut()
        .ok_or_else(|| McpToolError::internal("Rendered QA writer request is not a JSON object"))?;
    object.insert(
        "verification_protocol".to_string(),
        Value::String(QA_VERIFICATION_PROTOCOL.to_string()),
    );
    object.insert(
        "proposed_qa".to_string(),
        serde_json::from_str(proposed_qa).unwrap_or_else(|_| Value::String(proposed_qa.into())),
    );
    messages[1].content = serde_json::to_string(&user).map_err(|error| {
        McpToolError::internal(format!("Cannot render planned QA review: {error}"))
    })?;
    Ok(messages)
}

pub(crate) fn parse_planned_qa_verdicts(
    response: &str,
    plan: &QaDispositionPlan,
) -> Result<QaVerificationVerdicts, String> {
    let levels: Vec<PreparedQaVerdict> = serde_json::from_str(response)
        .map_err(|error| format!("invalid compact QA verification verdict JSON: {error}"))?;
    let generated_levels = plan
        .levels
        .iter()
        .filter_map(|level| match level {
            PlannedQaLevel::Generate { bloom_level, .. } => Some(bloom_level.as_str()),
            PlannedQaLevel::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();
    if levels.len() != generated_levels.len() {
        return Err(format!(
            "expected {} QA verification verdicts, received {}",
            generated_levels.len(),
            levels.len()
        ));
    }
    for (index, (level, expected)) in levels.iter().zip(generated_levels).enumerate() {
        if level.level != expected {
            return Err(format!(
                "verification level {index} expected '{expected}', received '{}'",
                level.level
            ));
        }
        if level
            .findings
            .iter()
            .any(|finding| finding.trim().is_empty())
        {
            return Err(format!(
                "verification level {index} contains a blank finding"
            ));
        }
        match level.verdict {
            PreparedQaVerdictKind::Accept
                if level.all_checks_pass() && level.findings.is_empty() => {}
            PreparedQaVerdictKind::Correct
                if !level.all_checks_pass() && !level.findings.is_empty() => {}
            PreparedQaVerdictKind::Accept => {
                return Err(format!(
                    "verification level {index} accept requires all checks true and no findings"
                ));
            }
            PreparedQaVerdictKind::Correct => {
                return Err(format!(
                    "verification level {index} correct requires a false check and nonempty findings"
                ));
            }
        }
    }
    Ok(QaVerificationVerdicts { levels })
}

pub(crate) fn render_planned_qa_correction_messages(
    prompt: &PreparedQaPrompt,
    plan: &QaDispositionPlan,
    proposed_qa: &str,
    verdicts: &QaVerificationVerdicts,
) -> Result<[ChatMessage; 2], McpToolError> {
    if !verdicts.requires_correction() {
        return Err(McpToolError::internal(
            "Cannot correct QA when every verification verdict accepts",
        ));
    }
    let mut messages = render_planned_qa_messages(prompt, plan)?.ok_or_else(|| {
        McpToolError::internal(
            "Cannot correct QA when the disposition plan has no generated levels",
        )
    })?;
    messages[0].content.push_str(
        " This is the single bounded refinement step. Correct the proposed QA using only the typed verification findings. The supplied plan, levels, conceptual relations, and evidence are fixed. Return the complete corrected ordered named-object array in the original writer schema only; do not output verdicts or commentary.",
    );
    let mut user: Value = serde_json::from_str(&messages[1].content).map_err(|error| {
        McpToolError::internal(format!(
            "Cannot parse rendered QA correction request: {error}"
        ))
    })?;
    let object = user.as_object_mut().ok_or_else(|| {
        McpToolError::internal("Rendered QA correction request is not a JSON object")
    })?;
    object.insert(
        "proposed_qa".to_string(),
        serde_json::from_str(proposed_qa).unwrap_or_else(|_| Value::String(proposed_qa.into())),
    );
    object.insert(
        "verification_findings".to_string(),
        serde_json::to_value(&verdicts.levels).map_err(|error| {
            McpToolError::internal(format!(
                "Cannot serialize QA verification findings: {error}"
            ))
        })?,
    );
    messages[1].content = serde_json::to_string(&user).map_err(|error| {
        McpToolError::internal(format!("Cannot render planned QA correction: {error}"))
    })?;
    Ok(messages)
}

pub(crate) fn enforce_reviewed_adjudication(
    plan: QaDispositionPlan,
    reviewed: &ReviewedQaAdjudication,
) -> Result<QaDispositionPlan, String> {
    if plan.levels.len() != reviewed.levels().len() {
        return Err(format!(
            "reviewed adjudication has {} levels but generator plan has {}",
            reviewed.levels().len(),
            plan.levels.len()
        ));
    }
    for (index, (planned, mandate)) in plan.levels.iter().zip(reviewed.levels()).enumerate() {
        match (planned, mandate) {
            (
                PlannedQaLevel::Generate { relation, .. },
                ReviewedLevelDecision::Generate {
                    relation: mandated_relation,
                },
            ) if relation == mandated_relation => {}
            (
                PlannedQaLevel::Generate { relation, .. },
                ReviewedLevelDecision::Generate {
                    relation: mandated_relation,
                },
            ) => {
                return Err(format!(
                    "reviewed mandate for level {index} requires generate relation {:?}, generator returned relation {:?}",
                    mandated_relation, relation
                ));
            }
            (
                PlannedQaLevel::Skipped { reason, .. },
                ReviewedLevelDecision::Skip {
                    reason: mandated_reason,
                },
            ) if reason == mandated_reason => {}
            (
                PlannedQaLevel::Skipped { reason, .. },
                ReviewedLevelDecision::Skip {
                    reason: mandated_reason,
                },
            ) => {
                return Err(format!(
                    "reviewed mandate for level {index} requires skip reason '{mandated_reason}', generator returned skip reason '{reason}'"
                ));
            }
            (PlannedQaLevel::Generate { .. }, ReviewedLevelDecision::Skip { reason }) => {
                return Err(format!(
                    "reviewed mandate for level {index} requires skip reason '{reason}', generator returned generate"
                ));
            }
            (
                PlannedQaLevel::Skipped { reason, .. },
                ReviewedLevelDecision::Generate { relation },
            ) => {
                return Err(format!(
                    "reviewed mandate for level {index} requires generate relation {:?}, generator returned skip reason '{reason}'",
                    relation
                ));
            }
        }
    }
    Ok(plan)
}

pub(crate) fn merge_disposition_plans(
    plan: &QaDispositionPlan,
    writer_response: Option<&str>,
) -> Result<String, String> {
    let mut drafts = match writer_response {
        Some(response) => serde_json::from_str::<Vec<PreparedQaDraft>>(response)
            .map_err(|error| format!("invalid compact planned QA JSON: {error}"))?
            .into_iter(),
        None if !plan.needs_writer() => Vec::new().into_iter(),
        None => return Err("planned generated levels require a writer response".to_string()),
    };
    let mut response = Vec::with_capacity(plan.levels.len());
    for (index, level) in plan.levels.iter().enumerate() {
        match level {
            PlannedQaLevel::Generate {
                bloom_level,
                evidence_ids,
                ..
            } => {
                let PreparedQaDraft {
                    level,
                    question,
                    answer,
                } = drafts
                    .next()
                    .ok_or_else(|| format!("writer omitted planned generated level {index}"))?;
                if level != *bloom_level {
                    return Err(format!(
                        "writer level {index} expected '{bloom_level}', received '{level}'"
                    ));
                }
                if question.trim().is_empty() || answer.trim().is_empty() {
                    return Err(format!(
                        "writer level {index} needs a nonblank question and answer"
                    ));
                }
                response.push(json!([level, question, answer, evidence_ids]));
            }
            PlannedQaLevel::Skipped {
                bloom_level,
                reason,
            } => response.push(json!([bloom_level, null, reason, []])),
        }
    }
    if drafts.next().is_some() {
        return Err("writer returned more QA rows than the disposition plan".to_string());
    }
    Ok(Value::Array(response).to_string())
}

#[derive(Deserialize)]
struct PreparedQaPair(String, Option<String>, String, Vec<String>);

pub(crate) fn support_absent_reason(qa_type: QaType) -> &'static str {
    match qa_type {
        QaType::Factual => "factual_support_absent",
        QaType::Conceptual => "conceptual_support_absent",
        QaType::Analyze => "analyze_support_absent",
        QaType::Evaluate => "evaluate_support_absent",
        QaType::Create => "create_support_absent",
    }
}

fn parse_prepared_qa_response(
    response: &str,
    prompt: &PreparedQaPrompt,
) -> Result<Vec<QaLevelDisposition>, String> {
    let raw: Vec<PreparedQaPair> = serde_json::from_str(response)
        .map_err(|error| format!("invalid compact QA JSON: {error}"))?;
    if raw.len() != prompt.qa_types.len() {
        return Err(format!(
            "expected {} QA level dispositions, received {}",
            prompt.qa_types.len(),
            raw.len()
        ));
    }
    let candidates = evidence_candidates(prompt);
    let candidates_by_id = candidates
        .iter()
        .map(|candidate| (candidate.local_id.as_str(), candidate))
        .collect::<HashMap<_, _>>();
    let dispositions = raw
        .into_iter()
        .zip(&prompt.qa_types)
        .enumerate()
        .map(
            |(index, (PreparedQaPair(level, question, answer, evidence_ids), expected))| {
                if level != expected.as_str() {
                    return Err(format!(
                        "pair {index} expected Bloom level '{}', received '{level}'",
                        expected.as_str()
                    ));
                }
                let Some(question) = question else {
                    if !evidence_ids.is_empty() {
                        return Err(format!(
                            "skip {index} must carry an empty evidence ID list"
                        ));
                    }
                    let reason = answer.trim();
                    if reason != "non_substantive_passage"
                        && reason != "contaminated_or_garbled"
                        && reason != support_absent_reason(*expected)
                    {
                        return Err(format!(
                            "skip {index} has unsupported reason '{reason}' for level '{}'",
                            expected.as_str()
                        ));
                    }
                    return Ok(QaLevelDisposition::Skipped {
                        bloom_level: level,
                        reason: reason.to_string(),
                    });
                };
                if question.trim().is_empty()
                    || answer.trim().is_empty()
                    || evidence_ids.is_empty()
                    || evidence_ids.len() > 3
                {
                    return Err(format!(
                        "pair {index} needs nonblank question and answer plus one to three evidence IDs"
                    ));
                }
                let mut seen = HashSet::with_capacity(evidence_ids.len());
                let mut evidence_quotes = Vec::with_capacity(evidence_ids.len());
                for evidence_id in evidence_ids {
                    if !seen.insert(evidence_id.clone()) {
                        return Err(format!(
                            "pair {index} repeats evidence candidate '{evidence_id}'"
                        ));
                    }
                    let candidate = candidates_by_id.get(evidence_id.as_str()).ok_or_else(|| {
                        format!("pair {index} cites unknown evidence candidate '{evidence_id}'")
                    })?;
                    evidence_quotes.push(hkask_types::corpus::QaEvidence {
                        chunk_ref: prompt.primary().chunk_ref.clone(),
                        source: prompt.primary().source.clone(),
                        quote: candidate.quote.clone(),
                    });
                }
                Ok(QaLevelDisposition::Generated(QaPair {
                    question,
                    answer,
                    bloom_level: level,
                    evidence_quotes,
                }))
            },
        )
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(global_reason) = dispositions.iter().find_map(|disposition| {
        let QaLevelDisposition::Skipped { reason, .. } = disposition else {
            return None;
        };
        matches!(
            reason.as_str(),
            "non_substantive_passage" | "contaminated_or_garbled"
        )
        .then_some(reason)
    }) {
        let all_levels_skipped_for_same_reason = dispositions.iter().all(|disposition| {
            matches!(
                disposition,
                QaLevelDisposition::Skipped { reason, .. } if reason == global_reason
            )
        });
        if !all_levels_skipped_for_same_reason {
            return Err(format!(
                "prompt-wide quality reason '{global_reason}' must skip every requested level"
            ));
        }
    }
    Ok(dispositions)
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

pub(crate) struct QaResponseMetadata {
    pub tokens_used: u64,
    pub completion_tokens: Option<u64>,
    pub finish_reason: Option<String>,
    pub cost_usd: Option<f64>,
}

pub(crate) struct QaCompletion {
    pub text: String,
    pub rejection: Option<String>,
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
    qa_levels_requested: usize,
    qa_rows_written: usize,
    qa_levels_skipped: usize,
    skip_reason_counts: BTreeMap<String, usize>,
    tokens_used: u64,
    completion_tokens_used: u64,
    completion_token_reports: usize,
    finish_reason_counts: BTreeMap<String, usize>,
    finish_reason_reports: usize,
    provider_responses: usize,
    cost_reports: usize,
    reported_cost_usd: f64,
    verification_model: Option<String>,
    adjudication_protocol: Option<String>,
    reviewed_passage_admits: usize,
    reviewed_passage_skips: usize,
    reviewed_level_generates: usize,
    reviewed_level_skips: usize,
}

impl<W: Write> QaOutput<W> {
    pub fn new(writer: W, prompts_total: usize) -> Self {
        Self {
            writer,
            prompts_total,
            prompts_succeeded: 0,
            prompts_failed: 0,
            qa_levels_requested: 0,
            qa_rows_written: 0,
            qa_levels_skipped: 0,
            skip_reason_counts: BTreeMap::new(),
            tokens_used: 0,
            completion_tokens_used: 0,
            completion_token_reports: 0,
            finish_reason_counts: BTreeMap::new(),
            finish_reason_reports: 0,
            provider_responses: 0,
            cost_reports: 0,
            reported_cost_usd: 0.0,
            verification_model: None,
            adjudication_protocol: None,
            reviewed_passage_admits: 0,
            reviewed_passage_skips: 0,
            reviewed_level_generates: 0,
            reviewed_level_skips: 0,
        }
    }

    pub fn set_verification_model(&mut self, model: &str) {
        self.verification_model = Some(model.to_string());
    }

    pub fn set_reviewed_adjudications(
        &mut self,
        protocol: &str,
        passage_admits: usize,
        passage_skips: usize,
        level_generates: usize,
        level_skips: usize,
    ) {
        self.adjudication_protocol = Some(protocol.to_string());
        self.reviewed_passage_admits = passage_admits;
        self.reviewed_passage_skips = passage_skips;
        self.reviewed_level_generates = level_generates;
        self.reviewed_level_skips = level_skips;
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

    fn record_response(&mut self, metadata: &QaResponseMetadata) {
        self.tokens_used += metadata.tokens_used;
        self.provider_responses += 1;
        if let Some(completion_tokens) = metadata.completion_tokens {
            self.completion_tokens_used += completion_tokens;
            self.completion_token_reports += 1;
        }
        if let Some(finish_reason) = metadata.finish_reason.as_ref() {
            *self
                .finish_reason_counts
                .entry(finish_reason.clone())
                .or_default() += 1;
            self.finish_reason_reports += 1;
        }
        if let Some(cost_usd) = metadata.cost_usd {
            self.cost_reports += 1;
            self.reported_cost_usd += cost_usd;
        }
    }

    pub fn complete_reviewed_skip(
        &mut self,
        prompt: &PreparedQaPrompt,
        reason: &str,
        model: &str,
    ) -> Result<(), McpToolError> {
        self.qa_levels_requested += prompt.qa_types.len();
        for qa_type in &prompt.qa_types {
            self.write_record(&qa_skip_envelope(
                prompt,
                qa_type.as_str(),
                reason,
                model,
                self.verification_model.as_deref(),
                self.adjudication_protocol.as_deref(),
            ))?;
            self.qa_levels_skipped += 1;
            *self
                .skip_reason_counts
                .entry(reason.to_string())
                .or_default() += 1;
        }
        self.prompts_succeeded += 1;
        if (self.prompts_succeeded + self.prompts_failed).is_multiple_of(10) {
            self.flush()?;
        }
        Ok(())
    }

    /// expect: A success count means accepted QA rows were written, not merely attempted.
    /// [P9] Motivating: Every prompt gets one truthful terminal outcome.
    /// pre: prompt was validated and is completed exactly once by its transport.
    /// post: malformed or failed inference emits an identified error row; output failures propagate.
    #[cfg(test)]
    pub fn complete(
        &mut self,
        prompt: &PreparedQaPrompt,
        completion: Result<QaCompletion, QaCompletionError>,
        model: &str,
    ) -> Result<(), McpToolError> {
        self.complete_with_prior(prompt, completion, &[], model)
    }

    pub fn complete_with_prior(
        &mut self,
        prompt: &PreparedQaPrompt,
        completion: Result<QaCompletion, QaCompletionError>,
        prior_responses: &[QaResponseMetadata],
        model: &str,
    ) -> Result<(), McpToolError> {
        self.qa_levels_requested += prompt.qa_types.len();
        for metadata in prior_responses {
            self.record_response(metadata);
        }
        let mut completion_metadata = None;
        let parsed = match completion {
            Ok(completion) => {
                let metadata = QaResponseMetadata {
                    tokens_used: completion.tokens_used,
                    completion_tokens: completion.completion_tokens,
                    finish_reason: completion.finish_reason.clone(),
                    cost_usd: completion.cost_usd,
                };
                self.record_response(&metadata);
                completion_metadata =
                    Some((completion.completion_tokens, completion.finish_reason));
                match completion.rejection {
                    Some(error) => Err(QaCompletionError::Rejected(error)),
                    None => parse_prepared_qa_response(
                        &extract_json_from_response(&completion.text),
                        prompt,
                    )
                    .map_err(QaCompletionError::Rejected),
                }
            }
            Err(error) => Err(error),
        };
        match parsed {
            Ok(dispositions) => {
                for disposition in dispositions {
                    match disposition {
                        QaLevelDisposition::Generated(pair) => {
                            self.write_record(&qa_result_envelope(
                                prompt,
                                pair,
                                model,
                                self.verification_model.as_deref(),
                                self.adjudication_protocol.as_deref(),
                            ))?;
                            self.qa_rows_written += 1;
                        }
                        QaLevelDisposition::Skipped {
                            bloom_level,
                            reason,
                        } => {
                            self.write_record(&qa_skip_envelope(
                                prompt,
                                &bloom_level,
                                &reason,
                                model,
                                self.verification_model.as_deref(),
                                self.adjudication_protocol.as_deref(),
                            ))?;
                            self.qa_levels_skipped += 1;
                            *self.skip_reason_counts.entry(reason).or_default() += 1;
                        }
                    }
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
        let expected_inferred_prompts = self
            .prompts_total
            .saturating_sub(self.reviewed_passage_skips);
        let cost_reporting_complete = self.provider_responses >= expected_inferred_prompts
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
            "qa_levels_requested": self.qa_levels_requested,
            "qa_rows_written": self.qa_rows_written,
            "qa_levels_skipped": self.qa_levels_skipped,
            "skip_reason_counts": self.skip_reason_counts,
            "verification_model": self.verification_model,
            "adjudication_protocol": self.adjudication_protocol,
            "reviewed_passage_admits": self.reviewed_passage_admits,
            "reviewed_passage_skips": self.reviewed_passage_skips,
            "reviewed_level_generates": self.reviewed_level_generates,
            "reviewed_level_skips": self.reviewed_level_skips,
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
        temperature: 0.0,
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
fn qa_result_envelope(
    prompt: &PreparedQaPrompt,
    pair: QaPair,
    model: &str,
    verification_model: Option<&str>,
    adjudication_protocol: Option<&str>,
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
            "verification_model": verification_model,
            "adjudication_protocol": adjudication_protocol,
            "passage_quality_protocol": PASSAGE_QUALITY_PROTOCOL,
            "disposition_plan_protocol": QA_DISPOSITION_PROTOCOL,
            "prompt_protocol": QA_GENERATION_PROTOCOL,
            "prepared_prompt_protocol": PREPARED_QA_PROTOCOL,
            "prompt_id": prompt.prompt_id,
            "source_chunk_ref": prompt.primary().chunk_ref,
        },
    })
}

fn qa_skip_envelope(
    prompt: &PreparedQaPrompt,
    bloom_level: &str,
    reason: &str,
    model: &str,
    verification_model: Option<&str>,
    adjudication_protocol: Option<&str>,
) -> serde_json::Value {
    json!({
        "prompt_id": prompt.prompt_id,
        "chunk_ref": prompt.primary().chunk_ref,
        "source": prompt.primary().source,
        "qa_type": bloom_level,
        "status": "skipped",
        "reason": reason,
        "provenance": {
            "generator_model": model,
            "verification_model": verification_model,
            "adjudication_protocol": adjudication_protocol,
            "passage_quality_protocol": PASSAGE_QUALITY_PROTOCOL,
            "disposition_plan_protocol": QA_DISPOSITION_PROTOCOL,
            "prompt_protocol": QA_GENERATION_PROTOCOL,
            "prepared_prompt_protocol": PREPARED_QA_PROTOCOL,
            "prompt_id": prompt.prompt_id,
            "source_chunk_ref": prompt.primary().chunk_ref,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: QA generation is deterministic and never enables model thinking.
    #[test]
    fn qa_generation_uses_deterministic_sampling() {
        let parameters = qa_llm_parameters();
        assert_eq!(parameters.temperature, 0.0);
        assert!(!parameters.thinking_allowed);
    }

    /// expect: The model selects server-owned evidence IDs instead of reproducing source bytes.
    #[test]
    fn prepared_prompt_uses_guarded_evidence_candidates() -> Result<(), Box<dyn std::error::Error>>
    {
        let messages = render_disposition_plan_messages(&prepared(), None)?;
        assert!(messages[0].content.contains(CONTENT_GUARD_INSTRUCTION));
        assert!(messages[0].content.contains("evidence IDs"));
        let user: serde_json::Value = serde_json::from_str(&messages[1].content)?;
        assert_eq!(user["evidence_candidates"][0]["id"], "e0");
        assert!(user.get("reviewed_level_mandates").is_none());
        assert!(
            user["evidence_candidates"][0]["text"]
                .as_str()
                .is_some_and(|text| text.contains("The answer is grounded here."))
        );
        assert!(messages[0].content.contains("[\"e0\"]"));
        assert!(!messages[0].content.contains("exact quote"));
        Ok(())
    }

    /// expect: Candidate quotes remain exact source substrings while covering the primary passage.
    #[test]
    fn evidence_candidates_are_exact_and_cover_primary_text() {
        let mut prompt = prepared();
        prompt.passages[0].text = (0..80)
            .map(|index| format!("word{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        let candidates = evidence_candidates(&prompt);
        assert!(candidates.len() > 1);
        assert_eq!(candidates[0].local_id, "e0");
        assert!(candidates[0].quote.starts_with("word0 "));
        assert!(
            candidates
                .last()
                .is_some_and(|last| last.quote.ends_with("word79"))
        );
        assert!(
            candidates
                .iter()
                .all(|candidate| prompt.primary().text.contains(&candidate.quote))
        );
    }

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
            text: json!([["factual", "Question?", "Answer.", ["e0"]]]).to_string(),
            rejection: None,
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
            r#"[["factual","","answer",["e0"]]]"#,
            r#"[["factual","question"," ",["e0"]]]"#,
            r#"[["create","question","answer",["e0"]]]"#,
            r#"[["factual","question","answer",["e0"]],["factual","extra","answer",["e0"]]]"#,
            r#"[["factual","question","answer",["e9"]]]"#,
            r#"[["factual","question","answer",[]]]"#,
            r#"[["factual","question","answer",["e0","e0"]]]"#,
            r#"[["factual","question","answer",[["p0","grounded"]]]]"#,
        ] {
            let mut bytes = Vec::new();
            let mut output = QaOutput::new(&mut bytes, 1);
            output.complete(
                &prepared(),
                Ok(QaCompletion {
                    text: response.into(),
                    rejection: None,
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
    fn qa_llm_parameters_remain_deterministic() {
        let params = qa_llm_parameters();
        assert_eq!(params.temperature, 0.0);
        assert_eq!(params.top_p, 0.95);
        assert!(!params.thinking_allowed);
    }

    /// expect: Conceptual generation cannot proceed without a closed supported relation.
    #[test]
    fn disposition_plan_rejects_conceptual_generation_without_relation() {
        let mut prompt = prepared();
        prompt.qa_types = vec![QaType::Factual, QaType::Conceptual];
        let response = json!([
            "clean",
            [
                {"level":"factual","disposition":"generate","relation":null,"reason":null,"evidence_ids":["e0"]},
                {"level":"conceptual","disposition":"generate","relation":null,"reason":null,"evidence_ids":["e0"]}
            ]
        ])
        .to_string();
        let error = parse_disposition_plan_response(&response, &prompt)
            .err()
            .expect("missing conceptual relation must be rejected");
        assert!(error.contains("needs a relation"));
    }

    /// expect: Every closed conceptual relation kind is accepted and no private label is admitted.
    #[test]
    fn disposition_plan_uses_the_closed_conceptual_relation_vocabulary() {
        let mut prompt = prepared();
        prompt.qa_types = vec![QaType::Conceptual];
        for relation in [
            "mechanism",
            "relationship",
            "causal_relationship",
            "distinction",
            "purpose",
            "framework",
            "transferable_principle",
        ] {
            let response = json!([
                "clean",
                [{"level":"conceptual","disposition":"generate","relation":relation,"reason":null,"evidence_ids":["e0"]}]
            ])
            .to_string();
            assert!(parse_disposition_plan_response(&response, &prompt).is_ok());
        }
        let response = json!([
            "clean",
            [{"level":"conceptual","disposition":"generate","relation":"private_relation","reason":null,"evidence_ids":["e0"]}]
        ])
        .to_string();
        assert!(parse_disposition_plan_response(&response, &prompt).is_err());
    }

    /// expect: Planned generation can use only unique evidence identities owned by the server.
    #[test]
    fn disposition_plan_rejects_unknown_or_repeated_evidence() {
        let prompt = prepared();
        for evidence in [json!(["missing"]), json!(["e0", "e0"])] {
            let response = json!([
                "clean",
                [{"level":"factual","disposition":"generate","relation":null,"reason":null,"evidence_ids":evidence}]
            ])
            .to_string();
            assert!(parse_disposition_plan_response(&response, &prompt).is_err());
        }
    }

    /// expect: The writer preserves source modality and never invents normative or causal premises.
    #[test]
    fn planned_writer_contract_forbids_semantic_leaks() {
        let prompt = prepared();
        let plan = parse_disposition_plan_response(
            &json!([
                "clean",
                [{"level":"factual","disposition":"generate","relation":null,"reason":null,"evidence_ids":["e0"]}]
            ])
            .to_string(),
            &prompt,
        )
        .expect("valid plan");
        let messages = render_planned_qa_messages(&prompt, &plan)
            .expect("render")
            .expect("writer required");
        let system = &messages[0].content;
        assert!(system.contains("does not mean X occurs"));
        assert!(system.contains("factual level asks only"));
        assert!(system.contains("Preserve negation and modality exactly"));
        assert!(system.contains("normative should"));
        assert!(system.contains("why-question only when"));
        assert!(system.contains("causal mechanism"));
        assert!(system.contains("source category name and intentionality"));
        assert!(system.contains("do not select new evidence"));
        assert!(system.contains("one outer JSON array"));
    }

    /// expect: Verification is a strict verdict channel: accept is internally
    /// consistent, correct carries actionable findings, and QA fields are forbidden.
    #[test]
    fn typed_qa_verdicts_enforce_checks_findings_and_writer_ownership() {
        let prompt = prepared();
        let plan = parse_disposition_plan_response(
            &json!([
                "clean",
                [{"level":"factual","disposition":"generate","relation":null,"reason":null,"evidence_ids":["e0"]}]
            ])
            .to_string(),
            &prompt,
        )
        .expect("valid plan");
        let accepted = json!([{
            "level":"factual",
            "verdict":"accept",
            "subject":true,
            "condition":true,
            "premise":true,
            "entailment":true,
            "completeness":true,
            "actual_difficulty":true,
            "findings":[]
        }])
        .to_string();
        assert!(
            !parse_planned_qa_verdicts(&accepted, &plan)
                .expect("valid accept")
                .requires_correction()
        );

        let correction = json!([{
            "level":"factual",
            "verdict":"correct",
            "subject":false,
            "condition":true,
            "premise":true,
            "entailment":true,
            "completeness":true,
            "actual_difficulty":true,
            "findings":["Restore the source subject."]
        }])
        .to_string();
        assert!(
            parse_planned_qa_verdicts(&correction, &plan)
                .expect("valid correction")
                .requires_correction()
        );

        let mut conceptual_prompt = prepared();
        conceptual_prompt.qa_types = vec![QaType::Factual, QaType::Conceptual];
        let conceptual_plan = parse_disposition_plan_response(
            &json!([
                "clean",
                [
                    {"level":"factual","disposition":"generate","relation":null,"reason":null,"evidence_ids":["e0"]},
                    {"level":"conceptual","disposition":"generate","relation":"mechanism","reason":null,"evidence_ids":["e0"]}
                ]
            ])
            .to_string(),
            &conceptual_prompt,
        )
        .expect("valid conceptual plan");
        let terminal_skip = json!([
            {
                "level":"factual","verdict":"accept","subject":true,"condition":true,
                "premise":true,"entailment":true,"completeness":true,
                "actual_difficulty":true,"findings":[]
            },
            {
                "level":"conceptual","verdict":"skip","subject":true,"condition":true,
                "premise":true,"entailment":true,"completeness":true,
                "actual_difficulty":false,"findings":["The fixed evidence cannot support the planned mechanism."]
            }
        ])
        .to_string();
        assert!(parse_planned_qa_verdicts(&terminal_skip, &conceptual_plan).is_err());

        let final_correction = json!([
            {
                "level":"factual","verdict":"accept","subject":true,"condition":true,
                "premise":true,"entailment":true,"completeness":true,
                "actual_difficulty":true,"findings":[]
            },
            {
                "level":"conceptual","verdict":"correct","subject":true,"condition":true,
                "premise":false,"entailment":true,"completeness":true,
                "actual_difficulty":true,"findings":["The corrected question still assumes an unstated distinction."]
            }
        ])
        .to_string();
        let final_verdicts = parse_planned_qa_verdicts(&final_correction, &conceptual_plan)
            .expect("valid final correction verdict");
        assert!(final_verdicts.requires_correction());

        for invalid in [
            json!([{
                "level":"factual","verdict":"accept","subject":false,"condition":true,
                "premise":true,"entailment":true,"completeness":true,
                "actual_difficulty":true,"findings":[]
            }]),
            json!([{
                "level":"factual","verdict":"correct","subject":true,"condition":true,
                "premise":true,"entailment":true,"completeness":true,
                "actual_difficulty":true,"findings":["No failed check."]
            }]),
            json!([{
                "level":"factual","verdict":"correct","subject":false,"condition":true,
                "premise":true,"entailment":true,"completeness":true,
                "actual_difficulty":true,"findings":[]
            }]),
            json!([{
                "level":"factual","verdict":"accept","subject":true,"condition":true,
                "premise":true,"entailment":true,"completeness":true,
                "actual_difficulty":true,"findings":[],"question":"Verifier rewrite"
            }]),
        ] {
            assert!(parse_planned_qa_verdicts(&invalid.to_string(), &plan).is_err());
        }
    }

    /// expect: Unsupported or contaminated levels are skipped explicitly instead of
    /// being downgraded to factual recall under a stronger label.
    #[test]
    fn prepared_contract_advertises_quality_gated_skips() {
        let mut prompt = prepared();
        prompt.qa_types = vec![QaType::Factual, QaType::Conceptual];
        let messages = render_disposition_plan_messages(&prompt, None).expect("render");
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("conceptual_support_absent"));
        assert!(rendered.contains("contaminated_or_garbled"));
        assert!(rendered.contains("only retrieve a name"));
        assert!(rendered.contains("components to distinct roles or interactions"));
        assert!(rendered.contains("entry count that constrains a formula"));
        assert!(rendered.contains("result language supports a relationship"));
        assert!(rendered.contains("urges an action to produce an outcome"));
        assert!(rendered.contains("hopes, or preferences alone do not establish why"));
        assert!(rendered.contains("Never emit a generate disposition with an empty evidence list"));
        assert!(rendered.contains("condition, decision, action, and resulting configuration"));
        assert!(rendered.contains("interface controls, media titles, or navigation residue"));
        assert!(
            rendered.contains("bibliographic navigation or an isolated table or figure caption")
        );
        assert!(rendered.contains("isolated anecdote or cross-document fragment"));
        assert!(rendered.contains("single broken word or line-break hyphen"));
        assert!(rendered.contains("merely asserts a relation"));
        assert!(!rendered.contains("Generate exactly 2 source-grounded QA pairs"));
    }

    #[test]
    fn staged_contract_restores_verified_canonical_evidence() {
        let mut prompt = prepared();
        prompt.qa_types = vec![QaType::Factual, QaType::Conceptual];
        let messages = render_disposition_plan_messages(&prompt, None).expect("render");
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("primary_passage"));
        assert!(rendered.contains("e0"));
        assert!(!rendered.contains("chunk-1"));
        assert!(!rendered.contains("source.txt"));

        let response = json!([
            ["factual", "What is grounded?", "The answer.", ["e0"]],
            ["conceptual", "How is it grounded?", "By evidence.", ["e0"]]
        ])
        .to_string();
        let dispositions = parse_prepared_qa_response(&response, &prompt).expect("parse");
        assert_eq!(dispositions.len(), 2);
        for disposition in dispositions {
            let QaLevelDisposition::Generated(pair) = disposition else {
                panic!("supported fixture level must generate QA");
            };
            assert_eq!(pair.evidence_quotes[0].chunk_ref, "chunk-1");
            assert_eq!(pair.evidence_quotes[0].source, "source.txt");
        }
    }

    /// expect: Every requested level receives one explicit terminal disposition;
    /// unsupported conceptual material is visible and never becomes training data.
    #[test]
    fn quality_skip_is_validated_written_and_accounted() -> Result<(), Box<dyn std::error::Error>> {
        let mut prompt = prepared();
        prompt.qa_types = vec![QaType::Factual, QaType::Conceptual];
        let completion = Ok(QaCompletion {
            text: json!([
                ["factual", "What is grounded?", "The answer.", ["e0"]],
                ["conceptual", null, "conceptual_support_absent", []],
            ])
            .to_string(),
            rejection: None,
            tokens_used: 10,
            completion_tokens: Some(5),
            finish_reason: Some("stop".into()),
            cost_usd: Some(0.01),
        });
        let mut bytes = Vec::new();
        let mut output = QaOutput::new(&mut bytes, 1);
        output.complete(&prompt, completion, "offline-model")?;
        let summary = output.finish("unused")?;
        assert_eq!(summary["prompts_succeeded"], 1);
        assert_eq!(summary["prompts_failed"], 0);
        assert_eq!(summary["qa_levels_requested"], 2);
        assert_eq!(summary["qa_rows_written"], 1);
        assert_eq!(summary["qa_levels_skipped"], 1);
        assert_eq!(
            summary["skip_reason_counts"]["conceptual_support_absent"],
            1
        );

        let rows = String::from_utf8(bytes)?
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(rows.len(), 2);
        assert!(rows[0].get("response").is_some());
        assert_eq!(rows[1]["status"], "skipped");
        assert_eq!(rows[1]["qa_type"], "conceptual");
        assert_eq!(rows[1]["reason"], "conceptual_support_absent");
        assert!(rows[1].get("response").is_none());
        assert_eq!(
            rows[1]["provenance"]["prompt_protocol"],
            QA_GENERATION_PROTOCOL
        );
        Ok(())
    }

    #[test]
    fn quality_skip_rejects_ambiguous_or_wrong_level_reasons() {
        let mut prompt = prepared();
        prompt.qa_types = vec![QaType::Conceptual];
        for response in [
            r#"[["conceptual",null,"unsupported",[]]]"#,
            r#"[["conceptual",null,"factual_support_absent",[]]]"#,
            r#"[["conceptual",null,"conceptual_support_absent",["e0"]]]"#,
        ] {
            assert!(parse_prepared_qa_response(response, &prompt).is_err());
        }

        prompt.qa_types = vec![QaType::Factual, QaType::Conceptual];
        let mixed_global_skip = json!([
            ["factual", "What is grounded?", "The answer.", ["e0"]],
            ["conceptual", null, "contaminated_or_garbled", []],
        ])
        .to_string();
        assert!(parse_prepared_qa_response(&mixed_global_skip, &prompt).is_err());
    }
}
