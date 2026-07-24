//! REPL handler for `/kanban` slash commands.

use crate::ReplState;
use hkask_services_kata_kanban::{
    ColumnDef, KanbanService, SpawnSpec, TaskFilter, TaskSpec, TaskStatus, socratic,
};
use hkask_storage::HMemStore;
use hkask_storage::database::driver::DatabaseDriver;
use hkask_types::WebID;
use std::sync::Arc;

/// Handle `/kanban` REPL commands.
pub fn handle_kanban(
    subcommand: &str,
    rest: &str,
    state: &mut ReplState,
    _rt: &tokio::runtime::Handle,
) {
    let service = kanban_service(state);
    let webid = state.agent_webid;

    match subcommand {
        "" | "help" => {
            println!("  \x1b[1mKanban Commands\x1b[0m");
            println!("    \x1b[36m/kanban board create <name>\x1b[0m     Create a board");
            println!("    \x1b[36m/kanban board list\x1b[0m              List boards");
            println!(
                "    \x1b[36m/kanban view <board> [filter]\x1b[0m  Board view, optional filter"
            );
            println!("    \x1b[36m/kanban task create <board> <title>\x1b[0m  Create a task");
            println!("    \x1b[36m/kanban task list <board> [status]\x1b[0m List tasks");
            println!("    \x1b[36m/kanban task show <id>\x1b[0m          Show task details");
            println!(
                "    \x1b[36m/kanban move <task> <status>\x1b[0m     Move task between columns"
            );
            println!("    \x1b[36m/kanban accept <task>\x1b[0m            Accept task assignment");
            println!(
                "    \x1b[36m/kanban submit <task> <evidence>\x1b[0m  Submit for verification"
            );
            println!(
                "    \x1b[36m/kanban decompose <board> <desc>\x1b[0m  Generate decomposition prompt"
            );
            println!("    \x1b[36m/kanban spawn <task>\x1b[0m            Spawn userpod to execute");
            println!("    \x1b[36m/kanban note <task> <text>\x1b[0m      Append a comment");
            println!("    \x1b[36m/kanban notes <task>\x1b[0m           List task comments");
            println!("    \x1b[36m/kanban deliver <task> <path>\x1b[0m   Add deliverable link");
            println!("    \x1b[36m/kanban phase add <board> <name>\x1b[0m Add a project phase");
            println!("    \x1b[36m/kanban phase set <task> <phase>\x1b[0m Assign task to phase");
            println!("    \x1b[36m/kanban phase list <board>\x1b[0m      List phases and tasks");
            println!("    \x1b[36m/kanban delete <task>\x1b[0m          Delete a task");
            println!("    \x1b[36m/kanban unassign <task>\x1b[0m        Remove assignee");
            println!("    \x1b[36m/kanban reopen <task>\x1b[0m         Reopen a completed task");
            println!("    \x1b[36m/kanban unjam <board>\x1b[0m         Scan for stuck tasks");
            println!();
        }

        "board" => handle_board(&service, webid, rest),

        "task" => handle_task(&service, webid, rest),

        "view" => {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let board_str = parts.first().copied().unwrap_or("");
            let filter = parts.get(1).filter(|s| !s.is_empty()).copied();
            if board_str.is_empty() {
                println!("  Usage: /kanban view <board-id> [status|priority|assignee|label]");
                return;
            }
            let bid = match board_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid board ID");
                    return;
                }
            };
            match service.board_view(bid, filter) {
                Ok(view) => println!("{}", view),
                Err(e) => println!("  Error: {e}"),
            }
        }

        "move" => {
            let parts: Vec<&str> = rest.splitn(3, ' ').collect();
            let task_str = parts.first().copied().unwrap_or("");
            let target_str = parts.get(1).copied().unwrap_or("");
            if task_str.is_empty() || target_str.is_empty() {
                println!("  Usage: /kanban move <task-id> <status>");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            let target = match TaskStatus::parse_str(target_str) {
                Some(s) => s,
                None => {
                    println!("  Invalid status: {target_str}");
                    return;
                }
            };
            match service.task_move(tid, target, webid) {
                Ok(task) => println!("  Task moved to {}", task.status),
                Err(e) => println!("  Error: {e}"),
            }
        }

        "accept" => {
            let task_str = rest.trim();
            if task_str.is_empty() {
                println!("  Usage: /kanban accept <task-id>");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.task_claim(tid, webid) {
                Ok(task) => println!("  Task accepted: {}", task.title),
                Err(e) => println!("  Error: {e}"),
            }
        }

        "submit" => handle_submit(&service, webid, rest),

        "verify" => {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let task_str = parts.first().copied().unwrap_or("");
            let evidence = parts.get(1).copied().unwrap_or("");
            if task_str.is_empty() || evidence.is_empty() {
                println!("  Usage: /kanban verify <task-id> <evidence>");
                println!(
                    "  Generates a structured LLM prompt, then use /kanban verify-llm <task-id> '<json>'"
                );
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.verification_prompt(tid, evidence) {
                Ok(prompt) => {
                    println!("  Verification prompt ready. Feed to your LLM:");
                    println!("  ---");
                    println!("{}", prompt);
                    println!("  ---");
                    println!(
                        "  Then: /kanban verify-llm {} '<llm-json-output>'",
                        task_str
                    );
                }
                Err(e) => println!("  Error: {e}"),
            }
        }

        "verify-llm" => {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let task_str = parts.first().copied().unwrap_or("");
            let json = parts.get(1).copied().unwrap_or("");
            if task_str.is_empty() || json.is_empty() {
                println!("  Usage: /kanban verify-llm <task-id> '<llm-json-output>'");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.verify_with_llm(tid, webid, json) {
                Ok((task, v)) => {
                    println!(
                        "  LLM Verification {} — {}",
                        if v.passed { "PASSED" } else { "FAILED" },
                        v.reasoning
                    );
                    println!("  Task status: {}", task.status);
                }
                Err(e) => println!("  Error: {e}"),
            }
        }

        "note" => {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let task_str = parts.first().copied().unwrap_or("");
            let text = parts.get(1).copied().unwrap_or("");
            if task_str.is_empty() || text.is_empty() {
                println!("  Usage: /kanban note <task-id> <text>");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.task_comment(tid, webid, text) {
                Ok(comment) => println!("  Comment added ({})", comment.id),
                Err(e) => println!("  Error: {e}"),
            }
        }

        "notes" => {
            let task_str = rest.trim();
            if task_str.is_empty() {
                println!("  Usage: /kanban notes <task-id>");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.task_comments(tid) {
                Ok(comments) => {
                    if comments.is_empty() {
                        println!("  No comments.");
                    } else {
                        for c in &comments {
                            println!(
                                "  [{}] {}: {}",
                                c.created_at.format("%H:%M"),
                                c.author.redacted_display(),
                                c.body
                            );
                        }
                    }
                }
                Err(e) => println!("  Error: {e}"),
            }
        }

        "deliver" => {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let task_str = parts.first().copied().unwrap_or("");
            let path = parts.get(1).copied().unwrap_or("");
            if task_str.is_empty() || path.is_empty() {
                println!("  Usage: /kanban deliver <task-id> <file-path-or-url>");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.task_add_deliverable(tid, path, webid) {
                Ok(task) => println!(
                    "  Deliverable added ({}) — {} total",
                    path,
                    task.deliverables.len()
                ),
                Err(e) => println!("  Error: {e}"),
            }
        }

        "phase" => handle_phase(&service, webid, rest),

        "delete" => {
            let task_str = rest.trim();
            if task_str.is_empty() {
                println!("  Usage: /kanban delete <task-id>");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.task_delete(tid) {
                Ok(()) => println!("  Task deleted."),
                Err(e) => println!("  Error: {e}"),
            }
        }

        "unassign" => {
            let task_str = rest.trim();
            if task_str.is_empty() {
                println!("  Usage: /kanban unassign <task-id>");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.task_unassign(tid, webid) {
                Ok(task) => println!("  Task '{}' unassigned.", task.title),
                Err(e) => println!("  Error: {e}"),
            }
        }

        "reopen" => {
            let task_str = rest.trim();
            if task_str.is_empty() {
                println!("  Usage: /kanban reopen <task-id>");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.task_reopen(tid, webid) {
                Ok(task) => println!("  Task '{}' reopened ({}).", task.title, task.status),
                Err(e) => println!("  Error: {e}"),
            }
        }

        "unjam" => handle_unjam(&service, webid, rest),

        "coach" => handle_coach(&service, webid, rest),

        "improve" => {
            let task_str = rest.trim();
            if task_str.is_empty() {
                println!("  Usage: /kanban improve <task-id>");
                println!("  Runs a 4-step improvement kata scoped to this task.");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.task_improvement_prompt(tid) {
                Ok(prompt) => {
                    println!("  Improvement Kata — task-scoped PDCA:");
                    println!("  ---");
                    println!("{}", prompt);
                    println!("  ---");
                    println!(
                        "  Record your experiment: /kanban note {} '<plan/do/check/act>'",
                        task_str
                    );
                }
                Err(e) => println!("  Error: {e}"),
            }
        }

        "practice" => {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let task_str = parts.first().copied().unwrap_or("");
            let focus = parts.get(1).copied().unwrap_or("current blocker");
            if task_str.is_empty() {
                println!("  Usage: /kanban practice <task-id> [focus]");
                println!("  Runs a starter kata observation drill on the task.");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.task_practice_prompt(tid, focus) {
                Ok(prompt) => {
                    println!("  Starter Kata — Observation Drill:");
                    println!("  ---");
                    println!("{}", prompt);
                    println!("  ---");
                    println!(
                        "  Record your observations: /kanban note {} '<facts vs interpretations>'",
                        task_str
                    );
                }
                Err(e) => println!("  Error: {e}"),
            }
        }

        "decompose" => {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let board_str = parts.first().copied().unwrap_or("");
            let project = parts.get(1).copied().unwrap_or("");
            if board_str.is_empty() || project.is_empty() {
                println!("  Usage: /kanban decompose <board-id> <project description>");
                println!("  Then: feed prompt to LLM, paste output with /kanban populate");
                return;
            }
            let bid = match board_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid board ID");
                    return;
                }
            };
            match service.decompose_prompt(bid, project, None, None) {
                Ok(prompt) => {
                    println!("  Prompt ready. Feed this to your LLM:");
                    println!("  ---");
                    println!("{}", prompt);
                    println!("  ---");
                    println!("  Then: /kanban populate {} '<llm-json>'", board_str);
                }
                Err(e) => println!("  Error: {e}"),
            }
        }

        "populate" => {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let board_str = parts.first().copied().unwrap_or("");
            let json = parts.get(1).copied().unwrap_or("");
            if board_str.is_empty() || json.is_empty() {
                println!("  Usage: /kanban populate <board-id> '<json-from-llm>'");
                return;
            }
            let bid = match board_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid board ID");
                    return;
                }
            };
            match service.decompose_populate(bid, webid, json) {
                Ok((count, recomposition)) => {
                    println!("  Created {} tasks.", count);
                    if let Some(r) = recomposition {
                        println!("  Recomposition: {}", r);
                    }
                    match service.board_view(bid, None) {
                        Ok(view) => println!("{}", view),
                        Err(e) => println!("  (view failed: {})", e),
                    }
                }
                Err(e) => println!("  Error: {e}"),
            }
        }

        "spawn" => {
            let task_str = rest.trim();
            if task_str.is_empty() {
                println!("  Usage: /kanban spawn <task-id> [capability-package]");
                println!("  Packages: backend-dev, docs-writer (registry/capabilities/)");
                return;
            }
            let parts: Vec<&str> = task_str.splitn(2, ' ').collect();
            let tid = match parts[0].parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            let spec = SpawnSpec::new(tid);
            match service.spawn_task(tid, spec, webid) {
                Ok(output) => println!("  {}", output),
                Err(e) => println!("  Error: {e}"),
            }
        }

        "socratic" => handle_socratic(&service, webid, rest),

        _ => {
            println!("  Unknown kanban subcommand: {subcommand}");
            println!(
                "  Try: board, view, task, move, accept, submit, note, notes, deliver, phase, delete, unassign, reopen, unjam, socratic, coach, improve, practice, decompose, spawn"
            );
            println!();
        }
    }
}

fn handle_board(service: &KanbanService, webid: WebID, rest: &str) {
    let parts: Vec<&str> = rest.splitn(3, ' ').collect();
    match parts.first().copied().unwrap_or("") {
        "create" => {
            let _rest_parts: Vec<&str> = rest.splitn(4, ' ').collect();
            let name = parts.get(1).copied().unwrap_or("Unnamed Board");
            let tmpl_flag = parts.get(2).copied().unwrap_or("");
            let tmpl_name = parts.get(3).copied().unwrap_or("");
            if tmpl_flag == "--template" && !tmpl_name.is_empty() {
                // Load template from registry
                let tmpl_path = format!("registry/board-templates/{}.yaml", tmpl_name);
                match std::fs::read_to_string(&tmpl_path) {
                    Ok(yaml) => match service.board_create_from_template(webid, name, &yaml) {
                        Ok(board) => {
                            println!(
                                "  Board created from template '{}': {} ({})",
                                tmpl_name, board.name, board.id
                            );
                            for c in &board.columns {
                                let wip = c
                                    .wip_limit
                                    .map_or("\u{221e}".to_string(), |l| l.to_string());
                                println!("    {} (WIP: {})", c.name, wip);
                            }
                        }
                        Err(e) => println!("  Error: {e}"),
                    },
                    Err(_) => println!(
                        "  Template '{}' not found. Available: {}",
                        tmpl_name,
                        KanbanService::list_templates().join(", ")
                    ),
                }
            } else {
                match service.board_create(webid, name, &default_columns()) {
                    Ok(board) => {
                        println!("  Board created: {} ({})", board.name, board.id);
                        println!(
                            "  Columns: Backlog \u{2192} Ready \u{2192} InProgress \u{2192} Review \u{2192} Done"
                        );
                    }
                    Err(e) => println!("  Error: {e}"),
                }
            }
        }
        "list" => match service.board_list(&webid) {
            Ok(boards) => {
                if boards.is_empty() {
                    println!("  No boards found.");
                } else {
                    for b in &boards {
                        println!("  {}  {}", b.id, b.name);
                    }
                }
            }
            Err(e) => println!("  Error: {e}"),
        },
        "delete" => {
            let board_id = parts.get(1).copied().unwrap_or("");
            if board_id.is_empty() {
                println!("  Usage: /kanban board delete <board-id>");
                return;
            }
            let bid = match board_id.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid board ID");
                    return;
                }
            };
            match service.board_delete(bid) {
                Ok(count) => println!("  Board deleted ({} tasks removed).", count),
                Err(e) => println!("  Error: {e}"),
            }
        }
        _ => println!("  Usage: /kanban board create|list|delete"),
    }
}

fn handle_task(service: &KanbanService, webid: WebID, rest: &str) {
    let parts: Vec<&str> = rest.splitn(4, ' ').collect();
    match parts.first().copied().unwrap_or("") {
        "create" => {
            let board = parts.get(1).copied().unwrap_or("");
            let title = parts.get(2).copied().unwrap_or("");
            if board.is_empty() || title.is_empty() {
                println!("  Usage: /kanban task create <board-id> <title>");
                return;
            }
            let bid = match board.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid board ID");
                    return;
                }
            };
            match service.task_create(bid, TaskSpec::new(title.into()), webid) {
                Ok(task) => println!("  Task created: {} ({})", task.title, task.id),
                Err(e) => println!("  Error: {e}"),
            }
        }
        "list" => {
            let board = parts.get(1).copied().unwrap_or("");
            let status = parts.get(2).copied();
            if board.is_empty() {
                println!("  Usage: /kanban task list <board-id> [status]");
                return;
            }
            let bid = match board.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid board ID");
                    return;
                }
            };
            let filter = match status.and_then(TaskStatus::parse_str) {
                Some(st) => TaskFilter::by_status(st),
                None => TaskFilter::all(),
            };
            match service.task_list(bid, filter) {
                Ok(tasks) => {
                    if tasks.is_empty() {
                        println!("  No tasks found.");
                    } else {
                        for (i, t) in tasks.iter().enumerate() {
                            let a = t
                                .assignee
                                .map(|a| a.redacted_display())
                                .unwrap_or_else(|| "unassigned".into());
                            println!("  {}. [{}] {} \u{2014} {}", i + 1, t.status, t.title, a);
                        }
                    }
                }
                Err(e) => println!("  Error: {e}"),
            }
        }
        "show" => {
            let tid_str = parts.get(1).copied().unwrap_or("");
            if tid_str.is_empty() {
                println!("  Usage: /kanban task show <task-id>");
                return;
            }
            let tid = match tid_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.task_get(tid) {
                Ok(Some(task)) => {
                    println!("  Task: {}", task.title);
                    println!("    ID:     {}", task.id);
                    println!("    Status: {}", task.status);
                    if let Some(ref d) = task.description {
                        println!("    Desc:   {d}");
                    }
                }
                Ok(None) => println!("  Task not found."),
                Err(e) => println!("  Error: {e}"),
            }
        }
        _ => println!("  Usage: /kanban task create|list|show"),
    }
}

