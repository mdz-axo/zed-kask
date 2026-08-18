//! Grounding contract — Rung 3 (Grounding) of the verification ladder.
//!
//! Stops a fully-typed agent ecology from being a fully-fabricated one.
//! Declares which output fields of a `kanban-task-*` agent must be sourced
//! from actual tool calls vs. inferred by the LLM. Per invocation, checks
//! which fields could have come from available tools. Nulls unsourced
//! fields, retains the removed value (paper §5.5: "tag, do not delete"),
//! scans narrative for leaked claims.
//!
//! The six-valued vocabulary (paper §5.5 extended with Derived and
//! UncommissionedInference):
//! - Sourced: a named tool returned it. Keep, mark verified.
//! - Inferred: judgement over sourced inputs, by design (commissioned).
//!   Keep, mark as inference.
//! - Derived: computed by platform code from a sourced value,
//!   deterministically. Distinct from Inferred because a derivation is
//!   reproducible and auditable. Keep, mark as derived.
//! - UncommissionedInference: the model produced a judgment that was not
//!   explicitly commissioned but is plausibly within the agent's scope.
//!   Keep, mark as uncommissioned inference, scan for unsupported claims.
//! - Narrative: prose. Keep, scan for claims it cannot support.
//! - Unsourced: no tool could supply it. Null it, record what was removed.
//!
//! The contract is hand-declared and therefore incomplete (paper §6).
//! Coverage is itself a metric.

use std::collections::HashMap;

/// The six-valued grounding vocabulary.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProvenanceTag {
    /// A named tool returned it.
    Sourced { tool: String },
    /// Judgement over sourced inputs, by design (commissioned by the
    /// system prompt).
    Inferred,
    /// Computed by platform code from a sourced value, deterministically.
    /// Distinct from Inferred because a derivation is reproducible and
    /// auditable — the same input yields the same output, and the transform
    /// can be read.
    Derived {
        /// The sourced field it is computed from.
        from: String,
        /// The transform, named so a reader can check it.
        how: String,
    },
    /// The model produced a judgment that was not explicitly commissioned
    /// but is plausibly within the agent's scope. Distinct from Unsourced
    /// because the agent was implicitly authorized to reason, not to
    /// fabricate facts.
    UncommissionedInference,
    /// Prose — kept, scanned for claims it cannot support.
    Narrative,
    /// No tool could supply it. Nulled, value retained for calibration.
    Unsourced {
        /// Truncated preview of the removed value (first 200 chars).
        /// The full value goes to the audit log, not the API response.
        removed_preview: String,
        /// Whether the declared tool was called but failed (transient error,
        /// retry) vs. no tool was called at all (capability gap). Distinct
        /// because the operator's remediation is different.
        tool_failed: bool,
    },
}

/// A field's source specification: which tools can supply it, and why
/// the contract declares this disposition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FieldSpec {
    /// Tools that can source this field. Empty = Inferred (commissioned
    /// judgment), not Unsourced.
    pub sources: Vec<String>,
    /// Dotted path into the tool's `result` where this field's value
    /// should appear. Required for Sourced fields — value-matching
    /// (Truth rung) checks the output value appears in the tool's return.
    /// Empty string means "search the entire result."
    #[serde(default)]
    pub response_path: String,
    /// Why this field has the disposition it has. Mandatory (≥40 chars).
    pub why: String,
}

/// A field → tool map for one agent type's structured output.
/// Hand-declared, therefore incomplete (paper §6). Coverage is itself
/// a metric.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GroundingContract {
    /// The agent_type this contract applies to (e.g. "task").
    pub agent_type: String,
    /// Map of output field name → FieldSpec (sources + why).
    /// Empty sources = Inferred (commissioned judgment), not Unsourced.
    pub field_sources: HashMap<String, FieldSpec>,
}

/// The result of grounding enforcement on one delegation output.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GroundingResult {
    /// Per-field provenance tags.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub provenance: HashMap<String, ProvenanceTag>,
    /// Fields that were nulled (Unsourced).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nulled_fields: Vec<String>,
    /// Narrative claims that leaked unsourced values. Each entry is a
    /// (substring_found, field_it_leaked) pair.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub narrative_leaks: Vec<(String, String)>,
}

impl GroundingResult {
    /// True when no violations were found. Used by the idempotency test —
    /// a second pass of `enforce_grounding` on an already-enforced document
    /// must be clean.
    pub fn is_clean(&self) -> bool {
        self.nulled_fields.is_empty() && self.narrative_leaks.is_empty()
    }
}

/// The built-in grounding contract for `kanban-task-*` agents.
///
/// These agents are spawned by `kanban_task_spawn` to execute a task using
/// declared skills. Their output may include:
/// - `deliverable_path`: a file path the agent claims to have written.
///   Must be sourced from an `edit_file`, `write_file`, or `terminal` tool
///   call that succeeded.
/// - `test_verdict`: a pass/fail claim about tests. Must be sourced from a
///   `terminal` tool call that succeeded (the test runner).
/// - `summary`: a prose summary of what the agent did. Inferred — the
///   agent was commissioned to summarize.
/// - `approach`: a description of the approach taken. Inferred.
///
/// Any other field in the output is treated as UncommissionedInference
/// (kept, marked) unless it matches a tool's output (Sourced) or has no
/// possible source (Unsourced, nulled).
pub fn task_agent_contract() -> GroundingContract {
    let mut field_sources = HashMap::new();
    field_sources.insert(
        "deliverable_path".to_string(),
        FieldSpec {
            sources: vec![
                "zed/edit_file".to_string(),
                "zed/write_file".to_string(),
                "zed/terminal".to_string(),
            ],
            response_path: "".to_string(),
            why: "A file path the agent claims to have written. Must be sourced \
                  from a file-writing tool that succeeded."
                .to_string(),
        },
    );
    field_sources.insert(
        "test_verdict".to_string(),
        FieldSpec {
            sources: vec!["zed/terminal".to_string()],
            response_path: "".to_string(),
            why: "A pass/fail claim about tests. Must be sourced from a terminal \
                  tool call that succeeded (the test runner)."
                .to_string(),
        },
    );
    // Commissioned judgments — empty source list = Inferred, not Unsourced.
    field_sources.insert(
        "summary".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "A prose summary of what the agent did. Commissioned by the \
                  system prompt — the agent was asked to summarize."
                .to_string(),
        },
    );
    field_sources.insert(
        "approach".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "A description of the approach taken. Commissioned by the \
                  system prompt — the agent was asked to describe its approach."
                .to_string(),
        },
    );
    GroundingContract {
        agent_type: "task".to_string(),
        field_sources,
    }
}

/// The built-in grounding contract for `research` agents.
///
/// Research agents are delegated to via `swarm_delegate_local` or
/// `swarm_create_agent` with `agent_type: "research"`. Their output may
/// include:
/// - `sources`: a list of sources the agent claims to have found. Must be
///   sourced from a `research_search`, `web_search`, `web_extract`, or
///   `fetch` tool call that succeeded.
/// - `findings`: a prose summary of what the agent found. Inferred — the
///   agent was commissioned to analyze and report.
/// - `summary`: a prose summary. Inferred.
///
/// Any other field is UncommissionedInference (kept, marked) unless it
/// matches a tool's output (Sourced) or has no possible source (Unsourced,
/// nulled).
pub fn research_agent_contract() -> GroundingContract {
    let mut field_sources = HashMap::new();
    field_sources.insert(
        "sources".to_string(),
        FieldSpec {
            sources: vec![
                "zed/research_search".to_string(),
                "zed/web_search".to_string(),
                "zed/web_extract".to_string(),
                "zed/fetch".to_string(),
            ],
            response_path: "".to_string(),
            why: "A list of sources the agent claims to have found. Must be \
                  sourced from a research or web search tool that succeeded."
                .to_string(),
        },
    );
    field_sources.insert(
        "findings".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "A prose summary of what the agent found. Commissioned by \
                  the system prompt — the agent was asked to analyze and report."
                .to_string(),
        },
    );
    field_sources.insert(
        "summary".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "A prose summary of the research. Commissioned by the \
                  system prompt — the agent was asked to summarize."
                .to_string(),
        },
    );
    GroundingContract {
        agent_type: "research".to_string(),
        field_sources,
    }
}

/// The built-in grounding contract for `narrator` agents.
///
/// Narrator agents produce prose content (stories, descriptions, narratives).
/// Their output may include:
/// - `content`: the main prose output. Inferred — the agent was commissioned
///   to produce this content.
/// - `summary`: a prose summary. Inferred.
///
/// Both fields are commissioned judgments (Inferred), not Unsourced. The
/// contract exists so narrator delegations are grounded (not coverage gaps)
/// and so any fabricated file paths or tool-sourced claims in the output are
/// caught by the grounding check.
pub fn narrator_agent_contract() -> GroundingContract {
    let mut field_sources = HashMap::new();
    field_sources.insert(
        "content".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "The main prose output. Commissioned by the system prompt — \
                  the agent was asked to produce narrative content."
                .to_string(),
        },
    );
    field_sources.insert(
        "summary".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "A prose summary of the narrative. Commissioned by the \
                  system prompt — the agent was asked to summarize."
                .to_string(),
        },
    );
    GroundingContract {
        agent_type: "narrator".to_string(),
        field_sources,
    }
}

