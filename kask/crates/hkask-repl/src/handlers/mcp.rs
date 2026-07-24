//! REPL MCP handler — /mcp slash command
//!
//! P2 (Affirmative Consent): No server starts without explicit user action.
//! Servers are listed with numbered status; user opts in by name, number pattern,
//! or "all".

use super::super::builtin_servers;

/// Handle /mcp — manage MCP server connections with affirmative consent.
///
/// Subcommands:
///   /mcp list                         — show all available servers with numbered status
///   /mcp start `name`                 — start a server by name (e.g. condenser)
///   /mcp start `n`                    — start a server by number from /mcp list
///   /mcp start `n,m,a-b`              — start by commas, ranges, and comparisons
///   /mcp start `<=6,>8`               — start first 6 and everything after 8
///   /mcp start all                   — start all available servers
///
/// Comparison operators: <4, <=3, >=7, >8
/// Example: /mcp start 1,<=6,>8  starts indices 1-6 plus 9,10.
pub fn handle_mcp(
    state: &mut super::super::ReplState,
    arg1: &str,
    arg2: &str,
    rt: &tokio::runtime::Handle,
) {
    let mcp_runtime = state.service_context.infra().mcp.clone().clone();
    let servers: Vec<(&str, &str)> = builtin_servers::BUILTIN_SERVERS.to_vec();

    match arg1 {
        "list" | "" => print_server_list(&mcp_runtime, &servers, rt),
        "start" => handle_start(arg2, &mcp_runtime, &servers, state, rt),
        _ => {
            println!("  Unknown /mcp subcommand: '{}'", arg1);
            println!(
                "  Usage: \x1b[36m/mcp list\x1b[0m | \x1b[36m/mcp start <pattern>\x1b[0m | \x1b[36m/mcp start all\x1b[0m"
            );
            println!();
        }
    }
}

/// Print the server list with 1-based indices and connection status.
fn print_server_list(
    mcp_runtime: &hkask_mcp::runtime::McpRuntime,
    servers: &[(&str, &str)],
    rt: &tokio::runtime::Handle,
) {
    let connected = rt.block_on(async {
        let s = mcp_runtime.list_servers().await;
        s.into_iter()
            .map(|s| s.id)
            .collect::<std::collections::HashSet<_>>()
    });

    println!();
    println!(
        "  \x1b[1mMCP Servers\x1b[0m — \x1b[36m/mcp start <name|pattern>\x1b[0m (e.g. \x1b[36m1,<=6,>8\x1b[0m, \x1b[36mall\x1b[0m)"
    );
    println!();

    let max_idx_width = servers.len().to_string().len();

    for (i, (server_id, binary)) in servers.iter().enumerate() {
        let num = i + 1;
        let status = if connected.contains(*server_id) {
            "\x1b[32m● connected\x1b[0m"
        } else {
            "\x1b[2m○ idle\x1b[0m"
        };
        println!(
            "    \x1b[2m{:>width$}\x1b[0m  {}  \x1b[36m{}\x1b[0m  (\x1b[2m{}\x1b[0m)",
            num,
            status,
            server_id,
            binary,
            width = max_idx_width,
        );
    }

    if connected.is_empty() {
        println!();
        println!("  \x1b[2mNo servers loaded. The userpod will run with inference only.\x1b[0m");
        println!(
            "  \x1b[2mType \x1b[36m/mcp start all\x1b[0m\x1b[2m to load everything, or pick by number.\x1b[0m"
        );
    } else {
        let tool_count: usize = rt.block_on(async { mcp_runtime.discover_tools().await.len() });
        println!();
        println!(
            "  \x1b[1m{} servers connected, {} tools available\x1b[0m",
            connected.len(),
            tool_count
        );
    }
    println!();
}

/// Handle /mcp start — parse arg2 as "all", a single name, or a selection pattern.
fn handle_start(
    arg2: &str,
    mcp_runtime: &hkask_mcp::runtime::McpRuntime,
    servers: &[(&str, &str)],
    state: &mut super::super::ReplState,
    rt: &tokio::runtime::Handle,
) {
    if arg2.is_empty() {
        println!(
            "  Usage: \x1b[36m/mcp start <name|pattern>\x1b[0m or \x1b[36m/mcp start all\x1b[0m"
        );
        println!(
            "  Patterns: \x1b[36m1,4-6,9\x1b[0m  \x1b[36m<=3,>8\x1b[0m  \x1b[36m>=5\x1b[0m  \x1b[36m<4\x1b[0m"
        );
        println!("  Use \x1b[36m/mcp list\x1b[0m to see available servers.");
        println!();
        return;
    }

    if arg2 == "all" {
        println!();
        let count = rt.block_on(builtin_servers::start_builtin_servers(mcp_runtime));
        if count > 0 {
            tracing::info!(target: "hkask.repl", servers = count, "MCP servers started via /mcp start all");
            println!("  \x1b[32mLoaded {} MCP servers\x1b[0m", count);
        } else {
            println!(
                "  \x1b[31mNo servers could be started.\x1b[0m Check that MCP binaries are on PATH."
            );
        }
        refresh_tool_section(state, rt);
        println!();
        return;
    }

    // Try as a selection pattern first (numbers, ranges, comparisons).
    // Fall back to exact name match.
    let selection = parse_selection(arg2, servers.len());
    if selection.is_empty() {
        match servers.iter().find(|(id, _)| *id == arg2) {
            Some((id, _)) => start_one(id, mcp_runtime, state, rt),
            None => {
                println!();
                println!(
                    "  \x1b[31mUnknown server '{}'.\\x1b[0m Use \x1b[36m/mcp list\x1b[0m to see available servers.",
                    arg2
                );
                println!();
            }
        }
    } else {
        start_selection(&selection, servers, mcp_runtime, state, rt);
    }
}