fn handle_submit(service: &KanbanService, webid: WebID, rest: &str) {
    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    let task_str = parts.first().copied().unwrap_or("");
    let evidence = parts.get(1).copied().unwrap_or("");
    if task_str.is_empty() || evidence.is_empty() {
        println!("  Usage: /kanban submit <task-id> <evidence>");
        println!("  For LLM verification: /kanban verify <task-id> <evidence>");
        return;
    }
    let tid = match task_str.parse() {
        Ok(id) => id,
        Err(_) => {
            println!("  Invalid task ID");
            return;
        }
    };
    // Evidence-based verification (non-empty evidence = user confirmed)
    match service.task_verify(tid, evidence, webid) {
        Ok((task, v)) => {
            println!(
                "  Verification {} — {}",
                if v.passed { "PASSED" } else { "FAILED" },
                v.reasoning
            );
            println!("  Task status: {}", task.status);
            println!("  (Contract-based. For LLM: /kanban verify {})", task_str);
        }
        Err(e) => println!("  Error: {e}"),
    }
}

fn handle_phase(service: &KanbanService, _webid: WebID, rest: &str) {
    let parts: Vec<&str> = rest.splitn(4, ' ').collect();
    let action = parts.first().copied().unwrap_or("");
    match action {
        "add" => {
            let board_str = parts.get(1).copied().unwrap_or("");
            let name = parts.get(2).copied().unwrap_or("");
            if board_str.is_empty() || name.is_empty() {
                println!("  Usage: /kanban phase add <board-id> <name>");
                return;
            }
            let bid = match board_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid board ID");
                    return;
                }
            };
            match service.board_add_phase(bid, name, 0) {
                Ok(phase) => println!("  Phase created: {} ({})", phase.name, phase.id),
                Err(e) => println!("  Error: {e}"),
            }
        }
        "set" => {
            let task_str = parts.get(1).copied().unwrap_or("");
            let phase_str = parts.get(2).copied().unwrap_or("");
            if task_str.is_empty() || phase_str.is_empty() {
                println!("  Usage: /kanban phase set <task-id> <phase-id>");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            let pid = match phase_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid phase ID");
                    return;
                }
            };
            match service.task_set_phase(tid, pid) {
                Ok(task) => println!("  Task '{}' assigned to phase", task.title),
                Err(e) => println!("  Error: {e}"),
            }
        }
        "list" => {
            let board_str = parts.get(1).copied().unwrap_or("");
            if board_str.is_empty() {
                println!("  Usage: /kanban phase list <board-id>");
                return;
            }
            let bid = match board_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid board ID");
                    return;
                }
            };
            match service.board_get(bid) {
                Ok(Some(board)) => {
                    if board.phases.is_empty() {
                        println!("  No phases defined.");
                    } else {
                        for p in &board.phases {
                            println!("  Phase: {} ({})", p.name, p.id);
                            if let Ok(tasks) = service.tasks_by_phase(bid, p.id) {
                                for t in &tasks {
                                    println!("    - [{}] {}", t.status, t.title);
                                }
                            }
                        }
                    }
                }
                Ok(None) => println!("  Board not found."),
                Err(e) => println!("  Error: {e}"),
            }
        }
        _ => println!("  Usage: /kanban phase add|set|list"),
    }
}

