//! The ```` ```swarm_delegate_results ```` block body model + parser.
//!
//! Mirrors the `LocalDelegateResult` array returned by the
//! `swarm_execute_plan_local` MCP tool (see
//! `kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs`). The agent (or the
//! swarm-steering skill) emits the array wrapped in a `{"viz": ...,
//! "results": [...]}` envelope as a fenced block; the widget parses it
//! passively (no `ToolInvoker` — the data is already in the chat stream).
//!
//! Fields are optional / defaulted so the parser is tolerant of partial bodies
//! and never fails on media-shaped or graph-shaped JSON (which have no `viz`
//! field or a different `viz` value). This is the same tolerance contract as
//! the other viz widgets (kanban, scenarios) — see `hkask-viz-core`'s
//! `try_create` guard.

use serde::Deserialize;

/// The discriminator-tagged body of a ```` ```swarm_delegate_results ```` block.
///
/// `viz` selects the renderer; `"swarm_delegate_results"` renders the per-agent
/// card grid. `results` carries the per-delegation entries. Both default so a
/// body emitted by an older caller (or a foreign-shaped JSON) parses without
/// error and is then rejected by the `VIZ_TAG` check rather than logged as
/// malformed.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SwarmBlockBody {
    #[serde(default)]
    pub viz: Option<String>,
    #[serde(default)]
    pub results: Vec<DelegateResultCard>,
    /// Ontology concept URI emitted by the swarm server (e.g. `pko:Procedure`).
    /// Carried so the compose-back body can reference it and a future "explain
    /// this delegation" affordance can dispatch on it. `None` on older blocks
    /// or when the server doesn't emit it. Pinned by the registry-level S4
    /// sensor test in `hkask-viz-core` (`.rules` "Ontology tag field-drop"
    /// trap).
    #[serde(default)]
    pub ontology: Option<String>,
}

/// A deserialization-friendly mirror of `LocalDelegateResult`. Every field is
/// `#[serde(default)]` so a result emitted by an older caller (or a partial
/// body) parses with empty/zero/`None` values rather than failing — the widget
/// renders what it has and surfaces missing fields as muted placeholders.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DelegateResultCard {
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub response: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub tokens_used: i64,
    /// Credits recorded for this delegation (capped at `credits_authorized`).
    /// See `LocalDelegateResult::cost` for the accounting caveat.
    #[serde(default)]
    pub cost: i64,
    /// What this delegation would have cost with no cap applied. When
    /// `cost_uncapped > cost`, the ledger is behind real spend by the
    /// difference — surfaced on the card so the understatement is visible.
    #[serde(default)]
    pub cost_uncapped: i64,
    /// Ledger balance after recording this delegation's spend. `None` means
    /// not measured (the balance read failed), never "zero" — rendered as a
    /// muted "—" rather than a fabricated 0 (`.rules` broken-feedback-loop trap).
    #[serde(default)]
    pub balance: Option<i64>,
    #[serde(default)]
    pub latency_ms: u64,
    /// Summary of tool calls made during the delegation. Opaque JSON values
    /// (qualified `server/tool` name + ok/error); the card renders a count.
    #[serde(default)]
    pub tool_calls: Vec<serde_json::Value>,
    /// Summary of skill cascades executed before the LLM call. Opaque JSON
    /// values; the card renders a count.
    #[serde(default)]
    pub executed_skills: Vec<serde_json::Value>,
    /// Optional deterministic task-success verdict stamped by the executor.
    /// `None` when no evaluator ran — rendered as a muted "not evaluated"
    /// badge rather than a fabricated pass/fail.
    #[serde(default)]
    pub task_success: Option<TaskSuccessVerdictCard>,
}

/// A deserialization-friendly mirror of `TaskSuccessVerdict`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskSuccessVerdictCard {
    #[serde(default)]
    pub pass: bool,
    /// Optional graded score in `[0.0, 1.0]`; when absent, `pass` is the
    /// binary signal.
    #[serde(default)]
    pub score: Option<f64>,
    /// Evaluator-readable detail (which check failed, the diff, the exit
    /// code, etc.).
    #[serde(default)]
    pub detail: Option<String>,
    /// How the verdict was produced. Parsed as an opaque string so an unknown
    /// provenance variant (e.g. a future `HumanJudged`) does not break
    /// deserialization — the card renders it verbatim.
    #[serde(default)]
    pub provenance: String,
}

