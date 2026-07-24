//! Sovereignty command handler — Magna Carta structural verification.
//!
//! Live consent grants/revokes/status/checks are runtime operations available
//! from the TUI REPL (`/sovereignty`) or the HTTP API. The CLI exposes only
//! the structural audit, which operates on the codebase, not the live system.

use crate::cli::SovereigntyAction;

/// Run the sovereignty CLI command (admin: structural verification only).
pub fn run(action: SovereigntyAction) {
    match action {
        SovereigntyAction::Verify { principle, json } => run_verify(principle, json),
    }
}

// ── Magna Carta verification ────────────────────────────────────────────

fn run_verify(principle: Option<String>, json: bool) {
    if json {
        let result = crate::verification::VerificationService::verify_json(principle.as_deref());
        println!(
            "{}",
            serde_json::to_string_pretty(&result)
                .unwrap_or_else(|e| { serde_json::json!({"error": e.to_string()}).to_string() })
        );
        return;
    }
    let report = crate::verification::VerificationService::verify(principle.as_deref());
    if report.principles.is_empty() {
        eprintln!(
            "No Magna Carta manifests found. Expected manifests in .agents/skills/magna-carta-verifier/manifests/"
        );
        std::process::exit(1);
    }

    println!("Magna Carta Verification Report");
    println!("==============================");
    println!();

    for pr in &report.principles {
        let mut principle_pass = 0usize;
        let mut principle_fail = 0usize;
        let mut principle_gap = 0usize;

        println!("## {}", pr.display_name);
        println!();

        for result in &pr.assertion_results {
            let status_icon = match result.status.as_str() {
                "pass" => "✓",
                "fail" => "✗",
                "gap" => "△",
                "skip" => "—",
                _ => "?",
            };
            println!(
                "  {status_icon} {} {}: {}",
                result.id, result.name, result.status
            );
            for finding in &result.findings {
                println!("    → {finding}");
            }
            for rec in &result.recommendations {
                println!("    ⚑ {rec}");
            }
            match result.status.as_str() {
                "pass" => principle_pass += 1,
                "fail" => principle_fail += 1,
                "gap" => principle_gap += 1,
                _ => {}
            }
        }

        println!();
        println!(
            "  Principle summary: {principle_pass} pass, {principle_fail} fail, {principle_gap} gap"
        );
        println!();
    }

    println!("---");
    println!(
        "Total: {} assertions — {} pass, {} fail, {} gap, {} skip",
        report.total_assertions,
        report.total_pass,
        report.total_fail,
        report.total_gap,
        report.total_skip
    );

    if report.total_fail > 0 || report.total_gap > 0 {
        println!();
        println!(
            "⚠ {} assertion(s) failed and {} have gaps.",
            report.total_fail, report.total_gap
        );
        println!("  Escalate to Curator for review with human user or userpod.");
    }
}