/// The built-in grounding contract for skill cascades (`agent_type: "skill"`).
///
/// Skill cascades produce LLM-synthesized text from Jinja2 templates. Some
/// skills (e.g. `diataxis-diagram`, `sankey-flow`) produce structured output
/// (Mermaid diagrams) with potentially fabricated content. Skills that
/// produce code may claim to have written files.
///
/// The contract checks for fabricated file paths in the output:
/// - `deliverable_path`: a file path the skill claims to have written. Must
///   be sourced from an `edit_file`, `write_file`, or `terminal` tool call
///   that succeeded (same pattern as `task_agent_contract`).
/// - `test_verdict`: a pass/fail claim about tests. Must be sourced from a
///   `terminal` tool call that succeeded.
/// - `diagram`: the main diagram or structured output. Inferred — the skill
///   was commissioned to produce this.
/// - `summary`: a prose summary. Inferred.
/// - `recommendations`: a list of recommendations. Inferred.
///
/// Any other field is UncommissionedInference (kept, marked) unless it
/// matches a tool's output (Sourced) or has no possible source (Unsourced,
/// nulled). This catches skills that fabricate file paths in their output
/// without actually calling a file-writing tool.
pub fn skill_agent_contract() -> GroundingContract {
    let mut field_sources = HashMap::new();
    field_sources.insert(
        "deliverable_path".to_string(),
        FieldSpec {
            sources: vec![
                "zed/edit_file".to_string(),
                "zed/write_file".to_string(),
                "zed/terminal".to_string(),
            ],
            response_path: "".to_string(),
            why: "A file path the skill claims to have written. Must be sourced \
                  from a file-writing tool that succeeded. Skills that produce \
                  code may fabricate file paths without actually writing files."
                .to_string(),
        },
    );
    field_sources.insert(
        "test_verdict".to_string(),
        FieldSpec {
            sources: vec!["zed/terminal".to_string()],
            response_path: "".to_string(),
            why: "A pass/fail claim about tests. Must be sourced from a terminal \
                  tool call that succeeded (the test runner)."
                .to_string(),
        },
    );
    field_sources.insert(
        "diagram".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "The main diagram or structured output. Commissioned by the \
                  skill's template — the skill was asked to produce this."
                .to_string(),
        },
    );
    field_sources.insert(
        "summary".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "A prose summary of the skill output. Commissioned by the \
                  skill's template — the skill was asked to summarize."
                .to_string(),
        },
    );
    field_sources.insert(
        "recommendations".to_string(),
        FieldSpec {
            sources: vec![],
            response_path: "".to_string(),
            why: "A list of recommendations produced by the skill. Commissioned \
                  by the skill's template — the skill was asked to propose actions."
                .to_string(),
        },
    );
    GroundingContract {
        agent_type: "skill".to_string(),
        field_sources,
    }
}

/// Extract the set of tools that successfully returned data from the
/// `tool_calls` summary on a `LocalDelegateResult`.
///
/// The `tool_calls` entries have shape `{"tool": "server/tool_name", "ok": true/false}`.
/// Only successful calls count — a tool that errored did not supply data.
fn successful_tools(tool_calls: &[serde_json::Value]) -> std::collections::HashSet<String> {
    tool_calls
        .iter()
        .filter_map(|tc| {
            let ok = tc.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            if !ok {
                return None;
            }
            tc.get("tool")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect()
}

/// Extract the set of tools that were called but failed (returned an error).
/// Used to distinguish `tool_failed` from `no tool called` in the Unsourced
/// tag — the operator's remediation is different (retry vs. wire up the tool).
fn failed_tools(tool_calls: &[serde_json::Value]) -> std::collections::HashSet<String> {
    tool_calls
        .iter()
        .filter_map(|tc| {
            let ok = tc.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            if ok {
                return None;
            }
            tc.get("tool")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect()
}

/// Check whether a field's value was sourced from a declared tool.
///
/// Truth rung: "sourced" means the value came from the tool's return,
/// not merely that the tool ran. Containment check: the field value
/// must appear in the values selected by `response_path` from the tool's
/// `result`. Returns the tool name if matched.
fn is_value_sourced(
    field_tools: &[String],
    successful: &std::collections::HashSet<String>,
    field_value: &serde_json::Value,
    tool_calls: &[serde_json::Value],
    response_path: &str,
) -> Option<String> {
    let field_values = match field_value {
        serde_json::Value::Array(arr) => arr.clone(),
        _ => vec![field_value.clone()],
    };
    for tc in tool_calls {
        let tool = tc.get("tool").and_then(|v| v.as_str()).unwrap_or("");
        if !field_tools.iter().any(|t| t == tool) || !successful.contains(tool) {
            continue;
        }
        let Some(result) = tc.get("result") else {
            continue;
        };
        let targets: Vec<serde_json::Value> = if response_path.is_empty() {
            vec![result.clone()]
        } else {
            select(result, &segments(response_path))
                .into_iter()
                .cloned()
                .collect()
        };
        let all_found = field_values
            .iter()
            .all(|fv| targets.iter().any(|t| value_contains(t, fv)));
        if all_found && !field_values.is_empty() {
            return Some(tool.to_string());
        }
    }
    None
}

/// Does `haystack` contain `needle`? Recurses into arrays/objects.
/// For strings, checks substring containment (handles paths in stdout).
fn value_contains(haystack: &serde_json::Value, needle: &serde_json::Value) -> bool {
    if haystack == needle {
        return true;
    }
    match haystack {
        serde_json::Value::Array(arr) => arr.iter().any(|v| value_contains(v, needle)),
        serde_json::Value::Object(obj) => obj.values().any(|v| value_contains(v, needle)),
        serde_json::Value::String(s) => needle.as_str().is_some_and(|n| s.contains(n)),
        _ => false,
    }
}

/// Check whether any of the field's declared tools was called but failed.
fn tool_was_called_but_failed(
    field_tools: &[String],
    failed: &std::collections::HashSet<String>,
) -> bool {
    field_tools.iter().any(|t| failed.contains(t))
}

/// The provenance string to stamp on the document for a given tag.
/// Follows Fermi's `PROV_*` vocabulary so consumers can read provenance
/// without parsing the `GroundingResult`.
pub fn provenance_stamp(tag: &ProvenanceTag) -> &'static str {
    match tag {
        ProvenanceTag::Sourced { .. } => "tool_verified",
        ProvenanceTag::Inferred => "model_inference",
        ProvenanceTag::Derived { .. } => "platform_derived",
        ProvenanceTag::UncommissionedInference => "uncommissioned_inference",
        ProvenanceTag::Narrative => "narrative",
        ProvenanceTag::Unsourced {
            tool_failed: true, ..
        } => "tool_no_match",
        ProvenanceTag::Unsourced {
            tool_failed: false, ..
        } => "unavailable_no_tool_source",
    }
}

/// Is this value an actual claim, as opposed to absent or a placeholder?
/// Mirrors Fermi's `is_claim` — `null`, empty string, `"..."`, `"null"`,
/// `"N/A"`, `"-"` are all absent, not fabricated. A model echoing a
/// placeholder has declined to answer, not invented one.
fn is_claim(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::String(s) => {
            let t = s.trim();
            !t.is_empty() && t != "..." && t != "null" && t != "N/A" && t != "-"
        }
        serde_json::Value::Array(a) => a.iter().any(is_claim),
        serde_json::Value::Object(o) => o.values().any(is_claim),
        _ => true,
    }
}

/// Split a dotted path, turning `foo[]` into `foo` + `[]`.
/// Mirrors Fermi's `segments` function — needed for array path support
/// (C8), so contracts can address fields inside arrays like
/// `deliverables[].path`.
fn segments(path: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for seg in path.split('.') {
        if let Some(name) = seg.strip_suffix("[]") {
            out.push(name);
            out.push("[]");
        } else {
            out.push(seg);
        }
    }
    out
}

/// Collect every value a path selects. A `[]` segment means "each element
/// of this array", so `deliverables[].path` selects the path of every
/// deliverable. Needed because agents with list-valued output would
/// silently pass a contract that can only address top-level scalars.
/// Mirrors Fermi's `select` function.
fn select<'a>(doc: &'a serde_json::Value, segs: &[&str]) -> Vec<&'a serde_json::Value> {
    let Some((head, rest)) = segs.split_first() else {
        return vec![doc];
    };
    if *head == "[]" {
        return match doc.as_array() {
            Some(items) => items.iter().flat_map(|it| select(it, rest)).collect(),
            None => vec![],
        };
    }
    match doc.get(head) {
        Some(v) => select(v, rest),
        None => vec![],
    }
}

/// Does any value this path selects constitute a claim?
fn path_has_claim(doc: &serde_json::Value, path: &str) -> bool {
    select(doc, &segments(path)).iter().any(|v| is_claim(v))
}

/// First value selected by a dotted path (which may contain `[]` segments).
/// Returns `None` if the path selects nothing. Used by value-matching to pull
/// the field value out of the output before checking it against tool results.
fn get_path<'a>(doc: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    select(doc, &segments(path)).into_iter().next()
}