/// Parse a ```` ```swarm_delegate_results ```` block body. Tolerant: missing
/// `viz`/`results` default to `None`/empty rather than erroring, so
/// media-shaped and graph-shaped JSON parse (and are then rejected by the
/// renderer on the `viz` check) instead of being logged as a malformed block.
pub fn parse_swarm_body(body: &str) -> anyhow::Result<SwarmBlockBody> {
    Ok(serde_json::from_str(body.trim())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_body() {
        let json = r#"{
            "viz": "swarm_delegate_results",
            "results": [
                {
                    "agent_id": "researcher",
                    "response": "Found 3 sources.",
                    "model": "gpt-4o",
                    "tokens_used": 1200,
                    "cost": 50,
                    "cost_uncapped": 50,
                    "balance": 950,
                    "latency_ms": 4200,
                    "tool_calls": [{"name":"web_search","ok":true}],
                    "executed_skills": [],
                    "task_success": {"pass": true, "score": 1.0, "provenance": "deterministic"}
                },
                {
                    "agent_id": "writer",
                    "response": "Drafted the summary.",
                    "model": "gpt-4o-mini",
                    "tokens_used": 800,
                    "cost": 20,
                    "cost_uncapped": 25,
                    "balance": 930,
                    "latency_ms": 2100,
                    "tool_calls": [],
                    "executed_skills": [{"id":"eqm","ok":true}],
                    "task_success": {"pass": false, "score": 0.4, "detail": "missing citation", "provenance": "deterministic"}
                }
            ]
        }"#;
        let body = parse_swarm_body(json).expect("valid body");
        assert_eq!(body.viz.as_deref(), Some("swarm_delegate_results"));
        assert_eq!(body.results.len(), 2);
        assert_eq!(body.results[0].agent_id, "researcher");
        assert_eq!(body.results[0].tool_calls.len(), 1);
        let verdict = body.results[0].task_success.as_ref().expect("verdict");
        assert!(verdict.pass);
        assert_eq!(verdict.score, Some(1.0));
        assert_eq!(verdict.provenance, "deterministic");
        assert_eq!(body.results[1].cost_uncapped, 25);
    }

    #[test]
    fn parses_minimal_body() {
        let body = parse_swarm_body(r#"{"viz":"swarm_delegate_results"}"#).expect("minimal body");
        assert_eq!(body.viz.as_deref(), Some("swarm_delegate_results"));
        assert!(body.results.is_empty());
    }

    #[test]
    fn parses_result_with_missing_fields() {
        // A result emitted by an older caller omits cost_uncapped, balance,
        // task_success. The card must still parse and render placeholders.
        let json = r#"{"viz":"swarm_delegate_results","results":[{"agent_id":"a","response":"r","model":"m","tokens_used":10,"cost":5,"latency_ms":100}]}"#;
        let body = parse_swarm_body(json).expect("partial body");
        let card = &body.results[0];
        assert_eq!(card.cost_uncapped, 0);
        assert_eq!(card.balance, None);
        assert!(card.task_success.is_none());
        assert!(card.tool_calls.is_empty());
    }

    #[test]
    fn media_body_not_claimed() {
        let body = parse_swarm_body(r#"{"kind":"video","src":"/clip.mp4"}"#).expect("json parses");
        assert_ne!(body.viz.as_deref(), Some("swarm_delegate_results"));
    }

    #[test]
    fn kanban_body_not_claimed() {
        let body = parse_swarm_body(r#"{"viz":"kanban","tasks":[]}"#).expect("json parses");
        assert_ne!(body.viz.as_deref(), Some("swarm_delegate_results"));
    }

    #[test]
    fn non_json_fails() {
        assert!(parse_swarm_body("not json").is_err());
    }

    #[test]
    fn ontology_field_parses_when_present() {
        let body =
            parse_swarm_body(r#"{"viz":"swarm_delegate_results","ontology":"pko:Procedure"}"#)
                .expect("valid body");
        assert_eq!(body.ontology.as_deref(), Some("pko:Procedure"));
    }
}