fn handle_unjam(service: &KanbanService, _webid: WebID, rest: &str) {
    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    let board_str = parts.first().copied().unwrap_or("");
    let flag = parts.get(1).copied().unwrap_or("");
    if board_str.is_empty() {
        println!("  Usage: /kanban unjam <board-id> [--fix]");
        return;
    }
    let bid = match board_str.parse() {
        Ok(id) => id,
        Err(_) => {
            println!("  Invalid board ID");
            return;
        }
    };
    if flag == "--fix" {
        match service.unjam_fix(bid) {
            Ok(fixes) => {
                if fixes.is_empty() {
                    println!("  Nothing to fix. Board is flowing.");
                } else {
                    println!("  Auto-fixed {} issue(s):", fixes.len());
                    for f in &fixes {
                        println!("    {} \u{2014} {}", f.task_title, f.action);
                    }
                }
            }
            Err(e) => println!("  Error: {e}"),
        }
    } else {
        match service.unjam_report(bid) {
            Ok(items) => {
                if items.is_empty() {
                    println!("  Board is flowing. No stuck tasks detected.");
                } else {
                    println!("  Found {} stuck task(s):", items.len());
                    for item in &items {
                        println!("    {} \u{2014} {}", item.task_title, item.issue);
                        println!("      \u{2192} {}", item.suggestion);
                    }
                    println!(
                        "  Run /kanban unjam {} --fix to auto-correct clear cases.",
                        board_str
                    );
                }
            }
            Err(e) => println!("  Error: {e}"),
        }
    }
}

