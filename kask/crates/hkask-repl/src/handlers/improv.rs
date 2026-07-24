//! REPL /improv handler — set or display the active improv mode.
//!
//! Improv modes (Plussing, Yes And, Yes But, Freestyling, Riffing, Cascade)
//! set the userpod's interaction posture. Uses the `ImprovMode` type from
//! `hkask-services-chat` directly — no stringly-typed intermediation.

use crate::ReplState;
use hkask_services_chat::ImprovMode;
use hkask_services_chat::MATRYOSHKA_LIMIT;
use hkask_services_chat::RiffReturn;
use std::time::Duration;

/// Mode descriptions for display.
fn mode_description(mode: &ImprovMode) -> &'static str {
    match mode {
        ImprovMode::Plussing => {
            "Extract agreeable, silently discard, build constructively (never negate)"
        }
        ImprovMode::YesAnd => "Accept whole contribution, extend with novel additive layer",
        ImprovMode::YesBut => "Accept whole, append constraint that narrows without contradicting",
        ImprovMode::Freestyling { .. } => {
            "Rapid collaborative short-response cycling (time-bounded, round-robin)"
        }
        ImprovMode::Riffing { .. } => {
            "Solo divergent exploration from a seed (return to group or spawn thread)"
        }
        ImprovMode::Cascade(_c) => {
            // Description is static; we can't format here, so return a generic one.
            // The display code below prints the step count separately.
            "Recursive cascade of improv modes (bounded by matryoshka limit of 7)"
        }
    }
}

/// Format a mode for display, including parameters.
fn format_mode(mode: &ImprovMode) -> String {
    match mode {
        ImprovMode::Plussing => "plussing".to_string(),
        ImprovMode::YesAnd => "yes-and".to_string(),
        ImprovMode::YesBut => "yes-but".to_string(),
        ImprovMode::Freestyling { time_bound } => {
            format!("freestyling ({}s bound)", time_bound.as_secs())
        }
        ImprovMode::Riffing { return_policy } => {
            let policy = match return_policy {
                RiffReturn::ReturnToGroup => "return-to-group",
                RiffReturn::SpawnThread => "spawn-thread",
                RiffReturn::ReturnAfterSteps { max_steps } => {
                    return format!("riffing (return after {} steps)", max_steps);
                }
            };
            format!("riffing ({})", policy)
        }
        ImprovMode::Cascade(c) => {
            let step_labels: Vec<String> = c.iter().map(|m| m.label().to_string()).collect();
            let total = ImprovMode::total_applications(c);
            format!(
                "cascade [{}] ({}/{})",
                step_labels.join(" → "),
                total,
                MATRYOSHKA_LIMIT
            )
        }
    }
}

