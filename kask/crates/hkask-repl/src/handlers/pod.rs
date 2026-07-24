//! `/pod` REPL commands — pod lifecycle management.
//!
//! Calls `ActivePods` directly via the service context's infra.

use crate::ReplState;
use hkask_pods::pod::{PodID, PodKind};

/// Handle `/pod` REPL commands.
pub fn handle_pod(
    subcommand: &str,
    rest: &str,
    state: &mut ReplState,
    rt: &tokio::runtime::Handle,
) {
    let pods = state.service_context.infra().pods.clone();

    match subcommand {
        "" | "help" => {
            println!("  \x1b[1mPod Commands\x1b[0m");
            println!("    \x1b[36m/pod list\x1b[0m                    List all pods");
            println!("    \x1b[36m/pod status <id>\x1b[0m              Show pod status");
            println!("    \x1b[36m/pod create <template> <persona> [name]\x1b[0m  Create a pod");
            println!("    \x1b[36m/pod activate <id>\x1b[0m             Activate a pod");
            println!("    \x1b[36m/pod deactivate <id>\x1b[0m           Deactivate a pod");
            println!();
        }

        "list" => {
            match rt.block_on(pods.list_pods()) {
                Ok(pod_list) => {
                    if pod_list.is_empty() {
                        println!("  No pods registered.");
                    } else {
                        println!("  \x1b[1mAgent pods ({})\x1b[0m", pod_list.len());
                        for pod in &pod_list {
                            println!(
                                "    \x1b[36m{}\x1b[0m [{}] {}",
                                pod.pod_id,
                                pod.state,
                                pod.name.as_deref().unwrap_or("unnamed")
                            );
                            println!("      WebID: {}", pod.webid);
                        }
                    }
                }
                Err(e) => eprintln!("  \x1b[31m✗\x1b[0m Pod listing failed: {}", e),
            }
            println!();
        }

        "status" => {
            let id_str = rest.trim();
            if id_str.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Pod ID required");
                println!("  Usage: \x1b[36m/pod status <id>\x1b[0m");
                println!();
                return;
            }
            let pod_id = match parse_pod_id(id_str) {
                Some(id) => id,
                None => {
                    eprintln!("  \x1b[31m✗\x1b[0m Invalid pod ID: {}", id_str);
                    println!();
                    return;
                }
            };
            match rt.block_on(pods.get_pod_status(&pod_id)) {
                Ok(status) => {
                    println!("  \x1b[1mPod {}\x1b[0m", status.pod_id);
                    println!(
                        "    Name:       {}",
                        status.name.as_deref().unwrap_or("unnamed")
                    );
                    println!("    State:      {}", status.state);
                    println!("    WebID:      {}", status.webid);
                    println!("    Template:   {}", status.template);
                    println!("    Created:    {}", status.created_at);
                    println!();
                }
                Err(e) => {
                    eprintln!("  \x1b[31m✗\x1b[0m Status failed: {}", e);
                    println!();
                }
            }
        }

        "create" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Template and name required");
                println!("  Usage: \x1b[36m/pod create <template> <name> [cap1,cap2,...]\x1b[0m");
                println!();
                return;
            }
            let template = parts[0];
            let name = if parts.len() > 1 {
                parts[1].to_string()
            } else {
                println!("  \x1b[31mError:\x1b[0m Pod name required");
                println!();
                return;
            };
            let capabilities: Vec<String> = parts
                .get(2)
                .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
                .unwrap_or_else(|| vec!["tool:execute".to_string()]);
            let webid = hkask_types::WebID::from_persona(name.as_bytes());

            match rt.block_on(pods.create_pod(
                template,
                &name,
                webid,
                capabilities,
                PodKind::UserPod,
            )) {
                Ok(pod_id) => {
                    println!("  \x1b[32m✓\x1b[0m Created pod: {}", pod_id);
                    println!();
                }
                Err(e) => {
                    eprintln!("  \x1b[31m✗\x1b[0m Create failed: {}", e);
                    println!();
                }
            }
        }

        "activate" => {
            let id_str = rest.trim();
            if id_str.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Pod ID required");
                println!("  Usage: \x1b[36m/pod activate <id>\x1b[0m");
                println!();
                return;
            }
            let pod_id = match parse_pod_id(id_str) {
                Some(id) => id,
                None => {
                    eprintln!("  \x1b[31m✗\x1b[0m Invalid pod ID: {}", id_str);
                    println!();
                    return;
                }
            };
            match rt.block_on(pods.activate_pod(&pod_id)) {
                Ok(()) => {
                    println!("  \x1b[32m✓\x1b[0m Pod {} activated", pod_id);
                    println!();
                }
                Err(e) => {
                    eprintln!("  \x1b[31m✗\x1b[0m Activate failed: {}", e);
                    println!();
                }
            }
        }

        "deactivate" => {
            let id_str = rest.trim();
            if id_str.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Pod ID required");
                println!("  Usage: \x1b[36m/pod deactivate <id>\x1b[0m");
                println!();
                return;
            }
            let pod_id = match parse_pod_id(id_str) {
                Some(id) => id,
                None => {
                    eprintln!("  \x1b[31m✗\x1b[0m Invalid pod ID: {}", id_str);
                    println!();
                    return;
                }
            };
            match rt.block_on(pods.sleep_pod(&pod_id)) {
                Ok(()) => {
                    println!("  \x1b[32m✓\x1b[0m Pod {} deactivated", pod_id);
                    println!();
                }
                Err(e) => {
                    eprintln!("  \x1b[31m✗\x1b[0m Deactivate failed: {}", e);
                    println!();
                }
            }
        }

        _ => {
            println!("  Unknown pod subcommand: \x1b[31m{}\x1b[0m", subcommand);
            println!("  Type \x1b[36m/pod help\x1b[0m for available commands.");
            println!();
        }
    }
}

fn parse_pod_id(id: &str) -> Option<PodID> {
    uuid::Uuid::parse_str(id).ok().map(PodID::from_uuid)
}
