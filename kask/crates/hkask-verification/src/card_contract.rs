//! Card-declared grounding contract validation (N1).
//!
//! Fermi's `card_contract.rs` pattern: validate a card's
//! `output_contract.grounding` at admission time. Every schema property
//! must have a grounding entry. Every `sourced` entry must name a tool the
//! agent declares. `why` is mandatory (≥40 chars). Closed status set.
//!
//! This is what makes grounding scale to third-party agents — a publisher
//! declares their own grounding contract in the card, and the platform
//! validates it at publish time. Without this, grounding only works for
//! agents someone has hand-written a compiled entry for.
//!
//! The card-declared contract is a JSON object inside the agent card's
//! `capabilities.output_contract.grounding` field:
//!
//! ```json
//! "output_contract": {
//!   "grounding": {
//!     "deliverable_path": {
//!       "status": "sourced",
//!       "tools": ["zed/edit_file", "zed/write_file"],
//!       "why": "A file path the agent claims to have written."
//!     },
//!     "summary": {
//!       "status": "inferred",
//!       "why": "A prose summary commissioned by the system prompt."
//!     }
//!   }
//! }
//! ```
//!
//! The validation rules:
//! 1. Every `status` must be one of the closed set: `sourced`, `inferred`,
//!    `narrative`, `unavailable`, `derived`.
//! 2. Every `sourced` entry must name at least one tool.
//! 3. Every `why` must be ≥40 chars.
//! 4. `derived` entries must name `from` (the sourced field they derive from).
//! 5. `sourced` entries without `response_path` get a warning (loose match).
//! 6. `derived` entries' `from` must name a key in the same grounding object.

use serde_json::Value;

use crate::grounding::{DerivedSpec, FieldSpec, GroundingContract};

/// Minimum length of a `why`. Short enough not to be tyrannical, long
/// enough that "n/a" and "tool" do not pass.
pub const MIN_WHY: usize = 40;

/// Dispositions an author may declare. Closed set: an open one would let
/// `"status": "estimated"` through, which is the fabrication reappearing as a
/// metadata value.
pub const GROUNDING_STATUSES: &[&str] =
    &["sourced", "inferred", "narrative", "unavailable", "derived"];

/// Finding checks that are warnings, not errors. A contract with only these
/// findings is still usable for enforcement.
pub const WARNING_CHECKS: &[&str] = &["grounding_sourced_response_path"];

/// One violation of the card-declared grounding contract, phrased for the
/// person who has to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Stable machine name for the check.
    pub check: &'static str,
    /// What is wrong and what to do about it.
    pub message: String,
}

impl Finding {
    /// Whether this finding is a warning, not an error. A contract with only
    /// warnings is still usable for enforcement.
    pub fn is_warning(&self) -> bool {
        WARNING_CHECKS.contains(&self.check)
    }
}

fn finding(check: &'static str, message: impl Into<String>) -> Finding {
    Finding {
        check,
        message: message.into(),
    }
}