pub fn handle_improv(arg1: &str, arg2: &str, state: &mut ReplState) {
    let mode_arg = if arg2.is_empty() {
        arg1.to_lowercase()
    } else {
        format!("{} {}", arg1.to_lowercase(), arg2.to_lowercase())
    };

    if mode_arg.is_empty() {
        // Display current mode.
        match &state.improv_mode {
            Some(mode) => {
                println!("  Active improv mode: \x1b[1m{}\x1b[0m", format_mode(mode));
                println!("  {}", mode_description(mode));
                if let ImprovMode::Cascade(c) = mode {
                    println!("  Steps: {}", c.len());
                    println!(
                        "  Total applications: {} (limit: {})",
                        ImprovMode::total_applications(c),
                        MATRYOSHKA_LIMIT
                    );
                }
            }
            None => {
                println!("  No improv mode active.");
                println!("  Available modes:");
                println!(
                    "    \x1b[36m/improv plussing\x1b[0m   — Extract agreeable, build constructively"
                );
                println!(
                    "    \x1b[36m/improv yes-and\x1b[0m     — Accept whole, extend additively"
                );
                println!("    \x1b[36m/improv yes-but\x1b[0m     — Accept whole, constrain scope");
                println!(
                    "    \x1b[36m/improv freestyle [S]\x1b[0m — Rapid group cycling (S=seconds)"
                );
                println!(
                    "    \x1b[36m/improv riff [POLICY]\x1b[0m — Solo tangent (group|spawn|steps:N)"
                );
                println!(
                    "    \x1b[36m/improv cascade M1 M2...\x1b[0m — Compose modes recursively (max 7)"
                );
            }
        }
        println!();
        return;
    }

    // Parse the mode.
    let parsed = parse_improv_mode(&mode_arg);
    match parsed {
        Ok(mode) => {
            state.improv_mode = Some(mode.clone());
            println!("  Improv mode set to: \x1b[1m{}\x1b[0m", format_mode(&mode));
            println!("  {}", mode_description(&mode));
            if let ImprovMode::Cascade(c) = &mode {
                println!("  Steps: {}", c.len());
                println!(
                    "  Total applications: {} (matryoshka limit: {})",
                    ImprovMode::total_applications(c),
                    MATRYOSHKA_LIMIT
                );
            }
        }
        Err(e) => {
            println!("  \x1b[31m{}\x1b[0m", e);
            println!("  Valid modes: plussing, yes-and, yes-but, freestyle [S], riff [POLICY]");
            println!("  Cascade: /improv cascade plussing yes-and riff:spawn (max 7 apps)");
        }
    }
    println!();
}

/// Parse a mode argument string into an `ImprovMode`.
fn parse_improv_mode(input: &str) -> anyhow::Result<ImprovMode> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let base = parts[0];

    match base {
        "plussing" => Ok(ImprovMode::Plussing),
        "yes-and" | "yesand" => Ok(ImprovMode::YesAnd),
        "yes-but" | "yesbut" => Ok(ImprovMode::YesBut),

        "freestyling" | "freestyle" => {
            let secs: u64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(300);
            Ok(ImprovMode::Freestyling {
                time_bound: Duration::from_secs(secs),
            })
        }

        "riffing" | "riff" => {
            let policy_str = parts.get(1).unwrap_or(&"group");
            let policy = parse_riff_policy(policy_str)?;
            Ok(ImprovMode::Riffing {
                return_policy: policy,
            })
        }

        "cascade" => {
            if parts.len() < 2 {
                return Err(anyhow::anyhow!(
                    "Cascade requires at least one mode. Example: /improv cascade plussing yes-and"
                ));
            }
            // Parse each subsequent part as a mode.
            let mut modes = Vec::new();
            for part in &parts[1..] {
                let mode = parse_improv_mode(part)?;
                modes.push(mode);
            }
            // Validate matryoshka limit.
            let total = ImprovMode::total_applications(&modes);
            if total > MATRYOSHKA_LIMIT {
                return Err(anyhow::anyhow!(
                    "Cascade exceeds matryoshka limit: {} applications (max {})",
                    total,
                    MATRYOSHKA_LIMIT
                ));
            }
            Ok(ImprovMode::Cascade(modes))
        }

        _ => Err(anyhow::anyhow!("Unknown improv mode: '{}'", base)),
    }
}

/// Parse a riff return policy string.
fn parse_riff_policy(s: &str) -> anyhow::Result<RiffReturn> {
    match s {
        "group" | "return" | "return-to-group" => Ok(RiffReturn::ReturnToGroup),
        "spawn" | "spawn-thread" | "new-thread" => Ok(RiffReturn::SpawnThread),
        other => {
            if let Some(stripped) = other.strip_prefix("steps:") {
                let max_steps: usize = stripped
                    .parse()
                    .map_err(|_| anyhow::anyhow!("Invalid step count in '{}'", other))?;
                Ok(RiffReturn::ReturnAfterSteps { max_steps })
            } else {
                Err(anyhow::anyhow!(
                    "Unknown riff policy: '{}'. Use group, spawn, or steps:N",
                    s
                ))
            }
        }
    }
}
