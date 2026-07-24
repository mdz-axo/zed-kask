//! `/bundle` REPL commands — skill bundle composition, listing, application.
//!
//! Calls `hkask-services-skill::BundleService` directly (same service layer
//! the deleted CLI command used). This is not a "custom command" — it's the
//! REPL invoking the service layer, same as `/kanban`, `/consolidate`, etc.

use crate::ReplState;
use hkask_services_skill::BundleService;
use hkask_types::visibility::Visibility;

/// Handle `/bundle` REPL commands.
pub fn handle_bundle(
    subcommand: &str,
    rest: &str,
    state: &mut ReplState,
    rt: &tokio::runtime::Handle,
) {
    let ctx = &state.service_context;

    match subcommand {
        "" | "help" => {
            println!("  \x1b[1mBundle Commands\x1b[0m");
            println!(
                "    \x1b[36m/bundle <skill1> <skill2> ...\x1b[0m   Compose a bundle from skills"
            );
            println!("    \x1b[36m/bundle list\x1b[0m                    List all bundles");
            println!("    \x1b[36m/bundle show <id>\x1b[0m                Show bundle details");
            println!("    \x1b[36m/bundle apply <id>\x1b[0m               Apply a bundle");
            println!("    \x1b[36m/bundle evolve <id>\x1b[0m             Evolve a bundle");
            println!("    \x1b[36m/bundle skills\x1b[0m                  List available skills");
            println!(
                "    \x1b[36m/bundle off\x1b[0m                     Deactivate current bundle"
            );
            println!();
        }

        "list" => {
            rt.block_on(async {
                match BundleService::list(ctx).await {
                    Ok(bundles) => {
                        if bundles.is_empty() {
                            println!("  No bundles registered.");
                        } else {
                            println!("  \x1b[1mSkill Bundles\x1b[0m");
                            for b in &bundles {
                                println!(
                                    "    {} — {} skills, visibility={}",
                                    b.id,
                                    b.skills.len(),
                                    b.visibility.as_str()
                                );
                            }
                        }
                        println!();
                    }
                    Err(e) => {
                        eprintln!("  \x1b[31m✗\x1b[0m List failed: {}", e);
                        println!();
                    }
                }
            });
        }

        "show" => {
            let id = rest.trim();
            if id.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Bundle ID required");
                println!("  Usage: \x1b[36m/bundle show <id>\x1b[0m");
                println!();
                return;
            }
            rt.block_on(async {
                match BundleService::get(ctx, id).await {
                    Ok(Some(b)) => {
                        println!("  \x1b[1mBundle: {}\x1b[0m", b.id);
                        println!("    Visibility: {}", b.visibility.as_str());
                        println!("    Skills ({}):", b.skills.len());
                        for s in &b.skills {
                            println!("      - {}", s.id);
                        }
                        println!();
                    }
                    Ok(None) => {
                        eprintln!("  Bundle '{}' not found", id);
                        println!();
                    }
                    Err(e) => {
                        eprintln!("  \x1b[31m✗\x1b[0m Show failed: {}", e);
                        println!();
                    }
                }
            });
        }

        "apply" => {
            let id = rest.trim();
            if id.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Bundle ID required");
                println!("  Usage: \x1b[36m/bundle apply <id>\x1b[0m");
                println!();
                return;
            }
            rt.block_on(async {
                match BundleService::apply(ctx, id).await {
                    Ok(b) => {
                        println!("  \x1b[32m✓\x1b[0m Applied bundle '{}'", b.id);
                        println!("    {} skills activated", b.skills.len());
                        println!();
                    }
                    Err(e) => {
                        eprintln!("  \x1b[31m✗\x1b[0m Apply failed: {}", e);
                        println!();
                    }
                }
            });
        }

        "evolve" => {
            let id = rest.trim();
            if id.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Bundle ID required");
                println!("  Usage: \x1b[36m/bundle evolve <id>\x1b[0m");
                println!();
                return;
            }
            let Some(inference_port) = ctx.inference_port() else {
                eprintln!("  \x1b[31m✗\x1b[0m No inference port available");
                println!();
                return;
            };
            let editor = state.current_agent.clone();
            rt.block_on(async {
                match BundleService::evolve(ctx, id, inference_port, &editor).await {
                    Ok(result) => {
                        println!("  \x1b[32m✓\x1b[0m Evolved bundle '{}'", id);
                        println!("    New bundle: {}", result.manifest.id);
                        println!("    {} skills", result.manifest.skills.len());
                        println!();
                    }
                    Err(e) => {
                        eprintln!("  \x1b[31m✗\x1b[0m Evolve failed: {}", e);
                        println!();
                    }
                }
            });
        }

        "skills" => {
            rt.block_on(async {
                let skills = BundleService::list_skills(ctx).await;
                if skills.is_empty() {
                    println!("  No skills available.");
                } else {
                    println!("  \x1b[1mAvailable Skills\x1b[0m");
                    for s in &skills {
                        println!("    {} [{}]", s.id, s.domain);
                    }
                }
                println!();
            });
        }

        "off" => {
            println!("  \x1b[2mBundle deactivation is handled by the runtime.\x1b[0m");
            println!(
                "  \x1b[2mUse /bundle apply <new-id> to switch, or restart the session.\x1b[0m"
            );
            println!();
        }

        // Default: treat the whole thing as a skill list to compose
        skills_arg => {
            let skill_ids: Vec<String> = skills_arg.split_whitespace().map(String::from).collect();

            if skill_ids.len() < 2 {
                println!("  \x1b[31mError:\x1b[0m A bundle requires at least 2 skills");
                println!("  Usage: \x1b[36m/bundle <skill1> <skill2> ...\x1b[0m");
                println!();
                return;
            }

            let Some(inference_port) = ctx.inference_port() else {
                eprintln!("  \x1b[31m✗\x1b[0m No inference port available");
                println!();
                return;
            };
            let editor = state.current_agent.clone();
            rt.block_on(async {
                match BundleService::compose(
                    ctx,
                    &skill_ids,
                    None,
                    Visibility::Private,
                    inference_port,
                    &editor,
                )
                .await
                {
                    Ok(result) => {
                        println!(
                            "  \x1b[32m✓\x1b[0m Composed bundle from {} skills",
                            skill_ids.len()
                        );
                        println!("    Bundle ID: {}", result.manifest.id);
                        println!("    Visibility: {}", result.manifest.visibility.as_str());
                        println!();
                    }
                    Err(e) => {
                        eprintln!("  \x1b[31m✗\x1b[0m Compose failed: {}", e);
                        println!();
                    }
                }
            });
        }
    }
}