/// Validate a card's `output_contract.grounding` against the card-declared
/// grounding rules.
///
/// `grounding` is the `output_contract.grounding` JSON object from the card.
/// `tool_names` is what the agent actually declares in `capabilities.mcp_tools`
/// — needed because a `sourced` entry naming a tool the agent doesn't have
/// is a contract that protects nothing.
///
/// Returns every finding rather than the first, so an author fixes a card
/// in one pass instead of playing whack-a-mole with a gate.
pub fn validate(grounding: Option<&Value>, tool_names: &[String]) -> Vec<Finding> {
    let mut out = Vec::new();

    let Some(grounding) = grounding else {
        out.push(finding(
            "grounding_declared",
            "No `output_contract.grounding`. Declare one entry per output \
             field stating where its value comes from: `sourced` (a tool \
             returns it), `inferred` (you reason it out), `narrative` \
             (prose), `unavailable` (nothing can supply it), or `derived` \
             (computed from a sourced field).",
        ));
        return out;
    };

    let Some(entries) = grounding.as_object() else {
        out.push(finding(
            "grounding_declared",
            "`output_contract.grounding` is not an object. It must be a \
             map of field name → { status, why, ... }.",
        ));
        return out;
    };

    if entries.is_empty() {
        out.push(finding(
            "grounding_declared",
            "`output_contract.grounding` is empty. Every output field needs \
             an entry — a field nobody has classified is exactly the kind \
             that gets filled from the model's memory and read as a \
             measurement.",
        ));
        return out;
    }

    for (field, spec) in entries {
        let status = spec.get("status").and_then(|v| v.as_str()).unwrap_or("");

        if !GROUNDING_STATUSES.contains(&status) {
            out.push(finding(
                "grounding_status_valid",
                format!(
                    "`grounding.{field}.status` is `{status}`, which is not \
                     one of {GROUNDING_STATUSES:?}. The set is closed on \
                     purpose: an open one would admit `estimated`, and an \
                     estimate presented in a data field is the problem this \
                     contract exists to stop."
                ),
            ));
            continue;
        }

        let why = spec.get("why").and_then(|v| v.as_str()).unwrap_or("");
        if why.trim().len() < MIN_WHY {
            out.push(finding(
                "grounding_explained",
                format!(
                    "`grounding.{field}.why` is missing or too short (needs \
                     {MIN_WHY}+ characters). Say why this field has the \
                     status it has. The next author cannot tell a considered \
                     `unavailable` from a lazy one, so they will copy \
                     whichever is nearest."
                ),
            ));
        }

        match status {
            "sourced" => {
                let tools = spec.get("tools").and_then(|v| v.as_array());
                match tools {
                    Some(arr) if !arr.is_empty() => {
                        for tool in arr {
                            if let Some(tool_name) = tool.as_str() {
                                if !tool_names.iter().any(|t| t == tool_name) {
                                    out.push(finding(
                                        "grounding_sourced_names_tool",
                                        format!(
                                            "`grounding.{field}` is `sourced` \
                                             naming tool `{tool_name}`, but \
                                             the agent does not declare it in \
                                             `capabilities.mcp_tools`. A \
                                             sourced field naming a tool the \
                                             agent cannot call protects \
                                             nothing."
                                        ),
                                    ));
                                }
                            }
                        }
                        // Warn when `response_path` is absent — the system
                        // silently defaults to the loosest matching mode (whole
                        // result), which admits false positives. Not an error
                        // for backward compat with existing cards.
                        if spec.get("response_path").is_none() {
                            out.push(finding(
                                "grounding_sourced_response_path",
                                format!(
                                    "`grounding.{field}` is `sourced` but has \
                                     no `response_path`. Without it, the \
                                     grounding check matches the field value \
                                     against the entire tool result (the \
                                     loosest mode), which admits false \
                                     positives. Declare `response_path` to \
                                     scope the match to the relevant sub-value."
                                ),
                            ));
                        }
                    }
                    _ => {
                        out.push(finding(
                            "grounding_sourced_names_tool",
                            format!(
                                "`grounding.{field}` is `sourced` but names \
                                 no `tools` array (or it is empty). A \
                                 sourced field must name at least one tool."
                            ),
                        ));
                    }
                }
            }
            "derived" => {
                let from = spec.get("from").and_then(|v| v.as_str()).unwrap_or("");
                if from.is_empty() {
                    out.push(finding(
                        "grounding_derived_names_source",
                        format!(
                            "`grounding.{field}` is `derived` but names no \
                             `from` field. A derivation must state which \
                             sourced field it is computed from."
                        ),
                    ));
                } else if !entries.contains_key(from) {
                    // `from` names a field that doesn't exist in the same
                    // grounding object — a dangling reference that would pass
                    // admission but fail at runtime (source not found).
                    out.push(finding(
                        "grounding_derived_source_exists",
                        format!(
                            "`grounding.{field}` is `derived` from \
                             `{from}`, but there is no `grounding.{from}` \
                             entry. A derivation must reference a field \
                             declared in the same grounding object."
                        ),
                    ));
                }
            }
            _ => {}
        }
    }

    out
}

