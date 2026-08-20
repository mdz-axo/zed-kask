//! The ABW agent/swarm composition contract — zed-kask's port of fermi's
//! `workflows/agent_contract.rs` requirement table.
//!
//! # Why this module exists
//!
//! fermi learned the hard way (`agent_contract.rs` module doc) that "what a
//! well-formed agent is" must have **one** definition, judged identically on
//! every path — its publish gate was four inline checks, and agents with no
//! `accepts`, `produces`, `sample_queries` or `valence` reached the public
//! catalogue because they satisfied every error the gate knew how to raise.
//!
//! zed-kask had the same shape of defect: the swarm panel's Validate button
//! sent the form fields to a one-shot LLM with a generic "you are an expert
//! at composing agent teams" prompt. The verdict was non-deterministic, the
//! fermi contract was not encoded anywhere, and an inference outage returned
//! `valid: false` with raw notes — the operator could not distinguish "my
//! agent is malformed" from "the model is down".
//!
//! This module is the deterministic floor. `swarm_ai_assist` runs it FIRST;
//! the LLM's contribution is demoted to advisory warnings, mirroring fermi's
//! Error/Warning severity split in `workflows/publish_pipeline.rs` (contract
//! = blocking, everything else = reported but never blocking).
//!
//! # What is deliberately NOT here
//!
//! - **The typed tier** (`output_contract` schema + grounding map). The
//!   panel cannot author it yet; a blocking check the form cannot satisfy
//!   would be theatre. It surfaces as a Warning that names the ABW publish
//!   gate as the enforcement point.
//! - **Valence-diversity / homophily** for swarms. fermi computes it from
//!   the hired agents' valence rows (`handlers/composition.rs`); the compose
//!   form only has agent *names* at validation time. The LLM advisory layer
//!   can flag it; a deterministic checker must not fabricate a verdict from
//!   data it does not have.

use serde_json::json;

/// Minimum roster size for a swarm to be worth launching. Mirrors the
/// panel's `MIN_AGENTS_TO_LAUNCH` launch gate (below it, variety and
/// diversity are trivially 0/1 and the composition loop converges without
/// doing composition work).
pub(crate) const MIN_SWARM_ROSTER: usize = 3;

/// Severity of a contract check. Error = blocking per the fermi contract;
/// Warning = advisory (fermi's publish pipeline reports but does not block).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    Error,
    Warning,
}

/// One contract check result — pass or fail, with the fermi-derived
/// operator-facing message. Mirrors fermi's `PublishCheck` shape.
#[derive(Debug, Clone)]
pub(crate) struct ContractCheck {
    /// Stable machine name (fermi's check ids, so UI copy and the fermi
    /// publish-checks endpoint can be correlated).
    pub(crate) name: &'static str,
    pub(crate) passed: bool,
    pub(crate) severity: Severity,
    pub(crate) message: String,
}

/// The agent-surface fields the contract judges. Supplied by the panel from
/// the Author form; every field is optional so a partially-filled form gets
/// a full report (fermi returns every finding, not the first, so an author
/// fixes a card in one pass instead of playing whack-a-mole with a gate).
#[derive(Debug, Default)]
pub(crate) struct AgentContractInput {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) system_prompt: String,
    pub(crate) tags: Vec<String>,
    pub(crate) sample_queries: Vec<String>,
    pub(crate) accepts: Vec<String>,
    pub(crate) produces: Vec<String>,
    pub(crate) has_valence: bool,
    /// "abw" or "local" — only the name-slug rule differs.
    pub(crate) mode: String,
}

/// The swarm-surface fields the contract judges, from the Compose form.
#[derive(Debug, Default)]
pub(crate) struct SwarmContractInput {
    pub(crate) name: String,
    pub(crate) mission: String,
    pub(crate) agents: Vec<String>,
    pub(crate) mode: String,
}

fn check(
    name: &'static str,
    passed: bool,
    severity: Severity,
    message: impl Into<String>,
) -> ContractCheck {
    ContractCheck {
        name,
        passed,
        severity,
        message: message.into(),
    }
}