/// Parse a selection pattern into a sorted, deduplicated list of 1-based indices.
///
/// Supported formats:
///   "1"           — single number
///   "1,4,9"       — comma-separated list
///   "4-6"         — inclusive range
///   "<=3"         — everything up to and including 3
///   ">=7"         — from 7 onward
///   "<4"          — strictly less than 4 (i.e. 1,2,3)
///   ">8"          — strictly greater than 8 (i.e. 9,10,..)
///   "1,<=6,>8"    — mixed: indices 1-6 plus 9,10
///
/// Returns empty vec if input doesn't look like a number pattern (name match fallback).
fn parse_selection(input: &str, max: usize) -> Vec<usize> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // Comparison operators: <=, >=, <, > are valid alongside digits/commas/hyphens
    let valid = trimmed.chars().all(|c| {
        c.is_ascii_digit() || c == ',' || c == '-' || c == ' ' || c == '<' || c == '>' || c == '='
    });
    if !valid || !trimmed.chars().any(|c| c.is_ascii_digit()) {
        return Vec::new();
    }

    let mut indices = Vec::new();

    for part in trimmed.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some(idx) = parse_single_part(part, max) {
            match idx {
                PartResult::Single(n) => indices.push(n),
                PartResult::Range(lo, hi) => indices.extend(lo..=hi),
            }
        } else {
            return Vec::new(); // invalid part → reject whole pattern
        }
    }

    indices.sort_unstable();
    indices.dedup();
    indices
}

enum PartResult {
    Single(usize),
    Range(usize, usize),
}

fn parse_single_part(part: &str, max: usize) -> Option<PartResult> {
    // Try comparison operators first
    if let Some(rest) = part.strip_prefix("<=") {
        let n: usize = rest.trim().parse().ok()?;
        if n < 1 {
            return None;
        }
        let hi = n.min(max);
        return Some(PartResult::Range(1, hi));
    }
    if let Some(rest) = part.strip_prefix(">=") {
        let n: usize = rest.trim().parse().ok()?;
        if n > max {
            return None;
        }
        return Some(PartResult::Range(n, max));
    }
    if let Some(rest) = part.strip_prefix('<') {
        let n: usize = rest.trim().parse().ok()?;
        if n <= 1 {
            return None;
        }
        let hi = (n - 1).min(max);
        return Some(PartResult::Range(1, hi));
    }
    if let Some(rest) = part.strip_prefix('>') {
        let n: usize = rest.trim().parse().ok()?;
        if n >= max {
            return None;
        }
        return Some(PartResult::Range(n + 1, max));
    }

    // Try range: a-b
    if let Some((start_str, end_str)) = part.split_once('-') {
        let start: usize = start_str.trim().parse().ok()?;
        let end: usize = end_str.trim().parse().ok()?;
        if start < 1 || start > max || end < 1 || end > max {
            return None;
        }
        let lo = start.min(end);
        let hi = start.max(end);
        return Some(PartResult::Range(lo, hi));
    }

    // Try single number
    let n: usize = part.parse().ok()?;
    if n < 1 || n > max {
        return None;
    }
    Some(PartResult::Single(n))
}

/// Start a single server by name.
fn start_one(
    server_id: &str,
    mcp_runtime: &hkask_mcp::runtime::McpRuntime,
    state: &mut super::super::ReplState,
    rt: &tokio::runtime::Handle,
) {
    println!();
    let ok = rt.block_on(builtin_servers::start_single_server(mcp_runtime, server_id));
    if ok {
        println!("  \x1b[32mLoaded MCP server: {}\x1b[0m", server_id);
        refresh_tool_section(state, rt);
    } else {
        println!(
            "  \x1b[31mFailed to start '{}'.\x1b[0m Check binary is on PATH.",
            server_id
        );
    }
    println!();
}

