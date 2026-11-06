//! Decisions — direct provider Decisions-API calls for the zed side of the
//! inference IPC bridge (mirrors `rerank.rs`): the zed side holds the API
//! key (read from the keychain via the GPUI credential channel) and calls
//! the provider's Decisions endpoint directly. The MCP server never sees
//! the credential — it sends `InferenceMethod::Decision` over the IPC
//! bridge and receives `InferenceOutcome::DecisionAnswers`.
//!
//! OpenRouter's Decisions API (`POST /api/alpha/decisions`) takes a state
//! plus typed questions and returns typed answers with calibrated
//! probabilities — never text (the System One model class: TypeSafe's Jev
//! et al.). One call answers every question. Only the `score` primitive is
//! wired today (an instruction plus ordered level descriptions, answered
//! with a probability distribution over those levels); the other
//! primitives (choice, noul, box) join when a consumer needs them.

use hkask_types::inference_ipc::{DecisionAnswer, DecisionLevelProbability, DecisionQuestion};
use hkask_types::ports::{DecisionBatch, InferenceUsage};

static DECISIONS_HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn decisions_http_client() -> &'static reqwest::Client {
    DECISIONS_HTTP_CLIENT.get_or_init(reqwest::Client::new)
}

/// Provider-prefixed model → (provider, clean model id). Only OpenRouter
/// has a Decisions endpoint among the registered providers; a model without
/// the `OpenRouter/` prefix is a configuration error, not a transient
/// outage — the caller surfaces it as such.
pub fn detect_decision_provider(model: &str) -> Option<(&'static str, String)> {
    let clean = model.strip_prefix("OpenRouter/")?;
    Some(("openrouter", clean.to_string()))
}

/// The provider's answer for one question, as it appears on the wire. The
/// `legend` (level-index → description echo) is ignored — the caller owns
/// the criteria; unknown fields are skipped by serde.
#[derive(Clone, serde::Deserialize)]
struct DecisionsAnswerWire {
    #[serde(rename = "type")]
    kind: String,
    score: Option<f64>,
    #[serde(default)]
    probabilities: std::collections::HashMap<String, f64>,
    confidence: Option<f64>,
}

#[derive(Default, Clone, serde::Deserialize)]
struct DecisionsUsageWire {
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
}

#[derive(Clone, serde::Deserialize)]
struct DecisionsResponseWire {
    /// The exact snapshot the provider served (e.g.
    /// `typesafe/jev-1.14-20261014`) — carried to the caller as provenance.
    model: String,
    answers: std::collections::HashMap<String, DecisionsAnswerWire>,
    #[serde(default)]
    usage: DecisionsUsageWire,
}

/// A decision-pipeline failure, structured by kind. The IPC server surfaces
/// the Display message verbatim in `InferenceOutcome::Error`.
#[derive(Debug, thiserror::Error)]
pub enum DecisionError {
    #[error("decision request failed: {source}")]
    Request {
        #[source]
        source: reqwest::Error,
    },
    #[error("decisions endpoint returned {status}: {body}")]
    Status {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("decision response unparseable: {source}")]
    Parse {
        #[source]
        source: reqwest::Error,
    },
    #[error("decision answer for '{id}' is malformed: {reason}")]
    Malformed { id: String, reason: String },
}

/// Parse one wire answer into the typed form: string-keyed level
/// probabilities (`{"0": 0.79, "1": 0.21}`) become ordered
/// `DecisionLevelProbability` entries — the one decode home (the child
/// never re-parses the provider's key format).
fn parse_answer(id: &str, wire: DecisionsAnswerWire) -> Result<DecisionAnswer, DecisionError> {
    if wire.kind != "score" {
        return Err(DecisionError::Malformed {
            id: id.to_string(),
            reason: format!(
                "unsupported answer type {:?} — only the score primitive is wired",
                wire.kind
            ),
        });
    }
    let score = wire.score.ok_or_else(|| DecisionError::Malformed {
        id: id.to_string(),
        reason: "no score field".to_string(),
    })?;
    let confidence = wire.confidence.ok_or_else(|| DecisionError::Malformed {
        id: id.to_string(),
        reason: "no confidence field".to_string(),
    })?;
    let mut level_probabilities = Vec::with_capacity(wire.probabilities.len());
    for (key, probability) in wire.probabilities {
        let level = key.parse::<u8>().map_err(|_| DecisionError::Malformed {
            id: id.to_string(),
            reason: format!("level key {key:?} is not a level index"),
        })?;
        level_probabilities.push(DecisionLevelProbability { level, probability });
    }
    level_probabilities.sort_by_key(|entry| entry.level);
    Ok(DecisionAnswer {
        id: id.to_string(),
        score,
        level_probabilities,
        confidence,
    })
}

