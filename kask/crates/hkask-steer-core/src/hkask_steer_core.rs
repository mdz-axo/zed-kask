//! hkask-steer-core — the zed-free half of the Steer prompt surface.
//!
//! Steer-mode panels advertise their MCP server's tools inside the system
//! prompt, and the advertisement must stay truthful against the server's
//! build.rs-generated `TOOL_NAMES`: a name the server does not expose fails
//! at dispatch ("tool not found"), and a new tool that goes unadvertised is
//! invisible to the model. Rendering and verification of that contract live
//! here with no editor dependencies (std + `log` only), so the
//! load-bearing prompt-truth logic builds and tests without the zed
//! closure.
//!
//! Split from `crates/hkask-steer` (2026-09-07) under the operator ruling
//! that only `kask_bridge` may depend on zed crates. `hkask-steer` keeps
//! the `ConversationView` lifecycle and re-exports everything here, so
//! panel call sites (`hkask_steer::render_tool_names` etc.) are unchanged.

/// Extract the backticked tool names carrying one of `prefixes` from a
/// Steer prompt. Backtick spans are the prompt's tool-advertisement
/// convention; a name must start with a prefix and have a non-empty suffix.
pub fn advertised_tool_names(prompt: &str, prefixes: &[&str]) -> Vec<String> {
    prompt
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|span| {
            span.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .filter(|name| {
            prefixes
                .iter()
                .any(|prefix| name.starts_with(prefix) && name.len() > prefix.len())
        })
        .collect()
}

/// Join tool names as backticked, comma-separated prose: "`a`, `b`, `c`".
/// This is the advertisement convention `advertised_tool_names` parses —
/// render every tool list with it so rendering and verification share one
/// format.
pub fn render_tool_names(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Render a complete, grouped tool advertisement from the server's canonical
/// tool names. Each group is `(label, prefixes)`; a tool lands in the first
/// group whose prefix it starts with, and tools matching no group land in a
/// trailing "Other tools" group. Empty groups are omitted.
///
/// The section always advertises exactly the server's surface — no more (a
/// removed tool cannot linger, because nothing is written by hand) and no
/// fewer (a new tool lands in its prefix group or "Other tools" either way).
/// Panels pass their server's build.rs-generated `TOOL_NAMES`; the labels and
/// prefixes are the panel's naming convention, never tool names.
pub fn render_grouped_tool_advertisement(
    server_tools: &[&str],
    groups: &[(&str, &[&str])],
) -> String {
    let mut unclaimed: Vec<usize> = (0..server_tools.len()).collect();
    let mut section = String::new();
    for (label, prefixes) in groups {
        let mut names: Vec<&str> = Vec::new();
        unclaimed.retain(|&index| {
            if prefixes
                .iter()
                .any(|prefix| server_tools[index].starts_with(prefix))
            {
                names.push(server_tools[index]);
                false
            } else {
                true
            }
        });
        if !names.is_empty() {
            section.push_str(&format!("**{label}**: {}.\n", render_tool_names(&names)));
        }
    }
    if !unclaimed.is_empty() {
        let names: Vec<&str> = unclaimed.iter().map(|&index| server_tools[index]).collect();
        section.push_str(&format!(
            "**Other tools**: {}.\n",
            render_tool_names(&names)
        ));
    }
    section
}

/// Verify that every tool name a Steer prompt advertises exists in the
/// server's canonical `TOOL_NAMES`. Advertising a tool the server does not
/// expose is worse than omitting it: the model calls a name that cannot
/// resolve and the turn fails at dispatch. Logs a warning always and
/// `debug_assert!`s in dev builds; panels should also pin this with a test.
/// The generated advertisement (`render_grouped_tool_advertisement`) cannot
/// drift by construction — this catches the residual risk of prose that
/// names tools by hand.
pub fn verify_tool_advertisement(prompt: &str, server_tools: &[&str], prefixes: &[&str]) {
    let unknown: Vec<String> = advertised_tool_names(prompt, prefixes)
        .into_iter()
        .filter(|name| !server_tools.contains(&name.as_str()))
        .collect();
    for name in &unknown {
        log::warn!(
            "hkask-steer: steer prompt advertises `{name}`, which is not in the \
             MCP server's TOOL_NAMES — the model will hit \"tool not found\" at dispatch"
        );
    }
    debug_assert!(
        unknown.is_empty(),
        "steer prompt advertises tools not exposed by the server: {unknown:?}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_names_extracts_backticked_prefixed_tools() {
        let prompt = "Use `kanban_task_move` and `kanban_task_list`. \
                      The `kata-kanban` server id is not a tool (no prefix match on \
                      the suffix rule applies to `kanban_` alone). \
                      Also `contract_propose_expect` and plain text.";
        let names = advertised_tool_names(prompt, &["kanban_", "contract_"]);
        assert_eq!(
            names,
            vec![
                "kanban_task_move".to_string(),
                "kanban_task_list".to_string(),
                "contract_propose_expect".to_string(),
            ]
        );
    }

    #[test]
    fn advertised_names_rejects_bare_prefix() {
        let names = advertised_tool_names("`kanban_` `kanban_task_create`", &["kanban_"]);
        assert_eq!(names, vec!["kanban_task_create".to_string()]);
    }

    #[test]
    fn verify_accepts_known_tools() {
        verify_tool_advertisement(
            "Use `kanban_task_move`.",
            &["kanban_task_move", "kanban_task_list"],
            &["kanban_"],
        );
    }

    #[test]
    fn verify_warns_on_unknown_tool() {
        // Exercises the log::warn branch; the debug_assert is compiled out
        // under `--release` but this test runs in dev, so we only call it
        // through a catch to keep the test itself from asserting.
        let result = std::panic::catch_unwind(|| {
            verify_tool_advertisement("Use `kanban_nope`.", &["kanban_task_move"], &["kanban_"]);
        });
        assert!(result.is_err(), "debug_assert must fire on unknown tool");
    }

    #[test]
    fn render_tool_names_joins_backticked() {
        assert_eq!(render_tool_names(&[]), "");
        assert_eq!(
            render_tool_names(&["kanban_task_move", "kanban_task_list"]),
            "`kanban_task_move`, `kanban_task_list`"
        );
    }

    #[test]
    fn grouped_advertisement_buckets_by_first_matching_prefix() {
        let section = render_grouped_tool_advertisement(
            &[
                "kanban_task_kata_prompt",
                "kanban_task_create",
                "contract_propose_expect",
            ],
            &[
                ("Kata coaching", &["kanban_task_kata_"]),
                ("Task tools", &["kanban_task_"]),
                ("Contract grounding", &["contract_"]),
            ],
        );
        assert!(section.contains("**Kata coaching**: `kanban_task_kata_prompt`.\n"));
        assert!(section.contains("**Task tools**: `kanban_task_create`.\n"));
        assert!(section.contains("**Contract grounding**: `contract_propose_expect`.\n"));
    }

    #[test]
    fn grouped_advertisement_omits_empty_groups_and_catches_the_rest() {
        let section = render_grouped_tool_advertisement(
            &["gallery_organize", "model_list"],
            &[("Gallery tools", &["gallery_"]), ("Face tools", &["face_"])],
        );
        assert!(section.contains("**Gallery tools**: `gallery_organize`.\n"));
        assert!(
            !section.contains("Face tools"),
            "empty group must be omitted"
        );
        // A tool matching no group is still advertised — completeness by
        // construction, so a new server tool can never be silently dropped.
        assert!(section.contains("**Other tools**: `model_list`.\n"));
    }

    #[test]
    fn grouped_advertisement_covers_every_tool_exactly_once() {
        let tools = [
            "gallery_organize",
            "gallery_search",
            "face_register",
            "model_list",
            "job_submit",
        ];
        let section = render_grouped_tool_advertisement(
            &tools,
            &[
                ("Gallery tools", &["gallery_"]),
                ("Face tools", &["face_"]),
                ("Model tools", &["model_"]),
            ],
        );
        for tool in tools {
            let occurrences = section.matches(&format!("`{tool}`")).count();
            assert_eq!(
                occurrences, 1,
                "`{tool}` must appear exactly once in the advertisement"
            );
        }
        // Round-trip: the verifier extracts exactly the advertised set from
        // the rendered section — renderer and verifier share one format.
        let advertised = advertised_tool_names(&section, &["gallery_", "face_", "model_", "job_"]);
        let mut advertised = advertised;
        advertised.sort();
        let mut expected = tools.to_vec();
        expected.sort();
        assert_eq!(advertised, expected);
    }
}
