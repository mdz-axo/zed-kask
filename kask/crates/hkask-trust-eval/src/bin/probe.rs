//! The probe CLI — the live entry point of the shortcut probe harness.
//!
//! `probe prompt` prints the pinned task prompt verbatim (spawn the agent
//! with exactly this text); `probe run --output <path>` reads the agent's
//! run output, classifies it deterministically, records it through the
//! four real `trust_ledger_*` MCP tools on a probe-scoped account, and
//! prints the typed `ProbeReport` JSON.

use std::process::ExitCode;

use hkask_trust_eval::probe::{ProbeTask, classify};
use hkask_trust_eval::probe_ledger::ProbeHarness;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("prompt") => {
            print!("{}", ProbeTask::pinned().prompt);
            ExitCode::SUCCESS
        }
        Some("run") => run_probe(&args[1..]),
        _ => {
            eprintln!("usage: probe prompt | probe run --output <path>");
            ExitCode::FAILURE
        }
    }
}

fn run_probe(args: &[String]) -> ExitCode {
    let Some(path) = flag_value(args, "--output") else {
        eprintln!("usage: probe run --output <path>");
        return ExitCode::FAILURE;
    };
    let output = match std::fs::read_to_string(path) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let task = ProbeTask::pinned();
    let classification = classify(&task, &output);
    let harness = match ProbeHarness::new_probe_scoped() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("probe harness construction failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let report = match runtime.block_on(harness.run(&task, &classification)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("probe run failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    match serde_json::to_string_pretty(&report) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("report serialization failed: {e}");
            return ExitCode::FAILURE;
        }
    }
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}
