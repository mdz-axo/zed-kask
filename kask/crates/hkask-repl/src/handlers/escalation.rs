//! REPL escalation handlers — /escalations, /resolve, /dismiss
//!
//! These use the existing service context from ReplState rather than
//! constructing a fresh AgentService (which would spawn an OS thread).

pub fn handle_escalations(state: &super::super::ReplState) {
    match state
        .service_context
        .governance()
        .list_pending_escalations()
    {
        Ok(escalations) => {
            if escalations.is_empty() {
                println!("  No pending escalations.");
            } else {
                println!("  {:<20} {:<15} {:<10} CONTEXT", "ID", "BOT", "CONFIDENCE");
                println!("  {}", "-".repeat(70));
                for esc in &escalations {
                    let bot_short = uuid::Uuid::parse_str(&esc.bot_id)
                        .map(|u| u.to_string().split('-').next().unwrap_or("?").to_string())
                        .unwrap_or_else(|_| esc.bot_id.chars().take(8).collect());
                    println!(
                        "  {:<20} {:<15} {:<10.2} {}",
                        &esc.id[..std::cmp::min(20, esc.id.len())],
                        bot_short,
                        esc.confidence,
                        &esc.error_context[..std::cmp::min(40, esc.error_context.len())],
                    );
                }
                println!("\n  Total: {} pending", escalations.len());
            }
        }
        Err(e) => println!("  Error: {}", e),
    }
    println!();
}

pub fn handle_resolve(arg1: &str, state: &super::super::ReplState) {
    if arg1.is_empty() {
        println!("  Usage: /resolve <ID>");
    } else {
        match state
            .service_context
            .governance()
            .resolve_escalation(arg1, "cli-user")
        {
            Ok(()) => println!("  Escalation \x1b[32m{}\x1b[0m resolved.", arg1),
            Err(e) => println!("  Error: {}", e),
        }
    }
    println!();
}

pub fn handle_dismiss(arg1: &str, state: &super::super::ReplState) {
    if arg1.is_empty() {
        println!("  Usage: /dismiss <ID>");
    } else {
        match state
            .service_context
            .governance()
            .dismiss_escalation(arg1, "cli-user")
        {
            Ok(()) => println!("  Escalation \x1b[33m{}\x1b[0m dismissed.", arg1),
            Err(e) => println!("  Error: {}", e),
        }
    }
    println!();
}