/// Null every value the path selects, returning the non-null ones removed.
/// Mirrors Fermi's `null_all` — null rather than removed so an absent key
/// is distinguishable from a serialisation bug.
fn null_all(doc: &mut serde_json::Value, segs: &[&str]) -> Vec<serde_json::Value> {
    let Some((head, rest)) = segs.split_first() else {
        if doc.is_null() {
            return vec![];
        }
        return vec![std::mem::replace(doc, serde_json::Value::Null)];
    };
    if *head == "[]" {
        return match doc.as_array_mut() {
            Some(items) => items.iter_mut().flat_map(|it| null_all(it, rest)).collect(),
            None => vec![],
        };
    }
    match doc.get_mut(head) {
        Some(v) => null_all(v, rest),
        None => vec![],
    }
}

/// Null the value at a dotted path (which may contain `[]` segments),
/// returning what was there. For array paths, all elements are nulled
/// and returned as a single `Value::Array` violation.
fn null_path(doc: &mut serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let removed = null_all(doc, &segments(path));
    match removed.len() {
        0 => None,
        1 => removed.into_iter().next(),
        _ => Some(serde_json::Value::Array(removed)),
    }
}

/// Top-level block a dotted path belongs to. `deliverables[].path` belongs
/// to block `deliverables`. Mirrors Fermi's `block_of`.
fn block_of(path: &str) -> &str {
    let head = path.split('.').next().unwrap_or(path);
    head.strip_suffix("[]").unwrap_or(head)
}

/// Truncate a value to a preview string for the Unsourced tag.
fn truncate_preview(value: &serde_json::Value) -> String {
    let s = match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if s.len() > 200 {
        let mut end = 200;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    } else {
        s
    }
}