fn handle_coach(service: &KanbanService, _webid: WebID, rest: &str) {
    let task_str = rest.trim();
    if task_str.is_empty() {
        println!("  Usage: /kanban coach <task-id>");
        println!("  Runs a 5-question coaching kata scoped to this task.");
        return;
    }
    let tid = match task_str.parse() {
        Ok(id) => id,
        Err(_) => {
            println!("  Invalid task ID");
            return;
        }
    };
    match service.task_coaching_prompt(tid) {
        Ok(prompt) => {
            println!("  Coaching Kata \u{2014} task-scoped 5 questions:");
            println!("  ---");
            println!("{}", prompt);
            println!("  ---");
            println!(
                "  Respond with a comment: /kanban note {} '<your answers>'",
                task_str
            );
        }
        Err(e) => println!("  Error: {e}"),
    }
}

fn handle_socratic(service: &KanbanService, webid: WebID, rest: &str) {
    let parts: Vec<&str> = rest.splitn(3, ' ').collect();
    let action = parts.first().copied().unwrap_or("");
    match action {
        "start" => {
            let board_str = parts.get(1).copied().unwrap_or("");
            let topic = parts.get(2).copied().unwrap_or("");
            if board_str.is_empty() {
                println!("  Usage: /kanban socratic start <board-id> <topic>");
                return;
            }
            let bid = match board_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid board ID");
                    return;
                }
            };
            let topic = if topic.is_empty() {
                "Untitled Inquiry"
            } else {
                topic
            };
            match socratic::create_inquiry(service, bid, topic, webid) {
                Ok(task) => {
                    println!("  Inquiry created: {} ({})", task.title, task.id);
                    match socratic::prompt(service, task.id) {
                        Ok((prompt, stage)) => {
                            println!("  Stage: {}", stage);
                            println!("{prompt}");
                        }
                        Err(e) => println!("  Error generating prompt: {e}"),
                    }
                }
                Err(e) => println!("  Error: {e}"),
            }
        }
        "continue" => {
            let task_str = parts.get(1).copied().unwrap_or("");
            let response = parts.get(2).copied().unwrap_or("");
            if task_str.is_empty() {
                println!("  Usage: /kanban socratic continue <task-id> <your response>");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            // Quality gate: evaluate before advancing
            if !response.is_empty() {
                match socratic::quality_check(service, tid, response) {
                    Ok(gate) if !gate.passed => {
                        println!("  Quality gate ({}) — response needs work:", gate.stage);
                        println!("  {}", gate.feedback);
                        println!(
                            "  Re-submit with: /kanban socratic continue {} <improved response>",
                            task_str
                        );
                        return;
                    }
                    Err(e) => {
                        println!("  Quality check error: {e}");
                        return;
                    }
                    _ => {} // passed, continue
                }
            }
            // Post response as comment
            if !response.is_empty()
                && let Err(e) = service.task_comment(tid, webid, response)
            {
                println!("  Error posting response: {e}");
                return;
            }
            let task = match service.task_get(tid) {
                Ok(Some(t)) => t,
                Ok(None) => {
                    println!("  Task not found");
                    return;
                }
                Err(e) => {
                    println!("  Error: {e}");
                    return;
                }
            };
            if task.status == TaskStatus::Review {
                if response.is_empty() {
                    println!("  Provide your summary as evidence to complete the inquiry.");
                    println!("  Usage: /kanban socratic continue <task-id> <your summary>");
                    return;
                }
                match service.task_verify(tid, response, webid) {
                    Ok((t, v)) => {
                        println!(
                            "  Inquiry complete — {}",
                            if v.passed { "PASSED" } else { "REVIEW" }
                        );
                        println!("  {}", v.reasoning);
                        println!("  Status: {}", t.status);
                    }
                    Err(e) => println!("  Error: {e}"),
                }
                return;
            }
            match socratic::advance(service, tid, webid) {
                Ok(msg) => {
                    println!("  {}", msg);
                    match socratic::prompt(service, tid) {
                        Ok((prompt, stage)) => {
                            println!("  Stage: {}", stage);
                            println!("{prompt}");
                        }
                        Err(e) => println!("  Error generating prompt: {e}"),
                    }
                }
                Err(e) => println!("  Error: {e}"),
            }
        }
        "team" => {
            let board_str = parts.get(1).copied().unwrap_or("");
            let topic = parts.get(2).copied().unwrap_or("");
            if board_str.is_empty() {
                println!("  Usage: /kanban socratic team <board-id> <topic>");
                println!("  Spawns Planner, Diagnoser, Tutor, and Assessor tasks");
                return;
            }
            let bid = match board_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid board ID");
                    return;
                }
            };
            let topic = if topic.is_empty() { "Untitled" } else { topic };
            match socratic::spawn_role_inquiries(service, bid, topic, webid) {
                Ok(tasks) => {
                    println!("  Role team spawned (4 tasks):");
                    for t in &tasks {
                        println!("    {} ({})", t.title, t.id);
                    }
                    match socratic::synthesize_roles(service, &tasks) {
                        Ok(report) => println!("{report}"),
                        Err(e) => println!("  Error: {e}"),
                    }
                }
                Err(e) => println!("  Error: {e}"),
            }
        }
        "status" => {
            let task_str = parts.get(1).copied().unwrap_or("");
            if task_str.is_empty() {
                println!("  Usage: /kanban socratic status <task-id>");
                return;
            }
            let tid = match task_str.parse() {
                Ok(id) => id,
                Err(_) => {
                    println!("  Invalid task ID");
                    return;
                }
            };
            match service.task_get(tid) {
                Ok(Some(task)) => {
                    let stage = socratic::stage_name(task.status);
                    println!("  {} — Stage: {} ({})", task.title, stage, task.status);
                    let comments = service.task_comments(tid).unwrap_or_default();
                    println!("  Comments: {}", comments.len());
                    if task.status != TaskStatus::Done
                        && task.status != TaskStatus::Review
                        && let Some(last) = comments.last()
                    {
                        match socratic::quality_check(service, tid, &last.body) {
                            Ok(gate) => {
                                if gate.passed {
                                    println!("  Quality: PASSED — ready to advance");
                                } else {
                                    println!("  Quality: NEEDS WORK");
                                    println!("  {}", gate.feedback);
                                }
                            }
                            Err(e) => println!("  Quality check error: {e}"),
                        }
                    }
                }
                Ok(None) => println!("  Task not found"),
                Err(e) => println!("  Error: {e}"),
            }
        }
        _ => {
            println!("  Socratic inquiry — structured 4-stage exploration");
            println!("    /kanban socratic start <board> <topic>        Begin inquiry");
            println!(
                "    /kanban socratic continue <task> <response>   Post + advance (with quality gate)"
            );
            println!("    /kanban socratic team <board> <topic>         Spawn 4-role inquiry team");
            println!(
                "    /kanban socratic status <task>                 Show stage + quality gate"
            );
            println!("  Stages: Elicit \u{2192} Structure \u{2192} Test \u{2192} Summarize");
        }
    }
}

fn kanban_service(state: &mut ReplState) -> KanbanService {
    // Use cached service or create new one
    state
        .kanban_service
        .get_or_insert_with(|| {
            let pool = hkask_storage::database::sqlite::SqliteDriver::in_memory_pool()
                .expect("in-memory pool");
            let driver = Arc::new(hkask_storage::database::sqlite::SqliteDriver::new(pool));
            driver
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS h_mems (
                        id TEXT PRIMARY KEY, entity TEXT NOT NULL, attribute TEXT NOT NULL,
                        value TEXT NOT NULL, valid_from TEXT NOT NULL, valid_to TEXT,
                        confidence REAL NOT NULL, perspective TEXT, visibility TEXT NOT NULL,
                        owner_webid TEXT NOT NULL
                    )",
                )
                .expect("DDL batch must succeed");
            KanbanService::new(HMemStore::from_driver(driver))
        })
        .clone()
}

fn default_columns() -> Vec<ColumnDef> {
    KanbanService::standard_columns()
}