/// Start multiple servers from a selection of 1-based indices.
fn start_selection(
    indices: &[usize],
    servers: &[(&str, &str)],
    mcp_runtime: &hkask_mcp::runtime::McpRuntime,
    state: &mut super::super::ReplState,
    rt: &tokio::runtime::Handle,
) {
    println!();
    let mut started = 0;
    let mut failed = 0;

    for &idx in indices {
        let (server_id, _binary) = servers[idx - 1];
        let ok = rt.block_on(builtin_servers::start_single_server(mcp_runtime, server_id));
        if ok {
            started += 1;
        } else {
            failed += 1;
            println!("  \x1b[31m✗ {}\x1b[0m — failed to start", server_id);
        }
    }

    if started > 0 {
        let names: Vec<&str> = indices.iter().map(|&idx| servers[idx - 1].0).collect();
        println!(
            "  \x1b[32mStarted {} server{}: {}\x1b[0m",
            started,
            if started == 1 { "" } else { "s" },
            names.join(", ")
        );
        refresh_tool_section(state, rt);
    }
    if failed > 0 {
        println!(
            "  \x1b[33m{} server{} failed to start.\x1b[0m",
            failed,
            if failed == 1 { "" } else { "s" }
        );
    }
    println!();
}

/// Refresh tool definitions after starting servers so the LLM
/// becomes aware of newly available tools.
fn refresh_tool_section(state: &mut super::super::ReplState, rt: &tokio::runtime::Handle) {
    let mcp = state.service_context.infra().mcp.clone();
    let tool_names = rt.block_on(mcp.discover_tools());
    let mut tools: Vec<hkask_capability::ToolInfo> = Vec::new();
    for name in &tool_names {
        if let Some(info) = rt.block_on(mcp.get_tool_info(name)) {
            tools.push(info);
        }
    }
    state.tool_definitions = tools
        .iter()
        .map(|tool| hkask_types::ChatToolDefinition {
            tool_type: "function".to_string(),
            function: hkask_types::ChatToolFunction {
                name: format!("{}/{}", tool.server_id, tool.name),
                description: tool.description.clone(),
                parameters: tool.input_schema.clone(),
            },
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_number() {
        assert_eq!(parse_selection("1", 10), vec![1]);
        assert_eq!(parse_selection("  5  ", 10), vec![5]);
    }

    #[test]
    fn parse_comma_list() {
        assert_eq!(parse_selection("1,4,6,9", 10), vec![1, 4, 6, 9]);
        assert_eq!(parse_selection("3,1,2", 10), vec![1, 2, 3]);
    }

    #[test]
    fn parse_range() {
        assert_eq!(parse_selection("4-6", 10), vec![4, 5, 6]);
        assert_eq!(parse_selection("6-4", 10), vec![4, 5, 6]);
    }

    #[test]
    fn parse_mixed() {
        assert_eq!(parse_selection("1,4-6,9", 10), vec![1, 4, 5, 6, 9]);
    }

    #[test]
    fn parse_with_spaces() {
        assert_eq!(parse_selection("1, 4 - 6, 9", 10), vec![1, 4, 5, 6, 9]);
    }

    #[test]
    fn parse_deduplicates() {
        assert_eq!(parse_selection("1,1,1,2-4,3", 10), vec![1, 2, 3, 4]);
    }

    #[test]
    fn parse_lte_comparison() {
        assert_eq!(parse_selection("<=3", 10), vec![1, 2, 3]);
        assert_eq!(parse_selection("<=1", 10), vec![1]);
    }

    #[test]
    fn parse_gte_comparison() {
        assert_eq!(parse_selection(">=8", 10), vec![8, 9, 10]);
        assert_eq!(parse_selection(">=10", 10), vec![10]);
    }

    #[test]
    fn parse_lt_comparison() {
        assert_eq!(parse_selection("<4", 10), vec![1, 2, 3]);
    }

    #[test]
    fn parse_gt_comparison() {
        assert_eq!(parse_selection(">8", 10), vec![9, 10]);
    }

    #[test]
    fn parse_comparison_at_boundary() {
        assert!(parse_selection(">10", 10).is_empty());
        assert!(parse_selection("<1", 10).is_empty());
        assert!(parse_selection(">=11", 10).is_empty());
        assert!(parse_selection("<=0", 10).is_empty());
    }

    #[test]
    fn parse_mixed_with_comparisons() {
        assert_eq!(
            parse_selection("1,<=6,>8", 10),
            vec![1, 2, 3, 4, 5, 6, 9, 10]
        );
    }

    #[test]
    fn parse_complex_mixed() {
        assert_eq!(
            parse_selection("<=3,5,7-9,>9", 10),
            vec![1, 2, 3, 5, 7, 8, 9, 10]
        );
    }

    #[test]
    fn parse_out_of_range_rejected() {
        assert!(parse_selection("0", 10).is_empty());
        assert!(parse_selection("11", 10).is_empty());
        assert!(parse_selection("5-11", 10).is_empty());
    }

    #[test]
    fn parse_name_string_not_treated_as_selection() {
        assert!(parse_selection("condenser", 10).is_empty());
        assert!(parse_selection("memory", 10).is_empty());
    }

    #[test]
    fn parse_empty_or_non_numeric() {
        assert!(parse_selection("", 10).is_empty());
        assert!(parse_selection("abc", 10).is_empty());
    }
}