/// Convert a card's `output_contract.grounding` JSON into an enforceable
/// `GroundingContract`. Returns `None` when the grounding is absent, not an
/// object, or empty. Call `validate` first to catch structural defects —
/// this function does not re-validate, it converts.
///
/// The card-declared contract takes precedence over the compiled contract
/// for the same `agent_type` (e.g. `task_agent_contract()`). This is the
/// intent: the card declaration is specific to the agent's actual output
/// fields, while the compiled contract is a generic default. If multiple
/// agents share the same `agent_type` but declare different grounding, the
/// last registration wins — a known limitation of `agent_type`-keyed
/// contracts.
///
/// Status mapping:
/// - `sourced` → `sources` from `tools`, `response_path` from card or `""`
/// - `inferred` / `narrative` / `unavailable` → empty `sources` (Inferred)
/// - `derived` → `derived_from` from card's `from` field
pub fn from_card_grounding(
    grounding: Option<&Value>,
    agent_type: &str,
) -> Option<GroundingContract> {
    let grounding = grounding?;
    let entries = grounding.as_object()?;
    if entries.is_empty() {
        return None;
    }

    let mut field_sources = HashMap::new();
    for (field, spec) in entries {
        let status = spec.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let why = spec
            .get("why")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let response_path = spec
            .get("response_path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let transform = spec
            .get("transform")
            .and_then(|v| v.as_str())
            .map(String::from);

        let field_spec = match status {
            "sourced" => {
                let tools = spec
                    .get("tools")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                FieldSpec {
                    sources: tools,
                    response_path,
                    why,
                    derived_from: None,
                    transform,
                }
            }
            "derived" => {
                let from = spec
                    .get("from")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                FieldSpec {
                    sources: vec![],
                    response_path: String::new(),
                    why,
                    derived_from: Some(DerivedSpec {
                        from,
                        how: "card_declared".to_string(),
                    }),
                    transform: None,
                }
            }
            // "inferred", "narrative", "unavailable" — commissioned
            // judgment or prose, no tool sourcing.
            _ => FieldSpec {
                sources: vec![],
                response_path: String::new(),
                why,
                derived_from: None,
                transform: None,
            },
        };
        field_sources.insert(field.clone(), field_spec);
    }

    Some(GroundingContract {
        agent_type: agent_type.to_string(),
        field_sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tools(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn missing_grounding_is_reported() {
        let findings = validate(None, &[]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "grounding_declared");
    }

    #[test]
    fn non_object_grounding_is_reported() {
        let findings = validate(Some(&json!("not an object")), &[]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "grounding_declared");
    }

    #[test]
    fn empty_grounding_is_reported() {
        let findings = validate(Some(&json!({})), &[]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "grounding_declared");
    }

    #[test]
    fn valid_sourced_entry_passes() {
        let grounding = json!({
            "deliverable_path": {
                "status": "sourced",
                "tools": ["zed/write_file"],
                "response_path": "path",
                "why": "A file path the agent claims to have written. Must be sourced from a file-writing tool."
            }
        });
        let findings = validate(Some(&grounding), &tools(&["zed/write_file"]));
        assert!(findings.is_empty(), "valid entry should pass: {findings:?}");
    }

    #[test]
    fn valid_inferred_entry_passes() {
        let grounding = json!({
            "summary": {
                "status": "inferred",
                "why": "A prose summary of what the agent did. Commissioned by the system prompt."
            }
        });
        let findings = validate(Some(&grounding), &[]);
        assert!(
            findings.is_empty(),
            "valid inferred should pass: {findings:?}"
        );
    }

    #[test]
    fn valid_derived_entry_passes() {
        let grounding = json!({
            "deliverable_path": {
                "status": "sourced",
                "tools": ["zed/write_file"],
                "response_path": "path",
                "why": "A file path the agent claims to have written. Must be sourced from a file-writing tool."
            },
            "file_extension": {
                "status": "derived",
                "from": "deliverable_path",
                "how": "extension extraction",
                "why": "Computed from deliverable_path by platform code. Reproducible and auditable."
            }
        });
        let findings = validate(Some(&grounding), &tools(&["zed/write_file"]));
        assert!(
            findings.is_empty(),
            "valid derived should pass: {findings:?}"
        );
    }

    #[test]
    fn invalid_status_is_reported() {
        let grounding = json!({
            "field": {
                "status": "estimated",
                "why": "This is long enough to pass the why check but the status is invalid."
            }
        });
        let findings = validate(Some(&grounding), &[]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "grounding_status_valid");
    }

    #[test]
    fn short_why_is_reported() {
        let grounding = json!({
            "field": {
                "status": "inferred",
                "why": "too short"
            }
        });
        let findings = validate(Some(&grounding), &[]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "grounding_explained");
    }

    #[test]
    fn sourced_without_tools_is_reported() {
        let grounding = json!({
            "field": {
                "status": "sourced",
                "why": "A field that should be sourced from a tool but no tools are named here."
            }
        });
        let findings = validate(Some(&grounding), &[]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "grounding_sourced_names_tool");
    }

    #[test]
    fn sourced_with_empty_tools_is_reported() {
        let grounding = json!({
            "field": {
                "status": "sourced",
                "tools": [],
                "why": "A field that should be sourced from a tool but the tools array is empty."
            }
        });
        let findings = validate(Some(&grounding), &[]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "grounding_sourced_names_tool");
    }

    #[test]
    fn sourced_naming_undeclared_tool_is_reported() {
        let grounding = json!({
            "field": {
                "status": "sourced",
                "tools": ["zed/write_file"],
                "response_path": "path",
                "why": "A field sourced from a tool the agent does not declare in mcp_tools."
            }
        });
        // Agent declares only zed/terminal, not zed/write_file.
        let findings = validate(Some(&grounding), &tools(&["zed/terminal"]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "grounding_sourced_names_tool");
    }

    #[test]
    fn derived_without_from_is_reported() {
        let grounding = json!({
            "field": {
                "status": "derived",
                "why": "A derived field that does not name its source field in the from key."
            }
        });
        let findings = validate(Some(&grounding), &[]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "grounding_derived_names_source");
    }

    #[test]
    fn multiple_findings_returned_at_once() {
        let grounding = json!({
            "bad_status": {
                "status": "estimated",
                "why": "This is long enough but the status is invalid."
            },
            "short_why": {
                "status": "inferred",
                "why": "short"
            },
            "no_tools": {
                "status": "sourced",
                "why": "Sourced but no tools array is provided here at all."
            }
        });
        let findings = validate(Some(&grounding), &[]);
        assert_eq!(
            findings.len(),
            3,
            "all three findings should be returned at once, not just the first"
        );
    }

    #[test]
    fn grounding_statuses_are_closed() {
        // The set must not contain "estimated" or other fabrication-adjacent
        // values. An open set would admit the problem this contract exists
        // to stop.
        assert!(!GROUNDING_STATUSES.contains(&"estimated"));
        assert!(!GROUNDING_STATUSES.contains(&"guess"));
        assert!(!GROUNDING_STATUSES.contains(&"approximate"));
    }

    #[test]
    fn sourced_without_response_path_is_warned() {
        // A sourced entry without response_path silently defaults to the
        // loosest matching mode — warn so the author knows to scope it.
        let grounding = json!({
            "deliverable_path": {
                "status": "sourced",
                "tools": ["zed/write_file"],
                "why": "A file path the agent claims to have written. Must be sourced from a file-writing tool."
            }
        });
        let findings = validate(Some(&grounding), &tools(&["zed/write_file"]));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "grounding_sourced_response_path");
    }

    #[test]
    fn sourced_with_response_path_passes() {
        // A sourced entry with response_path is the happy path — no warning.
        let grounding = json!({
            "deliverable_path": {
                "status": "sourced",
                "tools": ["zed/write_file"],
                "response_path": "path",
                "why": "A file path the agent claims to have written. Must be sourced from a file-writing tool."
            }
        });
        let findings = validate(Some(&grounding), &tools(&["zed/write_file"]));
        assert!(
            findings.is_empty(),
            "sourced with response_path should pass: {findings:?}"
        );
    }

    #[test]
    fn derived_with_dangling_from_is_reported() {
        // A derived entry whose `from` names a non-existent field is a
        // dangling reference — it would pass admission but fail at runtime.
        let grounding = json!({
            "file_extension": {
                "status": "derived",
                "from": "nonexistent_field",
                "how": "extension extraction",
                "why": "Computed from deliverable_path by platform code. Reproducible and auditable."
            }
        });
        let findings = validate(Some(&grounding), &[]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, "grounding_derived_source_exists");
    }

    #[test]
    fn derived_with_existing_from_passes() {
        // A derived entry whose `from` names a sibling in the same grounding
        // object is the happy path — no finding.
        let grounding = json!({
            "deliverable_path": {
                "status": "sourced",
                "tools": ["zed/write_file"],
                "response_path": "path",
                "why": "A file path the agent claims to have written. Must be sourced from a file-writing tool."
            },
            "file_extension": {
                "status": "derived",
                "from": "deliverable_path",
                "how": "extension extraction",
                "why": "Computed from deliverable_path by platform code. Reproducible and auditable."
            }
        });
        let findings = validate(Some(&grounding), &tools(&["zed/write_file"]));
        assert!(
            findings.is_empty(),
            "derived with existing from should pass: {findings:?}"
        );
    }
}
