//! REPL /status handler — system status display (Regulation, agent, gas, loops)

pub fn handle_status(
    state: &mut super::super::ReplState,
    template_id: Option<&str>,
    rt: &tokio::runtime::Handle,
) {
    let agent_display = state.current_agent.clone();
    let tpl = template_id.unwrap_or("auto-select");
    let gas_remaining = state.service_context.gas_remaining().unwrap_or(0);
    let gas_cap = state.service_context.gas_cap().unwrap_or(0);
    let gas_pct = if gas_cap > 0 {
        (gas_remaining as f64 / gas_cap as f64) * 100.0
    } else {
        0.0
    };
    let gas_bar = if gas_pct > 60.0 {
        "\x1b[32m■\x1b[0m" // green
    } else if gas_pct > 20.0 {
        "\x1b[33m■\x1b[0m" // yellow
    } else {
        "\x1b[31m■\x1b[0m" // red
    };
    println!("  Agent:      \x1b[1m{}\x1b[0m", agent_display);
    println!("  Model:      \x1b[1m{}\x1b[0m", state.current_model);
    println!("  Template:   {}", tpl);
    println!(
        "  Gas:        {} {}/{} ({:.0}%)",
        gas_bar, gas_remaining, gas_cap, gas_pct
    );
    // Check Regulation health
    let ledger_runtime = state.service_context.ledger().runtime.clone();
    let reg_health = rt.block_on(ledger_runtime.read());
    let reg_status = match rt.block_on(async { reg_health.health().await }) {
        health if health.critical_count > 0 => {
            format!(
                "\x1b[31m\u{26a0} CRITICAL\x1b[0m ({} critical, {} warnings)",
                health.critical_count, health.warning_count
            )
        }
        health if health.warning_count > 0 => {
            format!(
                "\x1b[33m\u{26a0} WARNING\x1b[0m ({} warnings)",
                health.warning_count
            )
        }
        _ => "\x1b[32mHEALTHY\x1b[0m (no alerts)".to_string(),
    };
    println!("  Regulation:        {}", reg_status);

    // ── Seam Watcher status ──
    let seam_summary = rt.block_on(state.service_context.seam_summary());
    match seam_summary {
        Some(summary) => {
            let coverage_bar = if summary.coverage_pct >= 60.0 {
                "\x1b[32m■\x1b[0m" // green
            } else if summary.coverage_pct >= 30.0 {
                "\x1b[33m■\x1b[0m" // yellow
            } else {
                "\x1b[31m■\x1b[0m" // red
            };
            println!(
                "  Seam:     {} watching {} crates | {}/{} covered ({:.0}%) | {} REQ tests",
                coverage_bar,
                summary.crate_count,
                summary.covered_items,
                summary.total_items,
                summary.coverage_pct,
                summary.req_tests,
            );
        }
        None => {
            println!("  Seam:     \x1b[2mdisabled\x1b[0m (no inventory available)");
        }
    }
    // Show LoopScheduler registered loops
    let loops = state.service_context.ledger().loops.clone();
    let loop_count = rt.block_on(loops.registered_count());
    let loop_ids = rt.block_on(loops.registered_loop_ids());
    let ids_str = loop_ids
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    println!("  Loops:      {} registered ({})", loop_count, ids_str);
    match &state.active_session {
        Some(session) => {
            println!(
                "  Session:    \x1b[33m{}\x1b[0m (dual-presence with Curator daemon)",
                session
            );
        }
        None => {
            println!("  Mode:       single-agent (dual-presence with Curator daemon)");
        }
    }
    println!();
}
