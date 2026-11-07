//! Pins the problem-order hypothesis against the archived receipts:
//! the Rust instrument's MT19937 shuffle order must reproduce the archived
//! `problem_id` sequences from the retired Python harness, so legs stay
//! row-for-row comparable with the historical receipts.
//!
//! Skips visibly (not fails) when the archive is absent — this is a
//! live-machine instrument check, not a portable unit test.

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

fn ids_from_order(order: &[usize]) -> Vec<String> {
    order.iter().map(|i| format!("gsm8k-train-{i}")).collect()
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

    let order = mt_order(n, 20261106);
    let mine = ids_from_order(&order);

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

/// Mirrors src/mt.rs exactly (the bin crate keeps its modules private;
/// a lib target is unnecessary for a harness bin).
fn mt_order(n: usize, seed: u64) -> Vec<usize> {
    let mut rng = PyRandomMirror::new(seed);
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = (rng.random() * (i as f64 + 1.0)) as usize;
        order.swap(i, j);
    }
    order
}

struct PyRandomMirror {
    mt: [u32; 624],
    idx: usize,
}

impl PyRandomMirror {
    fn new(seed: u64) -> Self {
        let mut key = vec![seed as u32];
        if seed >> 32 != 0 {
            key.push((seed >> 32) as u32);
        }
        Self::init_by_array(&key)
    }
    fn init_by_array(key: &[u32]) -> Self {
        let mut mt = [0u32; 624];
        mt[0] = 19650218u32;
        let mut i = 1usize;
        let mut j = 0usize;
        let mut k = if 624 > key.len() { 624 } else { key.len() };
        while k > 0 {
            mt[i] = mt[i] ^ mt[i - 1].wrapping_mul(0x9908_b0df).wrapping_add(1664525) ^ 0;
            mt[i] = mt[i].wrapping_add(key[j]);
            i += 1;
            j += 1;
            if i >= 624 {
                mt[0] = mt[623];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
            k -= 1;
        }
        let mut i = 1usize;
        while i < 624 {
            mt[i] = (mt[i] ^ mt[i - 1].wrapping_mul(0x9908_b0df)).wrapping_add(1566083941);
            i += 1;
            if i >= 624 {
                mt[0] = mt[623];
                i = 1;
            }
        }
        mt[0] = 0x8000_0000;
        Self { mt, idx: 624 }
    }
    fn next_u32(&mut self) -> u32 {
        if self.idx >= 624 {
            self.twist();
        }
        let mut y = self.mt[self.idx];
        self.idx += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }
    fn twist(&mut self) {
        for i in 0..624 {
            let y = (self.mt[i] & 0x8000_0000) | (self.mt[(i + 1) % 624] & 0x7fff_ffff);
            self.mt[i] = self.mt[(i + 397) % 624] ^ (y >> 1);
            if y & 1 != 0 {
                self.mt[i] ^= 0x9908_b0df;
            }
        }
        self.idx = 0;
    }
    fn random(&mut self) -> f64 {
        let a = self.next_u32() >> 5;
        let b = self.next_u32() >> 6;
        (a as f64 * 67108864.0 + b as f64) * (1.0 / 9007199254740992.0)
    }
}
