//! `/kata` REPL commands — kata manifest listing, inspection, and execution.
//!
//! Calls `KataEngine` from `hkask-services-kata-kanban` directly. Kata manifests
//! are loaded from `registry/manifests/` (same as the deleted CLI command).

use crate::ReplState;
use hkask_services_kata_kanban::{KataEngine, KataError};
use std::collections::HashMap;
use std::path::PathBuf;

/// Handle `/kata` REPL commands.
pub fn handle_kata(
    subcommand: &str,
    rest: &str,
    state: &mut ReplState,
    rt: &tokio::runtime::Handle,
) {
    match subcommand {
        "" | "help" => {
            println!("  \x1b[1mKata Commands\x1b[0m");
            println!("    \x1b[36m/kata list\x1b[0m                    List kata manifests");
            println!("    \x1b[36m/kata show <name>\x1b[0m              Show manifest details");
            println!("    \x1b[36m/kata start <name> [options]\x1b[0m   Execute a kata cycle");
            println!();
            println!("  \x1b[2mOptions for start:\x1b[0m");
            println!("  \x1b[2m  --bot <name>          Learner bot identity\x1b[0m");
            println!("  \x1b[2m  --ctx key=value      Context pairs\x1b[0m");
            println!("  \x1b[2m  --save <path>        Save state to file\x1b[0m");
            println!("  \x1b[2m  --resume <path>      Resume from saved state\x1b[0m");
            println!();
        }

        "list" => list_manifests(),

        "show" => {
            let name = rest.trim();
            if name.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Manifest name required");
                println!("  Usage: \x1b[36m/kata show <name>\x1b[0m");
                println!();
                return;
            }
            show_manifest(name);
        }

        "start" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Manifest name required");
                println!("  Usage: \x1b[36m/kata start <name> [--bot <name>]\x1b[0m");
                println!();
                return;
            }
            let name = parts[0];
            let mut bot = state.current_agent.clone();
            let mut context: Vec<String> = Vec::new();
            let mut save: Option<PathBuf> = None;
            let mut resume: Option<PathBuf> = None;

            let mut i = 1;
            while i < parts.len() {
                match parts[i] {
                    "--bot" if i + 1 < parts.len() => {
                        bot = parts[i + 1].to_string();
                        i += 2;
                    }
                    "--ctx" if i + 1 < parts.len() => {
                        context.push(parts[i + 1].to_string());
                        i += 2;
                    }
                    "--save" if i + 1 < parts.len() => {
                        save = Some(PathBuf::from(parts[i + 1]));
                        i += 2;
                    }
                    "--resume" if i + 1 < parts.len() => {
                        resume = Some(PathBuf::from(parts[i + 1]));
                        i += 2;
                    }
                    _ => i += 1,
                }
            }

            start_kata(
                rt,
                name,
                &bot,
                &context,
                save.as_deref(),
                resume.as_deref(),
                state,
            );
        }

        _ => {
            println!("  Unknown kata subcommand: \x1b[31m{}\x1b[0m", subcommand);
            println!("  Type \x1b[36m/kata help\x1b[0m for available commands.");
            println!();
        }
    }
}

fn manifests_dir() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.join("registry").join("manifests")
}

fn list_manifests() {
    let dir = manifests_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!(
                "  Failed to read manifests directory {}: {}",
                dir.display(),
                e
            );
            return;
        }
    };

    let mut kata_manifests: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains("kata") && name.ends_with(".yaml") {
            kata_manifests.push(name.trim_end_matches(".yaml").to_string());
        }
    }

    if kata_manifests.is_empty() {
        println!("  No kata manifests found in {}", dir.display());
        println!();
        return;
    }

    kata_manifests.sort();
    println!("  \x1b[1mKata manifests ({})\x1b[0m", kata_manifests.len());
    for m in &kata_manifests {
        println!("    {}", m);
    }
    println!();
}

