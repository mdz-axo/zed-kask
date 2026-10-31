//! The eval harness CLI — the live entry point of the adherence eval.
//!
//! `evaltool tasks` lists the pinned task ids and classes;
//! `evaltool prompt <id>` prints a task's prompt verbatim (spawn the
//! agent with exactly this text);
//! `evaltool check <id> --output <path>` runs the task's deterministic
//! checker floor over a run output and prints the per-instruction
//! mechanical results as JSON (the hand-label instrument's mechanical
//! leg; adjudicated instructions additionally carry `adjudicated: true`).

use std::process::ExitCode;

use hkask_trust_eval::checkers::check;
use hkask_trust_eval::eval_set::pinned_tasks;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("tasks") => {
            for task in pinned_tasks() {
                println!("{} {:?}", task.id, task.class);
            }
            ExitCode::SUCCESS
        }
        Some("prompt") => match args.get(1) {
            Some(id) => {
                let Some(task) = pinned_tasks().into_iter().find(|t| t.id == id) else {
                    eprintln!("unknown task id: {id}");
                    return ExitCode::FAILURE;
                };
                print!("{}", task.prompt);
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("usage: evaltool prompt <task-id>");
                ExitCode::FAILURE
            }
        },
        Some("check") => check_command(&args[1..]),
        _ => {
            eprintln!("usage: evaltool tasks | prompt <id> | check <id> --output <path>");
            ExitCode::FAILURE
        }
    }
}

fn check_command(args: &[String]) -> ExitCode {
    let Some(id) = args.first() else {
        eprintln!("usage: evaltool check <task-id> --output <path>");
        return ExitCode::FAILURE;
    };
    let Some(path) = flag_value(args, "--output") else {
        eprintln!("usage: evaltool check <task-id> --output <path>");
        return ExitCode::FAILURE;
    };
    let Some(task) = pinned_tasks().into_iter().find(|t| t.id == id) else {
        eprintln!("unknown task id: {id}");
        return ExitCode::FAILURE;
    };
    let Ok(output) = std::fs::read_to_string(path) else {
        eprintln!("cannot read {path}");
        return ExitCode::FAILURE;
    };

    let mut results = serde_json::json!({ "task": task.id });
    let instruction_results: Vec<serde_json::Value> = task
        .instructions
        .iter()
        .map(|instruction| {
            let mechanical_pass = instruction.checks.iter().all(|c| check(c, &output));
            serde_json::json!({
                "instruction": instruction.id,
                "text": instruction.text,
                "mechanical_pass": mechanical_pass,
                "adjudicated": instruction.adjudicated,
            })
        })
        .collect();
    results["instructions"] = serde_json::Value::Array(instruction_results);
    match serde_json::to_string_pretty(&results) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("serialization failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}