/// How a narrative leak needle is matched. Mirrors Fermi's `LeakRule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeakRule {
    /// A distinctive word. Plain substring match is safe.
    Word(&'static str),
    /// A unit that only implies a claim when a number precedes it.
    /// This variant exists because a plain `" gb"` needle matches "GBIF" —
    /// so an honest summary citing its own source was reported as leaking
    /// a genome size. A check that fires on correct output is worse than
    /// no check: it gets switched off (paper Rule 5.2).
    Quantity(&'static str),
}

impl LeakRule {
    /// Does this rule fire against an already-lowercased haystack?
    pub fn matches(&self, haystack: &str) -> bool {
        match self {
            LeakRule::Word(w) => haystack.contains(w),
            LeakRule::Quantity(unit) => {
                let bytes = haystack.as_bytes();
                let mut from = 0usize;
                while let Some(rel) = haystack[from..].find(unit) {
                    let at = from + rel;
                    // Walk back over the separators a writer puts between a
                    // number and its unit: "480 Mb", "420-480Mb", "~90 mya".
                    // ASCII tilde (0x7E) is included; the 0xE2 byte was
                    // removed because it matches the first byte of any 3-byte
                    // UTF-8 sequence in the U+2xxx range (thousands of CJK
                    // and punctuation characters), not just Unicode tilde.
                    let mut i = at;
                    while i > 0 && matches!(bytes[i - 1], b' ' | b'-' | b'~') {
                        i -= 1;
                    }
                    if i > 0 && bytes[i - 1].is_ascii_digit() {
                        return true;
                    }
                    from = at + unit.len();
                }
                false
            }
        }
    }
}

/// Domain-specific narrative leak rules. Each entry is a (block, rule)
/// pair: if the block is NOT sourced and the rule matches the narrative,
/// it's a leak. Deliberately narrow, matched against a lowercased
/// haystack. Extend per agent domain as each is brought under contract.
pub const NARRATIVE_LEAK_RULES: &[(&str, LeakRule)] = &[
    // File paths — a fabricated deliverable path restated in prose.
    ("deliverable_path", LeakRule::Word("/src/")),
    ("deliverable_path", LeakRule::Word("/home/")),
    // Test verdicts — a fabricated test result restated in prose.
    ("test_verdict", LeakRule::Word("all tests passed")),
    ("test_verdict", LeakRule::Word("tests passed")),
    ("test_verdict", LeakRule::Word("0 failed")),
];

/// Narrative leak rules for ABW cloud delegation responses. These are
/// broader than the field-specific `NARRATIVE_LEAK_RULES` because ABW
/// responses are free prose with no structured fields and no `tool_calls`
/// visibility — the entire response is narrative. The rules detect claims
/// that an ABW cloud agent might fabricate without tool access.
///
/// Deliberately conservative (paper Rule 5.2): a rule that fires on
/// legitimate output is worse than no rule because it gets switched off.
/// Each rule targets a distinctive pattern unlikely to appear in honest
/// summary prose.
pub const ABW_NARRATIVE_LEAK_RULES: &[(&str, LeakRule)] = &[
    // File paths — same as task agent; fabricated paths in prose.
    ("narrative", LeakRule::Word("/src/")),
    ("narrative", LeakRule::Word("/home/")),
    // Test verdicts — claimed test results without tool visibility.
    ("narrative", LeakRule::Word("all tests passed")),
    ("narrative", LeakRule::Word("tests passed")),
    ("narrative", LeakRule::Word("0 failed")),
    // Claimed code execution — the ABW agent claims to have run code
    // but we have no tool_calls to verify it.
    ("narrative", LeakRule::Word("i ran the tests")),
    ("narrative", LeakRule::Word("i executed the code")),
];

/// Scan a narrative string for leak patterns. Used for ABW cloud
/// delegation responses where the entire output is prose (no structured
/// JSON, no `tool_calls`). Returns a `GroundingResult` with any narrative
/// leaks found — no fields are nulled because there are no structured
/// fields to null.
///
/// This is the narrative-only grounding path (paper §5.5: Narrative
/// disposition — "keep, scan for claims it cannot support"). The ABW
/// agent was commissioned to produce a response, so the prose is kept;
/// we scan for claims that look fabricated (file paths, test results,
/// code execution) that the agent could not have produced without tool
/// access we can't verify.
pub fn scan_narrative_for_leaks(narrative: &str) -> GroundingResult {
    let haystack = narrative.to_ascii_lowercase();
    let mut result = GroundingResult::default();
    for (block, rule) in ABW_NARRATIVE_LEAK_RULES {
        if rule.matches(&haystack) {
            result
                .narrative_leaks
                .push((format!("{:?} rule matched", block), block.to_string()));
        }
    }
    result
}

/// Scan narrative text for mentions of a removed value. Returns the
/// matching substring if found.
///
/// This is deliberately conservative — it checks whether the removed
/// value's string representation appears as a substring of the narrative.
/// Over-reach (paper Rule 5.2) is the main risk: a short removed value
/// like "0" would match any narrative containing "0". We mitigate by
/// requiring the preview to be at least 10 characters before scanning.
fn scan_narrative_for_leak(
    narrative: &str,
    removed_preview: &str,
    field_name: &str,
) -> Option<(String, String)> {
    if removed_preview.len() < 10 {
        return None;
    }
    if narrative.contains(removed_preview) {
        Some((removed_preview.to_string(), field_name.to_string()))
    } else {
        None
    }
}

/// Run the grounding contract against a delegation output.
///
/// - Sourced fields: keep, mark verified.
/// - Inferred fields (empty source list): keep, mark as inference.
/// - Fields not in the contract: mark as UncommissionedInference.
/// - Unsourced fields (in contract, no matching tool call): null,
///   retain a truncated preview.
/// - Narrative: scan for leaked removed values.
///
/// Returns the grounding result and a cleaned output with unsourced
/// fields nulled.
pub fn enforce_grounding(
    contract: &GroundingContract,
    output: &serde_json::Value,
    tool_calls: &[serde_json::Value],
    narrative: &str,
) -> (GroundingResult, serde_json::Value) {
    let successful = successful_tools(tool_calls);
    let failed = failed_tools(tool_calls);
    let mut result = GroundingResult::default();
    let mut cleaned = output.clone();

    if !output.is_object() {
        return (result, cleaned);
    }

    // ── Pass 1: null unsourced fields, mark provenance ───────────────
    // Track which blocks have at least one sourced field, for the
    // partial-sourcing narrative leak check (C7).
    let mut sourced_blocks: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (field, spec) in &contract.field_sources {
        // Skip fields not present in the output.
        if !path_has_claim(output, field) {
            continue;
        }

        if spec.sources.is_empty() {
            // Commissioned judgment — Inferred. No nulling needed.
            continue;
        }

        // Value-matching (Truth rung): the field value must appear in
        // the tool's return, not merely have the tool's name match.
        let field_value = get_path(output, field).unwrap_or(&serde_json::Value::Null);
        if is_value_sourced(
            &spec.sources,
            &successful,
            field_value,
            tool_calls,
            &spec.response_path,
        )
        .is_some()
        {
            // Sourced — mark the block as having sourced content.
            sourced_blocks.insert(block_of(field).to_string());
            continue;
        }

        // Declared tools exist but none were called successfully.
        // The field claims a value no tool supplied — null it.
        let removed = null_path(&mut cleaned, field);
        if let Some(ref removed_value) = removed {
            let preview = truncate_preview(removed_value);
            let tool_failed = tool_was_called_but_failed(&spec.sources, &failed);
            result.nulled_fields.push(field.clone());
            // Store the preview for narrative leak scanning in pass 2,
            // after we know which blocks are sourced (C7).
            let tag = ProvenanceTag::Unsourced {
                removed_preview: preview.clone(),
                tool_failed,
            };
            result.provenance.insert(field.clone(), tag.clone());
            // Stamp provenance on the document.
            if let serde_json::Value::Object(clean_map) = &mut cleaned {
                clean_map.insert(
                    format!("{}_provenance", block_of(field)),
                    serde_json::Value::String(provenance_stamp(&tag).to_string()),
                );
            }
        }
    }

    // ── Pass 2: mark Sourced, Inferred, and UncommissionedInference ──
    // for fields that were not nulled in pass 1.
    if let serde_json::Value::Object(map) = &output {
        for (field, value) in map {
            // Skip provenance stamps from a previous enforcement pass.
            if field.ends_with("_provenance") {
                continue;
            }
            // Skip if already handled in pass 1 (nulled fields).
            if result.provenance.contains_key(field) {
                continue;
            }
            // Skip top-level keys that are prefixes of contract dotted
            // paths (e.g., "deliverables" is a prefix of "deliverables[].path").
            // These are handled by pass 1 via array-path nulling; marking
            // them UncommissionedInference here would be a false positive.
            // An exact match (field == contract key) is NOT skipped — it
            // is the field itself, not a prefix.
            if contract.field_sources.keys().any(|k| {
                k.len() > field.len()
                    && k.starts_with(field)
                    && (k.as_bytes()[field.len()] == b'.' || k.as_bytes()[field.len()] == b'[')
            }) {
                continue;
            }
            let tag = match contract.field_sources.get(field) {
                Some(spec) if spec.sources.is_empty() => ProvenanceTag::Inferred,
                Some(spec) => {
                    if let Some(tool) = is_value_sourced(
                        &spec.sources,
                        &successful,
                        value,
                        tool_calls,
                        &spec.response_path,
                    ) {
                        ProvenanceTag::Sourced { tool }
                    } else if !is_claim(value) {
                        ProvenanceTag::Unsourced {
                            removed_preview: String::new(),
                            tool_failed: tool_was_called_but_failed(&spec.sources, &failed),
                        }
                    } else {
                        // This shouldn't happen — pass 1 should have nulled it.
                        // But if it does (e.g., the path didn't match), mark it.
                        ProvenanceTag::Unsourced {
                            removed_preview: truncate_preview(value),
                            tool_failed: tool_was_called_but_failed(&spec.sources, &failed),
                        }
                    }
                }
                None => ProvenanceTag::UncommissionedInference,
            };
            result.provenance.insert(field.clone(), tag.clone());
            // Stamp provenance on the document.
            if let serde_json::Value::Object(clean_map) = &mut cleaned {
                clean_map.insert(
                    format!("{field}_provenance"),
                    serde_json::Value::String(provenance_stamp(&tag).to_string()),
                );
            }
        }
    }

    // ── Pass 3: scan narrative for leaked values (C7) ───────────────
    // Only flag a leak if the nulled field's block is NOT sourced.
    // A narrative mention of a sourced block's value is legitimate;
    // a mention of an unsourced block's value is a leak.
    //
    // Two scan strategies:
    // 1. Substring match on the removed preview (≥10 chars).
    // 2. Domain-specific NARRATIVE_LEAK_RULES (Word + Quantity matching).
    let haystack = narrative.to_ascii_lowercase();
    for (field, tag) in &result.provenance {
        if let ProvenanceTag::Unsourced {
            removed_preview, ..
        } = tag
        {
            let block = block_of(field);
            if sourced_blocks.contains(block) {
                continue; // Block is sourced — narrative mention is legitimate.
            }
            // Strategy 1: substring match on the removed preview.
            if !removed_preview.is_empty() {
                if let Some(leak) = scan_narrative_for_leak(narrative, removed_preview, field) {
                    result.narrative_leaks.push(leak);
                    continue;
                }
            }
            // Strategy 2: domain-specific leak rules.
            for (rule_block, rule) in NARRATIVE_LEAK_RULES {
                if *rule_block == block && rule.matches(&haystack) {
                    result
                        .narrative_leaks
                        .push((format!("{rule_block:?} rule matched"), field.clone()));
                    break;
                }
            }
        }
    }

    (result, cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool_call(tool: &str, ok: bool) -> serde_json::Value {
        json!({ "tool": tool, "ok": ok })
    }

    /// Construct a successful tool call carrying a `result` payload. The
    /// `result` field is the data the tool returned — value-matching (Truth
    /// rung) checks an output field's value appears in this payload. Tests
    /// that only need name-based sourcing still pass because `ok` is true.
    fn tool_call_with_result(tool: &str, result: serde_json::Value) -> serde_json::Value {
        json!({ "tool": tool, "ok": true, "result": result })
    }

    // ── GroundingTrendReport tests live in crate::trend ────────────────

    #[test]
    fn sourced_field_kept_when_tool_succeeded() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "summary": "I wrote the file."
        });
        // The tool's result contains the path — value-matching (Truth rung)
        // checks the output's deliverable_path appears in the tool's return.
        let tool_calls = vec![tool_call_with_result(
            "zed/write_file",
            json!({"path": "/src/main.rs"}),
        )];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        assert_eq!(cleaned["deliverable_path"], "/src/main.rs");
        match &result.provenance["deliverable_path"] {
            ProvenanceTag::Sourced { tool } => {
                assert_eq!(tool, "zed/write_file");
            }
            other => panic!("expected Sourced, got {other:?}"),
        }
    }

    #[test]
    fn unsourced_field_nulled_when_no_tool_called() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "summary": "I wrote the file."
        });
        // No tool calls — deliverable_path is unsourced.
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert_eq!(result.nulled_fields, vec!["deliverable_path"]);
        assert!(cleaned["deliverable_path"].is_null());
        match &result.provenance["deliverable_path"] {
            ProvenanceTag::Unsourced {
                removed_preview,
                tool_failed,
            } => {
                assert_eq!(removed_preview, "/src/main.rs");
                assert!(!tool_failed, "no tool was called at all");
            }
            other => panic!("expected Unsourced, got {other:?}"),
        }
    }

    #[test]
    fn unsourced_field_nulled_when_tool_failed() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs"
        });
        // Tool was called but failed — did not supply data.
        let tool_calls = vec![tool_call("zed/write_file", false)];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert_eq!(result.nulled_fields, vec!["deliverable_path"]);
        assert!(cleaned["deliverable_path"].is_null());
        // The tool was called but failed — tool_failed should be true.
        match &result.provenance["deliverable_path"] {
            ProvenanceTag::Unsourced {
                tool_failed: true, ..
            } => {}
            other => panic!("expected Unsourced with tool_failed=true, got {other:?}"),
        }
    }

    #[test]
    fn inferred_field_kept() {
        let contract = task_agent_contract();
        let output = json!({
            "summary": "I completed the task by writing a new module."
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        assert_eq!(
            cleaned["summary"],
            "I completed the task by writing a new module."
        );
        assert_eq!(result.provenance["summary"], ProvenanceTag::Inferred);
    }

    #[test]
    fn uncommissioned_field_marked_but_kept() {
        let contract = task_agent_contract();
        // "author_name" is not in the contract — UncommissionedInference.
        let output = json!({
            "author_name": "Jane Doe"
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        assert_eq!(cleaned["author_name"], "Jane Doe");
        assert_eq!(
            result.provenance["author_name"],
            ProvenanceTag::UncommissionedInference
        );
    }

    #[test]
    fn narrative_leak_detected() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/very/long/path/to/main.rs"
        });
        // No tool calls — deliverable_path is nulled.
        let tool_calls: Vec<serde_json::Value> = vec![];
        // The narrative restates the nulled value.
        let narrative = "I wrote the file at /src/very/long/path/to/main.rs and it works.";

        let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, narrative);
        assert_eq!(result.nulled_fields, vec!["deliverable_path"]);
        assert_eq!(result.narrative_leaks.len(), 1);
        assert_eq!(result.narrative_leaks[0].1, "deliverable_path");
    }

    #[test]
    fn narrative_leak_not_detected_for_short_values() {
        // Short values (< 10 chars) are not scanned — over-reach mitigation
        // (paper Rule 5.2).
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/x"
        });
        let tool_calls: Vec<serde_json::Value> = vec![];
        let narrative = "The path /x is correct.";

        let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, narrative);
        assert_eq!(result.nulled_fields, vec!["deliverable_path"]);
        assert!(result.narrative_leaks.is_empty());
    }

    #[test]
    fn no_contract_fields_means_all_uncommissioned() {
        // An output with no fields in the contract — all UncommissionedInference.
        let contract = GroundingContract {
            agent_type: "task".to_string(),
            field_sources: HashMap::new(),
        };
        let output = json!({
            "random_field": "value",
            "another": 42
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        // Values are unchanged (UncommissionedInference is kept).
        assert_eq!(cleaned["random_field"], "value");
        assert_eq!(cleaned["another"], 42);
        // Provenance stamps are added.
        assert_eq!(
            cleaned["random_field_provenance"],
            "uncommissioned_inference"
        );
        assert_eq!(
            result.provenance["random_field"],
            ProvenanceTag::UncommissionedInference
        );
    }

    #[test]
    fn non_object_output_no_grounding() {
        // Prose output (not JSON) — no fields to ground.
        let contract = task_agent_contract();
        let output = serde_json::Value::String("just prose".to_string());
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.provenance.is_empty());
        assert!(result.nulled_fields.is_empty());
        assert_eq!(cleaned, output);
    }

    #[test]
    fn test_verdict_sourced_from_terminal() {
        let contract = task_agent_contract();
        let output = json!({
            "test_verdict": "pass: 3 tests ran, 0 failed"
        });
        // Terminal output contains the test verdict line.
        let tool_calls = vec![tool_call_with_result(
            "zed/terminal",
            json!("pass: 3 tests ran, 0 failed"),
        )];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        assert_eq!(cleaned["test_verdict"], "pass: 3 tests ran, 0 failed");
        match &result.provenance["test_verdict"] {
            ProvenanceTag::Sourced { tool } => {
                assert_eq!(tool, "zed/terminal");
            }
            other => panic!("expected Sourced, got {other:?}"),
        }
    }

    #[test]
    fn test_verdict_nulled_when_no_terminal_call() {
        // The agent claims tests passed but never ran them.
        let contract = task_agent_contract();
        let output = json!({
            "test_verdict": "pass: all tests passed"
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert_eq!(result.nulled_fields, vec!["test_verdict"]);
        assert!(cleaned["test_verdict"].is_null());
    }

    #[test]
    fn multiple_fields_mixed_grounding() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "test_verdict": "pass",
            "summary": "Done.",
            "unknown_field": "surprise"
        });
        // Only write_file succeeded — terminal was not called.
        // The write_file result contains the path so value-matching passes.
        let tool_calls = vec![tool_call_with_result(
            "zed/write_file",
            json!({"path": "/src/main.rs"}),
        )];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        // deliverable_path: Sourced (write_file succeeded)
        assert!(cleaned["deliverable_path"].is_string());
        // test_verdict: Unsourced (terminal not called)
        assert!(cleaned["test_verdict"].is_null());
        assert!(result.nulled_fields.contains(&"test_verdict".to_string()));
        // summary: Inferred
        assert_eq!(result.provenance["summary"], ProvenanceTag::Inferred);
        // unknown_field: UncommissionedInference
        assert_eq!(
            result.provenance["unknown_field"],
            ProvenanceTag::UncommissionedInference
        );
    }

    #[test]
    fn successful_tools_filters_failed_calls() {
        let tool_calls = vec![
            tool_call("zed/terminal", true),
            tool_call("zed/write_file", false),
            tool_call("zed/edit_file", true),
        ];
        let successful = successful_tools(&tool_calls);
        assert!(successful.contains("zed/terminal"));
        assert!(successful.contains("zed/edit_file"));
        assert!(!successful.contains("zed/write_file"));
    }

    #[test]
    fn truncate_preview_long_values() {
        let long = serde_json::Value::String("x".repeat(300));
        let preview = truncate_preview(&long);
        assert!(preview.ends_with("..."));
        assert_eq!(preview.len(), 203); // 200 + "..."
    }

    #[test]
    fn truncate_preview_short_values_unchanged() {
        let short = serde_json::Value::String("short".to_string());
        let preview = truncate_preview(&short);
        assert_eq!(preview, "short");
    }

    #[test]
    fn truncate_preview_multibyte_utf8_does_not_panic() {
        // A string where byte 200 falls inside a multibyte character.
        // "é" is 2 bytes (0xC3 0xA9); 101 copies = 202 bytes.
        // Byte 200 is inside the 101st "é" — &s[..200] would panic.
        let value = serde_json::Value::String("é".repeat(101));
        let preview = truncate_preview(&value);
        assert!(preview.ends_with("..."));
        // Must not panic — that's the test.
    }

    // ── C1: Derived provenance variant ──────────────────────────────────

    #[test]
    fn derived_field_survives_and_is_marked() {
        // A derived field is computed by platform code from a sourced value.
        // It should survive grounding (not nulled) and be marked Derived.
        let mut field_sources = HashMap::new();
        field_sources.insert(
            "file_extension".to_string(),
            FieldSpec {
                sources: vec![],
                response_path: "".to_string(),
                why: "Derived from deliverable_path by platform code \
                      (extension extraction). Not a tool call."
                    .to_string(),
            },
        );
        // Override: mark as Derived by using empty sources (Inferred path)
        // — a true Derived would need platform code to compute it, which
        // is outside the grounding checker's scope. The tag exists so
        // platform code can stamp it when it computes a derivation.
        let contract = GroundingContract {
            agent_type: "task".to_string(),
            field_sources,
        };
        let output = json!({"file_extension": "rs"});
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        assert_eq!(cleaned["file_extension"], "rs");
        // With empty sources, it's Inferred (commissioned). A Derived tag
        // would be stamped by platform code after grounding, not by the
        // grounding checker itself.
        assert_eq!(result.provenance["file_extension"], ProvenanceTag::Inferred);
    }

    #[test]
    fn provenance_stamp_for_derived_tag() {
        // The provenance_stamp function maps Derived to "platform_derived".
        let tag = ProvenanceTag::Derived {
            from: "taxonomy.order".to_string(),
            how: "ncbi_tools::superorder_of".to_string(),
        };
        assert_eq!(provenance_stamp(&tag), "platform_derived");
    }

    // ── C2: tool_no_match distinction ───────────────────────────────────

    #[test]
    fn failed_tool_distinguished_from_no_tool() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs"
        });
        // Tool was called but failed.
        let tool_calls = vec![tool_call("zed/write_file", false)];

        let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        match &result.provenance["deliverable_path"] {
            ProvenanceTag::Unsourced {
                tool_failed: true, ..
            } => {}
            other => panic!("expected Unsourced with tool_failed=true, got {other:?}"),
        }
    }

    #[test]
    fn no_tool_called_is_not_tool_failed() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs"
        });
        // No tool calls at all.
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        match &result.provenance["deliverable_path"] {
            ProvenanceTag::Unsourced {
                tool_failed: false, ..
            } => {}
            other => panic!("expected Unsourced with tool_failed=false, got {other:?}"),
        }
    }

    #[test]
    fn failed_tools_extracts_failed_calls() {
        let tool_calls = vec![
            tool_call("zed/terminal", true),
            tool_call("zed/write_file", false),
        ];
        let failed = failed_tools(&tool_calls);
        assert!(failed.contains("zed/write_file"));
        assert!(!failed.contains("zed/terminal"));
    }

    // ── C3: provenance stamping on the document ─────────────────────────

    #[test]
    fn document_carries_provenance_keys() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "summary": "Done."
        });
        let tool_calls = vec![tool_call_with_result(
            "zed/write_file",
            json!({"path": "/src/main.rs"}),
        )];

        let (_result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert_eq!(cleaned["deliverable_path_provenance"], "tool_verified");
        assert_eq!(cleaned["summary_provenance"], "model_inference");
    }

    #[test]
    fn document_carries_unavailable_provenance_for_nulled_fields() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs"
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (_result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(cleaned["deliverable_path"].is_null());
        assert_eq!(
            cleaned["deliverable_path_provenance"],
            "unavailable_no_tool_source"
        );
    }

    #[test]
    fn document_carries_tool_no_match_for_failed_tools() {
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs"
        });
        let tool_calls = vec![tool_call("zed/write_file", false)];

        let (_result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert_eq!(cleaned["deliverable_path_provenance"], "tool_no_match");
    }

    #[test]
    fn document_carries_uncommissioned_for_unknown_fields() {
        let contract = task_agent_contract();
        let output = json!({
            "author_name": "Jane Doe"
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (_result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert_eq!(
            cleaned["author_name_provenance"],
            "uncommissioned_inference"
        );
    }

    // ── C5: idempotency ────────────────────────────────────────────────

    #[test]
    fn enforce_grounding_is_idempotent() {
        // A second pass of enforce_grounding on an already-enforced document
        // must find no new violations. Critical for cached/re-read results.
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "summary": "Done."
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (first_result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(
            !first_result.is_clean(),
            "first pass should find violations"
        );

        // Second pass on the cleaned document.
        let (second_result, _re_cleaned) = enforce_grounding(&contract, &cleaned, &tool_calls, "");
        assert!(
            second_result.is_clean(),
            "second pass must find nothing; otherwise the validator would \
             log an anomaly every time a cached result is re-read: {:?}",
            second_result
        );
    }

    // ── C6: why field mandatory ────────────────────────────────────────

    #[test]
    fn contract_rejects_short_why() {
        // The why field must be ≥40 chars. An unexplained entry is how
        // the contract rots. This test verifies the built-in contract
        // passes the check, and a deliberately short why fails it.
        let contract = task_agent_contract();
        for spec in contract.field_sources.values() {
            assert!(
                spec.why.len() >= 40,
                "built-in contract field has a short why: '{}'",
                spec.why
            );
        }
        // A deliberately short why should fail the check.
        let bad_spec = FieldSpec {
            sources: vec!["zed/terminal".to_string()],
            response_path: "".to_string(),
            why: "too short".to_string(),
        };
        assert!(bad_spec.why.len() < 40);
    }

    #[test]
    fn task_agent_contract_has_why_for_every_field() {
        let contract = task_agent_contract();
        for (field, spec) in &contract.field_sources {
            assert!(
                spec.why.len() >= 40,
                "field '{}' has a short why ({} chars): '{}'",
                field,
                spec.why.len(),
                spec.why
            );
        }
    }

    // ── C7: partial-sourcing awareness ─────────────────────────────────

    #[test]
    fn narrative_leak_not_flagged_when_block_is_sourced() {
        // If the deliverable_path block has a sourced field, a narrative
        // mention of a value from that block is legitimate, not a leak.
        // This is the partial-sourcing property: a leak is only flagged
        // if the block is NOT sourced.
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "summary": "I wrote the file at /src/main.rs and it works."
        });
        // write_file succeeded and its result contains the path —
        // deliverable_path is sourced (value-matched).
        let tool_calls = vec![tool_call_with_result(
            "zed/write_file",
            json!({"path": "/src/main.rs"}),
        )];

        let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        // No nulled fields, no narrative leaks.
        assert!(result.nulled_fields.is_empty());
        assert!(
            result.narrative_leaks.is_empty(),
            "narrative mentioning a sourced block's value is not a leak: {:?}",
            result.narrative_leaks
        );
    }

    #[test]
    fn narrative_leak_flagged_when_block_is_not_sourced() {
        // If the deliverable_path block is NOT sourced (no tool call),
        // a narrative mention of the nulled value IS a leak.
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/very/long/path/to/main.rs",
            "summary": "I wrote the file at /src/very/long/path/to/main.rs."
        });
        // No tool calls — deliverable_path is unsourced.
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, _cleaned) = enforce_grounding(
            &contract,
            &output,
            &tool_calls,
            "I wrote the file at /src/very/long/path/to/main.rs.",
        );
        assert!(
            result
                .nulled_fields
                .contains(&"deliverable_path".to_string())
        );
        assert_eq!(result.narrative_leaks.len(), 1);
        assert_eq!(result.narrative_leaks[0].1, "deliverable_path");
    }

    // ── C8: array path support ──────────────────────────────────────────

    #[test]
    fn array_path_nulls_all_elements() {
        // A contract with an array path: `deliverables[].path`.
        let mut field_sources = HashMap::new();
        field_sources.insert(
            "deliverables[].path".to_string(),
            FieldSpec {
                sources: vec!["zed/write_file".to_string()],
                response_path: "".to_string(),
                why: "Each deliverable's path must be sourced from a \
                      file-writing tool that succeeded."
                    .to_string(),
            },
        );
        let contract = GroundingContract {
            agent_type: "task".to_string(),
            field_sources,
        };
        let output = json!({
            "deliverables": [
                {"path": "/src/a.rs", "description": "module a"},
                {"path": "/src/b.rs", "description": "module b"}
            ]
        });
        // No tool calls — all paths are unsourced.
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(
            result
                .nulled_fields
                .contains(&"deliverables[].path".to_string()),
            "array path should be nulled"
        );
        // Both array elements should have their path nulled.
        let deliverables = cleaned["deliverables"].as_array().unwrap();
        assert_eq!(deliverables.len(), 2);
        assert!(deliverables[0]["path"].is_null(), "first path nulled");
        assert!(deliverables[1]["path"].is_null(), "second path nulled");
        // Non-contract fields survive.
        assert_eq!(deliverables[0]["description"], "module a");
        assert_eq!(deliverables[1]["description"], "module b");
    }

    #[test]
    fn array_path_sourced_when_tool_succeeded() {
        let mut field_sources = HashMap::new();
        field_sources.insert(
            "deliverables[].path".to_string(),
            FieldSpec {
                sources: vec!["zed/write_file".to_string()],
                response_path: "".to_string(),
                why: "Each deliverable's path must be sourced from a \
                      file-writing tool that succeeded."
                    .to_string(),
            },
        );
        let contract = GroundingContract {
            agent_type: "task".to_string(),
            field_sources,
        };
        let output = json!({
            "deliverables": [
                {"path": "/src/a.rs"},
                {"path": "/src/b.rs"}
            ]
        });
        // The tool's result contains both paths — value-matching checks
        // each array element appears in the tool's return.
        let tool_calls = vec![tool_call_with_result(
            "zed/write_file",
            json!([{"path": "/src/a.rs"}, {"path": "/src/b.rs"}]),
        )];

        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        let deliverables = cleaned["deliverables"].as_array().unwrap();
        assert_eq!(deliverables[0]["path"], "/src/a.rs");
        assert_eq!(deliverables[1]["path"], "/src/b.rs");
    }

    #[test]
    fn array_path_reported_as_one_violation_not_per_element() {
        let mut field_sources = HashMap::new();
        field_sources.insert(
            "deliverables[].path".to_string(),
            FieldSpec {
                sources: vec!["zed/write_file".to_string()],
                response_path: "".to_string(),
                why: "Each deliverable's path must be sourced from a \
                      file-writing tool that succeeded."
                    .to_string(),
            },
        );
        let contract = GroundingContract {
            agent_type: "task".to_string(),
            field_sources,
        };
        let output = json!({
            "deliverables": [
                {"path": "/src/a.rs"},
                {"path": "/src/b.rs"}
            ]
        });
        let tool_calls: Vec<serde_json::Value> = vec![];

        let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        // One violation for the array path, not one per element.
        let count = result
            .nulled_fields
            .iter()
            .filter(|f| *f == "deliverables[].path")
            .count();
        assert_eq!(
            count, 1,
            "array path should be one violation, not one per element"
        );
    }

    #[test]
    fn block_of_extracts_top_level_from_array_path() {
        assert_eq!(block_of("deliverables[].path"), "deliverables");
        assert_eq!(block_of("deliverable_path"), "deliverable_path");
        assert_eq!(block_of("threats[].species"), "threats");
    }

    #[test]
    fn segments_split_array_marker() {
        let segs = segments("deliverables[].path");
        assert_eq!(segs, vec!["deliverables", "[]", "path"]);
        let segs = segments("deliverable_path");
        assert_eq!(segs, vec!["deliverable_path"]);
    }

    #[test]
    fn select_walks_array_elements() {
        let doc = json!({
            "deliverables": [
                {"path": "/a.rs"},
                {"path": "/b.rs"}
            ]
        });
        let paths = select(&doc, &segments("deliverables[].path"));
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], "/a.rs");
        assert_eq!(paths[1], "/b.rs");
    }

    #[test]
    fn path_has_claim_detects_claims_in_arrays() {
        let doc = json!({
            "deliverables": [
                {"path": "/a.rs"},
                {"path": null}
            ]
        });
        assert!(path_has_claim(&doc, "deliverables[].path"));
        let doc_empty = json!({
            "deliverables": [
                {"path": null},
                {"path": null}
            ]
        });
        assert!(!path_has_claim(&doc_empty, "deliverables[].path"));
    }

    // ── N5: LeakRule::Quantity ─────────────────────────────────────────

    #[test]
    fn leak_rule_word_matches_substring() {
        assert!(LeakRule::Word("/src/").matches("i wrote the file at /src/main.rs"));
        assert!(!LeakRule::Word("/src/").matches("no path here"));
    }

    #[test]
    fn leak_rule_quantity_matches_number_before_unit() {
        // "480 mb" is a leak — a number precedes the unit.
        assert!(LeakRule::Quantity("mb").matches("a genome of 480 mb"));
        assert!(LeakRule::Quantity("mb").matches("~480mb"));
        assert!(LeakRule::Quantity("mb").matches("420-480 mb"));
    }

    #[test]
    fn leak_rule_quantity_does_not_match_word_containing_unit() {
        // "GBIF" contains "gb" but no digit precedes it — NOT a leak.
        // This is the paper's Rule 5.2 example: the first version used
        // a plain " gb" needle which matched "GBIF", so an honest summary
        // citing its source was flagged as fabricating a genome size.
        assert!(!LeakRule::Quantity("gb").matches("gbif taxonomy"));
        assert!(!LeakRule::Quantity("mb").matches("mb but no number"));
    }

    #[test]
    fn leak_rule_quantity_matches_with_separators() {
        // Separators between number and unit: space, dash, tilde.
        assert!(LeakRule::Quantity("mya").matches("diverged ~90 mya"));
        assert!(LeakRule::Quantity("mya").matches("90-100 mya"));
    }

    #[test]
    fn narrative_leak_rules_table_is_nonempty() {
        assert!(!NARRATIVE_LEAK_RULES.is_empty());
        // Every block in the rules table should be a plausible field name.
        for (block, _) in NARRATIVE_LEAK_RULES {
            assert!(!block.is_empty(), "empty block name in leak rules");
        }
    }

    // ── Property-based tests ─────────────────────────────────────────────
    // Uses the hkask-test-harness `arb_json_value` generator to verify
    // that `enforce_grounding` never panics on arbitrary JSON input and
    // preserves key invariants.

    use proptest::prelude::*;

    /// Generate a tool_calls summary entry: {"tool": <string>, "ok": <bool>}.
    fn arb_tool_call() -> BoxedStrategy<serde_json::Value> {
        ("[a-z][a-z0-9_/]*", any::<bool>())
            .prop_map(|(tool, ok)| json!({ "tool": tool, "ok": ok }))
            .boxed()
    }

    /// Generate a vec of tool_calls.
    fn arb_tool_calls() -> BoxedStrategy<Vec<serde_json::Value>> {
        prop::collection::vec(arb_tool_call(), 0..8).boxed()
    }

    proptest! {
        /// `enforce_grounding` never panics on arbitrary JSON output + tool_calls.
        /// This is the baseline robustness property — the grounding checker is
        /// on the hot path of every task-agent delegation and must not crash.
        #[test]
        fn enforce_grounding_never_panics(
            output in hkask_test_harness::arb_json_value(),
            tool_calls in arb_tool_calls(),
            narrative in "[a-zA-Z0-9 /._-]{0,200}",
        ) {
            let contract = task_agent_contract();
            let result = std::panic::catch_unwind(|| {
                enforce_grounding(&contract, &output, &tool_calls, &narrative)
            });
            prop_assert!(result.is_ok(), "panicked on output={output}, tool_calls={tool_calls:?}");
        }

        /// Nulled fields are always a subset of the output's keys. The
        /// grounding check must never null a field that doesn't exist in
        /// the output, and must never null a field that is Sourced or
        /// Inferred.
        #[test]
        fn nulled_fields_are_subset_of_output_keys(
            output in hkask_test_harness::arb_json_value(),
            tool_calls in arb_tool_calls(),
        ) {
            let contract = task_agent_contract();
            let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
            let output_keys: std::collections::HashSet<String> = match &output {
                serde_json::Value::Object(map) => {
                    map.keys().map(|k| k.to_string()).collect()
                }
                _ => std::collections::HashSet::new(),
            };
            for nulled in &result.nulled_fields {
                prop_assert!(
                    output_keys.contains(nulled),
                    "nulled field '{}' not in output keys {:?}",
                    nulled, output_keys
                );
            }
        }

        /// Every nulled field has an Unsourced provenance tag. A field
        /// that is Sourced, Inferred, or UncommissionedInference must not
        /// appear in nulled_fields.
        #[test]
        fn nulled_fields_have_unsourced_provenance(
            output in hkask_test_harness::arb_json_value(),
            tool_calls in arb_tool_calls(),
        ) {
            let contract = task_agent_contract();
            let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
            for nulled in &result.nulled_fields {
                let tag = result.provenance.get(nulled);
                prop_assert!(
                    matches!(tag, Some(ProvenanceTag::Unsourced { .. })),
                    "nulled field '{}' has provenance {:?}, expected Unsourced",
                    nulled, tag
                );
            }
        }

        /// The cleaned output preserves Sourced and Inferred fields
        /// unchanged. Only Unsourced fields are nulled.
        #[test]
        fn cleaned_output_preserves_sourced_and_inferred_fields(
            output in hkask_test_harness::arb_json_value(),
            tool_calls in arb_tool_calls(),
        ) {
            let contract = task_agent_contract();
            let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
            if let (serde_json::Value::Object(orig), serde_json::Value::Object(clean)) =
                (&output, &cleaned)
            {
                for (key, orig_value) in orig {
                    let tag = result.provenance.get(key);
                    match tag {
                        Some(ProvenanceTag::Sourced { .. })
                        | Some(ProvenanceTag::Inferred)
                        | Some(ProvenanceTag::Derived { .. })
                        | Some(ProvenanceTag::UncommissionedInference) => {
                            prop_assert_eq!(
                                clean.get(key),
                                Some(orig_value),
                                "field '{}' was modified despite non-Unsourced provenance",
                                key
                            );
                        }
                        Some(ProvenanceTag::Unsourced { .. }) => {
                            prop_assert_eq!(
                                clean.get(key),
                                Some(&serde_json::Value::Null),
                                "Unsourced field '{}' was not nulled in cleaned output",
                                key
                            );
                        }
                        _ => {}
                    }
                }
            }
        }

        /// `successful_tools` never includes a tool call where `ok` is false
        /// AND the tool does not also appear with `ok: true`. A tool called
        /// twice (once ok, once failed) appears in both sets — that's correct
        /// because it did supply data on one call.
        #[test]
        fn successful_tools_excludes_failed_calls(
            tool_calls in arb_tool_calls(),
        ) {
            let successful = successful_tools(&tool_calls);
            let failed = failed_tools(&tool_calls);
            // A tool that appears ONLY with ok=false must not be in successful.
            let only_failed: std::collections::HashSet<&str> = failed
                .iter()
                .filter(|t| !successful.contains(*t))
                .map(|s| s.as_str())
                .collect();
            for tc in &tool_calls {
                let ok = tc.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
                let tool = tc.get("tool").and_then(|v| v.as_str()).unwrap_or("");
                if !ok && only_failed.contains(tool) {
                    prop_assert!(
                        !successful.contains(tool),
                        "tool '{}' only failed but appeared in successful set",
                        tool
                    );
                }
            }
        }
    }

    // ── ABW narrative-only grounding tests ────────────────────────────────

    #[test]
    fn scan_narrative_detects_file_paths() {
        let result = scan_narrative_for_leaks("I wrote the file at /src/main.rs and it compiles.");
        assert!(!result.narrative_leaks.is_empty(), "must detect /src/ path");
        assert!(
            result.nulled_fields.is_empty(),
            "no fields nulled in narrative-only mode"
        );
    }

    #[test]
    fn scan_narrative_detects_test_verdicts() {
        let result = scan_narrative_for_leaks("All tests passed successfully.");
        assert!(
            !result.narrative_leaks.is_empty(),
            "must detect test verdict claim"
        );
    }

    #[test]
    fn scan_narrative_detects_code_execution_claims() {
        let result = scan_narrative_for_leaks("I ran the tests and they all passed.");
        assert!(
            !result.narrative_leaks.is_empty(),
            "must detect code execution claim"
        );
    }

    #[test]
    fn scan_narrative_clean_prose_has_no_leaks() {
        let result = scan_narrative_for_leaks(
            "The bestiary recommends the market_analyst agent for this task.",
        );
        assert!(
            result.narrative_leaks.is_empty(),
            "clean prose must not trigger leaks"
        );
        assert!(result.is_clean(), "clean prose must produce a clean result");
    }

    #[test]
    fn scan_narrative_empty_string_is_clean() {
        let result = scan_narrative_for_leaks("");
        assert!(
            result.is_clean(),
            "empty string must produce a clean result"
        );
    }

    // ── Research agent contract ──

    #[test]
    fn research_agent_contract_has_why_for_every_field() {
        let contract = research_agent_contract();
        for (field, spec) in &contract.field_sources {
            assert!(
                spec.why.len() >= 40,
                "field '{}' has a short why ({} chars): '{}'",
                field,
                spec.why.len(),
                spec.why
            );
        }
    }

    #[test]
    fn research_agent_contract_sources_field_nulled_when_no_search_tool_called() {
        // Falsification test: the `sources` field must be nulled when no
        // search tool was called. This is the check that proves the contract
        // is enforced — without it, the contract is inert.
        let contract = research_agent_contract();
        let output = json!({
            "sources": ["https://example.com/fabricated"],
            "findings": "some analysis",
            "summary": "research complete"
        });
        let (result, cleaned) = enforce_grounding(&contract, &output, &[], &output.to_string());
        assert!(
            result.nulled_fields.contains(&"sources".to_string()),
            "sources must be nulled when no search tool was called"
        );
        assert_eq!(
            cleaned.get("sources"),
            Some(&json!(null)),
            "sources must be null in cleaned output"
        );
    }

    #[test]
    fn research_agent_contract_sources_field_kept_when_search_tool_succeeded() {
        let contract = research_agent_contract();
        let output = json!({
            "sources": ["https://example.com/real"],
            "findings": "some analysis",
            "summary": "research complete"
        });
        // The tool's result contains the URL — value-matching (Truth rung)
        // checks the output's sources entry appears in the tool's return.
        let tool_calls = vec![tool_call_with_result(
            "zed/web_search",
            json!({"results": [{"url": "https://example.com/real"}]}),
        )];
        let (result, cleaned) =
            enforce_grounding(&contract, &output, &tool_calls, &output.to_string());
        assert!(
            !result.nulled_fields.contains(&"sources".to_string()),
            "sources must NOT be nulled when web_search succeeded"
        );
        assert_eq!(
            cleaned.get("sources"),
            Some(&json!(vec!["https://example.com/real"])),
            "sources must be preserved in cleaned output"
        );
    }

    #[test]
    fn research_agent_contract_findings_and_summary_are_inferred() {
        // `findings` and `summary` are commissioned judgments (empty source
        // list = Inferred). They must NOT be nulled even when no tools were
        // called — the agent was asked to produce them.
        let contract = research_agent_contract();
        let output = json!({
            "findings": "the analysis shows...",
            "summary": "research complete"
        });
        let (result, cleaned) = enforce_grounding(&contract, &output, &[], &output.to_string());
        assert!(
            !result.nulled_fields.contains(&"findings".to_string()),
            "findings must NOT be nulled (commissioned judgment)"
        );
        assert!(
            !result.nulled_fields.contains(&"summary".to_string()),
            "summary must NOT be nulled (commissioned judgment)"
        );
        assert_eq!(
            cleaned.get("findings"),
            Some(&json!("the analysis shows..."))
        );
        assert_eq!(cleaned.get("summary"), Some(&json!("research complete")));
    }

    // ── Narrator agent contract ──

    #[test]
    fn narrator_agent_contract_has_why_for_every_field() {
        let contract = narrator_agent_contract();
        for (field, spec) in &contract.field_sources {
            assert!(
                spec.why.len() >= 40,
                "field '{}' has a short why ({} chars): '{}'",
                field,
                spec.why.len(),
                spec.why
            );
        }
    }

    #[test]
    fn narrator_agent_contract_content_and_summary_are_inferred() {
        // Both `content` and `summary` are commissioned judgments. They must
        // NOT be nulled even when no tools were called.
        let contract = narrator_agent_contract();
        let output = json!({
            "content": "Once upon a time...",
            "summary": "a story about time"
        });
        let (result, cleaned) = enforce_grounding(&contract, &output, &[], &output.to_string());
        assert!(
            !result.nulled_fields.contains(&"content".to_string()),
            "content must NOT be nulled (commissioned judgment)"
        );
        assert!(
            !result.nulled_fields.contains(&"summary".to_string()),
            "summary must NOT be nulled (commissioned judgment)"
        );
        assert_eq!(cleaned.get("content"), Some(&json!("Once upon a time...")));
        assert_eq!(cleaned.get("summary"), Some(&json!("a story about time")));
    }

    #[test]
    fn narrator_agent_contract_unsourced_file_path_is_uncommissioned() {
        // A fabricated file path in the output (not in the contract) is
        // treated as UncommissionedInference (kept, marked) — not Unsourced
        // (nulled). The contract only nulls fields that are declared with
        // source tools but have no matching tool call. Undeclared fields are
        // kept as uncommissioned inferences.
        let contract = narrator_agent_contract();
        let output = json!({
            "content": "Once upon a time...",
            "summary": "a story about time",
            "file_path": "/output/story.txt"
        });
        let (result, cleaned) = enforce_grounding(&contract, &output, &[], &output.to_string());
        // file_path is NOT nulled — it's uncommissioned, not unsourced.
        assert!(
            !result.nulled_fields.contains(&"file_path".to_string()),
            "file_path must NOT be nulled (uncommissioned, not unsourced)"
        );
        // But it IS marked as uncommissioned in the provenance.
        assert_eq!(
            result.provenance.get("file_path"),
            Some(&ProvenanceTag::UncommissionedInference)
        );
        // The value is preserved.
        assert_eq!(cleaned.get("file_path"), Some(&json!("/output/story.txt")));
    }

    // ── Skill agent contract (Phase 5) ──

    #[test]
    fn skill_agent_contract_has_why_for_every_field() {
        let contract = skill_agent_contract();
        for (field, spec) in &contract.field_sources {
            assert!(
                spec.why.len() >= 40,
                "field '{}' has a short why ({} chars): '{}'",
                field,
                spec.why.len(),
                spec.why
            );
        }
    }

    #[test]
    fn skill_agent_contract_deliverable_path_nulled_when_no_file_tool_called() {
        // Falsification test: the `deliverable_path` field must be nulled when
        // no file-writing tool was called. This is the check that proves the
        // skill contract catches fabricated file paths.
        let contract = skill_agent_contract();
        let output = json!({
            "deliverable_path": "/src/generated.rs",
            "diagram": "graph TD\nA-->B",
            "summary": "generated a diagram"
        });
        let (result, cleaned) = enforce_grounding(&contract, &output, &[], &output.to_string());
        assert!(
            result
                .nulled_fields
                .contains(&"deliverable_path".to_string()),
            "deliverable_path must be nulled when no file-writing tool was called"
        );
        assert_eq!(
            cleaned.get("deliverable_path"),
            Some(&serde_json::Value::Null),
            "nulled field must be null in cleaned output"
        );
    }

    #[test]
    fn skill_agent_contract_deliverable_path_kept_when_file_tool_succeeded() {
        let contract = skill_agent_contract();
        let output = json!({
            "deliverable_path": "/src/generated.rs",
            "diagram": "graph TD\nA-->B",
            "summary": "generated a diagram"
        });
        // The tool's result contains the path — value-matching (Truth rung)
        // checks the output's deliverable_path appears in the tool's return.
        let tool_calls = vec![tool_call_with_result(
            "zed/write_file",
            json!({"path": "/src/generated.rs"}),
        )];
        let (result, cleaned) =
            enforce_grounding(&contract, &output, &tool_calls, &output.to_string());
        assert!(
            !result
                .nulled_fields
                .contains(&"deliverable_path".to_string()),
            "deliverable_path must NOT be nulled when write_file succeeded"
        );
        assert_eq!(
            cleaned.get("deliverable_path"),
            Some(&json!("/src/generated.rs"))
        );
    }

    #[test]
    fn skill_agent_contract_diagram_and_summary_are_inferred() {
        // `diagram`, `summary`, and `recommendations` are commissioned
        // judgments (empty source list = Inferred). They must NOT be nulled
        // even when no tools were called.
        let contract = skill_agent_contract();
        let output = json!({
            "diagram": "graph TD\nA-->B",
            "summary": "generated a diagram",
            "recommendations": [{"action": "test"}]
        });
        let (result, cleaned) = enforce_grounding(&contract, &output, &[], &output.to_string());
        assert!(
            !result.nulled_fields.contains(&"diagram".to_string()),
            "diagram must NOT be nulled (commissioned judgment)"
        );
        assert!(
            !result.nulled_fields.contains(&"summary".to_string()),
            "summary must NOT be nulled (commissioned judgment)"
        );
        assert!(
            !result
                .nulled_fields
                .contains(&"recommendations".to_string()),
            "recommendations must NOT be nulled (commissioned judgment)"
        );
        assert_eq!(cleaned.get("diagram"), Some(&json!("graph TD\nA-->B")));
    }

    #[test]
    fn skill_agent_contract_test_verdict_nulled_when_no_terminal_called() {
        let contract = skill_agent_contract();
        let output = json!({
            "test_verdict": "pass",
            "summary": "all tests passed"
        });
        let (result, _cleaned) = enforce_grounding(&contract, &output, &[], &output.to_string());
        assert!(
            result.nulled_fields.contains(&"test_verdict".to_string()),
            "test_verdict must be nulled when no terminal tool was called"
        );
    }

    // ── Production-scar tests (Truth rung value-matching) ─────────────

    #[test]
    fn sourced_field_with_matching_value_survives() {
        // The basic Truth rung: a field value that appears in the tool's
        // return is genuinely sourced. This is the happy path.
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "summary": "done"
        });
        let tool_calls = vec![tool_call_with_result(
            "zed/write_file",
            json!({"path": "/src/main.rs", "bytes_written": 42}),
        )];
        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(result.nulled_fields.is_empty());
        assert_eq!(cleaned["deliverable_path"], "/src/main.rs");
    }

    #[test]
    fn sourced_field_with_non_matching_value_is_nulled() {
        // The Antaxius-beieri scar: a field that looks sourced because a
        // source exists, but the value didn't come from the tool. The tool
        // returned "/src/real.rs" but the agent claimed "/src/fabricated.rs".
        // Without value-matching, this passes — the tool ran, so "sourced."
        // With value-matching, the value must appear in the tool's return.
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/fabricated.rs",
            "summary": "done"
        });
        let tool_calls = vec![tool_call_with_result(
            "zed/write_file",
            json!({"path": "/src/real.rs"}),
        )];
        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(
            result
                .nulled_fields
                .contains(&"deliverable_path".to_string()),
            "a value not in the tool's return must be nulled (Antaxius-beieri scar)"
        );
        assert!(cleaned["deliverable_path"].is_null());
    }

    #[test]
    fn sourced_field_with_missing_result_in_tool_call_is_nulled() {
        // A tool call without a `result` field (e.g. from an older code path
        // or a tool that returned nothing) cannot value-match. The field is
        // nulled — "tool ran" without a return is not "value came from tool."
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/main.rs",
            "summary": "done"
        });
        // Old-style tool call without `result` — value-matching cannot verify.
        let tool_calls = vec![json!({"tool": "zed/write_file", "ok": true})];
        let (result, cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        assert!(
            result
                .nulled_fields
                .contains(&"deliverable_path".to_string()),
            "a tool call without `result` cannot value-match — must be nulled"
        );
        assert!(cleaned["deliverable_path"].is_null());
    }

    #[test]
    fn tool_no_match_distinguishable_from_no_tool() {
        // A tool that was called and returned a result that doesn't contain
        // the field value is `tool_failed: false` (the tool ran, the value
        // just didn't come from it). A tool that was never called is also
        // `tool_failed: false`. The distinction is in whether the tool is
        // in the `failed` set vs absent from `successful`.
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "/src/invented.rs",
            "summary": "done"
        });
        // Tool ran successfully but returned a different path.
        let tool_calls = vec![tool_call_with_result(
            "zed/write_file",
            json!({"path": "/src/real.rs"}),
        )];
        let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        let tag = &result.provenance["deliverable_path"];
        match tag {
            ProvenanceTag::Unsourced { tool_failed, .. } => {
                // The tool ran and succeeded — it just didn't return this value.
                // So tool_failed is false (the tool didn't fail, the value didn't match).
                assert!(
                    !tool_failed,
                    "tool ran successfully, value just didn't match"
                );
            }
            other => panic!("expected Unsourced, got {other:?}"),
        }
    }

    #[test]
    fn placeholder_ellipsis_is_absence_not_fabrication() {
        // The card's own schema example uses "..." as filler. A model
        // echoing it has declined to answer, not invented a value.
        let contract = task_agent_contract();
        let output = json!({
            "deliverable_path": "...",
            "summary": "done"
        });
        let tool_calls: Vec<serde_json::Value> = vec![];
        let (result, _cleaned) = enforce_grounding(&contract, &output, &tool_calls, "");
        // "..." is not a claim (is_claim returns false for it), so it should
        // not be nulled as a violation — it's an absence.
        assert!(
            !result
                .nulled_fields
                .contains(&"deliverable_path".to_string()),
            "'...' is the card's filler, not a fabrication: {:?}",
            result.nulled_fields
        );
    }
}
