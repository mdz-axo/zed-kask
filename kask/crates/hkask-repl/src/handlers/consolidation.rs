//! REPL /consolidate handler — user-triggered episodic→semantic consolidation

use hkask_types::ConsolidationRequest;

pub fn handle_consolidate(
    arg: &str,
    state: &mut super::super::ReplState,
    _rt: &tokio::runtime::Handle,
) {
    let trimmed = arg.trim();

    // Show status — route through service_context for a fresh read.
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("status") {
        println!("  \x1b[1mConsolidation Status\x1b[0m");
        println!("  Agent: \x1b[36m{}\x1b[0m", state.current_agent);
        println!("  Agent WebID: {}", state.agent_webid);

        match state
            .service_context
            .consolidation_status_for(&state.current_agent)
        {
            Ok((candidates, semantic_count, low_conf)) => {
                println!();
                println!("  Consolidation candidates: {}", candidates);
                println!("  Semantic h_mem count: {}", semantic_count);
                println!("  Low-confidence h_mems (≤0.33): {}", low_conf);
                println!();
                println!("  Use \x1b[36m/consolidate run\x1b[0m to trigger consolidation");
            }
            Err(_) => {
                println!();
                println!(
                    "  \x1b[33mConsolidation service unavailable\x1b[0m (registry DB not accessible)"
                );
                println!("  Use \x1b[36mkask consolidate\x1b[0m for CLI-based consolidation");
            }
        }
        println!();
        return;
    }

    // "run" or other — execute consolidation with defaults
    // Unlike status (which uses service_context directly), the old
    // display block below uses consolidation_candidate_count — keep
    // the service_context path throughout.

    // Parse optional sub-arguments from "run [--floor F] [--max M] [--limit L]"
    // Supports both space-delimited (--floor 0.33) and equals-delimited (--floor=0.33)
    let mut confidence_floor: Option<f64> = None;
    let mut max_semantic_triples: Option<usize> = None;
    let mut limit: usize = 100;
    let mut unknown_flags: Vec<String> = Vec::new();

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let mut i = 0;
    while i < parts.len() {
        // Handle --flag=value syntax by splitting on '='
        let (flag, inline_value) = if parts[i].starts_with('-') && parts[i].contains('=') {
            let (f, v) = parts[i].split_once('=').expect("key=value format");
            (f, Some(v.to_string()))
        } else {
            (parts[i], None)
        };

        match flag {
            "run" => { /* skip keyword */ }
            "--floor" | "-f" => {
                let raw_value = inline_value
                    .as_deref()
                    .or_else(|| parts.get(i + 1).copied());
                match raw_value {
                    Some(v) => match v.parse::<f64>() {
                        Ok(val) => confidence_floor = Some(val),
                        Err(_) => {
                            println!("  \x1b[31mError:\x1b[0m Invalid --floor value: '{}'", v);
                            println!("  Expected a number between 0.0 and 1.0");
                            return;
                        }
                    },
                    None => {
                        println!(
                            "  \x1b[31mError:\x1b[0m --floor requires a value (e.g., --floor 0.33 or --floor=0.33)"
                        );
                        return;
                    }
                }
                if inline_value.is_none() {
                    i += 1;
                }
            }
            "--max" | "-m" => {
                let raw_value = inline_value
                    .as_deref()
                    .or_else(|| parts.get(i + 1).copied());
                match raw_value {
                    Some(v) => match v.parse::<usize>() {
                        Ok(val) => max_semantic_triples = Some(val),
                        Err(_) => {
                            println!("  \x1b[31mError:\x1b[0m Invalid --max value: '{}'", v);
                            println!("  Expected a positive integer");
                            return;
                        }
                    },
                    None => {
                        println!(
                            "  \x1b[31mError:\x1b[0m --max requires a value (e.g., --max 500 or --max=500)"
                        );
                        return;
                    }
                }
                if inline_value.is_none() {
                    i += 1;
                }
            }
            "--limit" | "-l" => {
                let raw_value = inline_value
                    .as_deref()
                    .or_else(|| parts.get(i + 1).copied());
                match raw_value {
                    Some(v) => match v.parse::<usize>() {
                        Ok(val) => limit = val,
                        Err(_) => {
                            println!("  \x1b[31mError:\x1b[0m Invalid --limit value: '{}'", v);
                            println!("  Expected a positive integer");
                            return;
                        }
                    },
                    None => {
                        println!(
                            "  \x1b[31mError:\x1b[0m --limit requires a value (e.g., --limit 50 or --limit=50)"
                        );
                        return;
                    }
                }
                if inline_value.is_none() {
                    i += 1;
                }
            }
            other if other.starts_with("--") || other.starts_with("-") => {
                unknown_flags.push(other.to_string());
            }
            other => {
                // Try to parse as a bare limit number (e.g., "/consolidate 50")
                if let Ok(n) = other.parse::<usize>() {
                    limit = n;
                }
            }
        }
        i += 1;
    }

    if !unknown_flags.is_empty() {
        println!(
            "  \x1b[33mWarning:\x1b[0m Unknown flags ignored: {}",
            unknown_flags.join(", ")
        );
        println!("  Valid flags: --floor, --max, --limit");
    }

    // Show pre-consolidation state via service_context (fresh DB read).
    if let Ok((candidates, semantic_count, low_conf)) = state
        .service_context
        .consolidation_status_for(&state.current_agent)
    {
        println!("  \x1b[1mPre-consolidation state:\x1b[0m");
        println!("  Consolidation candidates: {}", candidates);
        println!("  Semantic h_mem count: {}", semantic_count);
        println!("  Low-confidence h_mems (≤0.33): {}", low_conf);
    }

    let request = ConsolidationRequest {
        limit,
        confidence_floor,
        max_semantic_triples,
    };

    match state
        .service_context
        .consolidate_userpod_memory(&state.current_agent, request)
    {
        Ok(outcome) => {
            println!();
            println!("  \x1b[1mConsolidation complete:\x1b[0m");
            println!("  Consolidated: {}", outcome.consolidated_count);
            println!("  Deleted: {}", outcome.deleted_count);
            if outcome.failed_count > 0 {
                println!("  Failed: {}", outcome.failed_count);
            }
            // Post-consolidation count via service_context (fresh read).
            if let Ok((_, semantic_count, _)) = state
                .service_context
                .consolidation_status_for(&state.current_agent)
            {
                println!("  Post-consolidation semantic count: {}", semantic_count);
            }
        }
        Err(hkask_services_core::ServiceError::Domain {
            kind: hkask_services_core::ErrorKind::Forbidden,
            domain: hkask_services_core::DomainKind::Consent,
            message,
            ..
        }) => {
            println!();
            println!("  \x1b[31mConsent required:\x1b[0m {}", message);
            println!(
                "  Grant consent outside the REPL with: kask sovereignty grant --category episodic_memory"
            );
            println!(
                "                                      kask sovereignty grant --category semantic_memory"
            );
        }
        Err(e) => {
            println!();
            println!("  \x1b[31mConsolidation failed:\x1b[0m {}", e);
        }
    }
    println!();
}
