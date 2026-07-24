//! `/adapter` REPL commands — trained adapter lifecycle.
//!
//! Adapter listing and status route through the training MCP server's tools
//! via `/invoke`. Deploy/teardown require the `AdapterPort` which is not
//! wired into AgentService — those operations go through the HTTP API.

use crate::ReplState;

/// Handle `/adapter` REPL commands.
pub fn handle_adapter(subcommand: &str, _rest: &str, _state: &mut ReplState) {
    match subcommand {
        "" | "help" => {
            println!("  \x1b[1mAdapter Commands\x1b[0m");
            println!("    \x1b[36m/adapter list [skill]\x1b[0m            List trained adapters");
            println!(
                "    \x1b[36m/adapter status <job_id>\x1b[0m         Check training job status"
            );
            println!(
                "    \x1b[36m/adapter deploy <id> <provider>\x1b[0m  Deploy adapter (via API)"
            );
            println!(
                "    \x1b[36m/adapter teardown <id>\x1b[0m           Tear down endpoint (via API)"
            );
            println!();
            println!("  \x1b[2mAdapter list and status route through MCP:\x1b[0m");
            println!("  \x1b[2m  /invoke training/training_status\x1b[0m");
            println!();
            println!("  \x1b[2mDeploy/teardown require AdapterPort (HTTP API only):\x1b[0m");
            println!("  \x1b[2m  POST /api/v1/adapters/deploy\x1b[0m");
            println!();
        }

        "list" => {
            println!("  \x1b[2mAdapter listing via training MCP:\x1b[0m");
            println!("  \x1b[36m/invoke training/training_status\x1b[0m");
            println!();
            println!("  \x1b[2mFor full adapter inventory, use the HTTP API:\x1b[0m");
            println!("  \x1b[2m  GET /api/v1/adapters\x1b[0m");
            println!();
        }

        "status" => {
            let job_id = _rest.trim();
            if job_id.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Job ID required");
                println!("  Usage: \x1b[36m/adapter status <job_id>\x1b[0m");
                println!();
                return;
            }
            println!("  \x1b[2mCheck training status via MCP:\x1b[0m");
            println!(
                "  \x1b[36m/invoke training/training_status {{\"job_id\":\"{}\"}}\x1b[0m",
                job_id
            );
            println!();
        }

        "deploy" => {
            println!(
                "  \x1b[2mAdapter deployment requires AdapterPort (not wired into REPL).\x1b[0m"
            );
            println!("  \x1b[2mUse the HTTP API:\x1b[0m");
            println!("  \x1b[2m  POST /api/v1/adapters/deploy\x1b[0m");
            println!();
        }

        "teardown" => {
            println!(
                "  \x1b[2mAdapter teardown requires AdapterPort (not wired into REPL).\x1b[0m"
            );
            println!("  \x1b[2mUse the HTTP API:\x1b[0m");
            println!("  \x1b[2m  DELETE /api/v1/adapters/<id>\x1b[0m");
            println!();
        }

        _ => {
            println!(
                "  Unknown adapter subcommand: \x1b[31m{}\x1b[0m",
                subcommand
            );
            println!("  Type \x1b[36m/adapter help\x1b[0m for available commands.");
            println!();
        }
    }
}