/// Split a comma-separated form field into trimmed, non-empty entries.
pub(crate) fn split_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Split a newline-separated form field (sample queries contain commas, so
/// they are one-per-line, not CSV).
pub(crate) fn split_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Judge the agent form against the fermi ABW contract. Returns every check
/// (pass and fail) in the order an author would naturally fill the fields —
/// same ordering rationale as fermi's `requirements()` table.
pub(crate) fn agent_contract_checks(input: &AgentContractInput) -> Vec<ContractCheck> {
    let mut out = Vec::new();

    // ── name ───────────────────────────────────────────────────────
    let name = input.name.trim();
    let name_valid = if input.mode == "local" {
        // Local substrate: alphanumeric plus -_. (the substrate sanitizes;
        // chars outside this set would be stripped, changing the id).
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    } else {
        // ABW slug rule (fermi `slug::validate_http`): the name is
        // URL-routed via /api/agents/:id, so a non-slug breaks routing and
        // the @-mention parser downstream.
        let len = name.chars().count();
        (3..=64).contains(&len)
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    out.push(check(
        "name_set",
        name_valid,
        Severity::Error,
        if name.is_empty() {
            "Agent must have a name.".to_string()
        } else if input.mode == "local" {
            "Name contains characters the local substrate will strip (allowed: \
             letters, digits, -, _, .) — the id would change."
                .to_string()
        } else {
            "Name must be 3-64 chars: lowercase letters, digits, underscores only \
             (ABW slug rule — the name is URL-routed and @-mention parsed)."
                .to_string()
        },
    ));

    // ── description ────────────────────────────────────────────────
    out.push(check(
        "description_present",
        !input.description.trim().is_empty(),
        Severity::Error,
        "Description is required — it is the only thing most people read before \
         hiring (fermi `description_present`).",
    ));

    // ── system prompt ──────────────────────────────────────────────
    // The panel only authors LLM-backed agents (`executor: "llm"` in
    // `build_create_agent_card`), so fermi's `requires_persona` exemption
    // for deterministic executors never applies here.
    out.push(check(
        "system_prompt_present",
        !input.system_prompt.trim().is_empty(),
        Severity::Error,
        "System prompt is required for LLM-backed agents — without one the agent \
         has no persona or decision policy (fermi `system_prompt_present`).",
    ));

    // ── tags ───────────────────────────────────────────────────────
    out.push(check(
        "has_tags",
        !input.tags.is_empty(),
        Severity::Error,
        "At least one tag is required for catalogue discovery (fermi `has_tags`).",
    ));

    // ── sample queries ─────────────────────────────────────────────
    out.push(check(
        "has_sample_queries",
        !input.sample_queries.is_empty(),
        Severity::Error,
        "At least one sample query is required — without one, nobody can tell \
         what to ask this agent (fermi `has_sample_queries`).",
    ));

    // ── accepts / produces ─────────────────────────────────────────
    out.push(check(
        "declares_accepts",
        !input.accepts.is_empty(),
        Severity::Error,
        "`accepts` must declare at least one input type — composition planning \
         cannot route work to an agent with no declared inputs (fermi \
         `declares_accepts`).",
    ));
    out.push(check(
        "declares_produces",
        !input.produces.is_empty(),
        Severity::Error,
        "`produces` must declare at least one output type — downstream agents \
         match against it to build pipelines (fermi `declares_produces`).",
    ));

    // ── valence ────────────────────────────────────────────────────
    out.push(check(
        "has_valence",
        input.has_valence,
        Severity::Error,
        "`valence` is required — the affective signature drives valence-diversity \
         checks that stop a composition becoming an echo chamber (fermi \
         `has_valence`).",
    ));

    // ── typed tier (advisory here) ─────────────────────────────────
    // fermi blocks publish on the typed tier for new agents
    // (`card_contract::validate`), but the panel cannot author an
    // `output_contract` yet. A blocking check the form cannot satisfy
    // teaches operators the checks are theatre — so it is a Warning that
    // names the real enforcement point.
    out.push(check(
        "output_contract_present",
        false,
        Severity::Warning,
        "No `output_contract` declared. ABW blocks *publishing* new agents \
         without one (schema + per-field grounding map). Not yet authorable \
         in this form — not enforced here.",
    ));

    out
}

/// Judge the swarm form against the fermi composition semantics.
pub(crate) fn swarm_contract_checks(input: &SwarmContractInput) -> Vec<ContractCheck> {
    let mut out = Vec::new();

    out.push(check(
        "name_set",
        !input.name.trim().is_empty(),
        Severity::Error,
        "Swarm must have a name.".to_string(),
    ));

    // The panel's launch gate requires a mission (SENSE derives
    // required_transforms from it); validation must agree with the gate or
    // the operator gets a "valid" verdict on a form that create will reject.
    out.push(check(
        "mission_present",
        !input.mission.trim().is_empty(),
        Severity::Error,
        "Mission is required to launch a swarm — the composition loop derives \
         required transforms from it.",
    ));

    // Concreteness heuristic, deterministic: a mission under 20 characters
    // cannot state what the swarm should produce and when it is done. fermi
    // judges this with the curator; we can only afford the floor.
    let mission_words = input.mission.split_whitespace().count();
    out.push(check(
        "mission_concrete",
        input.mission.trim().chars().count() >= 20 && mission_words >= 4,
        Severity::Warning,
        "Mission is too vague to derive transforms from — state what the swarm \
         should produce and the conditions under which it is done.",
    ));

    out.push(check(
        "roster_min",
        input.agents.len() >= MIN_SWARM_ROSTER,
        Severity::Error,
        format!(
            "At least {MIN_SWARM_ROSTER} agents are required to launch a swarm \
             ({} provided) — below that, variety and diversity are trivially \
             degenerate.",
            input.agents.len()
        ),
    ));

    // Duplicate roster entries: a coherence deficit (near-duplicate roles)
    // and, on ABW, a wasted consent-gated hire.
    let mut seen = std::collections::HashSet::new();
    let duplicates: Vec<&String> = input
        .agents
        .iter()
        .filter(|a| !seen.insert((*a).clone()))
        .collect();
    out.push(check(
        "roster_no_duplicates",
        duplicates.is_empty(),
        Severity::Error,
        format!(
            "Roster contains duplicate agents ({}). Swarms need variety — \
             distinct roles, not duplicates.",
            duplicates
                .iter()
                .map(|d| d.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ));

    // Slug validity per mode: ABW hires are URL-routed agent names (strict
    // slug); local ids are filesystem-safe (the substrate sanitizes, so a
    // warning rather than an error — but the operator should know the id
    // will change).
    let slug_error = if input.mode == "local" {
        input.agents.iter().any(|a| {
            a.chars()
                .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
        })
    } else {
        input.agents.iter().any(|a| {
            let len = a.chars().count();
            !(3..=64).contains(&len)
                || a.chars()
                    .any(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
        })
    };
    out.push(check(
        "roster_slugs_valid",
        !slug_error,
        if input.mode == "local" {
            Severity::Warning
        } else {
            Severity::Error
        },
        if input.mode == "local" {
            "Some roster ids contain characters the local substrate will strip — \
             resolution at delegation time would miss them."
                .to_string()
        } else {
            "Some roster ids are not ABW slugs ([a-z0-9_]{3,64}) — the hire would \
             be rejected at the door."
                .to_string()
        },
    ));

    out
}

/// Render the checks as the `swarm_ai_assist` response payload: `valid` is
/// true iff no Error-severity check failed (fermi's `can_publish`), `issues`
/// carries the Error failures, `warnings` the advisory tier. The LLM's
/// advisory output is appended to `warnings` by the caller — it can never
/// flip `valid`.
pub(crate) fn checks_to_payload(checks: &[ContractCheck]) -> serde_json::Value {
    let valid = checks
        .iter()
        .all(|c| c.passed || c.severity == Severity::Warning);
    let issues: Vec<String> = checks
        .iter()
        .filter(|c| !c.passed && c.severity == Severity::Error)
        .map(|c| c.message.clone())
        .collect();
    let warnings: Vec<String> = checks
        .iter()
        .filter(|c| !c.passed && c.severity == Severity::Warning)
        .map(|c| c.message.clone())
        .collect();
    json!({
        "valid": valid,
        "issues": issues,
        "warnings": warnings,
        "checks": checks
            .iter()
            .map(|c| json!({
                "name": c.name,
                "passed": c.passed,
                "severity": if c.severity == Severity::Error { "error" } else { "warning" },
                "message": c.message,
            }))
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conforming_agent() -> AgentContractInput {
        AgentContractInput {
            name: "market_sentiment".into(),
            description: "Tracks public mood around companies.".into(),
            system_prompt: "You are Market Sentiment…".into(),
            tags: vec!["research".into()],
            sample_queries: vec!["What is the mood on Apple?".into()],
            accepts: vec!["text".into()],
            produces: vec!["sentiment_report".into()],
            has_valence: true,
            mode: "abw".into(),
        }
    }

    fn conforming_swarm() -> SwarmContractInput {
        SwarmContractInput {
            name: "research_team".into(),
            mission: "Produce a weekly competitive brief covering product launches.".into(),
            agents: vec![
                "market_analyst".into(),
                "tech_analyst".into(),
                "sentiment_tracker".into(),
            ],
            mode: "abw".into(),
        }
    }

    #[test]
    fn fully_formed_agent_conforms() {
        let payload = checks_to_payload(&agent_contract_checks(&conforming_agent()));
        assert!(payload["valid"].as_bool().unwrap());
        // The typed tier is advisory: a conforming agent still carries the
        // output_contract warning naming the ABW publish gate.
        let warnings = payload["warnings"].as_array().unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].as_str().unwrap().contains("output_contract"));
    }

    #[test]
    fn each_agent_requirement_is_independently_enforced() {
        // Same shape as fermi's `each_requirement_is_independently_enforced`:
        // nulling one field at a time must fail exactly that check.
        let cases: Vec<(&'static str, fn(&mut AgentContractInput))> = vec![
            ("name_set", |i| i.name = "!!".into()),
            ("description_present", |i| i.description = "  ".into()),
            ("system_prompt_present", |i| i.system_prompt = String::new()),
            ("has_tags", |i| i.tags.clear()),
            ("has_sample_queries", |i| i.sample_queries.clear()),
            ("declares_accepts", |i| i.accepts.clear()),
            ("declares_produces", |i| i.produces.clear()),
            ("has_valence", |i| i.has_valence = false),
        ];
        for (expected, mutate) in cases {
            let mut input = conforming_agent();
            mutate(&mut input);
            let checks = agent_contract_checks(&input);
            let failed: Vec<&str> = checks
                .iter()
                .filter(|c| !c.passed && c.severity == Severity::Error)
                .map(|c| c.name)
                .collect();
            assert_eq!(
                failed,
                vec![expected],
                "nulling {expected} must fail only {expected}"
            );
        }
    }

    #[test]
    fn abw_slug_rule_is_mode_aware() {
        // A dash is fine locally but breaks ABW URL routing.
        let mut local = conforming_agent();
        local.mode = "local".into();
        local.name = "my-agent".into();
        assert!(
            checks_to_payload(&agent_contract_checks(&local))["valid"]
                .as_bool()
                .unwrap()
        );

        let mut abw = conforming_agent();
        abw.name = "my-agent".into();
        let checks = agent_contract_checks(&abw);
        assert!(checks.iter().any(|c| c.name == "name_set" && !c.passed));
    }

    #[test]
    fn fully_formed_swarm_conforms() {
        let payload = checks_to_payload(&swarm_contract_checks(&conforming_swarm()));
        assert!(payload["valid"].as_bool().unwrap());
    }

    #[test]
    fn short_roster_and_duplicates_block() {
        let mut input = conforming_swarm();
        input.agents = vec!["market_analyst".into(), "market_analyst".into()];
        let checks = swarm_contract_checks(&input);
        assert!(
            checks
                .iter()
                .any(|c| c.name == "roster_min" && !c.passed && c.severity == Severity::Error)
        );
        assert!(
            checks
                .iter()
                .any(|c| c.name == "roster_no_duplicates" && !c.passed)
        );
    }

    #[test]
    fn vague_mission_is_a_warning_not_an_error() {
        let mut input = conforming_swarm();
        input.mission = "do research".into();
        let payload = checks_to_payload(&swarm_contract_checks(&input));
        // Advisory tier never flips `valid` — fermi's Warnings don't block.
        assert!(payload["valid"].as_bool().unwrap());
        assert!(
            payload["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w.as_str().unwrap().contains("vague"))
        );
    }

    #[test]
    fn checks_and_payload_agree() {
        // fermi's `checks_and_violations_agree`: the derived verdict must
        // match the check list exactly.
        let checks = agent_contract_checks(&AgentContractInput {
            name: "ok_name".into(),
            ..Default::default()
        });
        let payload = checks_to_payload(&checks);
        let error_failures = checks
            .iter()
            .filter(|c| !c.passed && c.severity == Severity::Error)
            .count();
        assert_eq!(payload["issues"].as_array().unwrap().len(), error_failures);
        assert!(!payload["valid"].as_bool().unwrap());
    }

    #[test]
    fn split_helpers_drop_empties() {
        assert_eq!(split_csv("a, b ,,c"), vec!["a", "b", "c"]);
        assert_eq!(split_lines("q1?\n\n q2? \n"), vec!["q1?", "q2?"]);
    }
}