fn show_manifest(name: &str) {
    let dir = manifests_dir();
    let path = dir.join(format!("{}.yaml", name));

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "  Failed to read manifest '{}' at {}: {}",
                name,
                path.display(),
                e
            );
            return;
        }
    };

    let parsed: serde_yaml_neo::Value = match serde_yaml_neo::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("  Failed to parse manifest '{}': {}", name, e);
            return;
        }
    };

    if let Some(manifest) = parsed.get("manifest") {
        println!("  \x1b[1m=== {} ===\x1b[0m", name);
        if let Some(n) = manifest.get("name") {
            println!("    Name: {}", n.as_str().unwrap_or("?"));
        }
        if let Some(d) = manifest.get("description") {
            println!("    Description: {}", d.as_str().unwrap_or("?"));
        }
        if let Some(kt) = manifest.get("kata_type") {
            println!("    Type: {}", kt.as_str().unwrap_or("?"));
        }
    }

    if let Some(gas) = parsed.get("gas")
        && let Some(cap) = gas.get("cap")
    {
        println!("    Gas cap: {}", cap.as_u64().unwrap_or(0));
    }

    if let Some(steps) = parsed.get("steps")
        && let Some(arr) = steps.as_sequence()
    {
        println!("    Steps: {}", arr.len());
        for (i, step) in arr.iter().enumerate() {
            let action = step.get("action").and_then(|a| a.as_str()).unwrap_or("?");
            let desc = step
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("?");
            println!("      {}. {} — {}", i + 1, action, desc);
        }
    }

    if let Some(practices) = parsed.get("practices")
        && let Some(arr) = practices.as_sequence()
    {
        println!("    Practices: {}", arr.len());
        for p in arr {
            if let Some(n) = p.get("name").and_then(|v| v.as_str()) {
                println!("      - {}", n);
            }
        }
    }
    println!();
}

fn start_kata(
    rt: &tokio::runtime::Handle,
    name: &str,
    bot: &str,
    context: &[String],
    _save: Option<&std::path::Path>,
    _resume: Option<&std::path::Path>,
    state: &mut ReplState,
) {
    let dir = manifests_dir();
    let path = dir.join(format!("{}.yaml", name));

    // Parse context key=value pairs
    let mut ctx: HashMap<String, String> = HashMap::new();
    for pair in context {
        if let Some((k, v)) = pair.split_once('=') {
            ctx.insert(k.to_string(), v.to_string());
        }
    }

    let manifest = match KataEngine::load_manifest(&path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("  \x1b[31m✗\x1b[0m Failed to load kata '{}': {}", name, e);
            return;
        }
    };

    let Some(inference_port) = state.service_context.inference_port() else {
        eprintln!("  \x1b[31m✗\x1b[0m No inference port available");
        return;
    };

    // Build a fresh in-memory registry for the kata engine (same as deleted CLI)
    let registry = match hkask_templates::SqliteRegistry::new(None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  \x1b[31m✗\x1b[0m Failed to initialize registry: {}", e);
            return;
        }
    };

    let engine = KataEngine::new(inference_port, registry);

    println!(
        "  \x1b[2mStarting kata '{}' for bot '{}'...\x1b[0m",
        name, bot
    );
    println!();

    let result = rt.block_on(async { engine.execute(&manifest, bot, ctx).await });

    match result {
        Ok(kata_result) => {
            println!("  \x1b[32m✓\x1b[0m Kata '{}' completed", name);
            println!("    Steps completed: {}", kata_result.steps_completed);
            println!("    Gas consumed: {}", kata_result.gas_consumed);
            println!();
        }
        Err(KataError::NoSteps(_)) => {
            eprintln!(
                "  \x1b[31m✗\x1b[0m Kata '{}' has no steps/questions/practices",
                name
            );
            println!();
        }
        Err(KataError::GasExceeded { .. }) => {
            eprintln!("  \x1b[31m✗\x1b[0m Kata '{}' exceeded gas budget", name);
            println!();
        }
        Err(e) => {
            eprintln!("  \x1b[31m✗\x1b[0m Kata '{}' failed: {}", name, e);
            println!();
        }
    }
}