/// Ask a decision model typed score questions about a state via OpenRouter's
/// Decisions API.
///
/// `api_key` is the OpenRouter key read from the keychain on the zed side.
/// `model` is the clean (prefix-stripped) model id, e.g.
/// `typesafe/jev-1.14`. Returns one `DecisionAnswer` per question the
/// provider answered, keyed by the caller's question id — a question the
/// provider omitted is simply absent (the caller records the omission;
/// nothing is zero-filled here).
pub async fn decide(
    api_key: &str,
    model: &str,
    state: &str,
    questions: &[DecisionQuestion],
) -> Result<DecisionBatch, DecisionError> {
    let mut questions_body = serde_json::Map::new();
    for question in questions {
        questions_body.insert(
            question.id.clone(),
            serde_json::json!({
                "type": "score",
                "instructions": question.instructions,
                "criteria": question.criteria,
            }),
        );
    }
    let response = decisions_http_client()
        .post("https://openrouter.ai/api/alpha/decisions")
        .bearer_auth(api_key)
        .json(&serde_json::json!({
            "model": model,
            "state": state,
            "questions": questions_body,
        }))
        .send()
        .await
        .map_err(|error| DecisionError::Request { source: error })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(DecisionError::Status { status, body });
    }

    let mut wire = response
        .json::<DecisionsResponseWire>()
        .await
        .map_err(|error| DecisionError::Parse { source: error })?;

    let mut answers = Vec::with_capacity(questions.len());
    for question in questions {
        if let Some(answer) = wire.answers.remove(&question.id) {
            answers.push(parse_answer(&question.id, answer)?);
        }
    }

    let usage = InferenceUsage {
        prompt_tokens: wire.usage.input_tokens.unwrap_or(0),
        completion_tokens: wire.usage.output_tokens.unwrap_or(0),
        total_tokens: wire.usage.input_tokens.unwrap_or(0) + wire.usage.output_tokens.unwrap_or(0),
        // The Decisions API reports both token counts; absence means the
        // counts are placeholders, nothing was measured.
        reported: wire.usage.input_tokens.is_some() && wire.usage.output_tokens.is_some(),
    };

    Ok(DecisionBatch {
        answers,
        model: wire.model,
        usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The provider's documented response shape (OpenRouter's Jev
    /// explainer, real calls 2026-10-16; recorded in research run
    /// `b1f1663057722f5d`): string-keyed probabilities, a
    /// probability-weighted score, a legend (ignored here), and a
    /// confidence. Pinned so a provider wire change surfaces at the decode
    /// home, not downstream in a consumer.
    #[test]
    fn score_answer_wire_parses_with_ordered_levels() {
        let body = serde_json::json!({
            "model": "typesafe/jev-1.14-20261014",
            "answers": {
                "severity": {
                    "type": "score",
                    "score": 1.21,
                    "legend": {"0": "Cosmetic", "1": "Degraded", "2": "Blocking"},
                    "probabilities": {"2": 0.21, "0": 0.0, "1": 0.79},
                    "confidence": 0.69
                }
            },
            "usage": {"input_tokens": 354, "output_tokens": 17, "cost": 1.486e-05},
            "id": "gen-dec-test",
            "provider": "TypeSafe"
        });
        let wire: DecisionsResponseWire =
            serde_json::from_value(body).expect("documented response shape parses");
        let answer = wire
            .answers
            .get("severity")
            .cloned()
            .expect("answer present");
        let parsed = parse_answer("severity", answer).expect("typed form");
        assert_eq!(parsed.score, 1.21);
        assert_eq!(parsed.confidence, 0.69);
        assert_eq!(
            parsed.level_probabilities,
            vec![
                DecisionLevelProbability {
                    level: 0,
                    probability: 0.0
                },
                DecisionLevelProbability {
                    level: 1,
                    probability: 0.79
                },
                DecisionLevelProbability {
                    level: 2,
                    probability: 0.21
                },
            ],
            "string-keyed probabilities decode to ordered typed levels"
        );
    }

    #[test]
    fn non_score_answers_and_missing_fields_refuse_rather_than_zero_fill() {
        let choice = DecisionsAnswerWire {
            kind: "choice".to_string(),
            score: Some(0.0),
            probabilities: Default::default(),
            confidence: Some(0.5),
        };
        assert!(
            matches!(
                parse_answer("team", choice),
                Err(DecisionError::Malformed { .. })
            ),
            "unwired primitives refuse, naming the type"
        );
        let missing = DecisionsAnswerWire {
            kind: "score".to_string(),
            score: None,
            probabilities: Default::default(),
            confidence: None,
        };
        assert!(
            matches!(
                parse_answer("q", missing),
                Err(DecisionError::Malformed { .. })
            ),
            "absent score/confidence refuse — never fabricated zeros"
        );
    }

    #[test]
    fn only_openrouter_prefixed_models_are_decision_eligible() {
        assert_eq!(
            detect_decision_provider("OpenRouter/typesafe/jev-1.14"),
            Some(("openrouter", "typesafe/jev-1.14".to_string()))
        );
        assert_eq!(detect_decision_provider("typesafe/jev-1.14"), None);
        assert_eq!(detect_decision_provider("ollama/qwen3.5:4b"), None);
    }
}
