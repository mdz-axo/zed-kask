//! Pins the problem-order hypothesis against the archived receipts:
//! the Rust instrument's MT19937 shuffle order must reproduce the archived
//! `problem_id` sequences from the retired Python harness, so legs stay
//! row-for-row comparable with the historical receipts.
//!
//! The test imports the PRODUCTION `dme_harness::mt::PyRandom` — the
//! lib target exists so this oracle pins the real problem order, never
//! a mirrored copy of the algorithm (a mirror can only verify itself;
//! the previous in-tree mirror carried the same never-terminating
//! init_by_array bug the production code did, found at consolidation
//! P4).
//!
//! Skips visibly (not fails) when the archive is absent — this is a
//! live-machine instrument check, not a portable unit test.

use dme_harness::mt::PyRandom;
use std::path::Path;

const ARCHIVE_LOGS: &str = "/home/mdz-axolotl/Documents/zk-data/direct-model-edit-archive/logs";

fn archived_problem_ids(file: &str) -> Vec<String> {
    let path = Path::new(ARCHIVE_LOGS).join(file);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["problem_id"].as_str().map(String::from))
        .collect()
}

#[test]
fn order_matches_archived_receipts() {
    let probe_a = archived_problem_ids("probe-A.jsonl");
    let probe_a96 = archived_problem_ids("probe-A-96.jsonl");
    let probe_b = archived_problem_ids("probe-B.jsonl");
    let probe_b_full = archived_problem_ids("probe-B-full.jsonl");
    if probe_a.is_empty() && probe_a96.is_empty() && probe_b.is_empty() && probe_b_full.is_empty() {
        eprintln!("SKIP: archived receipts not present at {ARCHIVE_LOGS}");
        return;
    }
    let problems = Path::new(
        "/home/mdz-axolotl/Documents/zk-data/direct-model-edit-rust/data/gsm8k-train.jsonl",
    );
    let Ok(raw) = std::fs::read_to_string(problems) else {
        eprintln!("SKIP: gsm8k train jsonl not present");
        return;
    };
    let n = raw.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(n, 7473, "gsm8k train must have 7,473 rows");

    // The production order, from the production RNG.
    let mine: Vec<String> = PyRandom::new(20261106)
        .shuffle_order(n)
        .iter()
        .map(|i| format!("gsm8k-train-{i}"))
        .collect();
    // The P4 gate's named first five.
    assert_eq!(
        &mine[..5],
        [
            "gsm8k-train-6819",
            "gsm8k-train-3171",
            "gsm8k-train-5037",
            "gsm8k-train-6229",
            "gsm8k-train-1026",
        ],
        "the problem-order oracle: the first selected ids must remain the pinned five"
    );

    let mut checked = 0;
    for archived in [&probe_a, &probe_a96, &probe_b, &probe_b_full] {
        for (k, id) in archived.iter().enumerate() {
            assert!(
                k < mine.len(),
                "archived row {k} beyond our order len {}",
                mine.len()
            );
            assert_eq!(
                &mine[k], id,
                "row {k}: Rust order {} != archived {id}",
                mine[k]
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no archived rows parsed to pin against");
    eprintln!("OK: {checked} archived problem_ids reproduced by the MT19937 order");
}
